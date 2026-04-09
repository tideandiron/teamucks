/// Connected client state for the teamucks server.
///
/// A [`ClientState`] is created when a client completes the Unix socket
/// handshake and removed when the connection is closed — either cleanly or
/// abruptly.  The server owns every [`ClientState`]; clients never share
/// ownership.
use std::sync::atomic::{AtomicU64, Ordering};

use tokio::net::UnixStream;

// ---------------------------------------------------------------------------
// ClientId
// ---------------------------------------------------------------------------

/// Stable, opaque identifier for a connected client.
///
/// Identifiers are allocated monotonically from a per-process counter and
/// are never reused within a server lifetime.
///
/// # Examples
///
/// ```
/// use teamucks_core::server::ClientId;
///
/// let id = ClientId::new(1);
/// assert_eq!(id, ClientId::new(1));
/// assert_ne!(id, ClientId::new(2));
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ClientId(u64);

impl ClientId {
    /// Create a `ClientId` from a raw counter value.
    ///
    /// Prefer [`ClientId::next`] for allocating new identifiers.
    #[must_use]
    pub fn new(raw: u64) -> Self {
        Self(raw)
    }

    /// Allocate the next unique `ClientId` using a global atomic counter.
    #[must_use]
    pub fn next() -> Self {
        static COUNTER: AtomicU64 = AtomicU64::new(1);
        Self(COUNTER.fetch_add(1, Ordering::Relaxed))
    }

    /// Return the raw numeric value of this identifier.
    #[must_use]
    pub fn as_raw(self) -> u64 {
        self.0
    }
}

impl std::fmt::Display for ClientId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "client#{}", self.0)
    }
}

// ---------------------------------------------------------------------------
// ClientState
// ---------------------------------------------------------------------------

/// State for a single connected client.
///
/// Stores the live [`UnixStream`] alongside the terminal dimensions reported
/// by the client during the handshake.  Dimensions default to 80 × 24 until
/// the handshake is completed (Feature 15).
pub struct ClientState {
    /// Stable identifier for this connection.
    pub id: ClientId,
    /// The underlying socket connection.
    pub stream: UnixStream,
    /// Terminal width in columns, as reported by the client.
    pub cols: u16,
    /// Terminal height in rows, as reported by the client.
    pub rows: u16,
}

impl ClientState {
    /// Create a new [`ClientState`] wrapping the given stream.
    ///
    /// Dimensions default to `80 × 24`; they will be updated once the
    /// handshake message arrives (Feature 15).
    #[must_use]
    pub fn new(stream: UnixStream) -> Self {
        Self { id: ClientId::next(), stream, cols: 80, rows: 24 }
    }

    /// Check whether the client connection is still alive.
    ///
    /// Returns `false` when the remote end has been closed. Uses a
    /// zero-length peek so no data is consumed.
    pub fn is_alive(&self) -> bool {
        use std::io::ErrorKind;
        let mut buf = [0u8; 1];
        match self.stream.try_read(&mut buf) {
            // 0 bytes → EOF → connection closed
            Ok(0) => false,
            // Data available — still alive
            Ok(_) => true,
            // Would block means no data yet but socket is alive
            Err(ref e) if e.kind() == ErrorKind::WouldBlock => true,
            // Any real error → treat as dead
            Err(_) => false,
        }
    }
}

impl std::fmt::Debug for ClientState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ClientState")
            .field("id", &self.id)
            .field("cols", &self.cols)
            .field("rows", &self.rows)
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
    fn test_client_id_equality_and_hash() {
        let a = ClientId::new(42);
        let b = ClientId::new(42);
        let c = ClientId::new(99);
        assert_eq!(a, b);
        assert_ne!(a, c);
        assert_eq!(a.as_raw(), 42);
    }

    #[test]
    fn test_client_id_next_is_unique() {
        let ids: Vec<_> = (0..10).map(|_| ClientId::next()).collect();
        let unique: std::collections::HashSet<_> = ids.iter().map(|id| id.as_raw()).collect();
        assert_eq!(unique.len(), 10, "all allocated IDs must be distinct");
    }

    #[test]
    fn test_client_id_display() {
        let id = ClientId::new(7);
        assert!(id.to_string().contains('7'));
    }
}
