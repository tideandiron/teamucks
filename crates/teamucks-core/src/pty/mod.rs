/// PTY management for teamucks-core.
///
/// This module is the **only** place in `teamucks-core` permitted to call
/// `openpty(2)`, perform raw fd I/O on a PTY master, or use `ioctl(TIOCSWINSZ)`.
/// All `unsafe` blocks carry a `// SAFETY:` comment naming the invariants
/// upheld.
///
/// # Unsafe surface
///
/// - `PtyMaster::open` calls `nix::pty::openpty` which is safe; no `unsafe`
///   is emitted there.
/// - `PtyMaster::set_window_size` calls `tiocswinsz`, a thin
///   `ioctl_write_ptr_bad!`-generated function whose `unsafe` block is
///   justified inline.
/// - `PtyMaster::read` and `PtyMaster::write` call `nix::unistd::read` /
///   `nix::unistd::write`, which are safe wrappers themselves.
pub mod child;

pub use child::{ChildProcess, ExitStatus};

use std::os::unix::io::{AsRawFd, OwnedFd, RawFd};

use nix::pty::{openpty, Winsize};

// ---------------------------------------------------------------------------
// ioctl binding for TIOCSWINSZ
// ---------------------------------------------------------------------------

// Generate a safe-ish wrapper around the TIOCSWINSZ ioctl.
// `ioctl_write_ptr_bad!` produces an `unsafe fn` named `tiocswinsz`.
nix::ioctl_write_ptr_bad!(tiocswinsz, libc::TIOCSWINSZ, libc::winsize);

// ---------------------------------------------------------------------------
// Error type
// ---------------------------------------------------------------------------

