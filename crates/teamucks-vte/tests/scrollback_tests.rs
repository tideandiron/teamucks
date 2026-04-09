//! Integration tests for the scrollback buffer.
//!
//! These tests verify that rows scrolling off the top of the primary screen
//! are captured in the scrollback buffer, and that the correct rows are
//! captured or skipped according to the design rules:
//!
//! - Full-screen scroll (region == full screen) → rows captured.
//! - Partial-region scroll → rows discarded (not captured).
//! - Alternate screen → rows discarded (not captured).
//!
//! Naming: `test_<unit>_<scenario>_<expected>`.

use teamucks_vte::terminal::Terminal;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Fill row `row` on terminal `t` with a repeated single character.
fn fill_row(t: &mut Terminal, row: usize, ch: char) {
    let cols = t.grid().cols();
    for col in 0..cols {
        t.grid_mut().cell_mut(col, row).set_grapheme_char(ch);
    }
}

// ---------------------------------------------------------------------------
// Basic scrollback capture via scroll_up_in_region
// ---------------------------------------------------------------------------

#[test]
fn test_scroll_up_appends_to_scrollback() {
    // 5-row grid; full-screen scroll region (default).  Fill row 0 with 'A'.
    // Scroll up 1 — row 0 should land in scrollback at index 0.
    let mut t = Terminal::new(10, 5);
    fill_row(&mut t, 0, 'A');

    // Trigger full-screen scroll via CSI S (scroll up 1).
    t.feed(b"\x1b[1S");

    assert_eq!(t.grid().scrollback_len(), 1);
    let text = t.grid().scrollback_text(0).expect("index 0 should exist");
    assert_eq!(text, "AAAAAAAAAA");
}

#[test]
fn test_scroll_up_multiple() {
    // 5-row grid; fill rows 0-2 distinctly then scroll up 3.
    let mut t = Terminal::new(5, 5);
    fill_row(&mut t, 0, 'X');
    fill_row(&mut t, 1, 'Y');
    fill_row(&mut t, 2, 'Z');

    // Scroll up 3 at once.
    t.feed(b"\x1b[3S");

    // 3 rows should be in scrollback.
    assert_eq!(t.grid().scrollback_len(), 3);
    // Most recent scrolled row is the last one drained (row 2, 'Z').
    // Order in scrollback: index 0 = most recent = Z, index 1 = Y, index 2 = X.
    assert_eq!(t.grid().scrollback_text(0).unwrap(), "ZZZZZ");
    assert_eq!(t.grid().scrollback_text(1).unwrap(), "YYYYY");
    assert_eq!(t.grid().scrollback_text(2).unwrap(), "XXXXX");
}

#[test]
fn test_scroll_region_does_not_append() {
    // Partial scroll region — scrolled-out rows must NOT enter scrollback.
    let mut t = Terminal::new(10, 10);
    fill_row(&mut t, 2, 'Q');

    // Set partial scroll region (rows 2..=6, 0-indexed) then scroll up within it.
    t.feed(b"\x1b[3;7r"); // 1-indexed 3;7 → 0-indexed 2..=6
    t.feed(b"\x1b[1S");

    // Scrollback must be empty.
    assert_eq!(t.grid().scrollback_len(), 0);
}

#[test]
fn test_alternate_screen_no_scrollback() {
    // While on the alternate screen, scrolling must NOT push rows to scrollback.
    let mut t = Terminal::new(10, 5);

    // Enter alternate screen.
    t.feed(b"\x1b[?1049h");
    fill_row(&mut t, 0, 'B');

    // Scroll up on the alternate screen.
    t.feed(b"\x1b[1S");

    // Exit alternate screen.
    t.feed(b"\x1b[?1049l");

    // Scrollback on the primary screen must still be empty.
    assert_eq!(t.grid().scrollback_len(), 0);
}

// ---------------------------------------------------------------------------
// LF triggering scrollback at screen bottom
// ---------------------------------------------------------------------------

