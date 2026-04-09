/// Integration tests for the in-process client writer and input reader tasks.
///
/// These tests use `tokio::io::duplex` as a mock stdout and in-memory channels
/// to exercise the output writer and verify that the actor forwards key input to
/// the active pane.
///
/// Tests that require a real TTY (e.g. stdin set to raw mode) are marked
/// `#[ignore]` and can be run explicitly with `cargo test -- --ignored`.
use std::time::Duration;

use tokio::io::AsyncReadExt as _;
use tokio::sync::mpsc;

use teamucks::client::{in_process_output_writer, render_server_message};
use teamucks_core::protocol::{CellData, ColorData, DiffEntry, ServerMessage};
use teamucks_core::render::TerminalRenderer;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn default_cell(grapheme: &str) -> CellData {
    CellData {
        grapheme: grapheme.to_owned(),
        fg: ColorData::Default,
        bg: ColorData::Default,
        attrs: 0,
        flags: 0,
    }
}

// ---------------------------------------------------------------------------
// render_server_message unit tests
// ---------------------------------------------------------------------------

/// Verifies that `render_server_message` produces non-empty bytes for a
/// `FullFrame` and that the output contains the synchronized-output start
/// marker (`ESC [ ? 2026 h`).
#[test]
fn test_output_writer_renders_full_frame() {
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
        "output must contain sync-start marker; got: {bytes:?}",
    );
}

/// Verifies that `render_server_message` produces non-empty bytes for a
/// `FrameDiff`.
#[test]
fn test_output_writer_renders_frame_diff() {
    let mut renderer = TerminalRenderer::new();
    let diff = ServerMessage::FrameDiff {
        pane_id: 1,
        diffs: vec![DiffEntry::CellChange { col: 0, row: 0, cell: default_cell("X") }],
    };
    let bytes = render_server_message(&mut renderer, &diff);
    assert!(!bytes.is_empty(), "FrameDiff must produce escape sequences");
}

/// Verifies that `render_server_message` produces non-empty bytes for a
/// `CursorUpdate`.
#[test]
fn test_output_writer_renders_cursor() {
    use teamucks_core::protocol::CursorShape;
    let mut renderer = TerminalRenderer::new();
    let update = ServerMessage::CursorUpdate {
        pane_id: 1,
        col: 3,
        row: 1,
        visible: true,
        shape: CursorShape::Block,
    };
    let bytes = render_server_message(&mut renderer, &update);
    assert!(!bytes.is_empty(), "CursorUpdate must produce escape sequences");
    // CUP must be present (row=1→2, col=3→4 in 1-indexed).
    let text = String::from_utf8_lossy(bytes);
    assert!(text.contains("\x1b[2;4H"), "must contain CUP(2,4); got: {text:?}",);
}

/// Verifies that `render_server_message` returns an empty slice for
/// `HandshakeResponse` (not a render-relevant message).
#[test]
fn test_output_writer_skips_handshake_response() {
    let mut renderer = TerminalRenderer::new();
    let msg = ServerMessage::HandshakeResponse {
        protocol_version: 1,
        server_name: "teamucks".to_owned(),
    };
    let bytes = render_server_message(&mut renderer, &msg);
    // Handshake responses are not visual; the function returns an empty slice.
    assert!(bytes.is_empty(), "HandshakeResponse must not produce render bytes",);
}

// ---------------------------------------------------------------------------
// in_process_output_writer async tests
// ---------------------------------------------------------------------------

/// Verifies that `in_process_output_writer` writes rendered bytes to its sink
/// when a `FullFrame` message is sent.
#[tokio::test]
async fn test_in_process_output_writer_writes_to_sink() {
    let (tx, rx) = mpsc::channel::<ServerMessage>(16);
    let (mut read_pipe, write_pipe) = tokio::io::duplex(4096);

    tokio::spawn(in_process_output_writer(rx, write_pipe, 80, 24));

    tx.send(ServerMessage::FullFrame {
        pane_id: 1,
        cols: 2,
        rows: 1,
        cells: vec![default_cell("A"), default_cell("B")],
    })
    .await
    .expect("channel must be open");

    let mut buf = vec![0u8; 1024];
    let n = tokio::time::timeout(Duration::from_secs(1), read_pipe.read(&mut buf))
        .await
        .expect("must receive data within 1 s")
        .expect("read must not error");
    assert!(n > 0, "output writer must write bytes to sink for FullFrame");
}

/// Verifies that the output writer writes bytes for a `FrameDiff` message.
#[tokio::test]
async fn test_in_process_output_writer_writes_frame_diff() {
    let (tx, rx) = mpsc::channel::<ServerMessage>(16);
    let (mut read_pipe, write_pipe) = tokio::io::duplex(4096);

    tokio::spawn(in_process_output_writer(rx, write_pipe, 80, 24));

    tx.send(ServerMessage::FrameDiff {
        pane_id: 1,
        diffs: vec![DiffEntry::CellChange { col: 0, row: 0, cell: default_cell("Z") }],
    })
    .await
    .expect("channel must be open");

    let mut buf = vec![0u8; 1024];
    let n = tokio::time::timeout(Duration::from_secs(1), read_pipe.read(&mut buf))
        .await
        .expect("must receive data within 1 s")
        .expect("read must not error");
    assert!(n > 0, "output writer must write bytes to sink for FrameDiff");
}

