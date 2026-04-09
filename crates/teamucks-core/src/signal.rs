/// Signal handling utilities for teamucks-core.
///
/// This module is the **only** place in `teamucks-core` other than
/// `src/pty/mod.rs` that is permitted to issue signals directly.  Callers
/// that need to signal a PTY child should use these helpers rather than
/// calling `nix::sys::signal::kill` directly, so that signal use is auditable
/// from a single location.
///
/// # Unsafe policy
///
/// `nix::sys::signal::kill` is safe (no `unsafe` keyword on its signature).
/// This module currently contains no `unsafe` blocks.  If raw signal-handler
/// registration (`sigaction`) is added later, the `unsafe` block must carry a
/// `// SAFETY:` comment.
use nix::sys::signal::{kill, Signal};
use nix::unistd::Pid;

/// Send `SIGWINCH` to the given process.
///
/// Terminal multiplexers send `SIGWINCH` to the child process after updating
/// the PTY window size so that TUI applications re-query the terminal
/// dimensions.
///
/// # Errors
///
/// Returns a [`nix::errno::Errno`] if `kill(2)` fails (e.g. `ESRCH` if the
/// process no longer exists).
///
/// # Examples
///
/// ```no_run
/// use nix::unistd::Pid;
/// use teamucks_core::signal::send_sigwinch;
///
/// // Notify the child that the window size changed.
/// send_sigwinch(Pid::from_raw(12345)).ok();
/// ```
pub fn send_sigwinch(pid: Pid) -> Result<(), nix::errno::Errno> {
    kill(pid, Signal::SIGWINCH)
}

/// Send `SIGTERM` to the given process.
///
/// Used to request graceful termination of a child process.  If the child
/// does not exit within the expected grace period, the caller should follow
/// up with `SIGKILL`.
///
/// # Errors
///
/// Returns a [`nix::errno::Errno`] if `kill(2)` fails.
///
/// # Examples
///
/// ```no_run
/// use nix::unistd::Pid;
/// use teamucks_core::signal::send_sigterm;
///
/// send_sigterm(Pid::from_raw(12345)).ok();
/// ```
pub fn send_sigterm(pid: Pid) -> Result<(), nix::errno::Errno> {
    kill(pid, Signal::SIGTERM)
}

// ---------------------------------------------------------------------------
// Unit tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// Sending SIGWINCH to the current process must succeed (we have
    /// permission to signal ourselves, and the default SIGWINCH handler is
    /// to ignore the signal).
    #[test]
    fn test_send_sigwinch_to_self() {
        let pid = nix::unistd::getpid();
        assert!(send_sigwinch(pid).is_ok(), "send_sigwinch to self must succeed");
    }

    /// Sending SIGWINCH to PID 1 (init/systemd) must fail with EPERM because
    /// we are not root.
    ///
    /// This test is skipped if running as root (CI with --privileged).
    #[test]
    fn test_send_sigwinch_to_pid1_eperm() {
        // SAFETY: geteuid(2) is always safe to call; it has no preconditions.
        if unsafe { libc::geteuid() } == 0 {
            return; // skip when privileged
        }
        let result = send_sigwinch(Pid::from_raw(1));
        assert!(result.is_err(), "send_sigwinch to init must fail without root");
    }
}
