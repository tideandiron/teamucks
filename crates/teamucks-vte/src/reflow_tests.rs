/// Feature 12: Reflow tests.
///
/// These tests validate that `Grid::resize()` correctly reflows soft-wrapped
/// content, preserves hard-wrapped lines, tracks cursor position, handles wide
/// characters, integrates scrollback, and skips reflow on the alternate screen.
use crate::grid::Grid;
use crate::terminal::Terminal;

// ---------------------------------------------------------------------------
// Basic reflow
// ---------------------------------------------------------------------------

/// Write a line that fits in 80 cols, shrink to 40 (it wraps), widen back to
/// 80 (it must unwrap to the original single row).
#[test]
fn test_reflow_narrow_to_wide() {
    // 80-char line in an 80-col grid — no wrap.
    let line: String = "ABCDEFGH".repeat(10); // 80 chars exactly
    assert_eq!(line.len(), 80);

    let mut t = Terminal::new(80, 5);
    t.feed(line.as_bytes());
    let grid = t.grid_mut();

    // Shrink to 40 cols — the 80-char line should now occupy 2 rows.
    grid.resize(40, 5);
    assert_eq!(grid.row_text(0), &line[..40], "first half of line in row 0");
    assert_eq!(grid.row_text(1), &line[40..], "second half of line in row 1");
    // Row 0 must be soft-wrapped.
    assert!(grid.row(0).is_soft_wrapped(), "row 0 must be soft-wrapped at 40 cols");

    // Widen back to 80 — should reflow back to a single row.
    grid.resize(80, 5);
    assert_eq!(grid.row_text(0), line, "line should reflow back to row 0 at 80 cols");
    assert!(!grid.row(0).is_soft_wrapped(), "row 0 must NOT be soft-wrapped after widen");
}

/// Write an 80-char line, shrink to 40. The line should occupy exactly 2 rows.
#[test]
fn test_reflow_wide_to_narrow() {
    let line: String = (b'A'..=b'Z').cycle().take(80).map(|b| b as char).collect();
    assert_eq!(line.len(), 80);

    let mut t = Terminal::new(80, 5);
    t.feed(line.as_bytes());
    let grid = t.grid_mut();

    grid.resize(40, 5);

    // Two rows must contain the split content.
    assert_eq!(grid.row_text(0), &line[..40]);
    assert_eq!(grid.row_text(1), &line[40..]);
    assert!(grid.row(0).is_soft_wrapped());
    assert!(!grid.row(1).is_soft_wrapped());
}

/// Two distinct logical lines (hard-wrapped) must remain independent after
/// reflow — neither bleeds into the other.
#[test]
fn test_reflow_hard_wrap_preserved() {
    // Write "AAAA\r\nBBBB" — explicit CR+LF creates a hard wrap.
    let mut t = Terminal::new(10, 5);
    t.feed(b"AAAA\r\nBBBB");
    let grid = t.grid_mut();

    // Shrink to 5 cols and then grow back to 10 — the two lines must remain
    // separate because the hard-wrap flag is preserved.
    grid.resize(5, 5);
    grid.resize(10, 5);

    let rows0 = grid.row_text(0);
    let rows1 = grid.row_text(1);
    assert_eq!(rows0, "AAAA", "first hard-wrapped line preserved");
    assert_eq!(rows1, "BBBB", "second hard-wrapped line preserved");
    assert!(!grid.row(0).is_soft_wrapped(), "hard-wrap line 0 must NOT be soft-wrapped");
    assert!(!grid.row(1).is_soft_wrapped(), "hard-wrap line 1 must NOT be soft-wrapped");
}

/// Mix of soft- and hard-wrapped lines reflow independently.
#[test]
fn test_reflow_multiple_logical_lines() {
    // Write a long line (will be soft-wrapped at 5 cols) then a hard newline
    // and a short line.
    // "ABCDEFGH\r\nXY" on a 5-col grid:
    //   row 0: "ABCDE"  soft_wrapped=true
    //   row 1: "FGH  "  soft_wrapped=false (hard newline follows)
    //   row 2: "XY   "  soft_wrapped=false
    let mut t = Terminal::new(5, 5);
    t.feed(b"ABCDEFGH\r\nXY");
    let grid = t.grid_mut();

    // Verify initial state.
    assert!(grid.row(0).is_soft_wrapped(), "row 0 should be soft-wrapped initially");
    assert!(!grid.row(1).is_soft_wrapped());

    // Shrink to 4 cols: "ABCDEFGH" becomes 2 rows (ABCD, EFGH); "XY" stays 1.
    // Then widen to 8 cols: "ABCDEFGH" collapses back to 1 row; "XY" stays.
    grid.resize(4, 5);
    grid.resize(8, 5);

    assert_eq!(grid.row_text(0), "ABCDEFGH", "soft-wrapped line joins back");
    assert_eq!(grid.row_text(1), "XY", "hard-wrapped line stays separate");
}

