//! Cursor movement CSI sequence integration tests for [`Terminal`].
//!
//! Tests are organized by sequence category: CUP, CUU, CUD, CUF, CUB, CNL,
//! CPL, CHA, VPA, HVP, DECSC/DECRC, wrap_pending interactions, and
//! integration scenarios.
//!
//! Every test follows the pattern: feed a byte sequence to a fresh
//! [`Terminal`], then inspect the grid cursor position for expected state.

use teamucks_vte::{
    style::{Attr, Color},
    terminal::Terminal,
};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Create a terminal with the given dimensions and feed `input`.
fn term_wh(cols: usize, rows: usize, input: &[u8]) -> Terminal {
    let mut t = Terminal::new(cols, rows);
    t.feed(input);
    t
}

/// Create an 80×24 terminal and feed `input`.
fn term(input: &[u8]) -> Terminal {
    term_wh(80, 24, input)
}

// ---------------------------------------------------------------------------
// CUP — Cursor Position (CSI row ; col H)
// ---------------------------------------------------------------------------

#[test]
fn test_cup_absolute_position() {
    // "\x1b[5;10H" → row 5, col 10 (1-indexed) → row 4, col 9 (0-indexed)
    let t = term(b"\x1b[5;10H");
    assert_eq!(t.grid().cursor_col(), 9, "col should be 9 (0-indexed from 1-indexed 10)");
    assert_eq!(t.grid().cursor_row(), 4, "row should be 4 (0-indexed from 1-indexed 5)");
}

#[test]
fn test_cup_default_home() {
    // "\x1b[H" with no params → (0, 0)
    let mut t = Terminal::new(80, 24);
    // Move cursor away first
    t.feed(b"\x1b[5;10H");
    t.feed(b"\x1b[H");
    assert_eq!(t.grid().cursor_col(), 0);
    assert_eq!(t.grid().cursor_row(), 0);
}

#[test]
fn test_cup_default_params() {
    // "\x1b[;H" — both params omitted/zero → (0, 0)
    let t = term(b"\x1b[;H");
    assert_eq!(t.grid().cursor_col(), 0);
    assert_eq!(t.grid().cursor_row(), 0);
}

#[test]
fn test_cup_row_only() {
    // "\x1b[5H" — only row given, col defaults to 1 → col 0, row 4
    let t = term(b"\x1b[5H");
    assert_eq!(t.grid().cursor_col(), 0, "col defaults to 0 when omitted");
    assert_eq!(t.grid().cursor_row(), 4);
}

#[test]
fn test_cup_clamps_to_bounds() {
    // 80×24 grid; "\x1b[999;999H" → clamped to (79, 23)
    let t = term(b"\x1b[999;999H");
    assert_eq!(t.grid().cursor_col(), 79);
    assert_eq!(t.grid().cursor_row(), 23);
}

#[test]
fn test_cup_zero_params() {
    // "\x1b[0;0H" — 0 is treated as default 1 → (0, 0)
    let t = term(b"\x1b[0;0H");
    assert_eq!(t.grid().cursor_col(), 0);
    assert_eq!(t.grid().cursor_row(), 0);
}

// ---------------------------------------------------------------------------
// CUU — Cursor Up (CSI n A)
// ---------------------------------------------------------------------------

#[test]
fn test_cuu_cursor_up() {
    // Position at row 5, then CUU 3 → row 2
    let t = term(b"\x1b[6;1H\x1b[3A");
    assert_eq!(t.grid().cursor_row(), 2, "row 5 - 3 = 2 (0-indexed)");
    assert_eq!(t.grid().cursor_col(), 0);
}

#[test]
fn test_cuu_default() {
    // "\x1b[A" with no param defaults to 1
    let t = term(b"\x1b[5;1H\x1b[A");
    assert_eq!(t.grid().cursor_row(), 3, "row 4 - 1 = 3 (0-indexed)");
}

#[test]
fn test_cuu_clamps_at_top() {
    // Cursor at row 0; CUU 5 → stays at row 0
    let t = term(b"\x1b[1;1H\x1b[5A");
    assert_eq!(t.grid().cursor_row(), 0, "cannot move above row 0");
}

