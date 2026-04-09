//! Scroll region tests for DECSTBM, SU/SD, LF-at-region-boundary, Index/Reverse-Index.
//!
//! All terminal sequences follow the convention:
//!   - Region parameters are 1-indexed and inclusive.
//!   - Grid coordinates are 0-indexed throughout the assertions.
//!
//! Naming: `test_<unit>_<scenario>_<expected>`.

use teamucks_vte::terminal::Terminal;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Create a terminal with given dimensions and feed `input`.
fn term_wh(cols: usize, rows: usize, input: &[u8]) -> Terminal {
    let mut t = Terminal::new(cols, rows);
    t.feed(input);
    t
}

/// Create an 80×24 terminal and feed `input`.
fn term(input: &[u8]) -> Terminal {
    term_wh(80, 24, input)
}

/// Fill row `row` on terminal `t` with a repeated single character using direct
/// grid mutation (bypasses VTE so we can set up known content quickly).
fn fill_row(t: &mut Terminal, row: usize, ch: char) {
    let cols = t.grid().cols();
    for col in 0..cols {
        t.grid_mut().cell_mut(col, row).set_grapheme_char(ch);
    }
}

// ---------------------------------------------------------------------------
// DECSTBM — Set Top and Bottom Margins (CSI top ; bottom r)
// ---------------------------------------------------------------------------

#[test]
fn test_decstbm_set_scroll_region() {
    // "\x1b[5;20r" should set scroll region to rows 4..=19 (0-indexed).
    let t = term(b"\x1b[5;20r");
    assert_eq!(t.grid().scroll_region(), (4, 19));
}

#[test]
fn test_decstbm_default_full_screen() {
    // First set a narrow region, then reset with "\x1b[r" (no params).
    let mut t = Terminal::new(80, 24);
    t.feed(b"\x1b[5;20r");
    t.feed(b"\x1b[r");
    // Should be reset to full screen: (0, rows-1).
    assert_eq!(t.grid().scroll_region(), (0, 23));
}

#[test]
fn test_decstbm_moves_cursor_home() {
    // After DECSTBM the cursor must move to (0, 0).
    let mut t = Terminal::new(80, 24);
    t.feed(b"\x1b[12;30H"); // move cursor somewhere
    t.feed(b"\x1b[5;20r");
    assert_eq!(t.grid().cursor_col(), 0, "DECSTBM must move cursor to col 0");
    assert_eq!(t.grid().cursor_row(), 0, "DECSTBM must move cursor to row 0");
}

#[test]
fn test_decstbm_invalid_top_gt_bottom() {
    // "\x1b[20;5r" — top > bottom, must be ignored; region unchanged.
    let mut t = Terminal::new(80, 24);
    t.feed(b"\x1b[5;20r"); // set a known region first
    t.feed(b"\x1b[20;5r"); // invalid — top > bottom
                           // Region must still be the previously-set region.
    assert_eq!(t.grid().scroll_region(), (4, 19), "invalid DECSTBM must be ignored");
}

#[test]
fn test_decstbm_invalid_out_of_bounds() {
    // "\x1b[1;999r" on a 24-row grid — bottom exceeds grid; must be ignored.
    let mut t = Terminal::new(80, 24);
    t.feed(b"\x1b[5;20r"); // set a known region
    t.feed(b"\x1b[1;999r"); // invalid — out of bounds
                            // Region must remain unchanged.
    assert_eq!(t.grid().scroll_region(), (4, 19), "out-of-bounds DECSTBM must be ignored");
}

// ---------------------------------------------------------------------------
// Region-aware scrolling within the Grid
// ---------------------------------------------------------------------------

