use std::{
    os::unix::io::RawFd,
    sync::{Mutex, OnceLock},
};

use nix::sys::termios::Termios;

// ── Public constants ──────────────────────────────────────────────────────────

/// Escape sequences written to stdout when entering the alternate screen.
///
/// Order:
/// - `\x1b[?1049h` — enter alternate screen buffer and save cursor position
/// - `\x1b[?25l`   — hide cursor (prevents flicker during initial render)
/// - `\x1b[?1000h` — enable X11 mouse reporting (button press/release)
/// - `\x1b[?1006h` — enable SGR mouse mode (large terminal coordinates)
/// - `\x1b[2J`     — clear screen
/// - `\x1b[H`      — cursor to home
pub const ALTERNATE_SCREEN_ENTER: &[u8] =
    b"\x1b[?1049h\x1b[?25l\x1b[?1000h\x1b[?1006h\x1b[2J\x1b[H";

/// Escape sequences written to stdout when exiting the alternate screen.
///
/// Order:
/// - `\x1b[?1049l` — exit alternate screen buffer and restore cursor position
/// - `\x1b[?25h`   — show cursor
/// - `\x1b[?1000l` — disable X11 mouse reporting
/// - `\x1b[?1006l` — disable SGR mouse mode
/// - `\x1b[0m`     — reset all attributes
pub const ALTERNATE_SCREEN_EXIT: &[u8] = b"\x1b[?1049l\x1b[?25h\x1b[?1000l\x1b[?1006l\x1b[0m";

// ── Global panic-hook state ───────────────────────────────────────────────────

/// Raw fd stored globally so the panic hook can restore the terminal.
///
/// Set once at startup by [`install_panic_hook`].  The fd is the host
/// terminal (stdin), which is open for the entire process lifetime.
static PANIC_RESTORE_FD: OnceLock<RawFd> = OnceLock::new();

/// The original [`libc::termios`] stored globally so the panic hook can
/// restore the terminal even after `TerminalGuard` has been dropped.
///
/// Protected by a `Mutex` so the panic hook can read it safely from any
/// thread.  It is a `libc::termios` (not `nix::Termios`) because `nix::Termios`
/// contains a `RefCell` and is therefore not `Sync`.
static PANIC_ORIGINAL_TERMIOS: OnceLock<Mutex<nix::libc::termios>> = OnceLock::new();

// ── Public API ────────────────────────────────────────────────────────────────

/// Switch `fd` to raw mode and return the previous [`Termios`] state.
///
/// Raw mode disables canonical line processing, echo, signal generation,
/// flow control, and output post-processing so that every keystroke is
/// delivered immediately and unmodified to the application.
///
/// The caller is responsible for restoring the terminal via [`TerminalGuard`].
///
/// # Errors
///
/// Returns [`nix::errno::Errno`] if `fd` is not a TTY or if the `tcgetattr` /
/// `tcsetattr` calls fail (e.g. `ENOTTY`).
///
/// # Example
///
/// ```no_run
/// use std::os::fd::AsRawFd;
/// use teamucks::terminal::enter_raw_mode;
///
/// let fd = std::io::stdin().as_raw_fd();
/// let original = enter_raw_mode(fd).expect("stdin must be a TTY");
/// // ... use terminal in raw mode ...
/// // TerminalGuard restores the state automatically on drop.
/// ```
pub fn enter_raw_mode(fd: RawFd) -> Result<Termios, nix::errno::Errno> {
    use nix::sys::termios::{
        tcgetattr, tcsetattr, ControlFlags, InputFlags, LocalFlags, OutputFlags, SetArg,
    };

    // SAFETY: `fd` is a caller-supplied file descriptor whose validity is the
    // caller's responsibility.  `BorrowedFd::borrow_raw` does not close the fd
    // and does not extend its lifetime beyond this call.
    let mut termios = tcgetattr(unsafe { std::os::fd::BorrowedFd::borrow_raw(fd) })?;
    let original = termios.clone();

    // Disable input flags:
    // BRKINT  — break sends SIGINT
    // ICRNL   — translate CR to NL on input
    // INPCK   — parity checking
    // ISTRIP  — strip eighth bit
    // IXON    — software flow control (XON/XOFF)
    termios.input_flags &= !(InputFlags::BRKINT
        | InputFlags::ICRNL
        | InputFlags::INPCK
        | InputFlags::ISTRIP
        | InputFlags::IXON);

    // Disable output post-processing (OPOST).
    termios.output_flags &= !OutputFlags::OPOST;

    // Set character size to 8 bits (CS8).
    termios.control_flags |= ControlFlags::CS8;

    // Disable local flags:
    // ECHO    — echo input characters
    // ICANON  — canonical mode (line-by-line processing)
    // IEXTEN  — implementation-defined input processing
    // ISIG    — signal generation (Ctrl-C, Ctrl-Z, etc.)
    termios.local_flags &=
        !(LocalFlags::ECHO | LocalFlags::ICANON | LocalFlags::IEXTEN | LocalFlags::ISIG);

    // VMIN=1, VTIME=0: block until at least 1 byte is available.
    termios.control_chars[nix::sys::termios::SpecialCharacterIndices::VMIN as usize] = 1;
    termios.control_chars[nix::sys::termios::SpecialCharacterIndices::VTIME as usize] = 0;

    // SAFETY: same as `tcgetattr` above.
    tcsetattr(unsafe { std::os::fd::BorrowedFd::borrow_raw(fd) }, SetArg::TCSANOW, &termios)?;

    Ok(original)
}

