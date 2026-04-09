//! Session actor: the single mutable owner of all session state.
//!
//! Submodules:
//! - [`pty_reader`]: async task that forwards PTY master output to the actor.
//!
//! The [`SessionActor`] runs a `tokio::select!` loop that drains all pending
//! [`SessionMsg`] messages before rendering.  This coalesces bursts of PTY
//! output from fast producers before painting, minimising redundant frames.
//!
//! # Event loop design
//!
//! ```text
//! loop {
//!     // Drain phase: process every pending message without blocking.
//!     while let Ok(msg) = rx.try_recv() { handle(msg); }
//!
//!     // Wait phase: block until a render tick fires OR a new message arrives.
//!     select! {
//!         _ = render_interval.tick() => handle_render_tick(),
//!         Some(msg) = rx.recv()      => handle(msg),
//!         else                       => break,
//!     }
//! }
//! ```
//!
//! # Note on `RenderTick`
//!
//! `RenderTick` is intentionally absent from [`SessionMsg`].  Rendering is
//! driven exclusively by the internal `tokio::time::interval` timer arm in the
//! select loop.  For tests that need to trigger a render synchronously, call
//! [`SessionActor::force_render`].

pub mod pty_reader;

use std::collections::{HashMap, HashSet};
use std::time::Duration;

use tokio::sync::mpsc;
use tokio::time::MissedTickBehavior;

use crate::{
    config::types::ValidatedConfig,
    input::prefix::{InputAction, InputStateMachine},
    pane::{Pane, PaneId},
    protocol::{ClientMessage, ServerMessage, PROTOCOL_VERSION},
    render::{statusbar::StatusBar, TerminalRenderer},
    server::ClientId,
    session::Session,
    window::WindowId,
};

// ---------------------------------------------------------------------------
// SessionMsg
// ---------------------------------------------------------------------------

/// All events processed by the session actor.
///
/// Every event in the system enters the actor through this enum.  The actor is
/// the only task that mutates session, window, pane, or layout state.
///
/// # Note
///
/// `RenderTick` is intentionally absent.  Rendering is driven exclusively by
/// the `tokio::time::interval` timer inside [`SessionActor::run`].
#[derive(Debug)]
pub enum SessionMsg {
    // -- Client lifecycle -----------------------------------------------------
    /// A new socket client connected.
    NewClient {
        /// Stable client identifier.
        id: ClientId,
        /// Client terminal width in columns.
        cols: u16,
        /// Client terminal height in rows.
        rows: u16,
        /// Per-client sender for pushing [`ServerMessage`] to the writer task.
        tx: mpsc::Sender<ServerMessage>,
    },

    /// A connected client sent input (key, mouse, resize, command, paste).
    ClientInput {
        /// Which client sent the input.
        id: ClientId,
        /// The decoded client message.
        message: ClientMessage,
    },

    /// A client disconnected (clean or abrupt).
    ClientDisconnected {
        /// Which client disconnected.
        id: ClientId,
    },

    // -- PTY output -----------------------------------------------------------
    /// A PTY reader read bytes from a pane's master fd.
    PtyOutput {
        /// Which pane produced the output.
        pane_id: PaneId,
        /// Raw bytes read from the PTY master.
        data: Vec<u8>,
    },

    /// A pane's child process exited.
    PaneDied {
        /// Which pane's child exited.
        pane_id: PaneId,
        /// The child's exit code.
        exit_code: i32,
    },

    // -- Signals --------------------------------------------------------------
    /// Host terminal was resized (SIGWINCH on the process's controlling tty).
    HostResize {
        /// New width in columns.
        cols: u16,
        /// New height in rows.
        rows: u16,
    },

    /// Server should shut down gracefully (SIGTERM or SIGINT).
    Shutdown,
}

// ---------------------------------------------------------------------------
// NavDirection
// ---------------------------------------------------------------------------

/// Directional navigation within a window's pane layout.
///
/// Used by commands that move focus between adjacent panes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NavDirection {
    /// Move focus to the pane to the left.
    Left,
    /// Move focus to the pane to the right.
    Right,
    /// Move focus to the pane above.
    Up,
    /// Move focus to the pane below.
    Down,
}

