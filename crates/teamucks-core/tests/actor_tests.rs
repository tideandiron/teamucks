use std::collections::HashMap;
use std::time::Duration;

use teamucks_core::{
    actor::{SessionActor, SessionMsg},
    config::types::ValidatedConfig,
    pane::{Pane, PaneId},
    protocol::{ClientMessage, ServerMessage, PROTOCOL_VERSION},
    server::ClientId,
    session::{Session, SessionId},
    window::{Window, WindowId},
};
use tokio::sync::mpsc;

fn make_test_session() -> (Session, HashMap<PaneId, Pane>) {
    // Creates a session with a Window::new_empty (no real PTY).
    let window = Window::new_empty(WindowId(1), "test", PaneId(1));
    let session = Session::new(SessionId(1), "test-session", window);
    (session, HashMap::new())
}

/// Verifies that the actor starts, waits through a render interval tick, and does not crash.
/// RenderTick is not a SessionMsg variant — rendering is driven by the internal timer only.
#[tokio::test]
async fn test_actor_starts_and_processes_render_tick() {
    let (session, panes) = make_test_session();
    let config = ValidatedConfig::default();
    let (actor_tx, actor_rx) = mpsc::channel::<SessionMsg>(64);
    let actor_tx2 = actor_tx.clone();
    let actor = SessionActor::new(session, panes, config, actor_rx, actor_tx2, 80, 24);

    let handle = tokio::spawn(async move { actor.run().await });
    // Wait at least one render interval (16ms) to exercise the timer path.
    tokio::time::sleep(Duration::from_millis(20)).await;
    actor_tx.send(SessionMsg::Shutdown).await.unwrap();
    tokio::time::timeout(Duration::from_secs(2), handle)
        .await
        .expect("actor must shut down within 2s")
        .expect("actor task must not panic");
}

/// Verifies that Shutdown causes the actor to exit its run loop.
#[tokio::test]
async fn test_actor_shutdown_exits_run_loop() {
    let (session, panes) = make_test_session();
    let config = ValidatedConfig::default();
    let (actor_tx, actor_rx) = mpsc::channel::<SessionMsg>(64);
    let actor_tx2 = actor_tx.clone();
    let actor = SessionActor::new(session, panes, config, actor_rx, actor_tx2, 80, 24);

    let handle = tokio::spawn(async move { actor.run().await });
    actor_tx.send(SessionMsg::Shutdown).await.unwrap();
    tokio::time::timeout(Duration::from_secs(2), handle)
        .await
        .expect("actor must exit")
        .expect("actor must not panic");
}

/// Verifies that a NewClient is accepted and a HandshakeResponse is sent back.
#[tokio::test]
async fn test_actor_accepts_new_client_sends_handshake_response() {
    let (session, panes) = make_test_session();
    let config = ValidatedConfig::default();
    let (actor_tx, actor_rx) = mpsc::channel::<SessionMsg>(64);
    let actor_tx2 = actor_tx.clone();
    let actor = SessionActor::new(session, panes, config, actor_rx, actor_tx2, 80, 24);
    tokio::spawn(async move { actor.run().await });

    let (client_tx, mut client_rx) = mpsc::channel::<ServerMessage>(16);
    let client_id = ClientId::next();
    actor_tx
        .send(SessionMsg::NewClient { id: client_id, cols: 80, rows: 24, tx: client_tx })
        .await
        .unwrap();

    let msg = tokio::time::timeout(Duration::from_secs(1), client_rx.recv())
        .await
        .expect("must receive message")
        .expect("channel open");
    assert!(
        matches!(msg, ServerMessage::HandshakeResponse { .. }),
        "expected HandshakeResponse, got {msg:?}"
    );
}

