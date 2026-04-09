//! Erase operation CSI sequence integration tests for [`Terminal`].
//!
//! Tests are organised by sequence category: ED (Erase in Display), EL (Erase
//! in Line), ECH (Erase Characters), wide-character boundary interactions, and
//! default-style verification.
//!
//! Every test feeds byte sequences into a fresh [`Terminal`] and inspects the
//! resulting grid state.  No implementation of the erase sequences exists
//! before these tests are written; they are written first and drive the
//! implementation (TDD).

use teamucks_vte::{
    style::{Attr, PackedStyle},
    terminal::Terminal,
};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Create a terminal with the given dimensions and feed `input`.
#[allow(dead_code)]
fn term_wh(cols: usize, rows: usize, input: &[u8]) -> Terminal {
    let mut t = Terminal::new(cols, rows);
    t.feed(input);
    t
}

/// Create an 80×24 terminal and feed `input`.
#[allow(dead_code)]
fn term(input: &[u8]) -> Terminal {
    term_wh(80, 24, input)
}

/// Assert that every cell in the range `col_start..=col_end` on `row` is a
/// default cell: space grapheme, default style, no wide/continuation flags.
fn assert_cells_default(t: &Terminal, row: usize, col_start: usize, col_end: usize) {
    for col in col_start..=col_end {
        let cell = t.grid().cell(col, row);
        assert_eq!(
            cell.grapheme(),
            " ",
            "cell ({col}, {row}) grapheme should be space, got {:?}",
            cell.grapheme()
        );
        assert_eq!(
            *cell.style(),
            PackedStyle::default(),
            "cell ({col}, {row}) style should be default"
        );
        assert!(!cell.is_wide(), "cell ({col}, {row}) should not be wide");
        assert!(!cell.is_continuation(), "cell ({col}, {row}) should not be continuation");
    }
}

/// Assert that every cell in `row` is a default cell.
fn assert_row_default(t: &Terminal, row: usize) {
    let cols = t.grid().cols();
    assert_cells_default(t, row, 0, cols - 1);
}

// ---------------------------------------------------------------------------
// ED — Erase in Display (CSI J)
// ---------------------------------------------------------------------------

/// ED 0: erase from cursor to end of current line, then all rows below.
#[test]
fn test_ed_0_erase_below() {
    // Fill the screen with 'A', move cursor to row 2 col 5, then ED 0.
    // Rows 0 and 1 should be fully intact.
    // Row 2 from col 0..=4 intact; col 5 to end erased.
    // Rows 3..23 fully erased.
    let mut t = Terminal::new(10, 5);
    // Write 'A' to every cell by filling each row.
    for _ in 0..5 {
        t.feed(b"AAAAAAAAAA\r\n");
    }
    // Cursor is now past the last row (scroll happened); reset to (0,0) via CUP.
    t.feed(b"\x1b[H"); // CUP 1;1 → (row=0, col=0)
                       // Fill again cleanly: 5 rows × 10 cols of 'A'.
                       // (After CUP the cursor is at 0,0; we write 50 'A's across all rows.)
                       // Actually use a more direct approach: build the full fill+erase sequence.

    let mut t = Terminal::new(10, 5);
    // Fill row by row using explicit cursor positioning.
    for r in 0..5u8 {
        // CUP: 1-indexed
        let cup = format!("\x1b[{};1H", r + 1);
        t.feed(cup.as_bytes());
        t.feed(b"AAAAAAAAAA");
    }
    // Move cursor to row 2 (0-indexed), col 5 (0-indexed) → CSI 3;6H (1-indexed)
    t.feed(b"\x1b[3;6H");
    // ED 0: erase from cursor to end of screen
    t.feed(b"\x1b[J");

    // Rows 0..=1: completely intact ('A' in every cell).
    for row in 0..=1 {
        for col in 0..10 {
            assert_eq!(t.grid().cell(col, row).grapheme(), "A", "row {row} col {col} should be A");
        }
    }
    // Row 2: cols 0..=4 intact, cols 5..=9 erased to default.
    for col in 0..=4 {
        assert_eq!(t.grid().cell(col, 2).grapheme(), "A", "row 2 col {col} should be A");
    }
    assert_cells_default(&t, 2, 5, 9);
    // Rows 3..=4: fully erased.
    for row in 3..=4 {
        assert_row_default(&t, row);
    }
}