#[test]
fn test_scroll_up_within_region() {
    // Set up a 10-row terminal. Fill every row with its row index as a char.
    // Set scroll region to rows 2..=6 (1-indexed: 3;7). Scroll up 1.
    // Rows 0-1 and 7-9 must be untouched; row 2's content disappears; row 6
    // becomes blank; rows 3-6 shift up.
    let mut t = Terminal::new(10, 10);
    // Fill each row: row 0 = '0', row 1 = '1', ..., row 9 = '9'
    for r in 0..10usize {
        let ch = char::from_digit(r as u32, 10).unwrap();
        fill_row(&mut t, r, ch);
    }
    // Set scroll region rows 3..=7 (1-indexed), i.e. 0-indexed 2..=6.
    t.feed(b"\x1b[3;7r");
    // Scroll up 1 within the region via CSI S.
    t.feed(b"\x1b[1S");

    // Rows outside the region must be untouched.
    assert_eq!(t.grid().cell(0, 0).grapheme(), "0", "row 0 must be untouched");
    assert_eq!(t.grid().cell(0, 1).grapheme(), "1", "row 1 must be untouched");
    assert_eq!(t.grid().cell(0, 7).grapheme(), "7", "row 7 must be untouched");
    assert_eq!(t.grid().cell(0, 8).grapheme(), "8", "row 8 must be untouched");
    assert_eq!(t.grid().cell(0, 9).grapheme(), "9", "row 9 must be untouched");

    // Inside the region: row 2 (which held '2') is gone; rows shift up.
    assert_eq!(t.grid().cell(0, 2).grapheme(), "3", "row 2 should now hold what was row 3");
    assert_eq!(t.grid().cell(0, 3).grapheme(), "4");
    assert_eq!(t.grid().cell(0, 4).grapheme(), "5");
    assert_eq!(t.grid().cell(0, 5).grapheme(), "6");
    // The bottom of the region (row 6) must be blank.
    assert_eq!(
        t.grid().cell(0, 6).grapheme(),
        " ",
        "bottom of region must be blank after scroll_up"
    );
}

#[test]
fn test_scroll_down_within_region() {
    // Similar setup: 10-row terminal, scroll region 3..=7 (0-indexed 2..=6).
    // Scroll down 1 within the region via CSI T.
    let mut t = Terminal::new(10, 10);
    for r in 0..10usize {
        let ch = char::from_digit(r as u32, 10).unwrap();
        fill_row(&mut t, r, ch);
    }
    t.feed(b"\x1b[3;7r");
    t.feed(b"\x1b[1T");

    // Rows outside the region must be untouched.
    assert_eq!(t.grid().cell(0, 0).grapheme(), "0");
    assert_eq!(t.grid().cell(0, 1).grapheme(), "1");
    assert_eq!(t.grid().cell(0, 7).grapheme(), "7");
    assert_eq!(t.grid().cell(0, 8).grapheme(), "8");
    assert_eq!(t.grid().cell(0, 9).grapheme(), "9");

    // Inside the region: row 6 (which held '6') disappears; rows shift down.
    // Top of region (row 2) must be blank.
    assert_eq!(
        t.grid().cell(0, 2).grapheme(),
        " ",
        "top of region must be blank after scroll_down"
    );
    assert_eq!(t.grid().cell(0, 3).grapheme(), "2", "row 3 should hold what was row 2");
    assert_eq!(t.grid().cell(0, 4).grapheme(), "3");
    assert_eq!(t.grid().cell(0, 5).grapheme(), "4");
    assert_eq!(t.grid().cell(0, 6).grapheme(), "5");
}

#[test]
fn test_su_scroll_up() {
    // CSI 2 S — scroll up 2 within the current scroll region.
    let mut t = Terminal::new(10, 10);
    for r in 0..10usize {
        let ch = char::from_digit(r as u32, 10).unwrap();
        fill_row(&mut t, r, ch);
    }
    t.feed(b"\x1b[3;7r"); // region 0-indexed 2..=6
    t.feed(b"\x1b[2S"); // scroll up 2

    // Row 2 held '2', row 3 held '3' — both gone; region shifts up by 2.
    assert_eq!(t.grid().cell(0, 2).grapheme(), "4");
    assert_eq!(t.grid().cell(0, 3).grapheme(), "5");
    assert_eq!(t.grid().cell(0, 4).grapheme(), "6");
    // Bottom 2 rows of region blank.
    assert_eq!(t.grid().cell(0, 5).grapheme(), " ");
    assert_eq!(t.grid().cell(0, 6).grapheme(), " ");
    // Outside rows untouched.
    assert_eq!(t.grid().cell(0, 1).grapheme(), "1");
    assert_eq!(t.grid().cell(0, 7).grapheme(), "7");
}