#[test]
fn test_cuu_zero_means_one() {
    // "\x1b[0A" — 0 is treated as default 1
    let t = term(b"\x1b[5;1H\x1b[0A");
    assert_eq!(t.grid().cursor_row(), 3, "0 treated as 1, row 4 - 1 = 3");
}

// ---------------------------------------------------------------------------
// CUD — Cursor Down (CSI n B)
// ---------------------------------------------------------------------------

#[test]
fn test_cud_cursor_down() {
    // Position at row 2, then CUD 3 → row 5
    let t = term(b"\x1b[3;1H\x1b[3B");
    assert_eq!(t.grid().cursor_row(), 5, "row 2 + 3 = 5 (0-indexed)");
}

#[test]
fn test_cud_clamps_at_bottom() {
    // Cursor at last row (23 in 24-row grid); CUD 5 → stays at row 23
    let t = term(b"\x1b[24;1H\x1b[5B");
    assert_eq!(t.grid().cursor_row(), 23, "cannot move below last row");
}

// ---------------------------------------------------------------------------
// CUF — Cursor Forward (CSI n C)
// ---------------------------------------------------------------------------

#[test]
fn test_cuf_cursor_forward() {
    // Position at col 5, then CUF 3 → col 8
    let t = term(b"\x1b[1;6H\x1b[3C");
    assert_eq!(t.grid().cursor_col(), 8, "col 5 + 3 = 8 (0-indexed)");
}

#[test]
fn test_cuf_clamps_at_right() {
    // Cursor at last col (79 in 80-col grid); CUF 5 → stays at col 79
    let t = term(b"\x1b[1;80H\x1b[5C");
    assert_eq!(t.grid().cursor_col(), 79, "cannot move past last column");
}

// ---------------------------------------------------------------------------
// CUB — Cursor Back (CSI n D)
// ---------------------------------------------------------------------------

#[test]
fn test_cub_cursor_back() {
    // Position at col 10, then CUB 3 → col 7
    let t = term(b"\x1b[1;11H\x1b[3D");
    assert_eq!(t.grid().cursor_col(), 7, "col 10 - 3 = 7 (0-indexed)");
}

#[test]
fn test_cub_clamps_at_left() {
    // Cursor at col 0; CUB 5 → stays at col 0
    let t = term(b"\x1b[1;1H\x1b[5D");
    assert_eq!(t.grid().cursor_col(), 0, "cannot move left of col 0");
}

// ---------------------------------------------------------------------------
// CNL — Cursor Next Line (CSI n E)
// ---------------------------------------------------------------------------

#[test]
fn test_cnl_moves_down_and_to_col_zero() {
    // Position at (col 5, row 3), then CNL 2 → (col 0, row 5)
    let t = term(b"\x1b[4;6H\x1b[2E");
    assert_eq!(t.grid().cursor_col(), 0, "CNL resets column to 0");
    assert_eq!(t.grid().cursor_row(), 5, "row 3 + 2 = 5 (0-indexed)");
}

// ---------------------------------------------------------------------------
// CPL — Cursor Previous Line (CSI n F)
// ---------------------------------------------------------------------------

#[test]
fn test_cpl_moves_up_and_to_col_zero() {
    // Position at (col 5, row 5), then CPL 2 → (col 0, row 3)
    let t = term(b"\x1b[6;6H\x1b[2F");
    assert_eq!(t.grid().cursor_col(), 0, "CPL resets column to 0");
    assert_eq!(t.grid().cursor_row(), 3, "row 5 - 2 = 3 (0-indexed)");
}

// ---------------------------------------------------------------------------
// CHA — Cursor Horizontal Absolute (CSI n G)
// ---------------------------------------------------------------------------

#[test]
fn test_cha_sets_column() {
    // "\x1b[10G" — 1-indexed col 10 → 0-indexed col 9
    let t = term(b"\x1b[10G");
    assert_eq!(t.grid().cursor_col(), 9, "1-indexed 10 → 0-indexed 9");
}

