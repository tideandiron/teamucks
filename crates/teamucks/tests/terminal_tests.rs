// Integration tests for terminal management.
//
// These tests verify the terminal management code without requiring a real TTY.
// They run in a test environment where stdin/stdout may not be a PTY.
//
// Tests that require an actual PTY are marked `#[ignore]` and must be run
// explicitly with `cargo test -- --ignored`.

use std::os::fd::AsRawFd;

use teamucks::terminal::{
    enter_raw_mode, query_terminal_size, TerminalGuard, ALTERNATE_SCREEN_ENTER,
    ALTERNATE_SCREEN_EXIT,
};

// ── Non-TTY error handling ────────────────────────────────────────────────────

/// Verifies that `enter_raw_mode` returns an error when called on a non-TTY fd.
#[test]
fn test_enter_raw_mode_on_non_tty_returns_error() {
    // Pipe fd — not a TTY; `tcgetattr` must return ENOTTY.
    let (read_fd, _write_fd) = nix::unistd::pipe().expect("pipe");
    let result = enter_raw_mode(read_fd.as_raw_fd());
    assert!(result.is_err(), "enter_raw_mode on a pipe must fail");
    // `read_fd` and `_write_fd` are `OwnedFd` — they close automatically.
}

// ── TerminalGuard infallible drop ─────────────────────────────────────────────

/// Verifies that `TerminalGuard` can be constructed and dropped without panicking.
///
/// Uses a pipe fd so that `tcsetattr` inside `drop` fails gracefully (it
/// returns an error which is silently ignored, as required by the design).
#[test]
fn test_terminal_guard_drop_does_not_panic() {
    // We cannot test actual terminal restoration without a real PTY.
    // This test verifies the guard compiles and its drop is infallible.
    let (read_fd, _write_fd) = nix::unistd::pipe().expect("pipe");
    // Construct with a fake termios; drop must not panic even on tcsetattr failure.
    let guard = TerminalGuard::new_for_test(read_fd.as_raw_fd());
    drop(guard); // must not panic
                 // `read_fd` and `_write_fd` close automatically via `OwnedFd::drop`.
}

/// Verifies that multiple `TerminalGuard`s can be dropped without panicking.
///
/// This exercises the case where multiple guards exist in nested scopes, which
/// can happen if startup fails partway through.
#[test]
fn test_terminal_guard_multiple_drops_do_not_panic() {
    let (fd_a, _write_a) = nix::unistd::pipe().expect("pipe a");
    let (fd_b, _write_b) = nix::unistd::pipe().expect("pipe b");

    {
        let _guard_a = TerminalGuard::new_for_test(fd_a.as_raw_fd());
        let _guard_b = TerminalGuard::new_for_test(fd_b.as_raw_fd());
        // Both guards drop here — inner first, then outer.
    }
    // `fd_a`, `_write_a`, `fd_b`, `_write_b` close automatically.
}

// ── Escape sequence constants ─────────────────────────────────────────────────

/// Verifies that the alternate screen enter sequence contains all required
/// sub-sequences in the correct order.
#[test]
fn test_alternate_screen_enter_bytes() {
    let enter = ALTERNATE_SCREEN_ENTER;

    // Required sequences (from design spec section 13):
    assert!(contains_subsequence(enter, b"\x1b[?1049h"), "must enter alt screen buffer");
    assert!(contains_subsequence(enter, b"\x1b[?25l"), "must hide cursor");
    assert!(contains_subsequence(enter, b"\x1b[?1000h"), "must enable X11 mouse reporting");
    assert!(contains_subsequence(enter, b"\x1b[?1006h"), "must enable SGR mouse mode");
    assert!(contains_subsequence(enter, b"\x1b[2J"), "must clear screen");
    assert!(contains_subsequence(enter, b"\x1b[H"), "must move cursor to home");
}

/// Verifies that the alternate screen enter sequence has the correct byte
/// content.
#[test]
fn test_alternate_screen_enter_exact_bytes() {
    assert_eq!(
        ALTERNATE_SCREEN_ENTER, b"\x1b[?1049h\x1b[?25l\x1b[?1000h\x1b[?1006h\x1b[2J\x1b[H",
        "enter sequence must match spec exactly"
    );
}

