// Integration tests for cell model and grid structures.

use teamucks_vte::{
    cell::Cell,
    grid::Grid,
    row::Row,
    style::{Attr, Color, PackedStyle},
};

// ---------------------------------------------------------------------------
// PackedStyle tests
// ---------------------------------------------------------------------------

#[test]
fn test_packed_style_default_is_zero() {
    let s = PackedStyle::default();
    assert_eq!(s.foreground(), Color::Default);
    assert_eq!(s.background(), Color::Default);
    assert_eq!(s.attrs(), Attr::empty());
}

#[test]
fn test_packed_style_set_foreground_rgb() {
    let mut s = PackedStyle::default();
    s.set_foreground(Color::Rgb(255, 0, 128));
    assert_eq!(s.foreground(), Color::Rgb(255, 0, 128));
    // Background must remain default.
    assert_eq!(s.background(), Color::Default);
}

#[test]
fn test_packed_style_set_background_indexed() {
    let mut s = PackedStyle::default();
    s.set_background(Color::Indexed(42));
    assert_eq!(s.background(), Color::Indexed(42));
    assert_eq!(s.foreground(), Color::Default);
}

#[test]
fn test_packed_style_set_named_color() {
    // Named(n) is stored as Indexed(n) — both are palette indices.
    let mut s = PackedStyle::default();
    s.set_foreground(Color::Named(1));
    // Named is stored and returned as Indexed.
    assert_eq!(s.foreground(), Color::Indexed(1));
}

#[test]
fn test_packed_style_set_attributes() {
    let mut s = PackedStyle::default();
    s.set_attr(Attr::BOLD);
    s.set_attr(Attr::ITALIC);
    assert!(s.has_attr(Attr::BOLD));
    assert!(s.has_attr(Attr::ITALIC));
    assert!(!s.has_attr(Attr::UNDERLINE));
}

#[test]
fn test_packed_style_clear_attr() {
    let mut s = PackedStyle::default();
    s.set_attr(Attr::BOLD);
    assert!(s.has_attr(Attr::BOLD));
    s.clear_attr(Attr::BOLD);
    assert!(!s.has_attr(Attr::BOLD));
}

#[test]
fn test_packed_style_reset() {
    let mut s = PackedStyle::default();
    s.set_foreground(Color::Rgb(10, 20, 30));
    s.set_background(Color::Indexed(5));
    s.set_attr(Attr::BOLD);
    s.reset();
    assert_eq!(s.foreground(), Color::Default);
    assert_eq!(s.background(), Color::Default);
    assert_eq!(s.attrs(), Attr::empty());
}

#[test]
fn test_packed_style_size() {
    assert!(std::mem::size_of::<PackedStyle>() <= 8);
}

// ---------------------------------------------------------------------------
// Cell tests (public interface only)
// ---------------------------------------------------------------------------

#[test]
fn test_cell_default_is_space() {
    let c = Cell::default();
    assert_eq!(c.grapheme(), " ");
    assert_eq!(c.style().foreground(), Color::Default);
    assert_eq!(c.style().background(), Color::Default);
    assert_eq!(c.style().attrs(), Attr::empty());
    assert!(!c.is_wide());
    assert!(!c.is_continuation());
}

#[test]
fn test_cell_set_ascii() {
    let mut c = Cell::default();
    c.set_grapheme("A");
    assert_eq!(c.grapheme(), "A");
}

#[test]
fn test_cell_set_emoji() {
    let mut c = Cell::default();
    c.set_grapheme("🎉");
    assert_eq!(c.grapheme(), "🎉");
}

// ---------------------------------------------------------------------------
// Row tests (public read interface only — mutation is pub(crate))
// ---------------------------------------------------------------------------

#[test]
fn test_row_creation() {
    let row = Row::new(80);
    assert_eq!(row.len(), 80);
    assert!(!row.is_empty());
}

#[test]
fn test_row_cell_read() {
    let row = Row::new(80);
    // Default cell is a space.
    assert_eq!(row.cell(0).grapheme(), " ");
}

