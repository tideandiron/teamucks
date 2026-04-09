/// Integration tests for the server daemon (Feature 14).
///
/// All socket paths use temporary directories to avoid conflicts between
/// parallel test runs. Every test that drives async I/O (connect, accept)
/// uses `#[tokio::test]`; pure synchronous tests use `#[test]`.
use std::path::PathBuf;
use std::time::Duration;

use teamucks_core::server::{default_socket_path, ClientId, Server, ServerError};
use tokio::net::UnixStream;
use tokio::time::timeout;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Return a unique socket path inside a fresh `TempDir`.
///
/// The caller must hold the returned `TempDir` for the duration of the test;
/// dropping it removes the directory.
fn temp_socket_path(name: &str) -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::TempDir::new().expect("temp dir must be creatable");
    let path = dir.path().join(format!("{name}.sock"));
    (dir, path)
}

// ---------------------------------------------------------------------------
// test_server_socket_path_default
// ---------------------------------------------------------------------------

#[test]
fn test_server_socket_path_default_xdg() {
    // When XDG_RUNTIME_DIR is set, the path is under it.
    std::env::set_var("XDG_RUNTIME_DIR", "/run/user/1000");
    let path = default_socket_path("default");
    // Restore to avoid polluting other tests.
    std::env::remove_var("XDG_RUNTIME_DIR");
    assert_eq!(path, PathBuf::from("/run/user/1000/teamucks/default.sock"));
}

#[test]
fn test_server_socket_path_default_tmpdir_fallback() {
    // When XDG_RUNTIME_DIR is absent, fall back to TMPDIR/teamucks-<uid>/<name>.sock.
    std::env::remove_var("XDG_RUNTIME_DIR");
    std::env::set_var("TMPDIR", "/tmp");
    let path = default_socket_path("myserver");
    // SAFETY: getuid() is always safe; returns the real UID of the process.
    let uid = unsafe { libc::getuid() };
    let expected = PathBuf::from(format!("/tmp/teamucks-{uid}/myserver.sock"));
    assert_eq!(path, expected);
}

// ---------------------------------------------------------------------------
// test_server_bind_creates_socket
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_server_bind_creates_socket() {
    let (_dir, path) = temp_socket_path("bind-creates");
    let server = Server::bind(&path).expect("bind must succeed");
    assert!(path.exists(), "socket file must exist after bind");
    drop(server);
}

// ---------------------------------------------------------------------------
// test_server_shutdown_removes_socket
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_server_shutdown_removes_socket() {
    let (_dir, path) = temp_socket_path("shutdown-removes");
    let mut server = Server::bind(&path).expect("bind must succeed");
    assert!(path.exists());
    server.shutdown();
    assert!(!path.exists(), "socket file must be removed after shutdown");
}

// ---------------------------------------------------------------------------
// test_server_socket_permissions
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_server_socket_permissions() {
    use std::os::unix::fs::PermissionsExt;

    let (_dir, path) = temp_socket_path("permissions");
    let server = Server::bind(&path).expect("bind must succeed");

    let parent = path.parent().expect("socket must have a parent directory");
    let meta = std::fs::metadata(parent).expect("parent dir must be stat-able");
    let mode = meta.permissions().mode() & 0o777;
    assert_eq!(mode, 0o700, "socket directory must have mode 0700, got {mode:o}");

    drop(server);
}

// ---------------------------------------------------------------------------
// test_server_bind_existing_socket — stale socket cleaned up on rebind
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_server_bind_existing_socket() {
    let (_dir, path) = temp_socket_path("stale-socket");

    // First bind — creates socket.
    let server1 = Server::bind(&path).expect("first bind must succeed");
    drop(server1);

    // Socket file is still present (server dropped without calling shutdown).
    assert!(path.exists(), "stale socket must be present");

    // Second bind — should clean up the stale socket and succeed.
    let server2 = Server::bind(&path).expect("second bind must succeed after stale cleanup");
    assert!(path.exists(), "socket must exist after second bind");
    drop(server2);
}

