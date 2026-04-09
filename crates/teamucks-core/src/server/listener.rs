/// Unix socket listener for the teamucks server.
///
/// [`ServerListener`] wraps a [`tokio::net::UnixListener`] and provides the
/// socket-path resolution utilities required by the server daemon.
///
/// # Socket directory
///
/// The directory that contains the socket file is created with mode `0700`
/// so that only the owning user can connect.  This is the only security
/// boundary before the handshake; any process running as the same user is
/// considered trusted.
use std::path::{Path, PathBuf};

// ---------------------------------------------------------------------------
// Socket path resolution
// ---------------------------------------------------------------------------

/// Return the default socket path for a named server.
///
/// Resolution order:
/// 1. `$XDG_RUNTIME_DIR/teamucks/<name>.sock`
/// 2. `$TMPDIR/teamucks-<uid>/<name>.sock`
/// 3. `/tmp/teamucks-<uid>/<name>.sock`
///
/// The returned path may not exist yet; call [`ServerListener::bind`] to
/// create it.
///
/// # Examples
///
/// ```no_run
/// use teamucks_core::server::default_socket_path;
///
/// let path = default_socket_path("default");
/// println!("socket: {}", path.display());
/// ```
#[must_use]
pub fn default_socket_path(server_name: &str) -> PathBuf {
    if let Ok(runtime_dir) = std::env::var("XDG_RUNTIME_DIR") {
        PathBuf::from(runtime_dir).join("teamucks").join(format!("{server_name}.sock"))
    } else {
        // SAFETY: `libc::getuid()` is always safe; it is a simple syscall
        // that returns the real user ID of the calling process.  There are no
        // pointer arguments and no failure modes.
        let uid = unsafe { libc::getuid() };
        let tmp = std::env::var("TMPDIR").unwrap_or_else(|_| "/tmp".to_string());
        PathBuf::from(tmp).join(format!("teamucks-{uid}")).join(format!("{server_name}.sock"))
    }
}

/// Ensure the parent directory of the socket path exists with mode `0700`.
///
/// Creates intermediate directories as needed.  If the directory already
/// exists its permissions are updated to `0700`.
///
/// # Errors
///
/// Returns an `io::Error` if directory creation or `chmod` fails.
pub(super) fn ensure_socket_dir(socket_path: &Path) -> std::io::Result<()> {
    use std::os::unix::fs::{DirBuilderExt, PermissionsExt};

    let dir = socket_path.parent().ok_or_else(|| {
        std::io::Error::new(std::io::ErrorKind::InvalidInput, "socket path has no parent directory")
    })?;

    std::fs::DirBuilder::new().recursive(true).mode(0o700).create(dir)?;

    // Always enforce 0700 — `recursive(true)` skips chmod if dir already exists.
    std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700))
}

/// Remove a stale socket file if it exists, ignoring "not found" errors.
///
/// # Errors
///
/// Returns an `io::Error` if the file exists but cannot be removed.
pub(super) fn remove_stale_socket(socket_path: &Path) -> std::io::Result<()> {
    match std::fs::remove_file(socket_path) {
        Ok(()) => Ok(()),
        Err(ref e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e),
    }
}

// ---------------------------------------------------------------------------
// Unit tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_socket_path_xdg() {
        std::env::set_var("XDG_RUNTIME_DIR", "/run/user/9999");
        let path = default_socket_path("default");
        std::env::remove_var("XDG_RUNTIME_DIR");
        assert_eq!(path, PathBuf::from("/run/user/9999/teamucks/default.sock"));
    }

    #[test]
    fn test_default_socket_path_tmpdir_fallback() {
        std::env::remove_var("XDG_RUNTIME_DIR");
        std::env::set_var("TMPDIR", "/tmp");
        let path = default_socket_path("srv");
        // SAFETY: always safe — no arguments, no failure modes.
        let uid = unsafe { libc::getuid() };
        assert_eq!(path, PathBuf::from(format!("/tmp/teamucks-{uid}/srv.sock")));
    }

    #[test]
    fn test_ensure_socket_dir_creates_with_mode_700() {
        use std::os::unix::fs::PermissionsExt;

        let tmp = tempfile::TempDir::new().expect("temp dir");
        let socket_path = tmp.path().join("sub").join("my.sock");
        ensure_socket_dir(&socket_path).expect("dir must be created");

        let dir = socket_path.parent().unwrap();
        let meta = std::fs::metadata(dir).expect("metadata");
        let mode = meta.permissions().mode() & 0o777;
        assert_eq!(mode, 0o700);
    }

    #[test]
    fn test_remove_stale_socket_no_file_is_ok() {
        let tmp = tempfile::TempDir::new().expect("temp dir");
        let path = tmp.path().join("nonexistent.sock");
        assert!(remove_stale_socket(&path).is_ok(), "missing file must not be an error");
    }

    #[test]
    fn test_remove_stale_socket_removes_existing_file() {
        let tmp = tempfile::TempDir::new().expect("temp dir");
        let path = tmp.path().join("stale.sock");
        std::fs::write(&path, b"").expect("create stale file");
        assert!(path.exists());
        remove_stale_socket(&path).expect("remove must succeed");
        assert!(!path.exists());
    }
}