#[test]
fn test_sd_scroll_down() {
    // CSI 2 T — scroll down 2 within the current scroll region.
    let mut t = Terminal::new(10, 10);
    for r in 0..10usize {
        let ch = char::from_digit(r as u32, 10).unwrap();
        fill_row(&mut t, r, ch);
    }
    t.feed(b"\x1b[3;7r"); // region 0-indexed 2..=6
    t.feed(b"\x1b[2T"); // scroll down 2

    // Rows 5, 6 (held '5', '6') — gone; region shifts down by 2.
    assert_eq!(t.grid().cell(0, 2).grapheme(), " ");
    assert_eq!(t.grid().cell(0, 3).grapheme(), " ");
    assert_eq!(t.grid().cell(0, 4).grapheme(), "2");
    assert_eq!(t.grid().cell(0, 5).grapheme(), "3");
    assert_eq!(t.grid().cell(0, 6).grapheme(), "4");
    // Outside rows untouched.
    assert_eq!(t.grid().cell(0, 1).grapheme(), "1");
    assert_eq!(t.grid().cell(0, 7).grapheme(), "7");
}

// ---------------------------------------------------------------------------
// LF at scroll region boundary
// ---------------------------------------------------------------------------

#[test]
fn test_lf_at_bottom_of_region_scrolls() {
    // Set scroll region to rows 2..=5 (0-indexed), place cursor at the bottom
    // of the region (row 5), then send LF (0x0A). Expect the region to scroll
    // up by 1 and the cursor to remain at row 5.
    let mut t = Terminal::new(10, 10);
    for r in 0..10usize {
        let ch = char::from_digit(r as u32, 10).unwrap();
        fill_row(&mut t, r, ch);
    }
    // Set region: 1-indexed 3;6 → 0-indexed 2..=5.
    t.feed(b"\x1b[3;6r");
    // Move cursor to row 5, col 0.
    t.feed(b"\x1b[6;1H");
    // Send LF.
    t.feed(b"\x0A");

    // Row 2 (was '2') must be gone; rows 3-5 shifted up.
    assert_eq!(t.grid().cell(0, 2).grapheme(), "3");
    assert_eq!(t.grid().cell(0, 3).grapheme(), "4");
    assert_eq!(t.grid().cell(0, 4).grapheme(), "5");
    // Bottom of region is blank.
    assert_eq!(t.grid().cell(0, 5).grapheme(), " ");
    // Rows outside region untouched.
    assert_eq!(t.grid().cell(0, 0).grapheme(), "0");
    assert_eq!(t.grid().cell(0, 1).grapheme(), "1");
    assert_eq!(t.grid().cell(0, 6).grapheme(), "6");
    // Cursor remains at the bottom of the region.
    assert_eq!(t.grid().cursor_row(), 5);
}

#[test]
fn test_lf_below_region_moves_down() {
    // Cursor below the scroll region — LF should just move down without scrolling.
    let mut t = Terminal::new(10, 10);
    for r in 0..10usize {
        let ch = char::from_digit(r as u32, 10).unwrap();
        fill_row(&mut t, r, ch);
    }
    // Region rows 1..=3 (0-indexed).
    t.feed(b"\x1b[2;4r");
    // Place cursor below the region (row 5).
    t.feed(b"\x1b[6;1H");
    // Send LF.
    t.feed(b"\x0A");

    // Cursor moved to row 6, no scrolling.
    assert_eq!(t.grid().cursor_row(), 6);
    // Rows within region must be untouched.
    assert_eq!(t.grid().cell(0, 1).grapheme(), "1");
    assert_eq!(t.grid().cell(0, 2).grapheme(), "2");
    assert_eq!(t.grid().cell(0, 3).grapheme(), "3");
}