// ---------------------------------------------------------------------------
// ConnectedClient
// ---------------------------------------------------------------------------

/// Per-client state owned by the session actor.
pub struct ConnectedClient {
    /// Stable client identifier.
    pub id: ClientId,
    /// Terminal width reported by this client.
    pub cols: u16,
    /// Terminal height reported by this client.
    pub rows: u16,
    /// Sender for pushing [`ServerMessage`] to this client's writer task.
    pub tx: mpsc::Sender<ServerMessage>,
}

// ---------------------------------------------------------------------------
// SessionActor
// ---------------------------------------------------------------------------

/// The session actor: owns all mutable multiplexer state.
///
/// Spawned as a single `tokio` task.  All other tasks communicate with it
/// exclusively through the [`SessionMsg`] channel.
///
/// # Fields marked `#[allow(dead_code)]`
///
/// Several fields are skeleton placeholders for state that will be actively
/// read in subsequent integration features (I2–I13).  The lint is suppressed
/// here rather than at the module level so the restriction remains tight.
pub struct SessionActor {
    /// Session model — actively used once window/pane lifecycle is wired (I4+).
    #[allow(dead_code)]
    session: Session,
    panes: HashMap<PaneId, Pane>,
    /// Per-pane child PIDs — used for signal delivery (SIGWINCH, SIGTERM).
    /// Populated when PTY tasks register in I2.
    #[allow(dead_code)]
    pane_pids: HashMap<PaneId, nix::unistd::Pid>,
    clients: HashMap<ClientId, ConnectedClient>,
    dirty_panes: HashSet<PaneId>,
    layout_dirty: bool,
    /// Input state machine — active in I3 when key routing is wired.
    #[allow(dead_code)]
    input_sm: InputStateMachine,
    focused_client: Option<ClientId>,
    /// Terminal renderer — active in I3 when frame diff is wired.
    #[allow(dead_code)]
    renderer: TerminalRenderer,
    status_bar: StatusBar,
    /// Pane ID counter — used when spawning new panes (I4+).
    #[allow(dead_code)]
    next_pane_id: u32,
    /// Window ID counter — used when creating new windows (I4+).
    #[allow(dead_code)]
    next_window_id: u32,
    /// Runtime configuration — active once key/resize routing is wired (I3+).
    #[allow(dead_code)]
    config: ValidatedConfig,
    rx: mpsc::Receiver<SessionMsg>,
    /// Cloned sender for PTY reader and signal tasks to post messages back.
    #[allow(dead_code)]
    self_tx: mpsc::Sender<SessionMsg>,
}

impl SessionActor {
    /// Create a new [`SessionActor`].
    ///
    /// # Parameters
    ///
    /// - `session`: The initial session model.
    /// - `panes`: Live pane map (may be empty at startup before PTY tasks register panes).
    /// - `config`: Validated runtime configuration.
    /// - `rx`: The actor's message receiver.
    /// - `self_tx`: A clone of the sender end of `rx`; given to PTY reader and
    ///   signal tasks so they can post messages back to the actor.
    /// - `initial_cols` / `initial_rows`: Host terminal dimensions at startup.
    ///
    /// # Examples
    ///
    /// ```
    /// use std::collections::HashMap;
    /// use teamucks_core::{
    ///     actor::{SessionActor, SessionMsg},
    ///     config::types::ValidatedConfig,
    ///     pane::PaneId,
    ///     session::{Session, SessionId},
    ///     window::{Window, WindowId},
    /// };
    /// use tokio::sync::mpsc;
    ///
    /// let window = Window::new_empty(WindowId(1), "main", PaneId(1));
    /// let session = Session::new(SessionId(1), "test", window);
    /// let (actor_tx, actor_rx) = mpsc::channel::<SessionMsg>(64);
    /// let self_tx = actor_tx.clone();
    /// let actor = SessionActor::new(session, HashMap::new(), ValidatedConfig::default(), actor_rx, self_tx, 80, 24);
    /// ```
    #[must_use]
    pub fn new(
        session: Session,
        panes: HashMap<PaneId, Pane>,
        config: ValidatedConfig,
        rx: mpsc::Receiver<SessionMsg>,
        self_tx: mpsc::Sender<SessionMsg>,
        initial_cols: u16,
        // Initial row count reserved for future resize logic (I6+).
        _initial_rows: u16,
    ) -> Self {
        let input_sm = InputStateMachine::new(config.prefix_key.clone(), Duration::from_secs(1));
        let status_bar = StatusBar::new(initial_cols);

        Self {
            session,
            panes,
            pane_pids: HashMap::new(),
            clients: HashMap::new(),
            dirty_panes: HashSet::new(),
            layout_dirty: false,
            input_sm,
            focused_client: None,
            renderer: TerminalRenderer::new(),
            status_bar,
            next_pane_id: 2,
            next_window_id: 2,
            config,
            rx,
            self_tx,
        }
    }