// ---------------------------------------------------------------------------
// test_server_accept_client
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_server_accept_client() {
    let (_dir, path) = temp_socket_path("accept-client");
    let mut server = Server::bind(&path).expect("bind must succeed");

    // Connect from a client in a background task.
    let path_clone = path.clone();
    let connect_task = tokio::spawn(async move {
        UnixStream::connect(&path_clone).await.expect("client connect must succeed")
    });

    // Accept one client with a short deadline.
    let client_id = timeout(Duration::from_secs(2), server.accept_client_from_listener())
        .await
        .expect("accept must complete within 2 s")
        .expect("accept must succeed");

    assert!(server.client_count() >= 1);
    assert!(server.has_client(client_id));

    connect_task.await.expect("connect task must not panic");
    server.shutdown();
}

// ---------------------------------------------------------------------------
// test_server_client_disconnect
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_server_client_disconnect() {
    let (_dir, path) = temp_socket_path("client-disconnect");
    let mut server = Server::bind(&path).expect("bind must succeed");

    let path_clone = path.clone();
    let client_task = tokio::spawn(async move {
        let stream = UnixStream::connect(&path_clone).await.expect("connect must succeed");
        // Keep alive briefly, then drop (disconnect).
        tokio::time::sleep(Duration::from_millis(50)).await;
        drop(stream);
    });

    let client_id = timeout(Duration::from_secs(2), server.accept_client_from_listener())
        .await
        .expect("accept must complete within 2 s")
        .expect("accept must succeed");

    assert!(server.has_client(client_id));

    // Wait for the client task to finish (client has disconnected).
    client_task.await.expect("client task must not panic");

    // Allow a brief moment for the disconnect to propagate.
    tokio::time::sleep(Duration::from_millis(100)).await;

    // Poll the server to detect the disconnect.
    server.remove_disconnected_clients();
    assert!(!server.has_client(client_id), "disconnected client must be removed");

    server.shutdown();
}

// ---------------------------------------------------------------------------
// test_server_multiple_clients
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_server_multiple_clients() {
    let (_dir, path) = temp_socket_path("multiple-clients");
    let mut server = Server::bind(&path).expect("bind must succeed");

    let path1 = path.clone();
    let path2 = path.clone();

    let t1 =
        tokio::spawn(async move { UnixStream::connect(&path1).await.expect("client 1 connect") });
    let t2 =
        tokio::spawn(async move { UnixStream::connect(&path2).await.expect("client 2 connect") });

    let id1 = timeout(Duration::from_secs(2), server.accept_client_from_listener())
        .await
        .expect("accept 1 within deadline")
        .expect("accept 1 must succeed");

    let id2 = timeout(Duration::from_secs(2), server.accept_client_from_listener())
        .await
        .expect("accept 2 within deadline")
        .expect("accept 2 must succeed");

    assert_ne!(id1, id2, "each client must have a distinct ClientId");
    assert_eq!(server.client_count(), 2);

    t1.await.expect("client 1 task ok");
    t2.await.expect("client 2 task ok");

    server.shutdown();
}

// ---------------------------------------------------------------------------
// test_server_socket_path_returns_bound_path
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_server_socket_path_returns_bound_path() {
    let (_dir, path) = temp_socket_path("path-accessor");
    let server = Server::bind(&path).expect("bind must succeed");
    assert_eq!(server.socket_path(), path.as_path());
    drop(server);
}

// ---------------------------------------------------------------------------
// test_client_id_uniqueness
// ---------------------------------------------------------------------------

#[test]
fn test_client_id_uniqueness() {
    // ClientId values produced by sequential allocation must be distinct.
    let a = ClientId::new(1);
    let b = ClientId::new(2);
    assert_ne!(a, b);
    assert_eq!(a, ClientId::new(1));
}

// ---------------------------------------------------------------------------
// test_server_error_display
// ---------------------------------------------------------------------------

#[test]
fn test_server_error_display() {
    let err = ServerError::Bind(std::io::Error::from(std::io::ErrorKind::AddrInUse));
    assert!(err.to_string().contains("bind"));

    let err = ServerError::Accept(std::io::Error::from(std::io::ErrorKind::BrokenPipe));
    assert!(err.to_string().contains("accept"));
}