// ---------------------------------------------------------------------------
// Cursor adjustment
// ---------------------------------------------------------------------------

/// After writing "hello", cursor is after 'o'. Shrink width so "hello" wraps.
/// Cursor must still be logically after 'o'.
#[test]
fn test_reflow_cursor_stays_on_same_char() {
    // Write "hello" in a 10-col grid — cursor lands at col 5 (just past 'o').
    let mut t = Terminal::new(10, 5);
    t.feed(b"hello");
    let grid = t.grid_mut();

    // Cursor should be at col 5, row 0 after writing "hello" (5 chars).
    assert_eq!(grid.cursor_col(), 5);
    assert_eq!(grid.cursor_row(), 0);

    // Shrink to 3 cols — "hello" wraps to:
    //   row 0: "hel"  (soft_wrapped)
    //   row 1: "lo"
    // Cursor should be at col 2, row 1 (after 'o' in "lo").
    grid.resize(3, 5);
    assert_eq!(grid.cursor_row(), 1, "cursor row after shrink");
    assert_eq!(grid.cursor_col(), 2, "cursor col after shrink");
}

/// Cursor at the very end of a long line. After shrink it should land at the
/// correct row and column.
#[test]
fn test_reflow_cursor_at_end_of_line() {
    // 6 chars in a 10-col grid, cursor ends at col 6.
    let mut t = Terminal::new(10, 5);
    t.feed(b"ABCDEF");
    let grid = t.grid_mut();

    assert_eq!(grid.cursor_col(), 6);
    assert_eq!(grid.cursor_row(), 0);

    // Shrink to 4 cols:
    //   row 0: "ABCD" (soft_wrapped)
    //   row 1: "EF"
    // Cursor is after 'F' → col 2, row 1.
    grid.resize(4, 5);
    assert_eq!(grid.cursor_row(), 1);
    assert_eq!(grid.cursor_col(), 2);
}

// ---------------------------------------------------------------------------
// Wide characters
// ---------------------------------------------------------------------------

/// A wide CJK character that would be split at a row boundary must move
/// entirely to the next row (the current cell gets a space placeholder).
#[test]
fn test_reflow_wide_char_at_boundary() {
    // '中' is width-2. Write 3 ASCII then '中' in a 5-col grid.
    // At 5 cols: "ABC中" — '中' fits at cols 3-4.
    // After resize to 4 cols: "ABC" + wide char doesn't fit (needs 2 cols
    // but col 3 is last), so:
    //   row 0: "ABC " (space placeholder, soft_wrapped)
    //   row 1: "中"   (wide char starts at col 0)
    let mut t = Terminal::new(5, 5);
    t.feed("ABC中".as_bytes());
    let grid = t.grid_mut();

    // Confirm initial state: row 0 has "ABC中" (5 cols: A,B,C,中,cont).
    assert_eq!(grid.cell(0, 0).grapheme(), "A");
    assert_eq!(grid.cell(1, 0).grapheme(), "B");
    assert_eq!(grid.cell(2, 0).grapheme(), "C");
    assert!(grid.cell(3, 0).is_wide(), "cell 3 should be wide");

    // Shrink to 4 cols: '中' at col 3-4 doesn't fit in 4-col row (col 3 is
    // the last col, no room for 2-wide char), so it wraps to next row.
    grid.resize(4, 5);

    // Row 0 should have A, B, C and a space (placeholder for wide char).
    assert_eq!(grid.cell(0, 0).grapheme(), "A");
    assert_eq!(grid.cell(1, 0).grapheme(), "B");
    assert_eq!(grid.cell(2, 0).grapheme(), "C");
    assert_eq!(grid.cell(3, 0).grapheme(), " ", "placeholder space for wide char");
    assert!(grid.row(0).is_soft_wrapped(), "row 0 soft-wrapped");

    // Row 1 should start with the wide char.
    assert!(grid.cell(0, 1).is_wide(), "wide char on row 1 col 0");
    assert!(grid.cell(1, 1).is_continuation(), "continuation on row 1 col 1");
}