/// ED 0: the portion of the current line from cursor to end is cleared.
#[test]
fn test_ed_0_cursor_to_end_of_line() {
    let mut t = Terminal::new(10, 3);
    // Fill row 0 with 'B'.
    t.feed(b"\x1b[1;1H");
    t.feed(b"BBBBBBBBBB");
    // Move cursor to col 3 (1-indexed: 4), row 0 (1-indexed: 1).
    t.feed(b"\x1b[1;4H");
    // ED 0
    t.feed(b"\x1b[J");

    // Cols 0..=2 intact ('B'), cols 3..=9 erased.
    for col in 0..=2 {
        assert_eq!(t.grid().cell(col, 0).grapheme(), "B", "col {col} should be B");
    }
    assert_cells_default(&t, 0, 3, 9);
}

/// ED 1: erase from start of screen to cursor (all rows above, plus start of
/// cursor row up to and including the cursor column).
#[test]
fn test_ed_1_erase_above() {
    let mut t = Terminal::new(10, 5);
    // Fill all rows with 'C'.
    for r in 0..5u8 {
        let cup = format!("\x1b[{};1H", r + 1);
        t.feed(cup.as_bytes());
        t.feed(b"CCCCCCCCCC");
    }
    // Move cursor to row 2 (0-indexed), col 5 (0-indexed) → CSI 3;6H
    t.feed(b"\x1b[3;6H");
    // ED 1
    t.feed(b"\x1b[1J");

    // Rows 0..=1: fully erased.
    for row in 0..=1 {
        assert_row_default(&t, row);
    }
    // Row 2: cols 0..=5 erased, cols 6..=9 intact ('C').
    assert_cells_default(&t, 2, 0, 5);
    for col in 6..=9 {
        assert_eq!(t.grid().cell(col, 2).grapheme(), "C", "row 2 col {col} should be C");
    }
    // Rows 3..=4: intact ('C').
    for row in 3..=4 {
        for col in 0..10 {
            assert_eq!(t.grid().cell(col, row).grapheme(), "C", "row {row} col {col} should be C");
        }
    }
}

/// ED 2: erase entire visible screen.
#[test]
fn test_ed_2_erase_all() {
    let mut t = Terminal::new(10, 5);
    // Fill every cell with 'D'.
    for r in 0..5u8 {
        let cup = format!("\x1b[{};1H", r + 1);
        t.feed(cup.as_bytes());
        t.feed(b"DDDDDDDDDD");
    }
    // ED 2
    t.feed(b"\x1b[2J");

    for row in 0..5 {
        assert_row_default(&t, row);
    }
}

/// ED with no parameter is identical to ED 0.
#[test]
fn test_ed_default_is_0() {
    // Two identical terminals: one uses "\x1b[0J", the other "\x1b[J".
    let mut t0 = Terminal::new(10, 3);
    let mut t1 = Terminal::new(10, 3);

    let setup = b"\x1b[1;1HEEEEEEEEEE\x1b[2;1HEEEEEEEEEE\x1b[3;1HEEEEEEEEEE";
    let move_cursor = b"\x1b[2;5H"; // row 2, col 5 (1-indexed) = row 1, col 4 (0-indexed)
    t0.feed(setup);
    t0.feed(move_cursor);
    t0.feed(b"\x1b[0J");

    t1.feed(setup);
    t1.feed(move_cursor);
    t1.feed(b"\x1b[J");

    // Both terminals should have identical grid state.
    for row in 0..3 {
        for col in 0..10 {
            assert_eq!(
                t0.grid().cell(col, row).grapheme(),
                t1.grid().cell(col, row).grapheme(),
                "mismatch at ({col}, {row})"
            );
        }
    }
}

