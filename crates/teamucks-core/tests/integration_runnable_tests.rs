/// Integration tests for Feature I3: Minimal Runnable Loop.
///
/// These tests verify that the session actor:
/// - Sends `HandshakeResponse` then `FullFrame` on `NewClient`.
/// - Marks a pane dirty on `PtyOutput` and delivers a frame on the next tick.
/// - Does NOT send spurious frames when nothing is dirty.
///
/// Note: tests that spawn real shells require a PTY-capable environment.
/// They are not `#[ignore]`-ed here because the PTY is available in the
/// standard Linux CI environment used by this project.
///
/// # Resize limitation
///
/// Terminal resize is not handled until Feature I14.  Resizing the host
/// terminal during any branch from I3 through I13 will cause display
/// corruption.  This is expected and will be addressed in I14.
use std::collections::HashMap;
use std::time::Duration;

use teamucks_core::{
    actor::{SessionActor, SessionMsg},
    config::types::ValidatedConfig,
    pane::{Pane, PaneId},
    protocol::ServerMessage,
    server::ClientId,
    session::{Session, SessionId},
    window::{Window, WindowId},
};
use tokio::sync::mpsc;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Build a [`SessionActor`] backed by a real shell pane (`/bin/sh`).
///
/// Returns `None` if the PTY cannot be opened (e.g. in a restricted sandbox).
/// Callers should skip the test gracefully in that case.
fn make_actor_with_shell(cols: u16, rows: u16) -> Option<(SessionActor, mpsc::Sender<SessionMsg>)> {
    let pane_id = PaneId(1);
    let pane = match Pane::spawn(pane_id, cols, rows, "/bin/sh", &[]) {
        Ok(p) => p,
        Err(_) => return None,
    };
    let pty_fd = pane.pty_fd();

    let window = Window::new_with_dimensions(WindowId(1), "main", pane_id, cols, rows);
    let session = Session::new(SessionId(1), "test", window);

    let mut panes = HashMap::new();
    panes.insert(pane_id, pane);

    let config = ValidatedConfig::default();
    let (actor_tx, actor_rx) = mpsc::channel::<SessionMsg>(64);

    // Spawn the PTY reader task so actual shell output flows into the actor.
    let reader_tx = actor_tx.clone();
    tokio::spawn(teamucks_core::actor::pty_reader::pty_reader(pane_id, pty_fd, reader_tx));

    let actor = SessionActor::new(session, panes, config, actor_rx, actor_tx.clone(), cols, rows);
    Some((actor, actor_tx))
}