/// CJK characters survive a round-trip reflow (shrink then expand).
#[test]
fn test_reflow_wide_char_preserved() {
    // Write two CJK chars (each width 2) in an 8-col grid: "日本" (4 cols total).
    let mut t = Terminal::new(8, 5);
    t.feed("日本".as_bytes());
    let grid = t.grid_mut();

    // Verify initial state.
    assert!(grid.cell(0, 0).is_wide());
    assert!(grid.cell(2, 0).is_wide());

    // Shrink to 4 cols — both chars still fit on one row (4 cols exactly).
    grid.resize(4, 5);
    assert!(grid.cell(0, 0).is_wide(), "日 wide after shrink");
    assert!(grid.cell(2, 0).is_wide(), "本 wide after shrink");

    // Grow back to 8 cols — still fits on one row.
    grid.resize(8, 5);
    assert!(grid.cell(0, 0).is_wide(), "日 wide after grow");
    assert!(grid.cell(2, 0).is_wide(), "本 wide after grow");
    assert_eq!(grid.row_text(0), "日本");
}

// ---------------------------------------------------------------------------
// Scrollback
// ---------------------------------------------------------------------------

/// Lines in scrollback reflow when the width changes.
#[test]
fn test_reflow_scrollback_included() {
    // 6-col grid, 3 rows. Write 4 lines so the first overflows into scrollback.
    let mut t = Terminal::new(6, 3);
    t.feed(b"AAAAAA\r\nBBBBBB\r\nCCCCCC\r\nDDDDDD");
    let grid = t.grid_mut();

    // Scrollback should have rows (the first line(s) scrolled off).
    assert!(grid.scrollback_len() > 0, "scrollback must have rows");
    let oldest_idx = grid.scrollback_len() - 1;
    let sb_text = grid.scrollback_text(oldest_idx).expect("oldest scrollback row");
    assert_eq!(sb_text, "AAAAAA", "oldest scrollback row is AAAAAA");

    // Shrink to 3 cols — each 6-char scrollback line should split into 2 rows.
    grid.resize(3, 3);

    // After reflow, the scrollback has more rows. Find the oldest two rows
    // (which are the first and second halves of "AAAAAA").
    let new_sb_len = grid.scrollback_len();
    let oldest = grid.scrollback_text(new_sb_len - 1).expect("oldest sb row after reflow");
    let second_oldest = grid.scrollback_text(new_sb_len - 2).expect("second oldest sb row");
    // "AAAAAA" split into "AAA" + "AAA".
    assert_eq!(oldest, "AAA", "first half of AAAAAA in oldest scrollback");
    assert_eq!(second_oldest, "AAA", "second half of AAAAAA");
}

/// After reflow, the split between scrollback and visible is at the correct
/// position: the last `new_rows` reflowed rows become visible, the rest go to
/// scrollback.
#[test]
fn test_reflow_scrollback_reflow_and_split() {
    // 4-col grid, 2 rows. Write 3 lines: the first 2 are visible, then a 3rd
    // pushes the first into scrollback.
    let mut t = Terminal::new(4, 2);
    t.feed(b"AAAA\r\nBBBB\r\nCCCC");
    let grid = t.grid_mut();

    // Visible: "BBBB", "CCCC". Scrollback: "AAAA".
    assert_eq!(grid.row_text(0), "BBBB");
    assert_eq!(grid.row_text(1), "CCCC");
    assert_eq!(grid.scrollback_len(), 1);

    // Shrink to 2 cols — reflow:
    // Logical lines in order (oldest first): "AAAA", "BBBB", "CCCC"
    // After rewrap to 2 cols each becomes 2 rows:
    //   "AA"(soft), "AA", "BB"(soft), "BB", "CC"(soft), "CC"  → 6 total
    // Last 2 become visible: "CC"(soft), "CC"
    // First 4 go to scrollback (oldest to newest):
    //   scrollback[3]="AA"(first half of AAAA), scrollback[2]="AA",
    //   scrollback[1]="BB"(first half of BBBB), scrollback[0]="BB"
    grid.resize(2, 2);

    assert_eq!(grid.row_text(0), "CC", "visible row 0 after reflow");
    assert_eq!(grid.row_text(1), "CC", "visible row 1 after reflow");
    assert_eq!(grid.scrollback_len(), 4, "4 rows in scrollback after reflow");
    // Most recent scrollback = second half of "BBBB".
    assert_eq!(grid.scrollback_text(0).as_deref(), Some("BB"), "sb[0] = second half of BBBB");
    assert_eq!(grid.scrollback_text(1).as_deref(), Some("BB"), "sb[1] = first half of BBBB");
    assert_eq!(grid.scrollback_text(2).as_deref(), Some("AA"), "sb[2] = second half of AAAA");
    assert_eq!(grid.scrollback_text(3).as_deref(), Some("AA"), "sb[3] = first half of AAAA");
}

// ---------------------------------------------------------------------------
// Alternate screen
// ---------------------------------------------------------------------------

