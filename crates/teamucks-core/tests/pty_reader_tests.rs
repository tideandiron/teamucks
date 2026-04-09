// crates/teamucks-core/tests/pty_reader_tests.rs
//
// Integration tests for the `pty_reader` async task (Feature I2).
//
// Each test opens a real PTY pair via `PtyMaster::open` and verifies the
// observable behaviour of the task through the `SessionMsg` channel.

use std::os::unix::io::AsRawFd as _;
use std::time::Duration;

use tokio::sync::mpsc;

use teamucks_core::{
    actor::{pty_reader::pty_reader, SessionMsg},
    pane::PaneId,
    pty::PtyMaster,
};

/// Verifies that bytes written to the slave PTY appear as `PtyOutput` messages.
#[tokio::test]
async fn test_pty_reader_forwards_output_as_pty_output() {
    let (master, slave) = PtyMaster::open().expect("PTY open");
    let master_fd = master.as_raw_fd();
    let pane_id = PaneId(1);
    let (tx, mut rx) = mpsc::channel::<SessionMsg>(16);

    tokio::spawn(pty_reader(pane_id, master_fd, tx));

    // Write to slave side to simulate child output.
    use std::io::Write as _;
    use std::os::unix::io::FromRawFd as _;
    // SAFETY: slave is a valid OwnedFd; we forget it to avoid double-close since
    // File::from_raw_fd takes ownership of the fd number.
    let slave_fd_raw = slave.as_raw_fd();
    std::mem::forget(slave);
    let mut slave_writer = unsafe { std::fs::File::from_raw_fd(slave_fd_raw) };
    slave_writer.write_all(b"hello").expect("write to slave");
    // Keep the slave open so we don't trigger EOF before verifying PtyOutput.
    std::mem::forget(slave_writer);

    let msg = tokio::time::timeout(Duration::from_secs(2), rx.recv())
        .await
        .expect("must receive within 2s")
        .expect("channel open");
    match msg {
        SessionMsg::PtyOutput { pane_id: id, data } => {
            assert_eq!(id, PaneId(1));
            assert!(!data.is_empty(), "data must not be empty");
        }
        other => panic!("expected PtyOutput, got {other:?}"),
    }
}

/// Verifies that closing the slave PTY causes `PaneDied` to be sent.
#[tokio::test]
async fn test_pty_reader_sends_pane_died_on_eof() {
    let (master, slave) = PtyMaster::open().expect("PTY open");
    let master_fd = master.as_raw_fd();
    let pane_id = PaneId(2);
    let (tx, mut rx) = mpsc::channel::<SessionMsg>(16);

    tokio::spawn(pty_reader(pane_id, master_fd, tx));

    // Close the slave fd to trigger EOF on master.
    drop(slave);

    // Drain any PtyOutput that may arrive before PaneDied.
    let deadline = tokio::time::Instant::now() + Duration::from_secs(2);
    loop {
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        let msg = tokio::time::timeout(remaining, rx.recv())
            .await
            .expect("must receive within 2s")
            .expect("channel open");
        if matches!(msg, SessionMsg::PaneDied { pane_id: PaneId(2), .. }) {
            return; // success
        }
    }
}

/// Verifies that the `pty_reader` task exits after sending `PaneDied` (no leak).
#[tokio::test]
async fn test_pty_reader_exits_after_pane_died() {
    let (master, slave) = PtyMaster::open().expect("PTY open");
    let master_fd = master.as_raw_fd();
    let pane_id = PaneId(3);
    let (tx, mut rx) = mpsc::channel::<SessionMsg>(16);

    let handle: tokio::task::JoinHandle<()> = tokio::spawn(pty_reader(pane_id, master_fd, tx));
    drop(slave); // trigger EOF

    tokio::time::timeout(Duration::from_secs(2), handle)
        .await
        .expect("task must exit within 2s")
        .expect("task must not panic");

    // Drain remaining messages — channel closes when task exits.
    while rx.try_recv().is_ok() {}
}

