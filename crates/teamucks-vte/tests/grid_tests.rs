// Integration tests for cell model and grid structures.
// Written before implementation (TDD): all tests must compile with stubs and fail
// for the right reason before any implementation exists.

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
// GraphemeStorage tests
// ---------------------------------------------------------------------------

use teamucks_vte::cell::GraphemeStorage;

#[test]
fn test_grapheme_storage_inline_ascii() {
    let g = GraphemeStorage::new("A");
    assert_eq!(g.as_str(), "A");
}

#[test]
fn test_grapheme_storage_multi_byte() {
    // 'é' is U+00E9, encoded as 2 UTF-8 bytes: 0xC3 0xA9.
    let g = GraphemeStorage::new("é");
    assert_eq!(g.as_str(), "é");
}

#[test]
fn test_grapheme_storage_four_byte() {
    // '🎉' is U+1F389, encoded as 4 UTF-8 bytes — exactly fills the inline buffer.
    let g = GraphemeStorage::new("🎉");
    assert_eq!(g.as_str(), "🎉");
}

#[test]
fn test_grapheme_storage_multi_codepoint() {
    // Family emoji cluster: multiple codepoints joined by ZWJ — exceeds 4 bytes.
    let cluster = "👨‍👩‍👧";
    let g = GraphemeStorage::new(cluster);
    assert_eq!(g.as_str(), cluster);
}

// ---------------------------------------------------------------------------
// Cell tests
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

#[test]
fn test_cell_wide_flag() {
    let mut c = Cell::default();
    assert!(!c.is_wide());
    c.set_wide(true);
    assert!(c.is_wide());
    c.set_wide(false);
    assert!(!c.is_wide());
}

#[test]
fn test_cell_continuation_flag() {
    let mut c = Cell::default();
    assert!(!c.is_continuation());
    c.set_continuation(true);
    assert!(c.is_continuation());
}

// ---------------------------------------------------------------------------
// Row tests
// ---------------------------------------------------------------------------

#[test]
fn test_row_creation() {
    let row = Row::new(80);
    assert_eq!(row.len(), 80);
    assert!(!row.is_empty());
}

#[test]
fn test_row_cell_access() {
    let mut row = Row::new(80);
    row.cell_mut(0).set_grapheme("X");
    assert_eq!(row.cell(0).grapheme(), "X");
    // Other cells remain default.
    assert_eq!(row.cell(1).grapheme(), " ");
}

#[test]
fn test_row_soft_wrap_flag() {
    let mut row = Row::new(80);
    assert!(!row.is_soft_wrapped());
    row.set_soft_wrapped(true);
    assert!(row.is_soft_wrapped());
}

#[test]
fn test_row_resize_grow() {
    let mut row = Row::new(80);
    row.cell_mut(0).set_grapheme("A");
    row.resize(120);
    assert_eq!(row.len(), 120);
    // Original content preserved.
    assert_eq!(row.cell(0).grapheme(), "A");
    // New cells are default.
    assert_eq!(row.cell(119).grapheme(), " ");
}

#[test]
fn test_row_resize_shrink() {
    let mut row = Row::new(80);
    row.cell_mut(0).set_grapheme("A");
    row.resize(40);
    assert_eq!(row.len(), 40);
    assert_eq!(row.cell(0).grapheme(), "A");
}

#[test]
fn test_row_clear() {
    let mut row = Row::new(80);
    row.cell_mut(0).set_grapheme("Z");
    row.set_soft_wrapped(true);
    row.clear();
    assert_eq!(row.cell(0).grapheme(), " ");
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
    g.cursor_mut().wrap_pending = true;
    g.set_cursor(0, 0);
    assert!(!g.cursor().wrap_pending);
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
    // Cursor stays at col 79, wrap_pending is set.
    assert_eq!(g.cursor_col(), 79);
    assert!(g.cursor().wrap_pending);
}

#[test]
fn test_grid_write_char_pending_wrap_then_char() {
    let mut g = Grid::new(80, 24);
    g.set_cursor(79, 0);
    g.write_char('Z');
    assert!(g.cursor().wrap_pending);
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
