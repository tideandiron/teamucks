/// In-process client tasks for the teamucks terminal multiplexer.
///
/// This module provides two async tasks and one synchronous helper that
/// together wire the in-process client to the session actor:
///
/// - [`render_server_message`] — converts a [`ServerMessage`] into escape
///   sequences using a [`TerminalRenderer`], returning a byte slice.
/// - [`in_process_output_writer`] — receives [`ServerMessage`] frames from
///   the actor and writes the rendered escape sequences to an
///   [`tokio::io::AsyncWrite`] sink (typically stdout).
/// - [`in_process_input_reader`] — reads raw bytes from stdin using a
///   non-blocking [`tokio::io::AsyncFd`] wrapper, wraps them as
///   [`ClientMessage::KeyEvent`], and sends them to the actor via a
///   [`SessionMsg::ClientInput`] message.
///
/// # Design notes
///
/// The output writer owns a [`TerminalRenderer`] and is the only place that
/// calls `render_*` methods.  The actor itself never touches the renderer;
/// it sends protocol messages and lets the writer handle presentation.
///
/// Stdin reading uses `tokio::io::stdin()` which is backed by an
/// `AsyncFd<RawFd>` internally, giving the tokio reactor visibility into
/// readability without blocking a worker thread.
use tokio::io::{AsyncWrite, AsyncWriteExt as _};
use tokio::sync::mpsc;

use teamucks_core::{
    actor::SessionMsg,
    protocol::{ClientMessage, ServerMessage},
    render::TerminalRenderer,
    server::ClientId,
};

// ---------------------------------------------------------------------------
// render_server_message
// ---------------------------------------------------------------------------

/// Convert a [`ServerMessage`] into escape sequences.
///
/// Only frame-bearing messages produce output:
///
/// | Variant | Method called |
/// |---|---|
/// | `FullFrame` | [`TerminalRenderer::render_full_frame`] |
/// | `FrameDiff` | [`TerminalRenderer::render_diff`] |
/// | `CursorUpdate` | [`TerminalRenderer::render_cursor`] |
/// | All others | returns empty slice (`&[]`) |
///
/// The returned slice borrows from the renderer's internal buffer and is
/// valid until the next call to any `render_*` method.
///
/// # Examples
///
/// ```
/// use teamucks_core::render::TerminalRenderer;
/// use teamucks_core::protocol::{ServerMessage, CellData, ColorData};
/// use teamucks::client::render_server_message;
///
/// let mut renderer = TerminalRenderer::new();
/// let frame = ServerMessage::FullFrame {
///     pane_id: 1,
///     cols: 1,
///     rows: 1,
///     cells: vec![CellData {
///         grapheme: "A".to_owned(),
///         fg: ColorData::Default,
///         bg: ColorData::Default,
///         attrs: 0,
///         flags: 0,
///     }],
/// };
/// let bytes = render_server_message(&mut renderer, &frame);
/// assert!(!bytes.is_empty());
/// ```
pub fn render_server_message<'a>(
    renderer: &'a mut TerminalRenderer,
    msg: &ServerMessage,
) -> &'a [u8] {
    match msg {
        ServerMessage::FullFrame { .. } => renderer.render_full_frame(msg),
        ServerMessage::FrameDiff { .. } => renderer.render_diff(msg),
        ServerMessage::CursorUpdate { .. } => renderer.render_cursor(msg),
        // Non-visual messages: HandshakeResponse, LayoutChange, StatusUpdate,
        // Bell, TitleChange.  These have no screen representation in Phase 1.
        _ => &[],
    }
}

// ---------------------------------------------------------------------------
// in_process_output_writer
// ---------------------------------------------------------------------------

/// Async task: receive [`ServerMessage`] frames from the actor and write
/// rendered escape sequences to `sink`.
///
/// The task runs until the channel is closed (the actor dropped all
/// [`mpsc::Sender`] handles), after which it returns cleanly.
///
/// `cols` and `rows` are the initial terminal dimensions, available for future
/// use when the renderer needs to know the viewport size (e.g., absolute cursor
/// positioning relative to the screen).
///
/// # Cancellation safety
///
/// The task exits cleanly when `rx` is closed.  Any in-flight
/// `write_all`/`flush` calls are awaited to completion before the task returns,
/// so no partial escape sequences are written to the sink.
///
/// # Errors
///
/// Write errors on `sink` are logged at `WARN` level and cause the task to
/// exit early (the terminal is probably gone).
pub async fn in_process_output_writer<W>(
    mut rx: mpsc::Receiver<ServerMessage>,
    mut sink: W,
    // Terminal dimensions — reserved for Phase 2 viewport-aware rendering.
    _cols: u16,
    _rows: u16,
) where
    W: AsyncWrite + Unpin,
{
    let mut renderer = TerminalRenderer::new();

    while let Some(msg) = rx.recv().await {
        let bytes = render_server_message(&mut renderer, &msg);

        if bytes.is_empty() {
            // Non-visual message; nothing to write.
            continue;
        }

        // Copy the rendered bytes into an owned Vec before calling write_all,
        // because write_all takes ownership of the &mut sink across an await
        // point and the renderer buffer is tied to the &mut renderer borrow.
        //
        // Allocation rationale: this is at most ~120 Hz; one small Vec per
        // frame does not violate the allocation discipline on the hot path
        // (which is inside the VTE parser, not here).
        let owned: Vec<u8> = bytes.to_vec();

        if let Err(e) = sink.write_all(&owned).await {
            tracing::warn!(error = %e, "output writer: write failed, exiting");
            return;
        }
        if let Err(e) = sink.flush().await {
            tracing::warn!(error = %e, "output writer: flush failed, exiting");
            return;
        }
    }

    tracing::debug!("output writer: channel closed, exiting");
}