/// Cursor position is unchanged after ED.
#[test]
fn test_ed_preserves_cursor() {
    let mut t = Terminal::new(10, 5);
    // Move to row 2, col 3 (0-indexed) → CSI 3;4H (1-indexed)
    t.feed(b"\x1b[3;4H");
    t.feed(b"\x1b[J"); // ED 0
    assert_eq!(t.grid().cursor_col(), 3, "cursor col should be unchanged");
    assert_eq!(t.grid().cursor_row(), 2, "cursor row should be unchanged");

    t.feed(b"\x1b[1J"); // ED 1
    assert_eq!(t.grid().cursor_col(), 3);
    assert_eq!(t.grid().cursor_row(), 2);

    t.feed(b"\x1b[2J"); // ED 2
    assert_eq!(t.grid().cursor_col(), 3);
    assert_eq!(t.grid().cursor_row(), 2);
}

/// ED 3 (scrollback erase) is a no-op for the visible grid in Phase 1.
#[test]
fn test_ed_3_is_noop() {
    let mut t = Terminal::new(10, 3);
    // Fill screen with 'F'.
    for r in 0..3u8 {
        let cup = format!("\x1b[{};1H", r + 1);
        t.feed(cup.as_bytes());
        t.feed(b"FFFFFFFFFF");
    }
    t.feed(b"\x1b[3J");
    // Visible grid must be unchanged.
    for row in 0..3 {
        for col in 0..10 {
            assert_eq!(
                t.grid().cell(col, row).grapheme(),
                "F",
                "row {row} col {col} should be F (ED 3 is no-op)"
            );
        }
    }
}

// ---------------------------------------------------------------------------
// EL — Erase in Line (CSI K)
// ---------------------------------------------------------------------------

/// EL 0: erase from cursor to end of line.
#[test]
fn test_el_0_erase_to_right() {
    let mut t = Terminal::new(10, 3);
    t.feed(b"\x1b[1;1H");
    t.feed(b"GGGGGGGGGG"); // row 0 full of 'G'
                           // Move cursor to col 4 (0-indexed) → CSI 1;5H
    t.feed(b"\x1b[1;5H");
    t.feed(b"\x1b[K"); // EL 0

    // Cols 0..=3 intact ('G'), cols 4..=9 erased.
    for col in 0..=3 {
        assert_eq!(t.grid().cell(col, 0).grapheme(), "G", "col {col} should be G");
    }
    assert_cells_default(&t, 0, 4, 9);

    // Other rows untouched (all default since we never wrote to them).
    assert_row_default(&t, 1);
    assert_row_default(&t, 2);
}

/// EL 1: erase from start of line to cursor (inclusive).
#[test]
fn test_el_1_erase_to_left() {
    let mut t = Terminal::new(10, 3);
    t.feed(b"\x1b[1;1H");
    t.feed(b"HHHHHHHHHH"); // row 0 full of 'H'
                           // Move cursor to col 5 (0-indexed) → CSI 1;6H
    t.feed(b"\x1b[1;6H");
    t.feed(b"\x1b[1K"); // EL 1

    // Cols 0..=5 erased, cols 6..=9 intact ('H').
    assert_cells_default(&t, 0, 0, 5);
    for col in 6..=9 {
        assert_eq!(t.grid().cell(col, 0).grapheme(), "H", "col {col} should be H");
    }
}

