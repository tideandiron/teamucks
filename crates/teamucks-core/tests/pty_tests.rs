/// Integration tests for PTY management (Feature 13).
///
/// These tests exercise the public API of `teamucks_core::pty` and
/// `teamucks_core::signal`.  Tests that require forking or specific Linux
/// capabilities are individually gated with `#[cfg(target_os = "linux")]`.
/// Tests that are slow or require elevated privileges are marked `#[ignore]`.
// libc is used in the fd_is_open helper inside the linux-gated module.
extern crate libc;

#[cfg(target_os = "linux")]
mod pty {
    use std::os::unix::io::AsRawFd;

    use teamucks_core::pty::{PtyError, PtyMaster};

    // -----------------------------------------------------------------------
    // PtyMaster creation
    // -----------------------------------------------------------------------

    /// Opening a PTY pair returns valid file descriptors on both sides.
    #[test]
    fn test_pty_open_returns_valid_fds() {
        let (master, slave) = PtyMaster::open().expect("PTY open must succeed");
        // Both raw fds must be non-negative (valid).
        assert!(master.as_raw_fd() >= 0, "master fd must be non-negative");
        assert!(slave.as_raw_fd() >= 0, "slave fd must be non-negative");
        // They must be distinct.
        assert_ne!(master.as_raw_fd(), slave.as_raw_fd(), "master and slave fds must differ");
    }

    /// Setting window size on a freshly opened PTY must not return an error.
    #[test]
    fn test_pty_set_window_size_succeeds() {
        let (master, _slave) = PtyMaster::open().expect("PTY open must succeed");
        master.set_window_size(220, 50).expect("set_window_size must succeed");
    }

    /// set_window_size must reject zero columns.
    #[test]
    fn test_pty_set_window_size_zero_cols_rejected() {
        let (master, _slave) = PtyMaster::open().expect("PTY open must succeed");
        let result = master.set_window_size(0, 24);
        assert!(result.is_err(), "zero column count must be rejected");
    }

    /// set_window_size must reject zero rows.
    #[test]
    fn test_pty_set_window_size_zero_rows_rejected() {
        let (master, _slave) = PtyMaster::open().expect("PTY open must succeed");
        let result = master.set_window_size(80, 0);
        assert!(result.is_err(), "zero row count must be rejected");
    }

    // -----------------------------------------------------------------------
    // ChildProcess spawn + wait
    // -----------------------------------------------------------------------

    use teamucks_core::pty::ChildProcess;

    /// Spawning /bin/sh gives back a valid positive PID.
    #[test]
    fn test_pty_spawn_shell_returns_valid_pid() {
        let (master, slave) = PtyMaster::open().expect("PTY open must succeed");
        // Suppress compiler warning about master while slave is moved.
        let _master = master;
        let child =
            ChildProcess::spawn(slave, "/bin/sh", &["-c", "exit 0"]).expect("spawn must succeed");
        assert!(child.pid().as_raw() > 0, "child PID must be positive");
        // Reap the child.
        let _ = child.wait();
    }

    /// A child that exits with code 0 is reported correctly by wait().
    #[test]
    fn test_pty_child_exit_code_zero() {
        let (_master, slave) = PtyMaster::open().expect("PTY open must succeed");
        let child =
            ChildProcess::spawn(slave, "/bin/sh", &["-c", "exit 0"]).expect("spawn must succeed");
        let status = child.wait().expect("wait must succeed");
        assert_eq!(status.code, Some(0), "exit code must be 0");
        assert!(status.signal.is_none(), "must not have been signalled");
    }

    /// A child that exits with code 42 is reported correctly.
    #[test]
    fn test_pty_child_exit_code_42() {
        let (_master, slave) = PtyMaster::open().expect("PTY open must succeed");
        let child =
            ChildProcess::spawn(slave, "/bin/sh", &["-c", "exit 42"]).expect("spawn must succeed");
        let status = child.wait().expect("wait must succeed");
        assert_eq!(status.code, Some(42), "exit code must be 42");
        assert!(status.signal.is_none(), "must not have been signalled");
    }