// ---------------------------------------------------------------------------
// in_process_input_reader
// ---------------------------------------------------------------------------

/// Async task: read raw bytes from stdin and forward them to the actor as
/// [`SessionMsg::ClientInput`] messages.
///
/// Stdin is read via `tokio::io::stdin()`, which internally uses `AsyncFd` to
/// integrate with the tokio reactor without blocking a worker thread.  The
/// calling code is responsible for putting stdin into raw mode before spawning
/// this task.
///
/// Each non-empty read produces one [`ClientMessage::KeyEvent`] with
/// `modifiers: 0`.  The raw bytes (UTF-8 sequences, CSI sequences, etc.) are
/// forwarded verbatim to the actor.  Key parsing and prefix routing occur in
/// the actor's `handle_client_input` handler (fully implemented in I7).
///
/// The task exits when:
/// - Stdin returns EOF (read returns 0 bytes).
/// - A read error occurs.
/// - The actor's channel is closed (`tx.send` returns `Err`).
pub async fn in_process_input_reader(tx: mpsc::Sender<SessionMsg>, client_id: ClientId) {
    use tokio::io::AsyncReadExt as _;

    let mut stdin = tokio::io::stdin();
    let mut buf = [0u8; 256];

    loop {
        let n = match stdin.read(&mut buf).await {
            Ok(0) => {
                tracing::debug!("input reader: stdin EOF, exiting");
                break;
            }
            Ok(n) => n,
            Err(e) => {
                tracing::warn!(error = %e, "input reader: stdin read error, exiting");
                break;
            }
        };

        let key = buf[..n].to_vec();
        let msg = SessionMsg::ClientInput {
            id: client_id,
            message: ClientMessage::KeyEvent { key, modifiers: 0 },
        };

        if tx.send(msg).await.is_err() {
            tracing::debug!("input reader: actor channel closed, exiting");
            break;
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use teamucks_core::protocol::{CellData, ColorData, CursorShape, DiffEntry, ServerMessage};

    fn default_cell(grapheme: &str) -> CellData {
        CellData {
            grapheme: grapheme.to_owned(),
            fg: ColorData::Default,
            bg: ColorData::Default,
            attrs: 0,
            flags: 0,
        }
    }

    #[test]
    fn test_render_server_message_full_frame_produces_bytes() {
        let mut renderer = TerminalRenderer::new();
        let frame = ServerMessage::FullFrame {
            pane_id: 1,
            cols: 2,
            rows: 1,
            cells: vec![default_cell("A"), default_cell("B")],
        };
        let bytes = render_server_message(&mut renderer, &frame);
        assert!(!bytes.is_empty(), "FullFrame must produce escape sequences");
        assert!(
            bytes.windows(8).any(|w| w == b"\x1b[?2026h"),
            "must have sync-start marker; got: {bytes:?}",
        );
    }

    #[test]
    fn test_render_server_message_frame_diff_produces_bytes() {
        let mut renderer = TerminalRenderer::new();
        let diff = ServerMessage::FrameDiff {
            pane_id: 1,
            diffs: vec![DiffEntry::CellChange { col: 0, row: 0, cell: default_cell("X") }],
        };
        let bytes = render_server_message(&mut renderer, &diff);
        assert!(!bytes.is_empty(), "FrameDiff must produce escape sequences");
    }

    #[test]
    fn test_render_server_message_cursor_update_produces_bytes() {
        let mut renderer = TerminalRenderer::new();
        let update = ServerMessage::CursorUpdate {
            pane_id: 1,
            col: 0,
            row: 0,
            visible: true,
            shape: CursorShape::Block,
        };
        let bytes = render_server_message(&mut renderer, &update);
        assert!(!bytes.is_empty(), "CursorUpdate must produce escape sequences");
    }

    #[test]
    fn test_render_server_message_handshake_returns_empty() {
        let mut renderer = TerminalRenderer::new();
        let msg = ServerMessage::HandshakeResponse {
            protocol_version: 1,
            server_name: "teamucks".to_owned(),
        };
        let bytes = render_server_message(&mut renderer, &msg);
        assert!(bytes.is_empty(), "HandshakeResponse must return empty slice");
    }

    #[test]
    fn test_render_server_message_layout_change_returns_empty() {
        let mut renderer = TerminalRenderer::new();
        let bytes = render_server_message(&mut renderer, &ServerMessage::LayoutChange);
        assert!(bytes.is_empty(), "LayoutChange must return empty slice");
    }

    #[test]
    fn test_render_server_message_status_update_returns_empty() {
        let mut renderer = TerminalRenderer::new();
        let msg = ServerMessage::StatusUpdate { content: "test".to_owned() };
        let bytes = render_server_message(&mut renderer, &msg);
        // StatusUpdate has no screen-level representation in Phase 1.
        assert!(bytes.is_empty(), "StatusUpdate must return empty slice");
    }

    #[test]
    fn test_render_server_message_bell_returns_empty() {
        let mut renderer = TerminalRenderer::new();
        let msg = ServerMessage::Bell { pane_id: 1 };
        let bytes = render_server_message(&mut renderer, &msg);
        assert!(bytes.is_empty(), "Bell must return empty slice");
    }
}