/// When in the alternate screen, resize must NOT reflow — it must do a basic
/// resize only.  Full-screen apps will redraw via SIGWINCH.
#[test]
fn test_reflow_not_in_alt_screen() {
    // Enter alternate screen, write a long line, resize — content must NOT be
    // reflowed.
    let mut t = Terminal::new(10, 5);
    t.feed(b"\x1b[?1049h"); // enter alternate screen
    t.feed(b"ABCDEFGHIJ"); // 10 chars exactly fills the row
    let grid = t.grid_mut();

    assert!(grid.is_alternate_screen());

    // Shrink to 5 cols. No reflow — content is NOT re-wrapped.
    // The last 5 chars are truncated (basic resize).
    grid.resize(5, 5);

    assert_eq!(grid.cols(), 5);
    // Row text should be truncated to 5 chars.
    assert_eq!(grid.row_text(0), "ABCDE", "alt screen: content truncated, not reflowed");
    // The row must NOT be marked soft-wrapped (basic resize, not reflow).
    assert!(!grid.row(0).is_soft_wrapped(), "alt screen: no soft-wrap flag set");
}

// ---------------------------------------------------------------------------
// Edge cases
// ---------------------------------------------------------------------------

/// An empty grid (all blank cells) resizes cleanly without panicking.
#[test]
fn test_reflow_empty_grid() {
    let mut grid = Grid::new(80, 24);
    // No content written — all rows are blank.
    grid.resize(40, 24);
    assert_eq!(grid.cols(), 40);
    assert_eq!(grid.rows(), 24);

    grid.resize(80, 24);
    assert_eq!(grid.cols(), 80);
    assert_eq!(grid.rows(), 24);
}

/// A single-row grid reflows correctly.
#[test]
fn test_reflow_single_row() {
    let mut t = Terminal::new(10, 1);
    t.feed(b"ABCDE"); // 5 chars, cursor at col 5
    let grid = t.grid_mut();

    // Shrink to 3 cols.
    // "ABCDE" reflowed to 1-row visible (last 3-col chunk is shown):
    //   reflow produces: "ABC"(soft), "DE"(hard)
    //   Last 1 row becomes visible: "DE"
    //   "ABC" goes to scrollback.
    grid.resize(3, 1);

    assert_eq!(grid.cols(), 3);
    assert_eq!(grid.rows(), 1);
    // Visible should contain the last reflowed row.
    assert_eq!(grid.row_text(0), "DE", "last reflow chunk is visible");
}

/// When only height changes (same width), no reflow is needed — just add or
/// remove rows at the bottom without touching soft-wrap flags.
#[test]
fn test_reflow_only_height_change() {
    // Write a line that exactly fills row 0.
    let mut t = Terminal::new(10, 5);
    t.feed(b"ABCDEFGHIJ"); // exactly 10 chars — fills row 0
    let grid = t.grid_mut();

    // Row 0 is full. Confirm it is NOT soft-wrapped (cursor parked at wrap-pending).
    assert!(!grid.row(0).is_soft_wrapped());

    // Change height only — no reflow should occur.
    grid.resize(10, 8);
    assert_eq!(grid.cols(), 10);
    assert_eq!(grid.rows(), 8);
    // Row 0 content must be unchanged.
    assert_eq!(grid.row_text(0), "ABCDEFGHIJ");
    // No soft-wrap introduced.
    assert!(!grid.row(0).is_soft_wrapped());

    // Shrink height only — rows drop from bottom, content on row 0 preserved.
    grid.resize(10, 3);
    assert_eq!(grid.cols(), 10);
    assert_eq!(grid.rows(), 3);
    assert_eq!(grid.row_text(0), "ABCDEFGHIJ");
}

/// Shrink to 1 column — every cell gets its own row.
#[test]
fn test_reflow_to_one_column() {
    let mut t = Terminal::new(5, 5);
    t.feed(b"ABC");
    let grid = t.grid_mut();

    // Shrink to 1 col — each char gets its own row.
    // "ABC" reflowed to 1 col:
    //   "A"(soft), "B"(soft), "C"(hard)
    // Last 5 rows become visible (we have 5 rows visible, content needs 3).
    grid.resize(1, 5);

    assert_eq!(grid.cols(), 1);
    // The last reflowed rows (A, B, C) fit in the 5-row visible area.
    assert_eq!(grid.row_text(0), "A");
    assert_eq!(grid.row_text(1), "B");
    assert_eq!(grid.row_text(2), "C");
    // Rows with actual content must have correct soft-wrap.
    assert!(grid.row(0).is_soft_wrapped(), "A row is soft-wrapped");
    assert!(grid.row(1).is_soft_wrapped(), "B row is soft-wrapped");
    assert!(!grid.row(2).is_soft_wrapped(), "C row is NOT soft-wrapped (last in logical line)");
}