/// Verifies that `pty_reader` handles large output without panicking and
/// forwards all bytes (implicit: 4096-byte read buffer loops correctly).
#[tokio::test]
async fn test_pty_reader_handles_large_output() {
    let (master, slave) = PtyMaster::open().expect("PTY open");
    let master_fd = master.as_raw_fd();
    let pane_id = PaneId(4);
    let (tx, mut rx) = mpsc::channel::<SessionMsg>(64);

    tokio::spawn(pty_reader(pane_id, master_fd, tx));

    // Write 8192 bytes (two read buffer sizes) synchronously through the slave fd.
    let slave_fd_raw = slave.as_raw_fd();
    // Forget the OwnedFd so we control the fd lifetime manually; we close it
    // at the end of the test.
    std::mem::forget(slave);

    let mut total_written = 0usize;
    let chunk = vec![b'x'; 4096];
    for _ in 0..2 {
        // SAFETY: slave_fd_raw is valid for the duration of this loop; we
        // borrow it read-only for the write call, no other thread touches it.
        let n = nix::unistd::write(
            unsafe { std::os::unix::io::BorrowedFd::borrow_raw(slave_fd_raw) },
            &chunk,
        )
        .unwrap();
        total_written += n;
    }

    let mut total_received = 0usize;
    let deadline = tokio::time::Instant::now() + Duration::from_secs(2);
    while total_received < total_written {
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        if remaining.is_zero() {
            break;
        }
        if let Ok(Some(SessionMsg::PtyOutput { data, .. })) =
            tokio::time::timeout(remaining, rx.recv()).await
        {
            total_received += data.len();
        }
    }
    assert_eq!(total_received, total_written, "all written bytes must be forwarded");

    // Close the slave fd to avoid leaking it.
    // SAFETY: slave_fd_raw is valid and uniquely owned here (we forgot the OwnedFd above).
    unsafe { libc::close(slave_fd_raw) };
}

/// Verifies that `PaneDied` carries the correct `pane_id`.
#[tokio::test]
async fn test_pty_reader_pane_died_carries_correct_pane_id() {
    let (master, slave) = PtyMaster::open().expect("PTY open");
    let master_fd = master.as_raw_fd();
    let pane_id = PaneId(42);
    let (tx, mut rx) = mpsc::channel::<SessionMsg>(16);

    tokio::spawn(pty_reader(pane_id, master_fd, tx));
    drop(slave);

    let deadline = tokio::time::Instant::now() + Duration::from_secs(2);
    loop {
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        let msg = tokio::time::timeout(remaining, rx.recv())
            .await
            .expect("must receive within 2s")
            .expect("channel open");
        if let SessionMsg::PaneDied { pane_id: id, .. } = msg {
            assert_eq!(id, PaneId(42), "PaneDied must carry the correct pane_id");
            return;
        }
    }
}

/// Verifies that `PtyOutput` messages carry the correct `pane_id`.
#[tokio::test]
async fn test_pty_reader_pty_output_carries_correct_pane_id() {
    let (master, slave) = PtyMaster::open().expect("PTY open");
    let master_fd = master.as_raw_fd();
    let pane_id = PaneId(99);
    let (tx, mut rx) = mpsc::channel::<SessionMsg>(16);

    tokio::spawn(pty_reader(pane_id, master_fd, tx));

    let slave_fd_raw = slave.as_raw_fd();
    std::mem::forget(slave);
    // SAFETY: slave_fd_raw is valid and uniquely owned (OwnedFd forgotten above).
    nix::unistd::write(unsafe { std::os::unix::io::BorrowedFd::borrow_raw(slave_fd_raw) }, b"ping")
        .unwrap();

    let msg = tokio::time::timeout(Duration::from_secs(2), rx.recv())
        .await
        .expect("must receive within 2s")
        .expect("channel open");
    if let SessionMsg::PtyOutput { pane_id: id, .. } = msg {
        assert_eq!(id, PaneId(99), "PtyOutput must carry the correct pane_id");
    } else {
        panic!("expected PtyOutput, got {msg:?}");
    }

    // SAFETY: same as above.
    unsafe { libc::close(slave_fd_raw) };
}
