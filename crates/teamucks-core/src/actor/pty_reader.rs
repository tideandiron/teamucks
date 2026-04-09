//! PTY reader async task.
//!
//! One [`pty_reader`] task runs per live pane.  It wraps the PTY master file
//! descriptor in [`tokio::io::unix::AsyncFd`] to integrate with tokio's epoll
//! reactor, then forwards every chunk of readable bytes to the session actor as
//! [`SessionMsg::PtyOutput`].  On EOF (child closed the slave side) or on an
//! unrecoverable I/O error it sends [`SessionMsg::PaneDied`] and **immediately
//! returns** — it never touches the fd again after that point.
//!
//! # Fd lifetime invariant
//!
//! The session actor owns the [`crate::pty::PtyMaster`] (through [`crate::pane::Pane`])
//! and will only drop it *after* receiving `PaneDied`.  Because this task sends
//! `PaneDied` and then returns, the actor is guaranteed to see `PaneDied` before
//! it drops the `PtyMaster`, meaning the fd remains valid for the entire lifetime
//! of this task.
//!
//! # Why `AsyncFd` instead of `spawn_blocking`
//!
//! PTY reads use the kernel's epoll/kqueue reactor — they are not blocking I/O
//! in the thread-pool sense.  `AsyncFd` integrates the fd directly into tokio's
//! I/O reactor so reads happen on a worker thread only when data is actually
//! available, leaving blocking threads free for true blocking work.

use std::os::unix::io::{AsRawFd as _, RawFd};

use tokio::io::unix::AsyncFd;
use tokio::io::Interest;
use tokio::sync::mpsc;

use crate::{actor::SessionMsg, pane::PaneId};