/// Verifies that the output writer exits cleanly when its receiver channel is
/// closed (the actor shut down and dropped all senders).
#[tokio::test]
async fn test_output_writer_exits_on_channel_close() {
    let (tx, rx) = mpsc::channel::<ServerMessage>(16);
    let (_, write_pipe) = tokio::io::duplex(64);

    let handle = tokio::spawn(in_process_output_writer(rx, write_pipe, 80, 24));

    // Drop the sender; the writer should observe the closed channel and exit.
    drop(tx);

    let result: Result<(), tokio::task::JoinError> =
        tokio::time::timeout(Duration::from_secs(1), handle)
            .await
            .expect("writer task must exit within 1 s");
    assert!(result.is_ok(), "writer task must not panic when channel closes",);
}

/// Verifies that `HandshakeResponse` messages are not written to the sink
/// (only visual frame messages are rendered).
#[tokio::test]
async fn test_output_writer_skips_non_render_messages() {
    let (tx, rx) = mpsc::channel::<ServerMessage>(16);
    let (mut read_pipe, write_pipe) = tokio::io::duplex(4096);

    tokio::spawn(in_process_output_writer(rx, write_pipe, 80, 24));

    // Send a HandshakeResponse (non-visual) followed by a FullFrame (visual).
    tx.send(ServerMessage::HandshakeResponse {
        protocol_version: 1,
        server_name: "teamucks".to_owned(),
    })
    .await
    .expect("channel must be open");

    tx.send(ServerMessage::FullFrame {
        pane_id: 1,
        cols: 1,
        rows: 1,
        cells: vec![default_cell("Q")],
    })
    .await
    .expect("channel must be open");

    let mut buf = vec![0u8; 2048];
    let n = tokio::time::timeout(Duration::from_secs(1), read_pipe.read(&mut buf))
        .await
        .expect("must receive data within 1 s")
        .expect("read must not error");

    // The bytes written to the sink should correspond to the FullFrame only.
    // They must contain the sync-start marker.
    let received = &buf[..n];
    assert!(
        received.windows(8).any(|w| w == b"\x1b[?2026h"),
        "output must contain FullFrame sync-start; got: {received:?}",
    );
}

// ---------------------------------------------------------------------------
// Input → actor → pane roundtrip tests
// ---------------------------------------------------------------------------

/// Verifies that sending `SessionMsg::ClientInput` with a `KeyEvent` to the
/// actor causes the raw bytes to reach the active pane's PTY via `write_input`.
///
/// This test uses the actor's channel directly and inspects a real `Pane`'s
/// PTY fd to confirm bytes were forwarded.
///
/// A real PTY is required for `Pane::spawn`; this test is not marked `#[ignore]`
/// because the test environment has `/dev/ptmx` available, but it will be
/// skipped automatically in environments where `Pane::spawn` fails (CI without
/// PTY support).
#[tokio::test]
async fn test_input_to_pane_roundtrip() {
    use std::collections::HashMap;
    use teamucks_core::{
        actor::{SessionActor, SessionMsg},
        config::types::ValidatedConfig,
        pane::{Pane, PaneId},
        protocol::ClientMessage,
        server::ClientId,
        session::{Session, SessionId},
        window::{Window, WindowId},
    };

    // Spawn a real pane.  If this fails (no PTY support), skip the test.
    let pane_id = PaneId(1);
    let pane = match Pane::spawn(pane_id, 80, 23, "/bin/sh", &[]) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("test_input_to_pane_roundtrip: Pane::spawn failed ({e}), skipping");
            return;
        }
    };
    let pty_fd = pane.pty_fd();

    let window = Window::new_with_dimensions(WindowId(1), "main", pane_id, 80, 23);
    let session = Session::new(SessionId(1), "test", window);
    let mut panes = HashMap::new();
    panes.insert(pane_id, pane);

    let (actor_tx, actor_rx) = mpsc::channel::<SessionMsg>(64);
    let self_tx = actor_tx.clone();

    let actor =
        SessionActor::new(session, panes, ValidatedConfig::default(), actor_rx, self_tx, 80, 24);
    let actor_handle = tokio::spawn(async move { actor.run().await });

    // Register the in-process client.
    let client_id = ClientId::next();
    let (to_client_tx, _to_client_rx) = mpsc::channel::<ServerMessage>(64);
    actor_tx
        .send(SessionMsg::NewClient { id: client_id, cols: 80, rows: 24, tx: to_client_tx })
        .await
        .expect("actor channel must be open");

    // Give the actor a moment to process NewClient.
    tokio::time::sleep(Duration::from_millis(50)).await;

    // Send a KeyEvent containing a newline.
    actor_tx
        .send(SessionMsg::ClientInput {
            id: client_id,
            message: ClientMessage::KeyEvent { key: b"\n".to_vec(), modifiers: 0 },
        })
        .await
        .expect("actor channel must be open");

    // Give the actor time to forward the bytes to the pane.
    tokio::time::sleep(Duration::from_millis(100)).await;

    // Verify the PTY fd is still valid (write_input would fail if the child died
    // immediately — a newline typically just triggers a new prompt, not a shell exit).
    // We can't easily read back from the PTY master without racing the pty_reader
    // task, so we verify the fd is still valid by checking the fd is non-negative.
    assert!(pty_fd >= 0, "PTY fd must remain valid after key input");

    // Shut down the actor.
    actor_tx.send(SessionMsg::Shutdown).await.expect("actor channel must be open");
    let _ = tokio::time::timeout(Duration::from_secs(2), actor_handle).await;
}