/// EL 2: erase entire current line.
#[test]
fn test_el_2_erase_line() {
    let mut t = Terminal::new(10, 3);
    t.feed(b"\x1b[1;1H");
    t.feed(b"IIIIIIIIII"); // row 0 full of 'I'
    t.feed(b"\x1b[2;1H");
    t.feed(b"JJJJJJJJJJ"); // row 1 full of 'J'
                           // Move cursor to row 1, col 5 → CSI 2;6H
    t.feed(b"\x1b[2;6H");
    t.feed(b"\x1b[2K"); // EL 2

    // Row 0: intact ('I').
    for col in 0..10 {
        assert_eq!(t.grid().cell(col, 0).grapheme(), "I", "row 0 col {col} should be I");
    }
    // Row 1: fully erased.
    assert_row_default(&t, 1);
    // Row 2: untouched (default).
    assert_row_default(&t, 2);
}

/// EL with no parameter is identical to EL 0.
#[test]
fn test_el_default_is_0() {
    let mut t0 = Terminal::new(10, 3);
    let mut t1 = Terminal::new(10, 3);

    let setup = b"\x1b[1;1HKKKKKKKKKK";
    let move_cursor = b"\x1b[1;4H";
    t0.feed(setup);
    t0.feed(move_cursor);
    t0.feed(b"\x1b[0K");

    t1.feed(setup);
    t1.feed(move_cursor);
    t1.feed(b"\x1b[K");

    for col in 0..10 {
        assert_eq!(
            t0.grid().cell(col, 0).grapheme(),
            t1.grid().cell(col, 0).grapheme(),
            "mismatch at col {col}"
        );
    }
}

/// Cursor position is unchanged after EL.
#[test]
fn test_el_preserves_cursor() {
    let mut t = Terminal::new(10, 5);
    // Move to row 1, col 4 (0-indexed) → CSI 2;5H
    t.feed(b"\x1b[2;5H");
    t.feed(b"\x1b[K"); // EL 0
    assert_eq!(t.grid().cursor_col(), 4, "cursor col should be unchanged after EL 0");
    assert_eq!(t.grid().cursor_row(), 1, "cursor row should be unchanged after EL 0");

    t.feed(b"\x1b[1K"); // EL 1
    assert_eq!(t.grid().cursor_col(), 4);
    assert_eq!(t.grid().cursor_row(), 1);

    t.feed(b"\x1b[2K"); // EL 2
    assert_eq!(t.grid().cursor_col(), 4);
    assert_eq!(t.grid().cursor_row(), 1);
}

// ---------------------------------------------------------------------------
// ECH — Erase Characters (CSI X)
// ---------------------------------------------------------------------------

/// ECH erases `n` characters starting at the cursor.
#[test]
fn test_ech_erase_characters() {
    let mut t = Terminal::new(10, 3);
    t.feed(b"\x1b[1;1H");
    t.feed(b"LLLLLLLLLL"); // row 0 full of 'L'
                           // Move cursor to col 2 (0-indexed) → CSI 1;3H
    t.feed(b"\x1b[1;3H");
    // ECH 5: erase 5 characters starting at col 2 → cols 2..=6 erased
    t.feed(b"\x1b[5X");

    // Cols 0..=1 intact ('L').
    for col in 0..=1 {
        assert_eq!(t.grid().cell(col, 0).grapheme(), "L", "col {col} should be L");
    }
    // Cols 2..=6 erased.
    assert_cells_default(&t, 0, 2, 6);
    // Cols 7..=9 intact ('L').
    for col in 7..=9 {
        assert_eq!(t.grid().cell(col, 0).grapheme(), "L", "col {col} should be L");
    }
}

/// ECH with no parameter erases exactly 1 character.
#[test]
fn test_ech_default_is_1() {
    let mut t = Terminal::new(10, 3);
    t.feed(b"\x1b[1;1H");
    t.feed(b"MMMMMMMMMM"); // row 0 full of 'M'
    t.feed(b"\x1b[1;4H"); // cursor at col 3 (0-indexed)
    t.feed(b"\x1b[X"); // ECH default (1 char)

    // Cols 0..=2 intact ('M').
    for col in 0..=2 {
        assert_eq!(t.grid().cell(col, 0).grapheme(), "M", "col {col} should be M");
    }
    // Col 3 erased.
    assert_cells_default(&t, 0, 3, 3);
    // Cols 4..=9 intact ('M').
    for col in 4..=9 {
        assert_eq!(t.grid().cell(col, 0).grapheme(), "M", "col {col} should be M");
    }
}