    /// Run the actor's event loop until shutdown.
    ///
    /// The loop follows a drain-then-render pattern:
    ///
    /// 1. Drain all pending messages via `try_recv` (non-blocking).
    /// 2. Wait in `select!` for either a render timer tick or a new message.
    ///
    /// `Shutdown` causes the loop to exit.  Channel disconnection also causes
    /// the loop to exit cleanly.
    pub async fn run(mut self) {
        // ~60 fps render interval.
        let mut render_interval = tokio::time::interval(Duration::from_millis(16));
        render_interval.set_missed_tick_behavior(MissedTickBehavior::Skip);

        loop {
            // ── Drain phase ─────────────────────────────────────────────────
            // Process every pending message without blocking so bursts of PTY
            // output are coalesced before the next render pass.
            loop {
                match self.rx.try_recv() {
                    Ok(msg) => {
                        if self.dispatch(msg) {
                            return; // Shutdown
                        }
                    }
                    Err(mpsc::error::TryRecvError::Empty) => break,
                    Err(mpsc::error::TryRecvError::Disconnected) => return,
                }
            }

            // ── Wait phase ───────────────────────────────────────────────────
            // All pending messages are drained.  Block until the render timer
            // fires or a new message arrives.
            tokio::select! {
                _ = render_interval.tick() => {
                    self.handle_render_tick();
                }

                Some(msg) = self.rx.recv() => {
                    if self.dispatch(msg) {
                        return; // Shutdown
                    }
                    // Fall through to drain on next loop iteration.
                }

                else => break,
            }
        }
    }

    /// Force an immediate render pass.
    ///
    /// This is a test-only escape hatch that calls [`handle_render_tick`]
    /// directly, bypassing the timer.  Use it in tests to trigger a render
    /// without waiting for the 16 ms interval.
    ///
    /// [`handle_render_tick`]: SessionActor::handle_render_tick
    #[cfg(test)]
    pub fn force_render(&mut self) {
        self.handle_render_tick();
    }

    // -------------------------------------------------------------------------
    // Message dispatch
    // -------------------------------------------------------------------------

    /// Dispatch a single [`SessionMsg`] to the appropriate handler.
    ///
    /// Returns `true` if the actor should shut down after this message.
    fn dispatch(&mut self, msg: SessionMsg) -> bool {
        match msg {
            SessionMsg::NewClient { id, cols, rows, tx } => {
                self.handle_new_client(id, cols, rows, tx);
            }
            SessionMsg::ClientInput { id, message } => {
                self.handle_client_input(id, &message);
            }
            SessionMsg::ClientDisconnected { id } => {
                self.handle_client_disconnected(id);
            }
            SessionMsg::PtyOutput { pane_id, data } => {
                self.handle_pty_output(pane_id, &data);
            }
            SessionMsg::PaneDied { pane_id, exit_code } => {
                self.handle_pane_died(pane_id, exit_code);
            }
            SessionMsg::HostResize { cols, rows } => {
                self.handle_host_resize(cols, rows);
            }
            SessionMsg::Shutdown => {
                self.handle_shutdown();
                return true;
            }
        }
        false
    }

    // -------------------------------------------------------------------------
    // Handlers
    // -------------------------------------------------------------------------