#[test]
fn test_row_soft_wrap_read() {
    let row = Row::new(80);
    assert!(!row.is_soft_wrapped());
}

// ---------------------------------------------------------------------------
// Grid tests
// ---------------------------------------------------------------------------

#[test]
fn test_grid_creation() {
    let g = Grid::new(80, 24);
    assert_eq!(g.cols(), 80);
    assert_eq!(g.rows(), 24);
}

#[test]
fn test_grid_cell_at() {
    let g = Grid::new(80, 24);
    let c = g.cell(0, 0);
    assert_eq!(c.grapheme(), " ");
}

#[test]
fn test_grid_cell_at_mut() {
    let mut g = Grid::new(80, 24);
    g.cell_mut(0, 0).set_grapheme("Q");
    assert_eq!(g.cell(0, 0).grapheme(), "Q");
}

#[test]
fn test_grid_cursor_within_bounds() {
    let mut g = Grid::new(80, 24);
    g.set_cursor(79, 23);
    assert_eq!(g.cursor_col(), 79);
    assert_eq!(g.cursor_row(), 23);
}

#[test]
fn test_grid_cursor_clamped() {
    let mut g = Grid::new(80, 24);
    g.set_cursor(100, 30);
    assert_eq!(g.cursor_col(), 79);
    assert_eq!(g.cursor_row(), 23);
}

#[test]
fn test_grid_cursor_clears_wrap_pending() {
    let mut g = Grid::new(80, 24);
    // Trigger wrap_pending by writing to the last column.
    g.set_cursor(79, 0);
    g.write_char('Z');
    // Now set_cursor must clear wrap_pending.
    g.set_cursor(0, 0);
    // Write another char — if wrap_pending were still set the cursor would
    // jump to row 1; instead it should stay on row 0.
    g.write_char('A');
    assert_eq!(g.cursor_row(), 0);
    assert_eq!(g.cursor_col(), 1);
}

#[test]
fn test_grid_resize_grow() {
    let mut g = Grid::new(80, 24);
    g.cell_mut(0, 0).set_grapheme("A");
    g.resize(120, 30);
    assert_eq!(g.cols(), 120);
    assert_eq!(g.rows(), 30);
    assert_eq!(g.cell(0, 0).grapheme(), "A");
}

#[test]
fn test_grid_resize_shrink() {
    let mut g = Grid::new(80, 24);
    g.cell_mut(0, 0).set_grapheme("B");
    g.resize(40, 12);
    assert_eq!(g.cols(), 40);
    assert_eq!(g.rows(), 12);
    assert_eq!(g.cell(0, 0).grapheme(), "B");
}

#[test]
fn test_grid_write_char_ascii() {
    let mut g = Grid::new(80, 24);
    g.write_char('A');
    assert_eq!(g.cell(0, 0).grapheme(), "A");
    assert_eq!(g.cursor_col(), 1);
    assert_eq!(g.cursor_row(), 0);
}

#[test]
fn test_grid_write_char_advances_cursor() {
    let mut g = Grid::new(80, 24);
    g.write_char('h');
    g.write_char('i');
    assert_eq!(g.cell(0, 0).grapheme(), "h");
    assert_eq!(g.cell(1, 0).grapheme(), "i");
    assert_eq!(g.cursor_col(), 2);
}

#[test]
fn test_grid_write_char_wraps_at_end() {
    let mut g = Grid::new(80, 24);
    // Advance cursor to last column.
    g.set_cursor(79, 0);
    g.write_char('Z');
    // Cursor stays at col 79, wrap_pending is set (tested via observable behaviour).
    assert_eq!(g.cursor_col(), 79);
    // Next write goes to row 1.
    g.write_char('A');
    assert_eq!(g.cursor_row(), 1);
}