/// Query the terminal dimensions (columns, rows) via `TIOCGWINSZ`.
///
/// # Errors
///
/// Returns [`nix::errno::Errno`] if `fd` is not a TTY or if the ioctl fails.
pub fn query_terminal_size(fd: RawFd) -> Result<(u16, u16), nix::errno::Errno> {
    use nix::libc;

    let mut ws = libc::winsize { ws_row: 0, ws_col: 0, ws_xpixel: 0, ws_ypixel: 0 };

    // SAFETY: `fd` is a valid file descriptor for the duration of this call,
    // and `ws` is a stack-allocated `winsize` that the kernel writes into.
    // `TIOCGWINSZ` is a standard ioctl that does not perform arbitrary memory
    // access beyond the `winsize` struct.
    let ret = unsafe { libc::ioctl(fd, libc::TIOCGWINSZ, &mut ws) };
    if ret == -1 {
        return Err(nix::errno::Errno::last());
    }
    Ok((ws.ws_col, ws.ws_row))
}

/// Install a panic hook that restores the host terminal before printing the
/// panic message.
///
/// Call this once in `main()` after [`enter_raw_mode`] succeeds, passing the
/// `fd` and the `original` termios returned by that call.  The hook:
///
/// 1. Writes [`ALTERNATE_SCREEN_EXIT`] to stdout via a direct `write(2)`
///    syscall so the terminal is restored before the panic message appears.
/// 2. Calls `tcsetattr(TCSANOW)` to restore the original termios.
/// 3. Delegates to the previous panic hook (preserves backtrace output).
///
/// If this function is called more than once, subsequent calls are no-ops (the
/// `OnceLock` guarantees exactly-once initialisation).
pub fn install_panic_hook(fd: RawFd, original: &Termios) {
    // Convert the nix `Termios` to a raw `libc::termios` so it can be stored
    // in a `Mutex` (which requires `Send + Sync`).  `nix::Termios` contains a
    // `RefCell` and is therefore not `Sync`.  We clone first because the
    // `From<Termios> for libc::termios` impl consumes its input.
    let raw_termios: nix::libc::termios = nix::libc::termios::from(original.clone());

    // OnceLock::set silently fails if already initialised — that is the
    // correct behaviour (only the first `install_panic_hook` call wins).
    let _ = PANIC_RESTORE_FD.set(fd);
    let _ = PANIC_ORIGINAL_TERMIOS.set(Mutex::new(raw_termios));

    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        // Restore the terminal first so the panic message is readable.
        restore_terminal_for_panic();
        // Then invoke the original hook (prints location + backtrace).
        previous(info);
    }));
}

// ── RAII guard ────────────────────────────────────────────────────────────────

/// RAII guard that restores raw terminal state on drop.
///
/// Construct via [`TerminalGuard::new`] after a successful [`enter_raw_mode`]
/// call.  When dropped — including on panic — the guard:
///
/// 1. Writes [`ALTERNATE_SCREEN_EXIT`] to stdout using a direct `write(2)`
///    syscall (no allocations, no tokio, no panics).
/// 2. Calls `tcsetattr(TCSANOW)` to restore the saved [`Termios`] state.
///
/// The `drop` implementation is intentionally infallible: errors from
/// `tcsetattr` or `write` are silently ignored because there is nothing
/// meaningful a destructor can do when terminal restoration fails, and
/// panicking inside `drop` is forbidden by the project guidelines.
pub struct TerminalGuard {
    fd: RawFd,
    original: Termios,
}

impl TerminalGuard {
    /// Construct a guard that will restore `original` on `fd` when dropped.
    ///
    /// `fd` must remain open for the lifetime of this guard.  In practice this
    /// is always `stdin` (fd 0), which is open for the entire process lifetime.
    ///
    /// # Example
    ///
    /// ```no_run
    /// use std::os::fd::AsRawFd;
    /// use teamucks::terminal::{enter_raw_mode, TerminalGuard};
    ///
    /// let fd = std::io::stdin().as_raw_fd();
    /// let original = enter_raw_mode(fd).expect("stdin must be a TTY");
    /// let _guard = TerminalGuard::new(fd, original);
    /// // Terminal is restored when `_guard` goes out of scope.
    /// ```
    #[must_use]
    pub fn new(fd: RawFd, original: Termios) -> Self {
        Self { fd, original }
    }