/// ECH does not move the cursor.
#[test]
fn test_ech_does_not_move_cursor() {
    let mut t = Terminal::new(10, 3);
    t.feed(b"\x1b[1;5H"); // cursor at row 0, col 4 (0-indexed)
    t.feed(b"\x1b[3X"); // ECH 3
    assert_eq!(t.grid().cursor_col(), 4, "cursor col should not move after ECH");
    assert_eq!(t.grid().cursor_row(), 0, "cursor row should not move after ECH");
}

/// ECH clamps at the end of the line — does not erase into the next line.
#[test]
fn test_ech_clamps_at_line_end() {
    let mut t = Terminal::new(10, 3);
    t.feed(b"\x1b[1;1H");
    t.feed(b"NNNNNNNNNN"); // row 0 full of 'N'
    t.feed(b"\x1b[2;1H");
    t.feed(b"OOOOOOOOOO"); // row 1 full of 'O'
                           // Move cursor to row 0, col 7 (0-indexed) → CSI 1;8H
    t.feed(b"\x1b[1;8H");
    // ECH 100 — much larger than remaining cols (3 remain: 7, 8, 9)
    t.feed(b"\x1b[100X");

    // Cols 0..=6 intact ('N').
    for col in 0..=6 {
        assert_eq!(t.grid().cell(col, 0).grapheme(), "N", "col {col} should be N");
    }
    // Cols 7..=9 erased.
    assert_cells_default(&t, 0, 7, 9);
    // Row 1 must be completely unaffected ('O').
    for col in 0..10 {
        assert_eq!(t.grid().cell(col, 1).grapheme(), "O", "row 1 col {col} should be O");
    }
}

// ---------------------------------------------------------------------------
// Wide character boundary interaction
// ---------------------------------------------------------------------------

/// ED 0: if the erase boundary falls on a wide character's leading half, both
/// halves are cleared to default.
#[test]
fn test_ed_erase_over_wide_char() {
    // Grid: 6 cols × 2 rows.
    // Row 0: write 'AB' (2 single-width), then '中' (CJK, double-width at cols 2-3),
    // then 'CD' (single-width).
    // Move cursor to col 2, row 0 (the start of the wide char).
    // ED 0: everything from col 2 onward on row 0, plus row 1, erased.
    let mut t = Terminal::new(6, 2);
    t.feed(b"\x1b[1;1H");
    t.feed("AB中CD".as_bytes());
    // Cursor is now at col 6 (wrap-pending). Move to col 2.
    t.feed(b"\x1b[1;3H"); // 1-indexed col 3 → 0-indexed col 2
    t.feed(b"\x1b[J"); // ED 0

    // Cols 0..=1: intact.
    assert_eq!(t.grid().cell(0, 0).grapheme(), "A");
    assert_eq!(t.grid().cell(1, 0).grapheme(), "B");
    // Cols 2..=5: erased (wide char and its continuation + CD).
    assert_cells_default(&t, 0, 2, 5);
    // Row 1: erased.
    assert_row_default(&t, 1);
}