#[test]
fn test_grid_write_char_pending_wrap_then_char() {
    let mut g = Grid::new(80, 24);
    g.set_cursor(79, 0);
    g.write_char('Z');
    // Writing another char should trigger actual wrap.
    g.write_char('A');
    assert_eq!(g.cursor_row(), 1);
    assert_eq!(g.cursor_col(), 1);
    assert_eq!(g.cell(0, 1).grapheme(), "A");
}

#[test]
fn test_grid_write_char_wide_character() {
    let mut g = Grid::new(80, 24);
    g.write_char('世'); // CJK, width 2
    assert_eq!(g.cell(0, 0).grapheme(), "世");
    assert!(g.cell(0, 0).is_wide());
    assert!(g.cell(1, 0).is_continuation());
    assert_eq!(g.cursor_col(), 2);
}

#[test]
fn test_grid_write_char_wide_at_end_wraps() {
    // Wide char at col 79 (last col of 80-wide grid) should soft-wrap.
    let mut g = Grid::new(80, 24);
    g.set_cursor(79, 0);
    g.write_char('世'); // width 2
                        // Should have wrapped: cursor is on row 1.
    assert_eq!(g.cursor_row(), 1);
    assert_eq!(g.cell(0, 1).grapheme(), "世");
    assert!(g.cell(0, 1).is_wide());
}

#[test]
fn test_grid_write_char_overwrites_wide_leading() {
    let mut g = Grid::new(80, 24);
    // Write a wide char at col 0.
    g.write_char('世');
    assert!(g.cell(0, 0).is_wide());
    assert!(g.cell(1, 0).is_continuation());
    // Overwrite the leading cell with an ASCII char.
    g.set_cursor(0, 0);
    g.write_char('A');
    // Trailing continuation cell must be cleared.
    assert!(!g.cell(1, 0).is_continuation());
    assert_eq!(g.cell(1, 0).grapheme(), " ");
}

#[test]
fn test_grid_write_char_overwrites_wide_trailing() {
    let mut g = Grid::new(80, 24);
    g.write_char('世');
    assert!(g.cell(1, 0).is_continuation());
    // Overwrite the trailing continuation cell.
    g.set_cursor(1, 0);
    g.write_char('B');
    // Leading wide cell must be cleared.
    assert!(!g.cell(0, 0).is_wide());
    assert_eq!(g.cell(0, 0).grapheme(), " ");
}

#[test]
fn test_grid_write_char_zero_width() {
    let mut g = Grid::new(80, 24);
    g.write_char('A');
    // Combining grave accent U+0300 (zero-width).
    g.write_char('\u{0300}');
    // Cursor should NOT have advanced; grapheme on previous cell combined.
    assert_eq!(g.cursor_col(), 1);
    // The grapheme at col 0 should now include the combining character.
    let grapheme = g.cell(0, 0).grapheme();
    assert!(grapheme.contains('A'));
    assert!(grapheme.contains('\u{0300}'));
}

#[test]
fn test_grid_scroll_up() {
    let mut g = Grid::new(80, 24);
    g.cell_mut(0, 0).set_grapheme("T");
    g.cell_mut(0, 1).set_grapheme("B");
    g.scroll_up(1);
    // Row 0 content is now what was row 1.
    assert_eq!(g.cell(0, 0).grapheme(), "B");
    // Bottom row is blank.
    assert_eq!(g.cell(0, 23).grapheme(), " ");
}

#[test]
fn test_grid_scroll_down() {
    let mut g = Grid::new(80, 24);
    g.cell_mut(0, 0).set_grapheme("T");
    g.scroll_down(1);
    // Row 1 content is now what was row 0.
    assert_eq!(g.cell(0, 1).grapheme(), "T");
    // Top row is blank.
    assert_eq!(g.cell(0, 0).grapheme(), " ");
}

#[test]
fn test_grid_save_restore_cursor() {
    let mut g = Grid::new(80, 24);
    g.set_cursor(5, 10);
    g.save_cursor();
    g.set_cursor(0, 0);
    g.restore_cursor();
    assert_eq!(g.cursor_col(), 5);
    assert_eq!(g.cursor_row(), 10);
}

