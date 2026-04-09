/// Child process wrapper for PTY-connected processes.
///
/// `ChildProcess` owns a child that was launched with its stdio wired to a PTY
/// slave device.  The implementation uses `std::process::Command` with a
/// `pre_exec` hook so that fork safety in a multi-threaded context is handled
/// by the standard library (it performs `exec` immediately after `fork`).
use std::ffi::OsStr;
use std::os::unix::io::OwnedFd;

use nix::sys::signal::Signal;
use nix::sys::wait::{waitpid, WaitPidFlag, WaitStatus};
use nix::unistd::Pid;

use super::PtyError;

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

/// Exit status of a child process.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExitStatus {
    /// The numeric exit code, if the process exited normally.
    pub code: Option<i32>,
    /// The signal number that terminated the process, if killed by a signal.
    pub signal: Option<i32>,
}

/// A child process connected to a PTY slave device.
///
/// # Safety note
///
/// `ChildProcess::spawn` forks internally via `std::process::Command`.  The
/// `pre_exec` closure runs between `fork` and `exec` in the child; only
/// async-signal-safe operations are performed there (dup2, setsid, close).
/// The parent side is safe in a multi-threaded context because `exec` is
/// called immediately in the child.
pub struct ChildProcess {
    pid: Pid,
}

impl ChildProcess {
    /// Fork a child process connected to the given PTY slave fd.
    ///
    /// The child:
    /// 1. Creates a new session with `setsid(2)`.
    /// 2. Duplicates `slave_fd` onto stdin, stdout, and stderr.
    /// 3. Closes all other file descriptors (best-effort).
    /// 4. `exec`s `command` with the provided `args`.
    ///
    /// # Errors
    ///
    /// Returns [`PtyError::Spawn`] if fork or exec fails.
    ///
    /// # Panics
    ///
    /// Panics if the child PID returned by the OS does not fit in `i32`.  This
    /// cannot happen in practice: Linux `PID_MAX` is 4,194,304 which is far
    /// below `i32::MAX`.
    pub fn spawn(slave_fd: OwnedFd, command: &str, args: &[&str]) -> Result<Self, PtyError> {
        use std::os::unix::{io::IntoRawFd, process::CommandExt};

        // Convert the slave OwnedFd into a raw fd so we can move it into the
        // pre_exec closure.  The closure runs in the child after fork; the fd
        // is valid because we haven't called exec yet.
        let slave_raw = slave_fd.into_raw_fd();

        let mut cmd = std::process::Command::new(command);
        for arg in args {
            cmd.arg(OsStr::new(arg));
        }

        // SAFETY: The closure is called after fork() and before execvp() in
        // the child process.  At that point only one thread exists in the
        // child (the forked thread).  We perform only async-signal-safe
        // syscalls: setsid(2), ioctl(TIOCSCTTY), dup2(2), and close(2).
        // `slave_raw` is a valid open fd because OwnedFd guarantees it was
        // open at the time of into_raw_fd(), and no other code runs in this
        // process between into_raw_fd() and the fork.  The execvp that the
        // runtime performs immediately after the closure returns replaces the
        // child image, so any mutated state is discarded.
        unsafe {
            cmd.pre_exec(move || {
                // 1. Create a new session so the slave becomes the
                //    controlling terminal.
                libc::setsid();

                // 2. Set the slave as the controlling terminal (TIOCSCTTY).
                //    Ignore errors; not all platforms require it.
                libc::ioctl(slave_raw, libc::TIOCSCTTY, 0);

                // 3. Wire slave_fd to stdin/stdout/stderr.
                for dest in [libc::STDIN_FILENO, libc::STDOUT_FILENO, libc::STDERR_FILENO] {
                    if slave_raw != dest && libc::dup2(slave_raw, dest) < 0 {
                        return Err(std::io::Error::last_os_error());
                    }
                }

                // 4. Close slave_fd itself if it is not one of the standard
                //    fds (we already dup'd it onto 0/1/2 above).
                if slave_raw > libc::STDERR_FILENO {
                    libc::close(slave_raw);
                }

                // 5. Close every other fd above stderr.  Reading
                //    /proc/self/fd is not async-signal-safe, so we use
                //    getrlimit to find the ceiling and close the range.
                //    EBADF from closing an already-closed fd is silently
                //    ignored.
                let mut rl = libc::rlimit { rlim_cur: 0, rlim_max: 0 };
                libc::getrlimit(libc::RLIMIT_NOFILE, &mut rl);
                let max_fd = rl.rlim_cur.min(4096) as i32;
                for fd in (libc::STDERR_FILENO + 1)..max_fd {
                    libc::close(fd);
                }

                Ok(())
            });
        }

        let child = cmd.spawn().map_err(PtyError::Spawn)?;

        // Convert std::process::Child PID to nix::unistd::Pid.
        // pid_t is i32 on Linux; child.id() returns u32.  Valid PIDs always
        // fit in i32 (Linux PID_MAX is 4_194_304 < i32::MAX).
        let pid = Pid::from_raw(
            child.id().try_into().expect("child PID fits in i32 — Linux PID_MAX < i32::MAX"),
        );

        // Forget the std::process::Child without waiting for it; we manage
        // the wait ourselves via nix::waitpid.
        std::mem::forget(child);

        Ok(Self { pid })
    }

    /// Send a signal to the child process.
    ///
    /// # Errors
    ///
    /// Returns [`PtyError::Signal`] if `kill(2)` fails.
    pub fn signal(&self, sig: Signal) -> Result<(), PtyError> {
        nix::sys::signal::kill(self.pid, sig).map_err(PtyError::Signal)
    }

    /// Check whether the child has exited without blocking.
    ///
    /// Returns `Ok(Some(status))` if the child has exited, `Ok(None)` if it
    /// is still running.
    ///
    /// # Errors
    ///
    /// Returns [`PtyError::Wait`] if `waitpid(2)` fails.
    pub fn try_wait(&self) -> Result<Option<ExitStatus>, PtyError> {
        let flags = WaitPidFlag::WNOHANG;
        match waitpid(self.pid, Some(flags)).map_err(PtyError::Wait)? {
            WaitStatus::StillAlive => Ok(None),
            ws => Ok(Some(wait_status_to_exit(ws))),
        }
    }

    /// Wait for the child to exit (blocking).
    ///
    /// # Errors
    ///
    /// Returns [`PtyError::Wait`] if `waitpid(2)` fails.
    pub fn wait(&self) -> Result<ExitStatus, PtyError> {
        let ws = waitpid(self.pid, None).map_err(PtyError::Wait)?;
        Ok(wait_status_to_exit(ws))
    }

    /// Return the PID of the child process.
    #[must_use]
    pub fn pid(&self) -> Pid {
        self.pid
    }
}

// ---------------------------------------------------------------------------
// Private helpers
// ---------------------------------------------------------------------------

fn wait_status_to_exit(ws: WaitStatus) -> ExitStatus {
    match ws {
        WaitStatus::Exited(_, code) => ExitStatus { code: Some(code), signal: None },
        WaitStatus::Signaled(_, sig, _) => ExitStatus { code: None, signal: Some(sig as i32) },
        // Stopped / Continued / PtraceEvent / StillAlive: treat as not yet
        // fully exited.  Callers are expected to call wait() again.
        _ => ExitStatus { code: None, signal: None },
    }
}