/// Errors that can arise from PTY operations.
#[derive(Debug, thiserror::Error)]
pub enum PtyError {
    /// Failed to open a new PTY pair.
    #[error("failed to open PTY: {0}")]
    Open(#[source] nix::errno::Errno),

    /// Failed to set the terminal window size.
    #[error("failed to set window size: {0}")]
    WindowSize(#[source] nix::errno::Errno),

    /// The requested window dimensions are invalid (zero width or height).
    #[error("invalid window size: cols={cols} rows={rows}")]
    InvalidWindowSize {
        /// Number of columns requested.
        cols: u16,
        /// Number of rows requested.
        rows: u16,
    },

    /// Failed to spawn the child process.
    #[error("failed to spawn child process: {0}")]
    Spawn(#[source] std::io::Error),

    /// Failed to send a signal to the child process.
    #[error("failed to send signal: {0}")]
    Signal(#[source] nix::errno::Errno),

    /// `waitpid(2)` failed.
    #[error("waitpid failed: {0}")]
    Wait(#[source] nix::errno::Errno),

    /// PTY I/O error.
    #[error("PTY I/O error: {0}")]
    Io(#[from] std::io::Error),
}

// ---------------------------------------------------------------------------
// PtyMaster
// ---------------------------------------------------------------------------

/// The master side of a PTY pair.
///
/// Created by [`PtyMaster::open`], which returns the slave side as a separate
/// [`OwnedFd`] that should be passed to [`ChildProcess::spawn`].
///
/// # Drop behaviour
///
/// When `PtyMaster` is dropped the underlying file descriptor is closed via
/// `OwnedFd`'s `Drop` implementation.  No explicit `drop` override is needed.
pub struct PtyMaster {
    fd: OwnedFd,
}

impl PtyMaster {
    /// Open a PTY pair and return `(master, slave)`.
    ///
    /// The slave [`OwnedFd`] should be passed to [`ChildProcess::spawn`] and
    /// must be closed in the parent after the child has been forked, or when
    /// no longer needed.
    ///
    /// # Errors
    ///
    /// Returns [`PtyError::Open`] if `openpty(2)` fails (e.g., no PTY devices
    /// available).
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use teamucks_core::pty::PtyMaster;
    ///
    /// let (master, slave) = PtyMaster::open().expect("PTY must be available");
    /// drop(slave); // close slave in parent when no child is spawned
    /// ```
    pub fn open() -> Result<(Self, OwnedFd), PtyError> {
        let result = openpty(None, None).map_err(PtyError::Open)?;
        Ok((Self { fd: result.master }, result.slave))
    }

    /// Return the raw file descriptor for async I/O registration (e.g. with
    /// `tokio::io::unix::AsyncFd`).
    ///
    /// The caller must not close or outlive this fd.
    #[must_use]
    pub fn as_raw_fd(&self) -> RawFd {
        self.fd.as_raw_fd()
    }

    /// Set the terminal window size.
    ///
    /// Sends `TIOCSWINSZ` to the PTY master.  The child process will receive
    /// `SIGWINCH` if it has registered a handler.
    ///
    /// # Errors
    ///
    /// Returns [`PtyError::InvalidWindowSize`] if `cols` or `rows` is zero.
    /// Returns [`PtyError::WindowSize`] if the `ioctl` call fails.
    pub fn set_window_size(&self, cols: u16, rows: u16) -> Result<(), PtyError> {
        if cols == 0 || rows == 0 {
            return Err(PtyError::InvalidWindowSize { cols, rows });
        }

        let ws = Winsize {
            ws_col: cols,
            ws_row: rows,
            // Pixel dimensions are optional; zero means "not specified".
            ws_xpixel: 0,
            ws_ypixel: 0,
        };

        // SAFETY: `tiocswinsz` is generated by `nix::ioctl_write_ptr_bad!`
        // for the `TIOCSWINSZ` request code with type `libc::winsize`.
        // Invariants:
        // 1. `self.fd.as_raw_fd()` is a valid open file descriptor for the
        //    PTY master because `OwnedFd` guarantees this for its lifetime.
        // 2. `&ws` is a valid pointer to an initialised `libc::winsize` on
        //    the stack; the ioctl copies it by value so no aliasing occurs.
        // 3. No other thread holds a mutable reference to `ws`.
        unsafe { tiocswinsz(self.fd.as_raw_fd(), &ws) }.map_err(PtyError::WindowSize)?;
        Ok(())
    }

    /// Read bytes from the PTY master.
    ///
    /// Returns the number of bytes placed into `buf`.  An `Ok(0)` result
    /// indicates EOF (the child closed the slave side, i.e. it exited).
    ///
    /// # Errors
    ///
    /// Returns [`PtyError::Io`] for underlying I/O errors.
    pub fn read(&self, buf: &mut [u8]) -> Result<usize, PtyError> {
        nix::unistd::read(self.fd.as_raw_fd(), buf)
            .map_err(|e| PtyError::Io(std::io::Error::from(e)))
    }

    /// Write bytes to the PTY master (sends data to the child's stdin).
    ///
    /// Returns the number of bytes written.
    ///
    /// # Errors
    ///
    /// Returns [`PtyError::Io`] for underlying I/O errors.
    pub fn write(&self, buf: &[u8]) -> Result<usize, PtyError> {
        nix::unistd::write(&self.fd, buf).map_err(|e| PtyError::Io(std::io::Error::from(e)))
    }
}

impl AsRawFd for PtyMaster {
    fn as_raw_fd(&self) -> RawFd {
        self.fd.as_raw_fd()
    }
}

impl std::fmt::Debug for PtyMaster {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PtyMaster").field("fd", &self.fd.as_raw_fd()).finish()
    }
}

// ---------------------------------------------------------------------------
// Unit tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pty_open_fds_are_distinct() {
        let (master, slave) = PtyMaster::open().expect("PTY open must succeed");
        assert_ne!(master.as_raw_fd(), slave.as_raw_fd());
    }

    #[test]
    fn test_pty_set_window_size_valid() {
        let (master, _slave) = PtyMaster::open().expect("PTY open must succeed");
        assert!(master.set_window_size(80, 24).is_ok());
    }

    #[test]
    fn test_pty_set_window_size_zero_cols_invalid() {
        let (master, _slave) = PtyMaster::open().expect("PTY open must succeed");
        let err = master.set_window_size(0, 24).unwrap_err();
        assert!(
            matches!(err, PtyError::InvalidWindowSize { cols: 0, rows: 24 }),
            "expected InvalidWindowSize, got {err}"
        );
    }

    #[test]
    fn test_pty_set_window_size_zero_rows_invalid() {
        let (master, _slave) = PtyMaster::open().expect("PTY open must succeed");
        let err = master.set_window_size(80, 0).unwrap_err();
        assert!(
            matches!(err, PtyError::InvalidWindowSize { cols: 80, rows: 0 }),
            "expected InvalidWindowSize, got {err}"
        );
    }

    #[test]
    fn test_pty_error_display() {
        let err = PtyError::Open(nix::errno::Errno::EMFILE);
        assert!(err.to_string().contains("open PTY"));

        let err = PtyError::WindowSize(nix::errno::Errno::EBADF);
        assert!(err.to_string().contains("window size"));

        let err = PtyError::InvalidWindowSize { cols: 0, rows: 0 };
        assert!(err.to_string().contains("invalid window size"));
    }
}