#[test]
fn test_grid_row_text() {
    let mut g = Grid::new(80, 24);
    for (i, ch) in "hello".chars().enumerate() {
        g.cell_mut(i, 0).set_grapheme_char(ch);
    }
    let text = g.row_text(0);
    assert_eq!(text, "hello");
}

#[test]
fn test_grid_row_text_with_wide() {
    let mut g = Grid::new(80, 24);
    g.write_char('世');
    g.write_char('界');
    let text = g.row_text(0);
    assert_eq!(text, "世界");
}

#[test]
fn test_grid_clear() {
    let mut g = Grid::new(80, 24);
    g.cell_mut(0, 0).set_grapheme("X");
    g.clear();
    assert_eq!(g.cell(0, 0).grapheme(), " ");
}

// ---------------------------------------------------------------------------
// Boundary tests (Fix 7)
// ---------------------------------------------------------------------------

#[test]
fn test_grid_restore_cursor_without_save() {
    let mut grid = Grid::new(80, 24);
    grid.set_cursor(5, 5);
    grid.restore_cursor(); // no-op when nothing saved
    assert_eq!(grid.cursor_col(), 5);
    assert_eq!(grid.cursor_row(), 5);
}

#[test]
fn test_grid_scroll_up_count_exceeds_rows() {
    let mut grid = Grid::new(80, 5);
    grid.write_char('A');
    grid.scroll_up(10); // count > rows — should not panic, grid should be empty
    assert_eq!(grid.row_text(0), "");
}

#[test]
fn test_grid_scroll_down_count_exceeds_rows() {
    let mut grid = Grid::new(80, 5);
    grid.write_char('A');
    grid.scroll_down(10); // count > rows — should not panic
    assert_eq!(grid.row_text(4), "");
}

#[test]
fn test_grid_resize_clamps_cursor() {
    let mut grid = Grid::new(80, 24);
    grid.set_cursor(79, 23);
    grid.resize(40, 12);
    assert_eq!(grid.cursor_col(), 39);
    assert_eq!(grid.cursor_row(), 11);
}

#[test]
fn test_grid_combine_at_origin() {
    let mut grid = Grid::new(80, 24);
    // Zero-width char at (0,0) with no previous cell — should not panic.
    grid.write_char('\u{0301}'); // combining acute accent; no crash is success
}

#[test]
fn test_grid_minimum_dimensions() {
    let grid = Grid::new(1, 1);
    assert_eq!(grid.cols(), 1);
    assert_eq!(grid.rows(), 1);
}

#[test]
fn test_cell_reset() {
    let mut cell = Cell::default();
    cell.set_grapheme("X");
    cell.style_mut().set_foreground(Color::Rgb(255, 0, 0));
    cell.reset();
    assert_eq!(cell.grapheme(), " ");
    assert!(!cell.is_wide());
    assert_eq!(cell.style(), &PackedStyle::default());
}

#[test]
fn test_grid_write_char_scroll_at_bottom() {
    let mut grid = Grid::new(80, 3);
    grid.set_cursor(0, 2); // last row — write past the end to trigger wrap + scroll
    for c in "hello world this is a long line that will wrap".chars() {
        grid.write_char(c);
    }
    // Should not panic; content was scrolled
}

#[test]
fn test_grid_write_char_wide_on_narrow_grid() {
    let mut grid = Grid::new(1, 3);
    grid.write_char('世'); // width 2, can't fit in 1-col grid — should not panic
}

// ---------------------------------------------------------------------------
// Cursor getter tests
// ---------------------------------------------------------------------------

#[test]
fn test_cursor_getters() {
    let g = Grid::new(80, 24);
    let cursor = g.cursor();
    assert_eq!(cursor.col(), 0);
    assert_eq!(cursor.row(), 0);
    assert_eq!(cursor.style(), &PackedStyle::default());
    // Cursor is visible by default: DECTCEM (mode 25) is enabled in the
    // default mode set that Grid::new applies.
    assert!(cursor.is_visible());
}