/// Async task that reads from a PTY master fd and forwards output to the session actor.
///
/// # Parameters
///
/// - `pane_id`: Identifier of the pane that owns this PTY.
/// - `fd`: Raw file descriptor of the PTY master.  **Must remain valid** for the
///   lifetime of this task (see module-level invariant).
/// - `tx`: Sender half of the session actor's [`SessionMsg`] channel.
///
/// # Termination
///
/// The task exits when:
/// - The PTY master returns `Ok(0)` (EOF — slave side closed).
/// - The PTY master returns an I/O error other than `WouldBlock`.
/// - The [`SessionMsg`] channel is closed (actor shut down).
///
/// In all termination cases the task sends [`SessionMsg::PaneDied`] before
/// returning, unless the channel is already closed.
///
/// # Examples
///
/// ```no_run
/// use tokio::sync::mpsc;
/// use teamucks_core::{actor::{pty_reader::pty_reader, SessionMsg}, pane::PaneId, pty::PtyMaster};
/// use std::os::unix::io::AsRawFd as _;
///
/// # async fn example() {
/// let (master, _slave) = PtyMaster::open().expect("PTY must be available");
/// let fd = master.as_raw_fd();
/// let pane_id = PaneId(1);
/// let (tx, _rx) = mpsc::channel::<SessionMsg>(64);
/// tokio::spawn(pty_reader(pane_id, fd, tx));
/// # }
/// ```
pub async fn pty_reader(pane_id: PaneId, fd: RawFd, tx: mpsc::Sender<SessionMsg>) {
    // Set O_NONBLOCK on the PTY master fd so that `try_io` returns
    // `WouldBlock` instead of blocking the thread when no data is available.
    // `AsyncFd` requires the underlying fd to be in non-blocking mode —
    // `openpty(2)` returns a blocking fd by default.
    //
    // SAFETY: `fd` is a valid open fd for the lifetime of this call.
    // `fcntl(F_SETFL, O_NONBLOCK)` is idempotent and safe to call on any fd.
    {
        // SAFETY: `fd` is valid and we are not reading or writing data here.
        let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
        if flags == -1 {
            let e = std::io::Error::last_os_error();
            tracing::error!(pane_id = %pane_id, error = %e, "fcntl F_GETFL failed on PTY fd");
            let _ = tx.send(SessionMsg::PaneDied { pane_id, exit_code: 0 }).await;
            return;
        }
        // SAFETY: `fd` is valid; setting O_NONBLOCK is safe on PTY fds.
        if unsafe { libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) } == -1 {
            let e = std::io::Error::last_os_error();
            tracing::error!(pane_id = %pane_id, error = %e, "fcntl F_SETFL O_NONBLOCK failed on PTY fd");
            let _ = tx.send(SessionMsg::PaneDied { pane_id, exit_code: 0 }).await;
            return;
        }
    }

    // SAFETY: `fd` is the PTY master fd opened by `PtyMaster::open()`.  It
    // remains valid for the lifetime of this task because the session actor
    // owns the `Pane` (and therefore the `PtyMaster`) and only drops it after
    // receiving `PaneDied`.  This task sends `PaneDied` immediately before
    // returning, so no use-after-free is possible.  No other task reads from
    // this fd concurrently — `pty_reader` is the sole reader.
    let async_fd = match AsyncFd::with_interest(fd, Interest::READABLE) {
        Ok(f) => f,
        Err(e) => {
            tracing::error!(pane_id = %pane_id, error = %e, "failed to register PTY fd with reactor");
            // Nothing to send PaneDied for — the fd was never readable.
            // The actor will handle the absence of PaneDied when it detects
            // the pane is gone.
            let _ = tx.send(SessionMsg::PaneDied { pane_id, exit_code: 0 }).await;
            return;
        }
    };

    let mut buf = vec![0u8; 4096];

    loop {
        // Wait until the fd has data available to read.
        let mut guard = match async_fd.readable().await {
            Ok(g) => g,
            Err(e) => {
                tracing::warn!(pane_id = %pane_id, error = %e, "PTY fd readable poll failed");
                break;
            }
        };

        match guard.try_io(|inner| {
            // SAFETY: `inner.as_raw_fd()` is the same PTY master fd passed in.
            // We hold the `ReadyGuard`, which guarantees the fd is readable.
            // `nix::unistd::read` is a safe syscall wrapper; the buffer is
            // exclusively owned here.
            let n = nix::unistd::read(inner.as_raw_fd(), &mut buf)
                .map_err(|e| std::io::Error::from_raw_os_error(e as i32))?;
            Ok(n)
        }) {
            // EOF: the slave side was closed (child exited or all slave fds closed).
            Ok(Ok(0)) => {
                tracing::debug!(pane_id = %pane_id, "PTY EOF — child exited");
                // Best-effort send: if the actor channel is full or closed the
                // actor already knows about the shutdown.
                // CRITICAL: return IMMEDIATELY after sending PaneDied.  The actor
                // will drop the Pane (and PtyMaster fd) after processing this
                // message.  Reading the fd again after this point is undefined
                // behaviour.
                let _ = tx.send(SessionMsg::PaneDied { pane_id, exit_code: 0 }).await;
                return;
            }

            // Successful read: forward the data to the session actor.
            Ok(Ok(n)) => {
                let data = buf[..n].to_vec();
                tracing::trace!(pane_id = %pane_id, bytes = n, "PTY output");
                if tx.send(SessionMsg::PtyOutput { pane_id, data }).await.is_err() {
                    // Actor channel closed — the actor shut down; exit quietly.
                    tracing::debug!(pane_id = %pane_id, "PTY reader: actor channel closed, exiting");
                    return;
                }
            }

            // `WouldBlock` from `try_io`: the guard fired spuriously.  Clear
            // the ready flag so `readable()` will re-arm and we will try again.
            Ok(Err(e)) if e.kind() == std::io::ErrorKind::WouldBlock => {
                guard.clear_ready();
            }

            // Unrecoverable I/O error.
            Ok(Err(e)) => {
                tracing::warn!(pane_id = %pane_id, error = %e, "PTY read error");
                break;
            }

            // `AsyncFd::try_io` returned `Err(WouldBlock)` — spurious wake.
            Err(_would_block) => {
                guard.clear_ready();
            }
        }
    }

    // Reaching here means we broke out of the loop due to a readable() error
    // or an unrecoverable read error.  Send PaneDied so the actor cleans up.
    tracing::debug!(pane_id = %pane_id, "PTY reader exiting — sending PaneDied");
    let _ = tx.send(SessionMsg::PaneDied { pane_id, exit_code: 0 }).await;
}