#[test]
fn test_cha_default() {
    // "\x1b[G" — default param is 1 → col 0
    let t = term(b"\x1b[5;5H\x1b[G");
    assert_eq!(t.grid().cursor_col(), 0, "default param 1 → col 0");
}

#[test]
fn test_cha_clamps() {
    // "\x1b[999G" on 80-col grid → col 79
    let t = term(b"\x1b[999G");
    assert_eq!(t.grid().cursor_col(), 79, "clamped to last column");
}

// ---------------------------------------------------------------------------
// VPA — Vertical Position Absolute (CSI n d)
// ---------------------------------------------------------------------------

#[test]
fn test_vpa_sets_row() {
    // "\x1b[5d" — 1-indexed row 5 → 0-indexed row 4
    let t = term(b"\x1b[5d");
    assert_eq!(t.grid().cursor_row(), 4, "1-indexed 5 → 0-indexed 4");
}

#[test]
fn test_vpa_default() {
    // "\x1b[d" — default param is 1 → row 0
    let t = term(b"\x1b[5;5H\x1b[d");
    assert_eq!(t.grid().cursor_row(), 0, "default param 1 → row 0");
}

#[test]
fn test_vpa_clamps() {
    // "\x1b[999d" on 24-row grid → row 23
    let t = term(b"\x1b[999d");
    assert_eq!(t.grid().cursor_row(), 23, "clamped to last row");
}

// ---------------------------------------------------------------------------
// HVP — Horizontal Vertical Position (CSI row ; col f)  — same as CUP
// ---------------------------------------------------------------------------

#[test]
fn test_hvp_same_as_cup() {
    // "\x1b[5;10f" should behave identically to "\x1b[5;10H"
    let cup = term(b"\x1b[5;10H");
    let hvp = term(b"\x1b[5;10f");
    assert_eq!(hvp.grid().cursor_col(), cup.grid().cursor_col());
    assert_eq!(hvp.grid().cursor_row(), cup.grid().cursor_row());
}

// ---------------------------------------------------------------------------
// DECSC / DECRC — ESC 7 / ESC 8
// ---------------------------------------------------------------------------

#[test]
fn test_decsc_decrc_position() {
    // Save cursor at (col 9, row 4), move to (0, 0), restore back to (9, 4)
    let t = term(b"\x1b[5;10H\x1b7\x1b[H\x1b8");
    assert_eq!(t.grid().cursor_col(), 9, "restored col should be 9");
    assert_eq!(t.grid().cursor_row(), 4, "restored row should be 4");
}

#[test]
fn test_decsc_saves_style() {
    // Save with bold+red foreground, change to italic, restore, verify bold+red
    let mut t = Terminal::new(80, 24);
    // Set bold + red foreground
    t.feed(b"\x1b[1;31m");
    // Save cursor (captures style)
    t.feed(b"\x1b7");
    // Change to italic only, clearing bold and resetting color
    t.feed(b"\x1b[0;3m");
    // Restore
    t.feed(b"\x1b8");

    let style = t.grid().cursor().style();
    assert!(style.attrs().contains(Attr::BOLD), "bold should be restored");
    assert!(!style.attrs().contains(Attr::ITALIC), "italic should not be present after restore");
    // Foreground should be red. Named(1) is stored and decoded as Indexed(1)
    // because PackedStyle stores Named and Indexed identically (same palette).
    assert_eq!(style.foreground(), Color::Indexed(1), "red foreground should be restored");
}

#[test]
fn test_decrc_without_save_is_noop() {
    // ESC 8 without a prior ESC 7 should leave cursor unchanged (at 0, 0)
    let t = term(b"\x1b8");
    assert_eq!(t.grid().cursor_col(), 0);
    assert_eq!(t.grid().cursor_row(), 0);
}

// ---------------------------------------------------------------------------
// Cursor movement clears wrap_pending
// ---------------------------------------------------------------------------