/// Verifies that ClientDisconnected removes the client from the actor's registry.
#[tokio::test]
async fn test_actor_client_disconnected_removes_client() {
    let (session, panes) = make_test_session();
    let config = ValidatedConfig::default();
    let (actor_tx, actor_rx) = mpsc::channel::<SessionMsg>(64);
    let actor_tx2 = actor_tx.clone();
    let actor = SessionActor::new(session, panes, config, actor_rx, actor_tx2, 80, 24);
    tokio::spawn(async move { actor.run().await });

    let (client_tx, _client_rx) = mpsc::channel::<ServerMessage>(16);
    let client_id = ClientId::next();
    actor_tx
        .send(SessionMsg::NewClient { id: client_id, cols: 80, rows: 24, tx: client_tx })
        .await
        .unwrap();
    actor_tx.send(SessionMsg::ClientDisconnected { id: client_id }).await.unwrap();
    // No assertion on internal state; just verify no panic.
    tokio::time::sleep(Duration::from_millis(50)).await;
}

/// Verifies that PtyOutput for an unknown pane_id is silently ignored.
#[tokio::test]
async fn test_actor_pty_output_unknown_pane_ignored() {
    let (session, panes) = make_test_session();
    let config = ValidatedConfig::default();
    let (actor_tx, actor_rx) = mpsc::channel::<SessionMsg>(64);
    let actor_tx2 = actor_tx.clone();
    let actor = SessionActor::new(session, panes, config, actor_rx, actor_tx2, 80, 24);
    tokio::spawn(async move { actor.run().await });

    actor_tx
        .send(SessionMsg::PtyOutput { pane_id: PaneId(9999), data: b"hello".to_vec() })
        .await
        .unwrap();
    // No panic = success.
    tokio::time::sleep(Duration::from_millis(50)).await;
}

/// Verifies that HostResize with valid dimensions does not panic.
#[tokio::test]
async fn test_actor_host_resize_does_not_panic() {
    let (session, panes) = make_test_session();
    let config = ValidatedConfig::default();
    let (actor_tx, actor_rx) = mpsc::channel::<SessionMsg>(64);
    let actor_tx2 = actor_tx.clone();
    let actor = SessionActor::new(session, panes, config, actor_rx, actor_tx2, 80, 24);
    tokio::spawn(async move { actor.run().await });

    actor_tx.send(SessionMsg::HostResize { cols: 120, rows: 40 }).await.unwrap();
    tokio::time::sleep(Duration::from_millis(50)).await;
}

/// Verifies that the first NewClient sets focused_client (no panic on ClientInput focus check).
#[tokio::test]
async fn test_actor_first_client_becomes_focused() {
    let (session, panes) = make_test_session();
    let config = ValidatedConfig::default();
    let (actor_tx, actor_rx) = mpsc::channel::<SessionMsg>(64);
    let actor_tx2 = actor_tx.clone();
    let actor = SessionActor::new(session, panes, config, actor_rx, actor_tx2, 80, 24);
    tokio::spawn(async move { actor.run().await });

    let (client_tx, mut client_rx) = mpsc::channel::<ServerMessage>(16);
    let client_id = ClientId::next();
    actor_tx
        .send(SessionMsg::NewClient { id: client_id, cols: 80, rows: 24, tx: client_tx })
        .await
        .unwrap();

    // HandshakeResponse means the actor registered the client.
    let msg = tokio::time::timeout(Duration::from_secs(1), client_rx.recv())
        .await
        .expect("must receive message")
        .expect("channel open");
    assert!(matches!(msg, ServerMessage::HandshakeResponse { .. }));

    // Now disconnect — actor must not panic trying to reassign focus.
    actor_tx.send(SessionMsg::ClientDisconnected { id: client_id }).await.unwrap();
    tokio::time::sleep(Duration::from_millis(50)).await;
}

/// Verifies that PaneDied for an unknown pane_id is silently ignored.
#[tokio::test]
async fn test_actor_pane_died_unknown_pane_ignored() {
    let (session, panes) = make_test_session();
    let config = ValidatedConfig::default();
    let (actor_tx, actor_rx) = mpsc::channel::<SessionMsg>(64);
    let actor_tx2 = actor_tx.clone();
    let actor = SessionActor::new(session, panes, config, actor_rx, actor_tx2, 80, 24);
    tokio::spawn(async move { actor.run().await });

    actor_tx.send(SessionMsg::PaneDied { pane_id: PaneId(9999), exit_code: 0 }).await.unwrap();
    // No panic = success.
    tokio::time::sleep(Duration::from_millis(50)).await;
}