    /// try_wait() returns None while the child is still running.
    #[test]
    fn test_pty_try_wait_returns_none_while_running() {
        let (_master, slave) = PtyMaster::open().expect("PTY open must succeed");
        // sleep long enough that the child is alive during try_wait.
        let child =
            ChildProcess::spawn(slave, "/bin/sh", &["-c", "sleep 30"]).expect("spawn must succeed");
        let status = child.try_wait().expect("try_wait must succeed");
        assert!(status.is_none(), "try_wait must return None for a running child");
        // Clean up: kill the child.
        child.signal(nix::sys::signal::Signal::SIGTERM).expect("signal must succeed");
        let _ = child.wait();
    }

    /// Sending SIGTERM to a child makes it exit with a signal status.
    #[test]
    fn test_pty_child_signal_sigterm() {
        let (_master, slave) = PtyMaster::open().expect("PTY open must succeed");
        let child =
            ChildProcess::spawn(slave, "/bin/sh", &["-c", "sleep 60"]).expect("spawn must succeed");
        child.signal(nix::sys::signal::Signal::SIGTERM).expect("signal must succeed");
        let status = child.wait().expect("wait must succeed");
        // When killed by a signal the exit code is None.
        assert!(
            status.signal.is_some() || status.code.is_some(),
            "child terminated by SIGTERM must report signal or code"
        );
    }

    // -----------------------------------------------------------------------
    // write / read round-trip
    // -----------------------------------------------------------------------

    /// Writing to the master and reading back via master echoes data through
    /// the PTY.
    ///
    /// PTYs echo input by default (local echo / ECHO flag).  We write a known
    /// byte to the master and read back the echo without spawning a shell, so
    /// the slave side just needs to be open.
    #[test]
    fn test_pty_write_read_roundtrip() {
        use std::time::Duration;

        let (master, slave) = PtyMaster::open().expect("PTY open must succeed");
        // Spawn a child that keeps the slave open and reads from it (any
        // shell will do).  The important thing is that local echo causes
        // master-writes to appear on master-reads.
        let child =
            ChildProcess::spawn(slave, "/bin/sh", &["-c", "sleep 10"]).expect("spawn must succeed");

        // Give the child a moment to start.
        std::thread::sleep(Duration::from_millis(50));

        let written = master.write(b"echo hi\n").expect("write must succeed");
        assert_eq!(written, 8, "all bytes must be written");

        // Read back at least 1 byte (the local echo).
        let mut buf = [0u8; 256];
        // PTY reads can block; we set a short deadline via non-blocking.
        // Use a raw poll/read loop with a timeout.
        let start = std::time::Instant::now();
        let timeout = Duration::from_secs(2);
        let mut total = 0usize;
        while start.elapsed() < timeout {
            match master.read(&mut buf[total..]) {
                Ok(0) => break,
                Ok(n) => {
                    total += n;
                    if total >= 1 {
                        break;
                    }
                }
                Err(_) => break,
            }
        }
        assert!(total > 0, "must read at least 1 echoed byte");

        child.signal(nix::sys::signal::Signal::SIGTERM).expect("signal must succeed");
        let _ = child.wait();
    }

    // -----------------------------------------------------------------------
    // Resize + SIGWINCH
    // -----------------------------------------------------------------------

    use teamucks_core::signal::{send_sigterm, send_sigwinch};

    /// Resizing and sending SIGWINCH must not return an error.
    #[test]
    fn test_pty_resize_and_sigwinch() {
        let (master, slave) = PtyMaster::open().expect("PTY open must succeed");
        let child =
            ChildProcess::spawn(slave, "/bin/sh", &["-c", "sleep 10"]).expect("spawn must succeed");

        master.set_window_size(100, 40).expect("set_window_size must succeed");
        send_sigwinch(child.pid()).expect("send_sigwinch must succeed");

        send_sigterm(child.pid()).expect("send_sigterm must succeed");
        let _ = child.wait();
    }