#[test]
fn test_cup_clears_wrap_pending() {
    // Write enough characters to trigger wrap_pending on the last column,
    // then use CUP — wrap_pending must be cleared.
    let mut t = Terminal::new(80, 24);
    // Fill row 0 completely to the last column (sets wrap_pending)
    let line: Vec<u8> = b"A".repeat(80).to_vec();
    t.feed(&line);
    // Cursor should be at last col with wrap_pending set
    assert!(t.grid().cursor().wrap_pending(), "wrap_pending should be set after filling row");
    // Issue CUP to move cursor
    t.feed(b"\x1b[1;1H");
    assert!(!t.grid().cursor().wrap_pending(), "CUP must clear wrap_pending");
    assert_eq!(t.grid().cursor_col(), 0);
    assert_eq!(t.grid().cursor_row(), 0);
}

#[test]
fn test_cuu_clears_wrap_pending() {
    // Same pattern with CUU
    let mut t = Terminal::new(80, 24);
    t.feed(b"\x1b[5;1H");
    let line: Vec<u8> = b"A".repeat(80).to_vec();
    t.feed(&line);
    assert!(t.grid().cursor().wrap_pending(), "wrap_pending should be set after filling row");
    t.feed(b"\x1b[2A");
    assert!(!t.grid().cursor().wrap_pending(), "CUU must clear wrap_pending");
}

// ---------------------------------------------------------------------------
// Integration — cursor movement with text
// ---------------------------------------------------------------------------

#[test]
fn test_cursor_movement_with_text() {
    // Write "Hello" at (0,0), move to (10, 2), write "World"
    let mut t = Terminal::new(80, 24);
    t.feed(b"Hello");
    t.feed(b"\x1b[3;11H");
    t.feed(b"World");

    // Row 0 should have "Hello" at cols 0-4
    assert_eq!(t.grid().cell(0, 0).grapheme(), "H");
    assert_eq!(t.grid().cell(4, 0).grapheme(), "o");

    // Row 2 should have "World" at cols 10-14
    assert_eq!(t.grid().cell(10, 2).grapheme(), "W");
    assert_eq!(t.grid().cell(14, 2).grapheme(), "d");

    // Cursor should be after "World" at col 15, row 2
    assert_eq!(t.grid().cursor_col(), 15);
    assert_eq!(t.grid().cursor_row(), 2);
}

#[test]
fn test_cuu_from_middle_of_grid() {
    // Move to row 10, col 20, then CUU 5 → row 5, col 20
    let t = term(b"\x1b[11;21H\x1b[5A");
    assert_eq!(t.grid().cursor_row(), 5, "row 10 - 5 = 5");
    assert_eq!(t.grid().cursor_col(), 20, "column unchanged by CUU");
}

#[test]
fn test_cud_from_middle_of_grid() {
    // Move to row 5, col 10, then CUD 7 → row 12, col 10
    let t = term(b"\x1b[6;11H\x1b[7B");
    assert_eq!(t.grid().cursor_row(), 12, "row 5 + 7 = 12");
    assert_eq!(t.grid().cursor_col(), 10, "column unchanged by CUD");
}

#[test]
fn test_cnl_clamps_at_bottom() {
    // CNL beyond last row should clamp at last row (row 23 for 24-row grid)
    let t = term(b"\x1b[22;5H\x1b[5E");
    assert_eq!(t.grid().cursor_row(), 23, "clamped at last row");
    assert_eq!(t.grid().cursor_col(), 0, "CNL resets col to 0");
}

#[test]
fn test_cpl_clamps_at_top() {
    // CPL beyond first row should clamp at row 0
    let t = term(b"\x1b[3;5H\x1b[10F");
    assert_eq!(t.grid().cursor_row(), 0, "clamped at row 0");
    assert_eq!(t.grid().cursor_col(), 0, "CPL resets col to 0");
}

#[test]
fn test_cha_zero_treated_as_default() {
    // "\x1b[0G" — 0 treated as default 1 → col 0
    let t = term(b"\x1b[1;10H\x1b[0G");
    assert_eq!(t.grid().cursor_col(), 0, "0 treated as default 1 → col 0");
}

#[test]
fn test_vpa_zero_treated_as_default() {
    // "\x1b[0d" — 0 treated as default 1 → row 0
    let t = term(b"\x1b[10;1H\x1b[0d");
    assert_eq!(t.grid().cursor_row(), 0, "0 treated as default 1 → row 0");
}
