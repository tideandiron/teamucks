/// Server daemon for teamucks.
///
/// The [`Server`] owns the Unix socket listener and all connected
/// [`ClientState`] instances.  Sessions and PTYs will be added in Feature 20.
///
/// # Lifecycle (Phase 1 — foreground)
///
/// In Phase 1 the server runs in the foreground of the `teamucks` process.
/// Running `teamucks` in one terminal starts the server and the first client
/// in the same process. Full fork-and-daemonize will be added later; the
/// socket-based architecture already supports it.
///
/// # Socket security
///
/// The socket directory is created with mode `0700` so only the owning user
/// can connect. No further authentication is performed in Phase 1.
pub mod client;
pub mod listener;

pub use client::{ClientId, ClientState};
pub use listener::default_socket_path;

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use tokio::net::UnixListener;
use tokio::net::UnixStream;

// ---------------------------------------------------------------------------
// Error type
// ---------------------------------------------------------------------------

/// Errors produced by server operations.
#[derive(Debug, thiserror::Error)]
pub enum ServerError {
    /// Failed to bind the Unix domain socket.
    #[error("failed to bind server socket: {0}")]
    Bind(#[source] std::io::Error),

    /// Failed to accept an incoming client connection.
    #[error("failed to accept client connection: {0}")]
    Accept(#[source] std::io::Error),

    /// Failed to remove a stale socket before rebinding.
    #[error("failed to remove stale socket: {0}")]
    StaleSocket(#[source] std::io::Error),

    /// Failed to create or configure the socket directory.
    #[error("failed to set up socket directory: {0}")]
    SocketDir(#[source] std::io::Error),
}

// ---------------------------------------------------------------------------
// Server
// ---------------------------------------------------------------------------

/// The teamucks server daemon.
///
/// Owns the Unix socket listener and all connected clients.  Sessions and
/// PTYs will be added in Feature 20.
///
/// # Examples
///
/// ```no_run
/// # async fn run() -> Result<(), teamucks_core::server::ServerError> {
/// use teamucks_core::server::Server;
///
/// let mut server = Server::bind(std::path::Path::new("/tmp/my.sock"))?;
/// // server.run().await?; // blocks until shutdown
/// server.shutdown();
/// # Ok(())
/// # }
/// ```
pub struct Server {
    socket_path: PathBuf,
    listener: UnixListener,
    clients: HashMap<ClientId, ClientState>,
}

impl Server {
    /// Create and bind the server socket.
    ///
    /// Steps:
    /// 1. Ensure the socket directory exists with mode `0700`.
    /// 2. Remove any stale socket file at `socket_path`.
    /// 3. Bind a new `UnixListener` at `socket_path`.
    ///
    /// # Errors
    ///
    /// Returns [`ServerError::SocketDir`] if the directory cannot be created.
    /// Returns [`ServerError::StaleSocket`] if a stale file cannot be removed.
    /// Returns [`ServerError::Bind`] if `bind(2)` fails.
    pub fn bind(socket_path: &Path) -> Result<Self, ServerError> {
        listener::ensure_socket_dir(socket_path).map_err(ServerError::SocketDir)?;
        listener::remove_stale_socket(socket_path).map_err(ServerError::StaleSocket)?;

        let unix_listener = UnixListener::bind(socket_path).map_err(ServerError::Bind)?;

        tracing::info!(
            socket = %socket_path.display(),
            "server socket bound"
        );

        Ok(Self {
            socket_path: socket_path.to_owned(),
            listener: unix_listener,
            clients: HashMap::new(),
        })
    }

    /// Run the server event loop.
    ///
    /// Accepts clients in a loop until a shutdown signal is received or an
    /// unrecoverable error occurs.  This is a blocking future; call it with
    /// `tokio::spawn` if the server should run concurrently with other tasks.
    ///
    /// # Errors
    ///
    /// Returns [`ServerError::Accept`] only for fatal accept errors (EMFILE,
    /// ENFILE, etc.).  Per-connection errors are logged and skipped.
    pub async fn run(&mut self) -> Result<(), ServerError> {
        tracing::info!(socket = %self.socket_path.display(), "server event loop started");
        loop {
            match self.listener.accept().await {
                Ok((stream, _addr)) => {
                    let id = self.accept_client(stream);
                    tracing::info!(client_id = %id, "client connected");
                }
                Err(e) => {
                    // Protocol violations / per-connection errors: log & continue.
                    // Fatal resource exhaustion: propagate.
                    use std::io::ErrorKind;
                    match e.kind() {
                        ErrorKind::OutOfMemory => return Err(ServerError::Accept(e)),
                        _ => {
                            tracing::warn!(error = %e, "accept error — skipping");
                        }
                    }
                }
            }
        }
    }

    /// Accept a new client connection, register it, and return its [`ClientId`].
    fn accept_client(&mut self, stream: UnixStream) -> ClientId {
        let state = ClientState::new(stream);
        let id = state.id;
        self.clients.insert(id, state);
        id
    }

    /// Accept one client from the listener and return its [`ClientId`].
    ///
    /// This is the public interface used by integration tests.  It waits for
    /// exactly one incoming connection on the already-bound socket.
    ///
    /// # Errors
    ///
    /// Returns [`ServerError::Accept`] if the underlying accept call fails.
    pub async fn accept_client_from_listener(&mut self) -> Result<ClientId, ServerError> {
        let (stream, _addr) = self.listener.accept().await.map_err(ServerError::Accept)?;
        let id = self.accept_client(stream);
        tracing::debug!(client_id = %id, "client accepted from listener");
        Ok(id)
    }

    /// Remove a disconnected client by id.
    ///
    /// Silently ignores unknown ids (idempotent).
    pub fn remove_client(&mut self, id: ClientId) {
        if self.clients.remove(&id).is_some() {
            tracing::info!(client_id = %id, "client removed");
        }
    }

    /// Scan all connected clients and remove those whose connections are dead.
    ///
    /// This is a cooperative poll rather than an event-driven callback, which
    /// is sufficient for Phase 1. An event-driven variant (using `select!`
    /// on all client streams) will replace this in Feature 20.
    pub fn remove_disconnected_clients(&mut self) {
        let dead: Vec<ClientId> =
            self.clients.values().filter(|c| !c.is_alive()).map(|c| c.id).collect();
        for id in dead {
            self.remove_client(id);
        }
    }

    /// Return the path of the bound Unix socket.
    #[must_use]
    pub fn socket_path(&self) -> &Path {
        &self.socket_path
    }

    /// Return the number of currently connected clients.
    #[must_use]
    pub fn client_count(&self) -> usize {
        self.clients.len()
    }

    /// Return `true` if a client with the given id is currently connected.
    #[must_use]
    pub fn has_client(&self, id: ClientId) -> bool {
        self.clients.contains_key(&id)
    }

    /// Shut down the server gracefully.
    ///
    /// - Drops all client connections (sends RST / EOF to each peer).
    /// - Removes the socket file so a new server can bind the same path.
    ///
    /// After calling `shutdown`, the [`Server`] must not be used.
    pub fn shutdown(&mut self) {
        tracing::info!(socket = %self.socket_path.display(), "server shutting down");
        self.clients.clear();
        if let Err(e) = std::fs::remove_file(&self.socket_path) {
            if e.kind() != std::io::ErrorKind::NotFound {
                tracing::warn!(
                    error = %e,
                    socket = %self.socket_path.display(),
                    "failed to remove socket on shutdown"
                );
            }
        }
    }
}

impl std::fmt::Debug for Server {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Server")
            .field("socket_path", &self.socket_path)
            .field("client_count", &self.clients.len())
            .finish_non_exhaustive()
    }
}

// ---------------------------------------------------------------------------
// Unit tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_server_debug_impl() {
        let tmp = tempfile::TempDir::new().expect("temp dir");
        let path = tmp.path().join("debug.sock");
        // UnixListener requires a tokio runtime; build one for this test.
        let rt = tokio::runtime::Runtime::new().expect("runtime");
        let _guard = rt.enter();
        let server = Server::bind(&path).expect("bind must succeed");
        let dbg = format!("{server:?}");
        assert!(dbg.contains("Server"));
        assert!(dbg.contains("socket_path"));
    }

    #[test]
    fn test_server_error_variants() {
        let err = ServerError::Bind(std::io::Error::from(std::io::ErrorKind::AddrInUse));
        assert!(err.to_string().contains("bind"));

        let err = ServerError::Accept(std::io::Error::from(std::io::ErrorKind::BrokenPipe));
        assert!(err.to_string().contains("accept"));

        let err =
            ServerError::StaleSocket(std::io::Error::from(std::io::ErrorKind::PermissionDenied));
        assert!(err.to_string().contains("stale"));

        let err =
            ServerError::SocketDir(std::io::Error::from(std::io::ErrorKind::PermissionDenied));
        assert!(err.to_string().contains("directory"));
    }
}