    /// Construct a guard with a zeroed [`Termios`] for use in tests.
    ///
    /// The guard's `drop` implementation will attempt `tcsetattr` on `fd` with
    /// a zeroed termios and write [`ALTERNATE_SCREEN_EXIT`] to stdout.  Both
    /// operations may silently fail (e.g. when `fd` is a pipe), which is the
    /// intended test behaviour — the point is that `drop` never panics.
    #[must_use]
    pub fn new_for_test(fd: RawFd) -> Self {
        // Build a dummy `Termios` from a zeroed `libc::termios`.
        //
        // SAFETY: A zeroed `libc::termios` is a valid bit pattern for the
        // struct — all fields are integer types with 0 being a defined value.
        // We use this only as a placeholder so that `drop` can exercise the
        // restoration path; `tcsetattr` on a non-TTY fd will fail and the
        // error will be silently ignored.
        let raw: nix::libc::termios = unsafe { std::mem::zeroed() };
        let original = Termios::from(raw);
        Self { fd, original }
    }
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        use nix::libc;

        // Write ALTERNATE_SCREEN_EXIT directly via the `write(2)` syscall.
        //
        // SAFETY: We bypass `std::io::Write` because `drop` must not allocate,
        // must not interact with the tokio runtime, and must not panic.
        // `ALTERNATE_SCREEN_EXIT` is `'static` so its pointer is always valid.
        // If stdout is closed, `write` returns -1 and we stop — which is the
        // correct destructor behaviour.
        write_all_raw(libc::STDOUT_FILENO, ALTERNATE_SCREEN_EXIT);

        // Restore the original terminal settings.
        //
        // SAFETY: `BorrowedFd::borrow_raw` requires the fd to be valid for the
        // duration of the call.  `self.fd` is the host terminal (stdin), which
        // remains open for the entire process lifetime; we never close it.
        let _ = nix::sys::termios::tcsetattr(
            unsafe { std::os::fd::BorrowedFd::borrow_raw(self.fd) },
            nix::sys::termios::SetArg::TCSANOW,
            &self.original,
        );
    }
}

// ── Private helpers ───────────────────────────────────────────────────────────

/// Write all bytes in `buf` to `fd` using raw `write(2)` syscalls.
///
/// Continues writing until all bytes are written or the syscall returns an
/// error / zero.  All errors are silently ignored — this function is used
/// exclusively in `drop` and panic hook paths where infallibility is required.
///
/// # Safety
///
/// `fd` must be a valid open file descriptor.  `buf` must be valid for reads
/// for its entire length.
fn write_all_raw(fd: nix::libc::c_int, buf: &[u8]) {
    use nix::libc;

    let mut written = 0usize;
    while written < buf.len() {
        // SAFETY: `buf.as_ptr().add(written)` is always within `buf` because
        // `written < buf.len()` is the loop invariant.
        let n = unsafe {
            libc::write(fd, buf.as_ptr().add(written).cast::<libc::c_void>(), buf.len() - written)
        };
        // `write` returns -1 on error or 0 on unexpected EOF.  In both cases
        // we stop rather than looping forever.
        let n_written = usize::try_from(n).unwrap_or(0);
        if n_written == 0 {
            break;
        }
        written += n_written;
    }
}

/// Restore the terminal in the panic hook using the globally stored state.
///
/// This is kept separate from `TerminalGuard::drop` so that the hook can
/// restore the terminal even when no `TerminalGuard` is alive (e.g. if the
/// guard was already dropped before the panic occurred, or if a panic happens
/// before the guard is constructed).
fn restore_terminal_for_panic() {
    use nix::libc;

    // Write ALTERNATE_SCREEN_EXIT unconditionally so the panic message is
    // visible outside the alternate screen buffer.
    write_all_raw(libc::STDOUT_FILENO, ALTERNATE_SCREEN_EXIT);

    // Restore the original termios if available.
    if let (Some(&fd), Some(termios_lock)) = (PANIC_RESTORE_FD.get(), PANIC_ORIGINAL_TERMIOS.get())
    {
        // `try_lock` avoids deadlock if the panic occurred while `install_panic_hook`
        // was writing to the mutex.  If the lock is poisoned or contended, we
        // skip restoration — the terminal is still usable (just in raw mode).
        if let Ok(raw) = termios_lock.try_lock() {
            // SAFETY: `BorrowedFd::borrow_raw` requires the fd to be valid for
            // the duration of the call.  `fd` is the host terminal (stdin),
            // open for the process lifetime, so this invariant holds.
            let borrowed = unsafe { std::os::fd::BorrowedFd::borrow_raw(fd) };
            let nix_termios = Termios::from(*raw);
            let _ = nix::sys::termios::tcsetattr(
                borrowed,
                nix::sys::termios::SetArg::TCSANOW,
                &nix_termios,
            );
        }
    }
}