/// Build a [`SessionActor`] with NO panes (no real shell, for no-op tests).
fn make_actor_no_panes(cols: u16, rows: u16) -> (SessionActor, mpsc::Sender<SessionMsg>) {
    let pane_id = PaneId(1);
    let window = Window::new_with_dimensions(WindowId(1), "main", pane_id, cols, rows);
    let session = Session::new(SessionId(1), "test", window);

    let config = ValidatedConfig::default();
    let (actor_tx, actor_rx) = mpsc::channel::<SessionMsg>(64);
    let actor =
        SessionActor::new(session, HashMap::new(), config, actor_rx, actor_tx.clone(), cols, rows);
    (actor, actor_tx)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

/// Verifies that after `NewClient`, the actor sends `HandshakeResponse` then
/// `FullFrame`.
///
/// The `FullFrame` must carry `pane_id == 1`, `cols == 80`, and `rows > 0`.
#[tokio::test]
async fn test_actor_new_client_receives_full_frame_after_handshake() {
    let Some((actor, actor_tx)) = make_actor_with_shell(80, 23) else {
        eprintln!("skip: PTY unavailable");
        return;
    };
    tokio::spawn(async move { actor.run().await });

    let (client_tx, mut client_rx) = mpsc::channel::<ServerMessage>(16);
    actor_tx
        .send(SessionMsg::NewClient { id: ClientId::next(), cols: 80, rows: 24, tx: client_tx })
        .await
        .unwrap();

    let mut saw_handshake = false;
    let mut saw_full_frame = false;
    let deadline = tokio::time::Instant::now() + Duration::from_secs(3);

    while !saw_full_frame {
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        if remaining.is_zero() {
            break;
        }
        match tokio::time::timeout(remaining, client_rx.recv()).await {
            Ok(Some(ServerMessage::HandshakeResponse { .. })) => {
                saw_handshake = true;
            }
            Ok(Some(ServerMessage::FullFrame { pane_id, cols, rows, .. })) => {
                assert_eq!(pane_id, 1, "FullFrame must be for pane 1");
                assert_eq!(cols, 80, "FullFrame cols must match spawn dimensions");
                assert!(rows > 0, "FullFrame rows must be positive");
                saw_full_frame = true;
            }
            Ok(Some(_)) => {}
            _ => break,
        }
    }

    assert!(saw_handshake, "must receive HandshakeResponse before FullFrame");
    assert!(saw_full_frame, "must receive FullFrame on NewClient");
}

/// Verifies that `PtyOutput` marks a pane dirty and the next render tick
/// delivers a `FrameDiff` or `FullFrame` to connected clients.
#[tokio::test]
async fn test_actor_pty_output_marks_pane_dirty() {
    let Some((actor, actor_tx)) = make_actor_with_shell(80, 23) else {
        eprintln!("skip: PTY unavailable");
        return;
    };
    tokio::spawn(async move { actor.run().await });

    let (client_tx, mut client_rx) = mpsc::channel::<ServerMessage>(32);
    actor_tx
        .send(SessionMsg::NewClient { id: ClientId::next(), cols: 80, rows: 24, tx: client_tx })
        .await
        .unwrap();

    // Drain initial HandshakeResponse and FullFrame.
    tokio::time::sleep(Duration::from_millis(150)).await;
    while client_rx.try_recv().is_ok() {}

    // Inject PTY output directly (bypassing the real shell).
    actor_tx
        .send(SessionMsg::PtyOutput { pane_id: PaneId(1), data: b"hello_from_pty\r\n".to_vec() })
        .await
        .unwrap();

    // The actor's internal render interval fires every ~16 ms.  Wait up to 2 s
    // for either a FrameDiff or a FullFrame to arrive.
    let deadline = tokio::time::Instant::now() + Duration::from_secs(2);
    let mut saw_diff = false;

    while !saw_diff {
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        if remaining.is_zero() {
            break;
        }
        match tokio::time::timeout(remaining, client_rx.recv()).await {
            Ok(Some(ServerMessage::FrameDiff { .. } | ServerMessage::FullFrame { .. })) => {
                saw_diff = true;
            }
            Ok(Some(_)) => {}
            _ => break,
        }
    }

    assert!(
        saw_diff,
        "must receive FrameDiff or FullFrame after PtyOutput (driven by internal timer)"
    );
}

/// Verifies that the render tick is a no-op when no panes are dirty and no
/// clients have received output since the last snapshot.
#[tokio::test]
async fn test_actor_render_tick_no_op_when_nothing_dirty() {
    // Use no-pane actor so no real shell is needed.
    let (actor, actor_tx) = make_actor_no_panes(80, 24);
    tokio::spawn(async move { actor.run().await });

    let (client_tx, mut client_rx) = mpsc::channel::<ServerMessage>(16);
    actor_tx
        .send(SessionMsg::NewClient { id: ClientId::next(), cols: 80, rows: 24, tx: client_tx })
        .await
        .unwrap();

    // Drain HandshakeResponse and any initial FullFrame (none expected without
    // panes, but drain to be safe).
    tokio::time::sleep(Duration::from_millis(100)).await;
    while client_rx.try_recv().is_ok() {}

    // Wait for two render intervals (~32 ms) — nothing dirty, so nothing should
    // arrive.
    tokio::time::sleep(Duration::from_millis(40)).await;

    assert!(
        client_rx.try_recv().is_err(),
        "must not receive a frame message when nothing is dirty"
    );
}

/// Verifies that a second `NewClient` also receives a `HandshakeResponse` and
/// a `FullFrame`, independent of the first client.
#[tokio::test]
async fn test_actor_second_client_also_receives_full_frame() {
    let Some((actor, actor_tx)) = make_actor_with_shell(80, 23) else {
        eprintln!("skip: PTY unavailable");
        return;
    };
    tokio::spawn(async move { actor.run().await });

    // First client.
    let (tx1, mut rx1) = mpsc::channel::<ServerMessage>(16);
    actor_tx
        .send(SessionMsg::NewClient { id: ClientId::next(), cols: 80, rows: 24, tx: tx1 })
        .await
        .unwrap();

    // Drain first client messages.
    tokio::time::sleep(Duration::from_millis(150)).await;
    while rx1.try_recv().is_ok() {}

    // Second client.
    let (tx2, mut rx2) = mpsc::channel::<ServerMessage>(16);
    actor_tx
        .send(SessionMsg::NewClient { id: ClientId::next(), cols: 80, rows: 24, tx: tx2 })
        .await
        .unwrap();

    let mut saw_handshake = false;
    let mut saw_frame = false;
    let deadline = tokio::time::Instant::now() + Duration::from_secs(3);

    while !saw_frame {
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        if remaining.is_zero() {
            break;
        }
        match tokio::time::timeout(remaining, rx2.recv()).await {
            Ok(Some(ServerMessage::HandshakeResponse { .. })) => saw_handshake = true,
            Ok(Some(ServerMessage::FullFrame { .. })) => saw_frame = true,
            Ok(Some(_)) => {}
            _ => break,
        }
    }

    assert!(saw_handshake, "second client must receive HandshakeResponse");
    assert!(saw_frame, "second client must receive FullFrame");
}