#[test]
fn test_lf_at_bottom_appends() {
    // 5-row grid; fill row 0 with 'M'. Move cursor to last row and send LF.
    // Row 0 should land in scrollback.
    let mut t = Terminal::new(10, 5);
    fill_row(&mut t, 0, 'M');

    // Move cursor to last row (row 4, 1-indexed 5).
    t.feed(b"\x1b[5;1H");
    // LF scrolls the full screen up.
    t.feed(b"\x0A");

    assert_eq!(t.grid().scrollback_len(), 1);
    assert_eq!(t.grid().scrollback_text(0).unwrap(), "MMMMMMMMMM");
}

// ---------------------------------------------------------------------------
// Content fidelity
// ---------------------------------------------------------------------------

#[test]
fn test_scrollback_content_preserved() {
    // Write "hello" to row 0 via VTE, then scroll it off.
    let mut t = Terminal::new(80, 5);
    // Position cursor at (0,0) and type "hello".
    t.feed(b"\x1b[1;1H");
    t.feed(b"hello");
    // Move cursor to last row and LF to trigger scroll.
    t.feed(b"\x1b[5;1H");
    t.feed(b"\x0A");

    assert_eq!(t.grid().scrollback_len(), 1);
    assert_eq!(t.grid().scrollback_text(0).unwrap(), "hello");
}

#[test]
fn test_scrollback_wide_chars() {
    // Write a wide (CJK) character to row 0, then scroll it off.
    // The wide character and its continuation cell must survive in scrollback.
    let mut t = Terminal::new(10, 5);
    t.feed(b"\x1b[1;1H");
    // U+4E2D (中) — 3-byte UTF-8, width 2.
    t.feed("\u{4E2D}".as_bytes());
    // Move to last row and LF.
    t.feed(b"\x1b[5;1H");
    t.feed(b"\x0A");

    assert_eq!(t.grid().scrollback_len(), 1);
    let row = t.grid().scrollback().get(0).expect("row should exist");
    // The first cell should be wide.
    assert!(row.cell(0).is_wide(), "first cell of wide char must be wide");
    // The second cell must be a continuation.
    assert!(row.cell(1).is_continuation(), "second cell must be continuation");
}

// ---------------------------------------------------------------------------
// Default capacity
// ---------------------------------------------------------------------------

#[test]
fn test_scrollback_default_capacity_from_grid() {
    // The Grid's scrollback buffer must have the default max of 10 000 lines.
    let t = Terminal::new(80, 24);
    assert_eq!(t.grid().scrollback().max_lines(), 10_000);
}

// ---------------------------------------------------------------------------
// Scrollback overflow via Terminal accessor
// ---------------------------------------------------------------------------

#[test]
fn test_scrollback_overflow_via_terminal() {
    // Use a tiny scrollback capacity: create grid with max 3, scroll 4 rows.
    // We access via the Terminal's scrollback() accessor.
    let mut t = Terminal::new(5, 5);
    // We can set max_lines on the scrollback via the Grid accessor.
    t.grid_mut().scrollback_mut().set_max_lines(3);

    // Write distinct content to 4 rows, then scroll all of them off.
    for i in 0..4_usize {
        let ch = char::from_digit(i as u32, 10).unwrap();
        fill_row(&mut t, i.min(4), ch);
    }
    // Scroll up 4 rows (all of the 5-row grid, clamped to height).
    t.feed(b"\x1b[4S");

    // Only 3 rows should be kept (oldest dropped).
    assert_eq!(t.scrollback().len(), 3);
}

// ---------------------------------------------------------------------------
// Terminal-level scrollback accessor
// ---------------------------------------------------------------------------

#[test]
fn test_terminal_scrollback_accessor() {
    // Terminal::scrollback() returns the same data as Grid::scrollback().
    let mut t = Terminal::new(10, 5);
    fill_row(&mut t, 0, 'T');
    t.feed(b"\x1b[5;1H");
    t.feed(b"\x0A");

    // Both accessors must agree.
    assert_eq!(t.scrollback().len(), t.grid().scrollback_len());
    assert_eq!(t.scrollback().len(), 1);
}