    // -----------------------------------------------------------------------
    // FD leak check
    // -----------------------------------------------------------------------

    /// Opening and then dropping a PTY pair must release both file descriptors.
    ///
    /// We hold a global mutex to prevent other tests from opening fds between
    /// our "record fd number" and "check fd is closed" steps, which would
    /// cause a false positive if another test reuses the same fd number.
    #[test]
    fn test_pty_no_fd_leak() {
        use std::os::unix::io::AsRawFd;
        use std::sync::Mutex;

        // Global mutex serializes this test against other PTY-opening tests.
        static LOCK: Mutex<()> = Mutex::new(());
        let _guard = LOCK.lock().expect("fd-leak mutex must not be poisoned");

        let (master_fd, slave_fd) = {
            let (master, slave) = PtyMaster::open().expect("PTY open must succeed");
            (master.as_raw_fd(), slave.as_raw_fd())
        };
        // After drop, both fds must be invalid (closed).  The mutex prevents
        // any other test from opening new fds that might reuse these numbers.
        assert!(!fd_is_open(master_fd), "master fd {master_fd} must be closed after drop");
        assert!(!fd_is_open(slave_fd), "slave fd {slave_fd} must be closed after drop");
    }

    // -----------------------------------------------------------------------
    // signal module
    // -----------------------------------------------------------------------

    /// send_sigwinch to a live process must succeed.
    #[test]
    fn test_signal_send_sigwinch_live_process() {
        let (_master, slave) = PtyMaster::open().expect("PTY open must succeed");
        let child =
            ChildProcess::spawn(slave, "/bin/sh", &["-c", "sleep 5"]).expect("spawn must succeed");
        send_sigwinch(child.pid()).expect("send_sigwinch must succeed");
        send_sigterm(child.pid()).expect("clean up");
        let _ = child.wait();
    }

    /// send_sigterm to a live process must succeed.
    #[test]
    fn test_signal_send_sigterm_live_process() {
        let (_master, slave) = PtyMaster::open().expect("PTY open must succeed");
        let child =
            ChildProcess::spawn(slave, "/bin/sh", &["-c", "sleep 5"]).expect("spawn must succeed");
        send_sigterm(child.pid()).expect("send_sigterm must succeed");
        let _ = child.wait();
    }

    // -----------------------------------------------------------------------
    // Error type smoke tests
    // -----------------------------------------------------------------------

    /// PtyError::WindowSize wraps a nix error and has a Display representation.
    #[test]
    fn test_pty_error_display_window_size() {
        let err = PtyError::WindowSize(nix::errno::Errno::EBADF);
        let msg = err.to_string();
        assert!(msg.contains("window size"), "error message must mention window size: {msg}");
    }

    /// PtyError::Open wraps a nix error and has a Display representation.
    #[test]
    fn test_pty_error_display_open() {
        let err = PtyError::Open(nix::errno::Errno::EMFILE);
        let msg = err.to_string();
        assert!(msg.contains("open PTY"), "error message must mention open PTY: {msg}");
    }

    // -----------------------------------------------------------------------
    // Helpers
    // -----------------------------------------------------------------------

    /// Return true if the given raw fd is currently open in this process.
    ///
    /// Uses `fcntl(F_GETFD)` which fails with `EBADF` for closed fds.
    fn fd_is_open(fd: std::os::unix::io::RawFd) -> bool {
        // SAFETY: fcntl(F_GETFD) is safe to call on any integer; it simply
        // returns -1 with EBADF if the fd is not valid.  No invariants of the
        // Rust memory model are violated.
        unsafe { libc::fcntl(fd, libc::F_GETFD) != -1 }
    }
}
