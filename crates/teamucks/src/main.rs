#![deny(clippy::all, clippy::pedantic)]
#![allow(clippy::module_name_repetitions)]

use std::collections::HashMap;

use clap::Parser;
use teamucks_core::{
    actor::{pty_reader::pty_reader, SessionActor, SessionMsg},
    config::load_config,
    pane::{Pane, PaneId},
    protocol::ServerMessage,
    server::{default_socket_path, ClientId},
    session::{Session, SessionId},
    window::{Window, WindowId},
};
use tokio::sync::mpsc;

#[derive(Parser, Debug)]
#[command(name = "teamucks", about = "A modern terminal multiplexer")]
struct Cli {
    /// Server name (selects the socket to connect to or create)
    #[arg(long, default_value = "default")]
    server: String,

    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(clap::Subcommand, Debug)]
enum Command {
    /// Attach to an existing session
    Attach {
        /// Session name
        session: Option<String>,
    },
    /// List sessions
    List,
    /// Start the server in the foreground (Phase 1)
    StartServer,
}

fn main() {
    tracing_subscriber::fmt::init();
    let cli = Cli::parse();
    tracing::info!(server = %cli.server, "teamucks starting");

    let config = load_config();
    tracing::info!(
        prefix = ?config.prefix_key,
        scrollback = config.scrollback_lines,
        shell = %config.default_shell,
        "configuration loaded"
    );

    match cli.command {
        None | Some(Command::StartServer) => {
            start_server(&cli.server, &config);
        }
        Some(Command::Attach { session }) => {
            println!("teamucks: attach not yet implemented (session: {session:?})");
        }
        Some(Command::List) => println!("teamucks: list not yet implemented"),
    }
}

/// Start the server in the foreground using a new tokio runtime.
///
/// Startup flow (Feature I3):
/// 1. Spawn a real shell pane using `$SHELL` (from validated config).
/// 2. Create a Window and Session containing that pane.
/// 3. Create the actor channel bus.
/// 4. Spawn the PTY reader task.
/// 5. Create an in-process client channel pair.
/// 6. Spawn the session actor task.
/// 7. Send `NewClient` to the actor so it emits `HandshakeResponse` + `FullFrame`.
/// 8. Block on the in-process client receiver, logging received frames.
///    Full rendering (I4/I5) will replace the logging once the alternate screen
///    and terminal renderer are wired in.
///
/// # Resize limitation
///
/// Terminal resize is not handled until Feature I14.  Resizing the host
/// terminal during branches I3–I13 will cause display corruption.
fn start_server(server_name: &str, config: &teamucks_core::config::types::ValidatedConfig) {
    let socket_path = default_socket_path(server_name);
    tracing::info!(socket = %socket_path.display(), "starting server");

    let rt = tokio::runtime::Runtime::new().expect("tokio runtime must be constructable");
    rt.block_on(async move {
        // ── 1. Spawn the initial pane ─────────────────────────────────────────
        let pane_id = PaneId(1);
        let cols: u16 = 80;
        // Reserve one row for the status bar (rendered in I5+).
        let pane_rows: u16 = 23;
        let total_rows: u16 = 24;

        let pane = match Pane::spawn(pane_id, cols, pane_rows, &config.default_shell, &[]) {
            Ok(p) => p,
            Err(e) => {
                eprintln!("teamucks: failed to spawn shell: {e}");
                std::process::exit(1);
            }
        };
        let pty_fd = pane.pty_fd();
        tracing::info!(shell = %config.default_shell, "shell pane spawned");

        // ── 2. Create window and session ──────────────────────────────────────
        let window = Window::new_with_dimensions(WindowId(1), "main", pane_id, cols, pane_rows);
        let session = Session::new(SessionId(1), "default", window);
        let mut panes = HashMap::new();
        panes.insert(pane_id, pane);

        // ── 3. Create channel bus ─────────────────────────────────────────────
        let (actor_tx, actor_rx) = mpsc::channel::<SessionMsg>(512);

        // ── 4. Spawn PTY reader task ──────────────────────────────────────────
        tokio::spawn(pty_reader(pane_id, pty_fd, actor_tx.clone()));

        // ── 5. Create in-process client channel ───────────────────────────────
        let in_process_id = ClientId::next();
        let (to_client_tx, mut to_client_rx) = mpsc::channel::<ServerMessage>(64);

        // ── 6. Spawn session actor ────────────────────────────────────────────
        let actor = SessionActor::new(
            session,
            panes,
            config.clone(),
            actor_rx,
            actor_tx.clone(),
            cols,
            total_rows,
        );
        let actor_handle = tokio::spawn(async move { actor.run().await });

        // ── 7. Register in-process client ─────────────────────────────────────
        actor_tx
            .send(SessionMsg::NewClient {
                id: in_process_id,
                cols,
                rows: total_rows,
                tx: to_client_tx,
            })
            .await
            .expect("actor channel must be open at startup");

        tracing::info!(client_id = %in_process_id, "in-process client registered");

        // ── 8. Drive the in-process client receiver ───────────────────────────
        // Full rendering (I5) will write frames to stdout via TerminalRenderer.
        // For now we log each ServerMessage at DEBUG level so the actor exercise
        // is observable without a real screen.
        //
        // The loop exits when the actor shuts down and closes the sender.
        while let Some(msg) = to_client_rx.recv().await {
            match &msg {
                ServerMessage::HandshakeResponse { protocol_version, server_name } => {
                    tracing::debug!(protocol_version, server_name, "handshake response received");
                }
                ServerMessage::FullFrame { pane_id, cols, rows, .. } => {
                    tracing::debug!(pane_id, cols, rows, "full frame received");
                }
                ServerMessage::FrameDiff { pane_id, diffs } => {
                    tracing::debug!(pane_id, diffs = diffs.len(), "frame diff received");
                }
                ServerMessage::CursorUpdate { pane_id, col, row, visible, .. } => {
                    tracing::debug!(pane_id, col, row, visible, "cursor update received");
                }
                other => {
                    tracing::debug!(?other, "server message received");
                }
            }
        }

        tracing::info!("in-process client channel closed — server shutting down");
        // Wait for the actor task to finish.
        let _ = actor_handle.await;
    });
}