    /// Handle a new client connecting.
    ///
    /// Stores the [`ConnectedClient`], assigns focus if this is the first
    /// client, sends a [`ServerMessage::HandshakeResponse`], and immediately
    /// sends a [`ServerMessage::FullFrame`] for every pane in the active window.
    fn handle_new_client(
        &mut self,
        id: ClientId,
        cols: u16,
        rows: u16,
        tx: mpsc::Sender<ServerMessage>,
    ) {
        tracing::debug!(client_id = %id, cols, rows, "new client");

        // First client becomes the focused client.
        if self.focused_client.is_none() {
            self.focused_client = Some(id);
            tracing::debug!(client_id = %id, "client is now focused");
        }

        let client = ConnectedClient { id, cols, rows, tx };

        // ── HandshakeResponse ─────────────────────────────────────────────────
        let response = ServerMessage::HandshakeResponse {
            protocol_version: PROTOCOL_VERSION,
            server_name: "teamucks".to_owned(),
        };

        // try_send: if the channel is full the client will see it on the next
        // message.  We never block the actor on a single slow client.
        if let Err(e) = client.tx.try_send(response) {
            tracing::warn!(client_id = %id, error = %e, "failed to send HandshakeResponse");
        }

        // ── FullFrame for every visible pane in the active window ─────────────
        // Collect pane IDs from the active window's layout tree.
        let mut pane_ids = Vec::new();
        self.session.active_window().layout().root.collect_ids(&mut pane_ids);

        for pane_id in pane_ids {
            if let Some(pane) = self.panes.get_mut(&pane_id) {
                let full_frame = pane.full_frame();
                if let Err(e) = client.tx.try_send(full_frame) {
                    tracing::warn!(
                        client_id = %id,
                        pane_id = %pane_id,
                        error = %e,
                        "failed to send FullFrame to new client"
                    );
                }
            }
        }

        self.clients.insert(id, client);
    }

    /// Handle input from a connected client.
    ///
    /// Only the focused client drives the input state machine.  All other
    /// client input is silently dropped.
    ///
    /// For [`ClientMessage::KeyEvent`], the raw bytes are forwarded directly
    /// to the active pane's PTY via [`Pane::write_input`].  Full prefix-key
    /// routing (the `InputStateMachine`) is wired in Feature I7.
    fn handle_client_input(&mut self, id: ClientId, message: &ClientMessage) {
        tracing::debug!(client_id = %id, "client input");
        if self.focused_client != Some(id) {
            return;
        }

        if let ClientMessage::KeyEvent { key, .. } = message {
            // Determine the active pane for the focused session window.
            let active_pane_id = self.session.active_window().active_pane_id();

            if let Some(pane) = self.panes.get(&active_pane_id) {
                if let Err(e) = pane.write_input(key) {
                    // PTY write failure is logged but not propagated — the
                    // child may have exited; the pty_reader will send PaneDied.
                    tracing::warn!(
                        pane_id = %active_pane_id,
                        error = %e,
                        "failed to write key input to pane PTY"
                    );
                }
            } else {
                tracing::debug!(pane_id = %active_pane_id, "active pane not found — input dropped");
            }
        }
        // MouseEvent, Resize, Command, PasteEvent: handled in subsequent features (I6+).
    }

    /// Handle a client disconnecting.
    ///
    /// Removes the client from the registry.  If the disconnected client held
    /// focus, focus is transferred to the first remaining client (or cleared if
    /// no clients remain).
    fn handle_client_disconnected(&mut self, id: ClientId) {
        tracing::debug!(client_id = %id, "client disconnected");
        self.clients.remove(&id);

        if self.focused_client == Some(id) {
            // Reassign focus to an arbitrary remaining client.
            self.focused_client = self.clients.keys().next().copied();
            if let Some(new_focus) = self.focused_client {
                tracing::debug!(client_id = %new_focus, "focus reassigned");
            }
        }
    }

    /// Handle PTY output from a pane (stub).
    ///
    /// Looks up the pane by `pane_id`, feeds the data into the terminal
    /// emulator, and marks the pane dirty for the next render tick.  If the
    /// pane is not found the output is silently ignored.
    fn handle_pty_output(&mut self, pane_id: PaneId, data: &[u8]) {
        tracing::debug!(pane_id = %pane_id, bytes = data.len(), "pty output");
        if let Some(pane) = self.panes.get_mut(&pane_id) {
            pane.feed(data);
            self.dirty_panes.insert(pane_id);
        } else {
            tracing::debug!(pane_id = %pane_id, "pty output for unknown pane — ignored");
        }
    }