#[test]
fn test_lf_above_region_moves_down() {
    // Cursor above the scroll region — LF should just move down without scrolling.
    let mut t = Terminal::new(10, 10);
    for r in 0..10usize {
        let ch = char::from_digit(r as u32, 10).unwrap();
        fill_row(&mut t, r, ch);
    }
    // Region rows 3..=6 (0-indexed).
    t.feed(b"\x1b[4;7r");
    // Place cursor above the region (row 1).
    t.feed(b"\x1b[2;1H");
    // Send LF.
    t.feed(b"\x0A");

    // Cursor moved down to row 2, no scrolling.
    assert_eq!(t.grid().cursor_row(), 2);
    // Rows inside region must be untouched.
    assert_eq!(t.grid().cell(0, 3).grapheme(), "3");
    assert_eq!(t.grid().cell(0, 6).grapheme(), "6");
}

#[test]
fn test_lf_at_screen_bottom_no_region() {
    // No explicit region set (defaults to full screen). Cursor at last row.
    // LF should scroll entire screen up.
    let mut t = Terminal::new(10, 5);
    for r in 0..5usize {
        let ch = char::from_digit(r as u32, 10).unwrap();
        fill_row(&mut t, r, ch);
    }
    // Move to last row.
    t.feed(b"\x1b[5;1H");
    // Send LF.
    t.feed(b"\x0A");

    // Screen scrolled up: row 0 now holds what was row 1.
    assert_eq!(t.grid().cell(0, 0).grapheme(), "1");
    assert_eq!(t.grid().cell(0, 3).grapheme(), "4");
    // Last row is blank.
    assert_eq!(t.grid().cell(0, 4).grapheme(), " ");
    // Cursor stays at last row.
    assert_eq!(t.grid().cursor_row(), 4);
}

// ---------------------------------------------------------------------------
// ESC D (Index) and ESC M (Reverse Index)
// ---------------------------------------------------------------------------

#[test]
fn test_index_at_bottom_of_region() {
    // ESC D at the bottom of the scroll region should scroll up within the region.
    let mut t = Terminal::new(10, 10);
    for r in 0..10usize {
        let ch = char::from_digit(r as u32, 10).unwrap();
        fill_row(&mut t, r, ch);
    }
    // Region rows 2..=5 (0-indexed).
    t.feed(b"\x1b[3;6r");
    // Move cursor to row 5 (bottom of region).
    t.feed(b"\x1b[6;1H");
    // ESC D = Index.
    t.feed(b"\x1bD");

    // Region scrolled up: row 2 gone, rows 3-5 shifted up.
    assert_eq!(t.grid().cell(0, 2).grapheme(), "3");
    assert_eq!(t.grid().cell(0, 3).grapheme(), "4");
    assert_eq!(t.grid().cell(0, 4).grapheme(), "5");
    assert_eq!(t.grid().cell(0, 5).grapheme(), " ");
    // Outside rows untouched.
    assert_eq!(t.grid().cell(0, 1).grapheme(), "1");
    assert_eq!(t.grid().cell(0, 6).grapheme(), "6");
    // Cursor stays at bottom of region.
    assert_eq!(t.grid().cursor_row(), 5);
}

#[test]
fn test_index_not_at_bottom() {
    // ESC D not at the bottom of the region — just move cursor down.
    let mut t = Terminal::new(10, 10);
    for r in 0..10usize {
        let ch = char::from_digit(r as u32, 10).unwrap();
        fill_row(&mut t, r, ch);
    }
    t.feed(b"\x1b[3;6r"); // region 0-indexed 2..=5
    t.feed(b"\x1b[4;1H"); // cursor at row 3
    t.feed(b"\x1bD"); // ESC D — just move down

    assert_eq!(t.grid().cursor_row(), 4);
    // No scrolling: rows inside region untouched.
    assert_eq!(t.grid().cell(0, 2).grapheme(), "2");
    assert_eq!(t.grid().cell(0, 5).grapheme(), "5");
}