/// EL boundary on the trailing half (continuation) of a wide character: both
/// halves must be cleared.
#[test]
fn test_el_erase_boundary_on_wide_trailing() {
    // Grid: 6 cols.
    // Row 0: 'A', '中' (cols 1-2), 'B', 'C', 'D'
    // EL 1 with cursor on col 2 (the continuation half of '中') →
    // clear cols 0..=2, which includes the wide leading half at col 1.
    let mut t = Terminal::new(6, 2);
    t.feed(b"\x1b[1;1H");
    t.feed("A中BCD".as_bytes());
    // 'A' at col 0, '中' at cols 1-2, 'B' at col 3, 'C' at col 4, 'D' at col 5.
    // Move cursor to col 2 (0-indexed) → CSI 1;3H
    t.feed(b"\x1b[1;3H");
    t.feed(b"\x1b[1K"); // EL 1: clear start of line to cursor (inclusive)

    // Cols 0..=2 erased (including both halves of '中').
    assert_cells_default(&t, 0, 0, 2);
    // Cols 3..=5 intact.
    assert_eq!(t.grid().cell(3, 0).grapheme(), "B");
    assert_eq!(t.grid().cell(4, 0).grapheme(), "C");
    assert_eq!(t.grid().cell(5, 0).grapheme(), "D");
}

/// ECH starting on a continuation (trailing) half of a wide character must
/// clean up the leading half.
#[test]
fn test_ech_over_wide_char() {
    // Grid: 6 cols.
    // Row 0: 'AB' then '中' (cols 2-3) then 'CD'.
    // Move cursor to col 3 (the continuation half of '中').
    // ECH 1: should erase col 3 AND clean up the wide leading at col 2.
    let mut t = Terminal::new(6, 2);
    t.feed(b"\x1b[1;1H");
    t.feed("AB中CD".as_bytes());
    // Move cursor to col 3 (0-indexed) → CSI 1;4H
    t.feed(b"\x1b[1;4H");
    t.feed(b"\x1b[1X"); // ECH 1

    // Col 2 (leading wide) must be reset to default (not left as a dangling wide cell).
    assert!(
        !t.grid().cell(2, 0).is_wide(),
        "col 2 should not remain wide after ECH on continuation"
    );
    assert_eq!(t.grid().cell(2, 0).grapheme(), " ", "col 2 should be space");
    // Col 3 (was continuation, now erased) default.
    assert_cells_default(&t, 0, 3, 3);
    // Cols 0..=1: intact.
    assert_eq!(t.grid().cell(0, 0).grapheme(), "A");
    assert_eq!(t.grid().cell(1, 0).grapheme(), "B");
    // Cols 4..=5: intact.
    assert_eq!(t.grid().cell(4, 0).grapheme(), "C");
    assert_eq!(t.grid().cell(5, 0).grapheme(), "D");
}

// ---------------------------------------------------------------------------
// Erase uses default style (not cursor's current SGR style)
// ---------------------------------------------------------------------------

/// Erased cells must have default style regardless of the current cursor SGR.
///
/// This tests the spec requirement that erased cells use the default style, not
/// the active foreground/background/attributes.
#[test]
fn test_erase_uses_default_style() {
    let mut t = Terminal::new(10, 3);
    // Write content with a coloured style.
    t.feed(b"\x1b[1;1H");
    t.feed(b"PPPPPPPPPP"); // row 0 full of 'P'
                           // Set cursor style to bold + red foreground.
    t.feed(b"\x1b[1;31m"); // bold=1, fg=red=31
                           // Move cursor to col 4 (0-indexed) → CSI 1;5H
    t.feed(b"\x1b[1;5H");
    // EL 0: erase from col 4 to end of line — should use default style, not bold+red.
    t.feed(b"\x1b[K");

    // Verify the cursor's active style is indeed bold+red (not changed by erase).
    assert!(
        t.grid().cursor().style().attrs().contains(Attr::BOLD),
        "cursor style should still be bold after EL"
    );

    // Verify erased cells have default style.
    assert_cells_default(&t, 0, 4, 9);

    // Now apply ECH and check default style.
    t.feed(b"\x1b[1;3H"); // cursor to col 2
    t.feed(b"\x1b[3X"); // ECH 3 → erases cols 2, 3 (already default), does NOT change style
    assert_cells_default(&t, 0, 2, 4);

    // ED 2 and check default style.
    t.feed(b"\x1b[2J");
    for row in 0..3 {
        assert_row_default(&t, row);
    }
}