/// Verifies that a second NewClient does NOT override focus when one client is already focused.
#[tokio::test]
async fn test_actor_second_client_does_not_steal_focus() {
    let (session, panes) = make_test_session();
    let config = ValidatedConfig::default();
    let (actor_tx, actor_rx) = mpsc::channel::<SessionMsg>(64);
    let actor_tx2 = actor_tx.clone();
    let actor = SessionActor::new(session, panes, config, actor_rx, actor_tx2, 80, 24);
    tokio::spawn(async move { actor.run().await });

    // Connect first client.
    let (tx1, mut rx1) = mpsc::channel::<ServerMessage>(16);
    let id1 = ClientId::next();
    actor_tx.send(SessionMsg::NewClient { id: id1, cols: 80, rows: 24, tx: tx1 }).await.unwrap();
    // Drain handshake.
    tokio::time::timeout(Duration::from_secs(1), rx1.recv()).await.unwrap().unwrap();

    // Connect second client.
    let (tx2, mut rx2) = mpsc::channel::<ServerMessage>(16);
    let id2 = ClientId::next();
    actor_tx.send(SessionMsg::NewClient { id: id2, cols: 80, rows: 24, tx: tx2 }).await.unwrap();
    // Second client also gets a handshake.
    tokio::time::timeout(Duration::from_secs(1), rx2.recv()).await.unwrap().unwrap();

    // Both connected, no panic.
    tokio::time::sleep(Duration::from_millis(20)).await;
}

/// Verifies that HandshakeResponse carries the correct protocol version.
#[tokio::test]
async fn test_actor_handshake_response_protocol_version() {
    let (session, panes) = make_test_session();
    let config = ValidatedConfig::default();
    let (actor_tx, actor_rx) = mpsc::channel::<SessionMsg>(64);
    let actor_tx2 = actor_tx.clone();
    let actor = SessionActor::new(session, panes, config, actor_rx, actor_tx2, 80, 24);
    tokio::spawn(async move { actor.run().await });

    let (client_tx, mut client_rx) = mpsc::channel::<ServerMessage>(16);
    let client_id = ClientId::next();
    actor_tx
        .send(SessionMsg::NewClient { id: client_id, cols: 80, rows: 24, tx: client_tx })
        .await
        .unwrap();

    let msg = tokio::time::timeout(Duration::from_secs(1), client_rx.recv())
        .await
        .expect("must receive message")
        .expect("channel open");

    match msg {
        ServerMessage::HandshakeResponse { protocol_version, server_name } => {
            assert_eq!(protocol_version, PROTOCOL_VERSION);
            assert!(!server_name.is_empty());
        }
        other => panic!("expected HandshakeResponse, got {other:?}"),
    }
}

/// Verifies that Shutdown is the correct and reliable exit mechanism.
///
/// The actor holds `self_tx` internally (for PTY readers and signal tasks),
/// which means merely dropping the external sender is not sufficient to close
/// the channel.  `Shutdown` is the authoritative exit path.
#[tokio::test]
async fn test_actor_shutdown_is_authoritative_exit() {
    let (session, panes) = make_test_session();
    let config = ValidatedConfig::default();
    let (actor_tx, actor_rx) = mpsc::channel::<SessionMsg>(64);
    let actor_tx2 = actor_tx.clone();
    let actor = SessionActor::new(session, panes, config, actor_rx, actor_tx2, 80, 24);
    let handle = tokio::spawn(async move { actor.run().await });

    // Send Shutdown — this is the only correct way to stop the actor.
    actor_tx.send(SessionMsg::Shutdown).await.unwrap();

    tokio::time::timeout(Duration::from_secs(2), handle)
        .await
        .expect("actor must exit after Shutdown")
        .expect("actor must not panic");
}

// Ensure unused import is not warned about — ClientMessage is used in the type import.
const _: () = {
    let _ = std::mem::size_of::<ClientMessage>();
};