#[test]
fn test_reverse_index_at_top_of_region() {
    // ESC M at the top of the scroll region should scroll down within the region.
    let mut t = Terminal::new(10, 10);
    for r in 0..10usize {
        let ch = char::from_digit(r as u32, 10).unwrap();
        fill_row(&mut t, r, ch);
    }
    // Region rows 2..=5 (0-indexed).
    t.feed(b"\x1b[3;6r");
    // Move cursor to row 2 (top of region).
    t.feed(b"\x1b[3;1H");
    // ESC M = Reverse Index.
    t.feed(b"\x1bM");

    // Region scrolled down: row 5 gone, top of region (row 2) is blank.
    assert_eq!(t.grid().cell(0, 2).grapheme(), " ");
    assert_eq!(t.grid().cell(0, 3).grapheme(), "2");
    assert_eq!(t.grid().cell(0, 4).grapheme(), "3");
    assert_eq!(t.grid().cell(0, 5).grapheme(), "4");
    // Outside rows untouched.
    assert_eq!(t.grid().cell(0, 1).grapheme(), "1");
    assert_eq!(t.grid().cell(0, 6).grapheme(), "6");
    // Cursor stays at top of region.
    assert_eq!(t.grid().cursor_row(), 2);
}

#[test]
fn test_reverse_index_not_at_top() {
    // ESC M not at the top of the region — just move cursor up.
    let mut t = Terminal::new(10, 10);
    for r in 0..10usize {
        let ch = char::from_digit(r as u32, 10).unwrap();
        fill_row(&mut t, r, ch);
    }
    t.feed(b"\x1b[3;6r"); // region 0-indexed 2..=5
    t.feed(b"\x1b[5;1H"); // cursor at row 4
    t.feed(b"\x1bM"); // ESC M — just move up

    assert_eq!(t.grid().cursor_row(), 3);
    // No scrolling.
    assert_eq!(t.grid().cell(0, 2).grapheme(), "2");
    assert_eq!(t.grid().cell(0, 5).grapheme(), "5");
}

// ---------------------------------------------------------------------------
// Region preserves content outside
// ---------------------------------------------------------------------------

#[test]
fn test_region_scroll_preserves_outside() {
    // Comprehensive test: content above and below the region must survive
    // multiple scrolls within the region.
    let mut t = Terminal::new(5, 8);
    // Above region (rows 0-1): 'A', 'B'
    fill_row(&mut t, 0, 'A');
    fill_row(&mut t, 1, 'B');
    // Inside region (rows 2-5): 'C', 'D', 'E', 'F'
    fill_row(&mut t, 2, 'C');
    fill_row(&mut t, 3, 'D');
    fill_row(&mut t, 4, 'E');
    fill_row(&mut t, 5, 'F');
    // Below region (rows 6-7): 'G', 'H'
    fill_row(&mut t, 6, 'G');
    fill_row(&mut t, 7, 'H');

    // Set region 0-indexed 2..=5 (1-indexed 3..=6).
    t.feed(b"\x1b[3;6r");
    // Scroll up 2 within the region.
    t.feed(b"\x1b[2S");

    // Rows above region: untouched.
    assert_eq!(t.grid().cell(0, 0).grapheme(), "A");
    assert_eq!(t.grid().cell(0, 1).grapheme(), "B");
    // Rows below region: untouched.
    assert_eq!(t.grid().cell(0, 6).grapheme(), "G");
    assert_eq!(t.grid().cell(0, 7).grapheme(), "H");
    // Region: 'C' and 'D' gone, 'E' and 'F' shifted up, bottom 2 rows blank.
    assert_eq!(t.grid().cell(0, 2).grapheme(), "E");
    assert_eq!(t.grid().cell(0, 3).grapheme(), "F");
    assert_eq!(t.grid().cell(0, 4).grapheme(), " ");
    assert_eq!(t.grid().cell(0, 5).grapheme(), " ");
}

// ---------------------------------------------------------------------------
// Resize resets scroll region
// ---------------------------------------------------------------------------

#[test]
fn test_resize_resets_scroll_region() {
    // After resize, the scroll region must be reset to the full new screen.
    let mut t = Terminal::new(80, 24);
    t.feed(b"\x1b[5;20r"); // set a custom region
    assert_eq!(t.grid().scroll_region(), (4, 19), "region should be set");
    t.resize(80, 30);
    assert_eq!(t.grid().scroll_region(), (0, 29), "resize must reset scroll region to full screen");
}