/// Verifies that the alternate screen exit sequence contains all required
/// sub-sequences.
#[test]
fn test_alternate_screen_exit_bytes() {
    let exit = ALTERNATE_SCREEN_EXIT;

    // Required sequences (from design spec section 13):
    assert!(contains_subsequence(exit, b"\x1b[?1049l"), "must exit alt screen buffer");
    assert!(contains_subsequence(exit, b"\x1b[?25h"), "must show cursor");
    assert!(contains_subsequence(exit, b"\x1b[?1000l"), "must disable X11 mouse reporting");
    assert!(contains_subsequence(exit, b"\x1b[?1006l"), "must disable SGR mouse mode");
    assert!(contains_subsequence(exit, b"\x1b[0m"), "must reset all attributes");
}

/// Verifies that the alternate screen exit sequence has the correct byte
/// content.
#[test]
fn test_alternate_screen_exit_exact_bytes() {
    assert_eq!(
        ALTERNATE_SCREEN_EXIT, b"\x1b[?1049l\x1b[?25h\x1b[?1000l\x1b[?1006l\x1b[0m",
        "exit sequence must match spec exactly"
    );
}

/// Verifies that enter and exit sequences do not share the same byte content
/// (a sanity check for accidental duplication).
#[test]
fn test_alternate_screen_enter_and_exit_are_distinct() {
    assert_ne!(
        ALTERNATE_SCREEN_ENTER, ALTERNATE_SCREEN_EXIT,
        "enter and exit sequences must be distinct"
    );
}

// ── Real-TTY tests (ignored by default) ──────────────────────────────────────

/// Verifies that `enter_raw_mode` succeeds on stdin when stdin is a real TTY.
///
/// This test must be run in a terminal emulator, not in CI.  Use:
/// ```shell
/// cargo test -- --ignored test_enter_raw_mode_on_real_tty_succeeds
/// ```
#[test]
#[ignore]
fn test_enter_raw_mode_on_real_tty_succeeds() {
    let fd = std::io::stdin().as_raw_fd();
    let result = enter_raw_mode(fd);
    assert!(result.is_ok(), "enter_raw_mode must succeed on a real TTY");

    // Restore immediately — we must not leave the test terminal in raw mode.
    if let Ok(original) = result {
        let _guard = TerminalGuard::new(fd, original);
        // guard drops here, restoring the terminal.
    }
}

/// Verifies that `query_terminal_size` returns non-zero dimensions on stdin
/// when stdin is a real TTY.
#[test]
#[ignore]
fn test_query_terminal_size_on_real_tty_returns_nonzero() {
    let fd = std::io::stdin().as_raw_fd();
    let result = query_terminal_size(fd);
    assert!(result.is_ok(), "query_terminal_size must succeed on a real TTY");
    let (cols, rows) = result.unwrap();
    assert!(cols > 0, "columns must be nonzero");
    assert!(rows > 0, "rows must be nonzero");
}

/// Verifies that `TerminalGuard` restores the terminal on drop when given a
/// real TTY.
#[test]
#[ignore]
fn test_terminal_guard_restores_on_real_tty() {
    use nix::sys::termios::tcgetattr;

    let fd = std::io::stdin().as_raw_fd();
    let original = enter_raw_mode(fd).expect("enter_raw_mode must succeed on real TTY");

    // Clone the input/output/local flags for post-drop comparison.
    let saved_input = original.input_flags;
    let saved_output = original.output_flags;
    let saved_local = original.local_flags;

    // Drop the guard, which should restore the termios.
    {
        let _guard = TerminalGuard::new(fd, original);
    }

    // Read termios after drop and verify it matches the saved original.
    let restored = tcgetattr(unsafe { std::os::fd::BorrowedFd::borrow_raw(fd) })
        .expect("tcgetattr must succeed after guard drop");

    assert_eq!(restored.input_flags, saved_input, "input flags must be restored");
    assert_eq!(restored.output_flags, saved_output, "output flags must be restored");
    assert_eq!(restored.local_flags, saved_local, "local flags must be restored");
}

// ── Helpers ───────────────────────────────────────────────────────────────────

/// Returns `true` if `haystack` contains `needle` as a contiguous subsequence.
fn contains_subsequence(haystack: &[u8], needle: &[u8]) -> bool {
    haystack.windows(needle.len()).any(|w| w == needle)
}