    /// Handle a pane's child process dying (stub).
    ///
    /// Unknown panes are silently ignored.  Full exit-behavior dispatch
    /// (close/hold/respawn) will be implemented in a subsequent feature.
    fn handle_pane_died(&mut self, pane_id: PaneId, exit_code: i32) {
        tracing::debug!(pane_id = %pane_id, exit_code, "pane died");
        if !self.panes.contains_key(&pane_id) {
            tracing::debug!(pane_id = %pane_id, "PaneDied for unknown pane — ignored");
        }
        // Full cascade (WindowEmpty → SessionEmpty → Shutdown) in I4/I5.
    }

    /// Handle a host terminal resize (stub).
    ///
    /// Stores the new dimensions on the status bar.  Full resize propagation
    /// (SIGWINCH to children, layout reflow, `FullFrame` broadcast) will be
    /// implemented in a subsequent feature.
    fn handle_host_resize(&mut self, cols: u16, rows: u16) {
        tracing::debug!(cols, rows, "host resize");
        self.status_bar = StatusBar::new(cols);
        self.layout_dirty = true;
    }

    /// Handle the render timer tick.
    ///
    /// For each pane marked dirty since the last tick:
    /// 1. Call [`Pane::compute_diff`] to produce an incremental
    ///    [`ServerMessage::FrameDiff`] (or [`ServerMessage::CursorUpdate`] if
    ///    only the cursor moved).
    /// 2. `try_send` the message to every connected client.  Slow clients are
    ///    never blocked — if their channel is full the diff is dropped and the
    ///    next tick will produce a new diff.
    ///
    /// After processing all dirty panes the dirty set is cleared.
    ///
    /// # Resize limitation
    ///
    /// Terminal resize is not handled until Feature I14.  Resizing the host
    /// terminal during branches I3–I13 will cause display corruption.
    fn handle_render_tick(&mut self) {
        if self.dirty_panes.is_empty() && !self.layout_dirty {
            return;
        }

        // Collect dirty pane IDs into a local Vec so we can mutably borrow
        // `self.panes` separately from `self.clients`.
        let dirty: Vec<PaneId> = self.dirty_panes.drain().collect();
        self.layout_dirty = false;

        for pane_id in dirty {
            let Some(pane) = self.panes.get_mut(&pane_id) else {
                // Pane was removed between the dirty mark and this tick.
                continue;
            };

            let msg = pane.compute_diff();

            // Broadcast to every connected client.  We never block on a slow
            // client — if the channel is full we log at TRACE and move on.
            for client in self.clients.values() {
                if let Err(e) = client.tx.try_send(msg.clone()) {
                    tracing::trace!(
                        client_id = %client.id,
                        pane_id = %pane_id,
                        error = %e,
                        "render tick: client channel full, diff dropped"
                    );
                }
            }
        }
    }

    /// Handle a graceful shutdown request (stub).
    ///
    /// Logs the event and returns.  Full SIGTERM delivery and socket cleanup
    /// will be implemented in Feature I13.
    ///
    /// `&mut self` is required here even though no state is mutated yet:
    /// the I13 implementation will SIGTERM all pane children via `pane_pids`.
    #[allow(clippy::unused_self)]
    fn handle_shutdown(&mut self) {
        tracing::info!("session actor shutting down");
        // Full shutdown (SIGTERM to pane children, socket removal) in I13.
    }

    // -------------------------------------------------------------------------
    // Accessors (used in tests)
    // -------------------------------------------------------------------------

    /// Return the [`ClientId`] of the currently focused client, if any.
    #[cfg(test)]
    #[must_use]
    pub fn focused_client(&self) -> Option<ClientId> {
        self.focused_client
    }

    /// Return the number of currently connected clients.
    #[cfg(test)]
    #[must_use]
    pub fn client_count(&self) -> usize {
        self.clients.len()
    }
}

// ---------------------------------------------------------------------------
// Private helpers
// ---------------------------------------------------------------------------

// The `_window_id_helper` is referenced only in future features; suppress the
// dead-code lint on WindowId which is imported for future use.
const _: () = {
    let _ = std::mem::size_of::<WindowId>();
    let _ = std::mem::size_of::<InputAction>();
};
