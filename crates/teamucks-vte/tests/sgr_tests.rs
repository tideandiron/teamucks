//! SGR (Select Graphic Rendition) integration tests for [`Terminal`].
//!
//! Tests are organized by category: attribute flags, named colours, extended
//! colours, C0 control characters, OSC sequences, and integration scenarios.
//!
//! Every test follows the pattern: feed a byte sequence to a fresh
//! [`Terminal`], then inspect the grid for expected cell state.

use teamucks_vte::{
    style::{Attr, Color},
    terminal::Terminal,
};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Create an 80×24 terminal and feed `input`.
fn term(input: &[u8]) -> Terminal {
    let mut t = Terminal::new(80, 24);
    t.feed(input);
    t
}

/// Feed `input` to an 80×24 terminal and return the cell at `(col, row)`.
fn cell_at(input: &[u8], col: usize, row: usize) -> (String, Color, Color, Attr) {
    let t = term(input);
    let cell = t.grid().cell(col, row);
    (
        cell.grapheme().to_owned(),
        cell.style().foreground(),
        cell.style().background(),
        cell.style().attrs(),
    )
}

/// Feed, write 'A', return the cell at (0,0).
fn styled_a(sgr: &[u8]) -> (Color, Color, Attr) {
    let mut input = sgr.to_vec();
    input.push(b'A');
    let (_, fg, bg, attrs) = cell_at(&input, 0, 0);
    (fg, bg, attrs)
}

// ---------------------------------------------------------------------------
// SGR attribute flags
// ---------------------------------------------------------------------------

#[test]
fn test_sgr_bold() {
    let (_, _, attrs) = styled_a(b"\x1b[1m");
    assert!(attrs.contains(Attr::BOLD), "expected BOLD: got {attrs:?}");
}

#[test]
fn test_sgr_dim() {
    let (_, _, attrs) = styled_a(b"\x1b[2m");
    assert!(attrs.contains(Attr::DIM), "expected DIM: got {attrs:?}");
}

#[test]
fn test_sgr_italic() {
    let (_, _, attrs) = styled_a(b"\x1b[3m");
    assert!(attrs.contains(Attr::ITALIC), "expected ITALIC: got {attrs:?}");
}

#[test]
fn test_sgr_underline() {
    let (_, _, attrs) = styled_a(b"\x1b[4m");
    assert!(attrs.contains(Attr::UNDERLINE), "expected UNDERLINE: got {attrs:?}");
}

#[test]
fn test_sgr_blink() {
    let (_, _, attrs) = styled_a(b"\x1b[5m");
    assert!(attrs.contains(Attr::BLINK), "expected BLINK: got {attrs:?}");
}

#[test]
fn test_sgr_inverse() {
    let (_, _, attrs) = styled_a(b"\x1b[7m");
    assert!(attrs.contains(Attr::INVERSE), "expected INVERSE: got {attrs:?}");
}

#[test]
fn test_sgr_hidden() {
    let (_, _, attrs) = styled_a(b"\x1b[8m");
    assert!(attrs.contains(Attr::HIDDEN), "expected HIDDEN: got {attrs:?}");
}

#[test]
fn test_sgr_strikethrough() {
    let (_, _, attrs) = styled_a(b"\x1b[9m");
    assert!(attrs.contains(Attr::STRIKETHROUGH), "expected STRIKETHROUGH: got {attrs:?}");
}

#[test]
fn test_sgr_curly_underline_sgr21() {
    // SGR 21 sets CURLY_UNDERLINE in this implementation.
    let (_, _, attrs) = styled_a(b"\x1b[21m");
    assert!(attrs.contains(Attr::CURLY_UNDERLINE), "expected CURLY_UNDERLINE: got {attrs:?}");
}

// ---------------------------------------------------------------------------
// SGR attribute clear codes
// ---------------------------------------------------------------------------

#[test]
fn test_sgr_clear_bold() {
    // Set bold, then clear with SGR 22.
    let (_, _, attrs) = styled_a(b"\x1b[1m\x1b[22m");
    assert!(!attrs.contains(Attr::BOLD), "expected BOLD cleared: got {attrs:?}");
}

#[test]
fn test_sgr_clear_dim() {
    // SGR 22 also clears DIM.
    let (_, _, attrs) = styled_a(b"\x1b[2m\x1b[22m");
    assert!(!attrs.contains(Attr::DIM), "expected DIM cleared: got {attrs:?}");
}

#[test]
fn test_sgr_clear_italic() {
    let (_, _, attrs) = styled_a(b"\x1b[3m\x1b[23m");
    assert!(!attrs.contains(Attr::ITALIC), "expected ITALIC cleared: got {attrs:?}");
}

#[test]
fn test_sgr_clear_underline() {
    let (_, _, attrs) = styled_a(b"\x1b[4m\x1b[24m");
    assert!(!attrs.contains(Attr::UNDERLINE), "expected UNDERLINE cleared: got {attrs:?}");
}

#[test]
fn test_sgr_clear_blink() {
    let (_, _, attrs) = styled_a(b"\x1b[5m\x1b[25m");
    assert!(!attrs.contains(Attr::BLINK), "expected BLINK cleared: got {attrs:?}");
}

#[test]
fn test_sgr_clear_inverse() {
    let (_, _, attrs) = styled_a(b"\x1b[7m\x1b[27m");
    assert!(!attrs.contains(Attr::INVERSE), "expected INVERSE cleared: got {attrs:?}");
}

#[test]
fn test_sgr_clear_hidden() {
    let (_, _, attrs) = styled_a(b"\x1b[8m\x1b[28m");
    assert!(!attrs.contains(Attr::HIDDEN), "expected HIDDEN cleared: got {attrs:?}");
}

#[test]
fn test_sgr_clear_strikethrough() {
    let (_, _, attrs) = styled_a(b"\x1b[9m\x1b[29m");
    assert!(!attrs.contains(Attr::STRIKETHROUGH), "expected STRIKETHROUGH cleared: got {attrs:?}");
}

// ---------------------------------------------------------------------------
// SGR reset (code 0)
// ---------------------------------------------------------------------------

#[test]
fn test_sgr_reset_clears_all_attributes() {
    // Set bold + italic, then reset with SGR 0.
    let (fg, bg, attrs) = styled_a(b"\x1b[1m\x1b[3m\x1b[0m");
    assert_eq!(attrs, Attr::empty(), "all attrs must be cleared by SGR 0");
    assert_eq!(fg, Color::Default);
    assert_eq!(bg, Color::Default);
}

#[test]
fn test_sgr_reset_combined_with_attrs() {
    // "\x1b[1;3m" then "\x1b[0m".
    let (fg, bg, attrs) = styled_a(b"\x1b[1;3m\x1b[0m");
    assert_eq!(attrs, Attr::empty());
    assert_eq!(fg, Color::Default);
    assert_eq!(bg, Color::Default);
}

#[test]
fn test_sgr_no_params_is_reset() {
    // "\x1b[m" with no params is equivalent to SGR 0.
    let (fg, bg, attrs) = styled_a(b"\x1b[1m\x1b[m");
    assert_eq!(attrs, Attr::empty(), "no-param SGR must reset all attrs");
    assert_eq!(fg, Color::Default);
    assert_eq!(bg, Color::Default);
}

// ---------------------------------------------------------------------------
// Named foreground colours (30-37)
// ---------------------------------------------------------------------------

#[test]
fn test_sgr_fg_named_black() {
    let (fg, _, _) = styled_a(b"\x1b[30m");
    assert_eq!(fg, Color::Indexed(0));
}

#[test]
fn test_sgr_fg_named_red() {
    let (fg, _, _) = styled_a(b"\x1b[31m");
    assert_eq!(fg, Color::Indexed(1));
}

#[test]
fn test_sgr_fg_named_green() {
    let (fg, _, _) = styled_a(b"\x1b[32m");
    assert_eq!(fg, Color::Indexed(2));
}

#[test]
fn test_sgr_fg_named_yellow() {
    let (fg, _, _) = styled_a(b"\x1b[33m");
    assert_eq!(fg, Color::Indexed(3));
}

#[test]
fn test_sgr_fg_named_blue() {
    let (fg, _, _) = styled_a(b"\x1b[34m");
    assert_eq!(fg, Color::Indexed(4));
}

#[test]
fn test_sgr_fg_named_magenta() {
    let (fg, _, _) = styled_a(b"\x1b[35m");
    assert_eq!(fg, Color::Indexed(5));
}

#[test]
fn test_sgr_fg_named_cyan() {
    let (fg, _, _) = styled_a(b"\x1b[36m");
    assert_eq!(fg, Color::Indexed(6));
}

#[test]
fn test_sgr_fg_named_white() {
    let (fg, _, _) = styled_a(b"\x1b[37m");
    assert_eq!(fg, Color::Indexed(7));
}

#[test]
fn test_sgr_fg_named_all_30_to_37() {
    for n in 0u8..8 {
        let code = 30 + n;
        let seq = format!("\x1b[{code}mA");
        let t = term(seq.as_bytes());
        let fg = t.grid().cell(0, 0).style().foreground();
        assert_eq!(fg, Color::Indexed(n), "SGR {code}: expected Indexed({n}), got {fg:?}");
    }
}

// ---------------------------------------------------------------------------
// Named background colours (40-47)
// ---------------------------------------------------------------------------

#[test]
fn test_sgr_bg_named_green() {
    let (_, bg, _) = styled_a(b"\x1b[42m");
    assert_eq!(bg, Color::Indexed(2));
}

#[test]
fn test_sgr_bg_named_all_40_to_47() {
    for n in 0u8..8 {
        let code = 40 + n;
        let seq = format!("\x1b[{code}mA");
        let t = term(seq.as_bytes());
        let bg = t.grid().cell(0, 0).style().background();
        assert_eq!(bg, Color::Indexed(n), "SGR {code}: expected Indexed({n}), got {bg:?}");
    }
}

// ---------------------------------------------------------------------------
// Bright (high-intensity) colours (90-97 fg, 100-107 bg)
// ---------------------------------------------------------------------------

#[test]
fn test_sgr_fg_bright_red() {
    let (fg, _, _) = styled_a(b"\x1b[91m");
    assert_eq!(fg, Color::Indexed(9));
}

#[test]
fn test_sgr_fg_bright_all_90_to_97() {
    for n in 0u8..8 {
        let code = 90 + n;
        let seq = format!("\x1b[{code}mA");
        let t = term(seq.as_bytes());
        let fg = t.grid().cell(0, 0).style().foreground();
        assert_eq!(
            fg,
            Color::Indexed(8 + n),
            "SGR {code}: expected Indexed({}), got {fg:?}",
            8 + n
        );
    }
}

#[test]
fn test_sgr_bg_bright() {
    let (_, bg, _) = styled_a(b"\x1b[100m");
    assert_eq!(bg, Color::Indexed(8));
}

#[test]
fn test_sgr_bg_bright_all_100_to_107() {
    for n in 0u8..8 {
        let code = 100 + n;
        let seq = format!("\x1b[{code}mA");
        let t = term(seq.as_bytes());
        let bg = t.grid().cell(0, 0).style().background();
        assert_eq!(
            bg,
            Color::Indexed(8 + n),
            "SGR {code}: expected Indexed({}), got {bg:?}",
            8 + n
        );
    }
}

// ---------------------------------------------------------------------------
// Extended colour: 256-colour indexed (38;5;N and 48;5;N)
// ---------------------------------------------------------------------------

#[test]
fn test_sgr_fg_256() {
    let (fg, _, _) = styled_a(b"\x1b[38;5;42m");
    assert_eq!(fg, Color::Indexed(42));
}

#[test]
fn test_sgr_bg_256() {
    let (_, bg, _) = styled_a(b"\x1b[48;5;200m");
    assert_eq!(bg, Color::Indexed(200));
}

#[test]
fn test_sgr_fg_256_boundary_zero() {
    let (fg, _, _) = styled_a(b"\x1b[38;5;0m");
    assert_eq!(fg, Color::Indexed(0));
}

#[test]
fn test_sgr_fg_256_boundary_255() {
    let (fg, _, _) = styled_a(b"\x1b[38;5;255m");
    assert_eq!(fg, Color::Indexed(255));
}

// ---------------------------------------------------------------------------
// Extended colour: RGB true colour (38;2;R;G;B and 48;2;R;G;B)
// ---------------------------------------------------------------------------

#[test]
fn test_sgr_fg_rgb() {
    let (fg, _, _) = styled_a(b"\x1b[38;2;255;128;0m");
    assert_eq!(fg, Color::Rgb(255, 128, 0));
}

#[test]
fn test_sgr_bg_rgb() {
    let (_, bg, _) = styled_a(b"\x1b[48;2;0;255;128m");
    assert_eq!(bg, Color::Rgb(0, 255, 128));
}

#[test]
fn test_sgr_fg_rgb_all_zeros() {
    let (fg, _, _) = styled_a(b"\x1b[38;2;0;0;0m");
    assert_eq!(fg, Color::Rgb(0, 0, 0));
}

#[test]
fn test_sgr_fg_rgb_all_255() {
    let (fg, _, _) = styled_a(b"\x1b[38;2;255;255;255m");
    assert_eq!(fg, Color::Rgb(255, 255, 255));
}

// ---------------------------------------------------------------------------
// Default colour reset codes (39, 49)
// ---------------------------------------------------------------------------

#[test]
fn test_sgr_reset_fg_to_default() {
    // Set red fg, then reset to default with SGR 39.
    let (fg, _, _) = styled_a(b"\x1b[31m\x1b[39m");
    assert_eq!(fg, Color::Default);
}

#[test]
fn test_sgr_reset_bg_to_default() {
    // Set green bg, then reset to default with SGR 49.
    let (_, bg, _) = styled_a(b"\x1b[42m\x1b[49m");
    assert_eq!(bg, Color::Default);
}

// ---------------------------------------------------------------------------
// Multiple parameters in a single sequence
// ---------------------------------------------------------------------------

#[test]
fn test_sgr_multiple_in_one_sequence() {
    // "\x1b[1;31;42m" — bold + red fg + green bg.
    let (fg, bg, attrs) = styled_a(b"\x1b[1;31;42m");
    assert!(attrs.contains(Attr::BOLD), "expected BOLD");
    assert_eq!(fg, Color::Indexed(1), "expected red fg");
    assert_eq!(bg, Color::Indexed(2), "expected green bg");
}

#[test]
fn test_sgr_bold_persists_across_chars() {
    // "\x1b[1mhi" — both 'h' and 'i' should have BOLD.
    let t = term(b"\x1b[1mhi");
    let h_attrs = t.grid().cell(0, 0).style().attrs();
    let i_attrs = t.grid().cell(1, 0).style().attrs();
    assert!(h_attrs.contains(Attr::BOLD), "'h' must have BOLD");
    assert!(i_attrs.contains(Attr::BOLD), "'i' must have BOLD");
}

// ---------------------------------------------------------------------------
// Robustness: invalid / unknown codes must not crash
// ---------------------------------------------------------------------------

#[test]
fn test_sgr_extended_color_invalid_missing_index() {
    // "\x1b[38;5m" — 5 subcommand with no following index.
    // Must not panic and must leave terminal in a consistent state.
    let t = term(b"\x1b[38;5mA");
    // After writing 'A', cell (0,0) should exist and be 'A'.
    assert_eq!(t.grid().cell(0, 0).grapheme(), "A");
}

#[test]
fn test_sgr_extended_color_invalid_wrong_subtype() {
    // "\x1b[38;9m" — unknown sub-type (not 2 or 5).
    // Must not panic.
    let t = term(b"\x1b[38;9mA");
    assert_eq!(t.grid().cell(0, 0).grapheme(), "A");
}

#[test]
fn test_sgr_unknown_code_ignored() {
    // "\x1b[99m" is not a defined SGR code — should be silently ignored.
    let (fg, bg, attrs) = styled_a(b"\x1b[99m");
    assert_eq!(fg, Color::Default);
    assert_eq!(bg, Color::Default);
    assert_eq!(attrs, Attr::empty());
}

#[test]
fn test_sgr_rgb_missing_component_no_crash() {
    // "\x1b[38;2;255m" — RGB with only R, no G or B.
    let t = term(b"\x1b[38;2;255mA");
    assert_eq!(t.grid().cell(0, 0).grapheme(), "A");
}

// ---------------------------------------------------------------------------
// C0 control character tests
// ---------------------------------------------------------------------------

#[test]
fn test_execute_cr_sets_col_to_zero() {
    // Write some chars, then CR — cursor should be at column 0.
    let t = term(b"hello\r");
    assert_eq!(t.grid().cursor_col(), 0);
    assert_eq!(t.grid().cursor_row(), 0);
}

#[test]
fn test_execute_lf_moves_cursor_down() {
    let t = term(b"hello\n");
    assert_eq!(t.grid().cursor_row(), 1);
}

#[test]
fn test_execute_lf_scrolls_at_bottom() {
    // Feed 24 newlines on a 24-row terminal — cursor must stay at last row.
    let mut t = Terminal::new(80, 24);
    for _ in 0..24 {
        t.feed(b"\n");
    }
    assert_eq!(t.grid().cursor_row(), 23, "cursor must stay at last row");
}

#[test]
fn test_execute_lf_content_scrolls() {
    // Write 'A' on row 0, then fill all rows with LF, then write 'B'.
    // 'A' should have scrolled off; row 23 should have 'B'.
    let mut t = Terminal::new(80, 24);
    t.feed(b"A");
    for _ in 0..24 {
        t.feed(b"\n");
    }
    t.feed(b"B");
    // Row 0 should no longer contain 'A' (it scrolled off).
    assert_ne!(t.grid().row_text(0), "A", "'A' should have scrolled off");
    // Last row should contain 'B'.
    assert!(t.grid().row_text(23).contains('B'), "row 23 must contain 'B'");
}

#[test]
fn test_execute_bs_moves_cursor_left() {
    let t = term(b"ab\x08");
    assert_eq!(t.grid().cursor_col(), 1);
}

#[test]
fn test_execute_bs_clamps_at_zero() {
    // BS at column 0 must stay at 0.
    let t = term(b"\x08");
    assert_eq!(t.grid().cursor_col(), 0);
}

#[test]
fn test_execute_tab_moves_to_next_tab_stop() {
    // Default tab stops every 8 columns; from col 0, tab goes to col 8.
    let t = term(b"\t");
    assert_eq!(t.grid().cursor_col(), 8);
}

#[test]
fn test_execute_tab_from_nonzero_col() {
    // From col 1 (after 'A'), tab goes to col 8.
    let t = term(b"A\t");
    assert_eq!(t.grid().cursor_col(), 8);
}

#[test]
fn test_execute_tab_multiple_stops() {
    // Two tabs from col 0 → col 8 → col 16.
    let t = term(b"\t\t");
    assert_eq!(t.grid().cursor_col(), 16);
}

#[test]
fn test_execute_tab_at_last_column_clamps() {
    // On an 8-column terminal, tab from col 0 would want col 8 but must
    // clamp to col 7 (last col).
    let mut t = Terminal::new(8, 4);
    t.feed(b"\t");
    assert_eq!(t.grid().cursor_col(), 7, "tab at last col must clamp to last col");
}

#[test]
fn test_execute_vt_moves_cursor_down() {
    // VT (0x0B) behaves like LF.
    let t = term(b"\x0B");
    assert_eq!(t.grid().cursor_row(), 1);
}

#[test]
fn test_execute_ff_moves_cursor_down() {
    // FF (0x0C) behaves like LF.
    let t = term(b"\x0C");
    assert_eq!(t.grid().cursor_row(), 1);
}

// ---------------------------------------------------------------------------
// OSC sequence tests
// ---------------------------------------------------------------------------

#[test]
fn test_osc_set_title_osc0() {
    let t = term(b"\x1b]0;hello\x07");
    assert_eq!(t.title(), "hello");
}

#[test]
fn test_osc_set_title_osc2() {
    let t = term(b"\x1b]2;world\x07");
    assert_eq!(t.title(), "world");
}

#[test]
fn test_osc_set_title_with_st_terminator() {
    // ST (String Terminator) = ESC backslash.
    let t = term(b"\x1b]0;my title\x1b\\");
    assert_eq!(t.title(), "my title");
}

#[test]
fn test_osc_unknown_command_ignored() {
    // OSC 99 is not handled — title should remain empty.
    let t = term(b"\x1b]99;ignored\x07");
    assert_eq!(t.title(), "");
}

#[test]
fn test_osc_title_update() {
    // Set title twice — second update wins.
    let t = term(b"\x1b]0;first\x07\x1b]0;second\x07");
    assert_eq!(t.title(), "second");
}

// ---------------------------------------------------------------------------
// Integration tests
// ---------------------------------------------------------------------------

#[test]
fn test_terminal_feed_mixed_text_and_sgr() {
    // "hello\x1b[31m world\x1b[0m!" — correct text and styles.
    let t = term(b"hello\x1b[31m world\x1b[0m!");
    // Positions 0-4: "hello" — default style.
    for col in 0..5 {
        let fg = t.grid().cell(col, 0).style().foreground();
        assert_eq!(fg, Color::Default, "col {col} must have default fg");
    }
    // Position 5 is space, positions 6-10 are " world", with red fg.
    for col in 5..11 {
        let fg = t.grid().cell(col, 0).style().foreground();
        assert_eq!(fg, Color::Indexed(1), "col {col} must have red fg");
    }
    // Position 11: '!' — default fg (after reset).
    let bang_fg = t.grid().cell(11, 0).style().foreground();
    assert_eq!(bang_fg, Color::Default, "! must have default fg after reset");
}

#[test]
fn test_terminal_resize() {
    let mut t = Terminal::new(80, 24);
    t.feed(b"hello");
    t.resize(40, 12);
    assert_eq!(t.grid().cols(), 40);
    assert_eq!(t.grid().rows(), 12);
}

#[test]
fn test_terminal_title_empty_by_default() {
    let t = Terminal::new(80, 24);
    assert_eq!(t.title(), "");
}

#[test]
fn test_sgr_combined_attrs_and_color() {
    // "\x1b[1;4;32m" — bold + underline + green fg.
    let (fg, _, attrs) = styled_a(b"\x1b[1;4;32m");
    assert!(attrs.contains(Attr::BOLD));
    assert!(attrs.contains(Attr::UNDERLINE));
    assert_eq!(fg, Color::Indexed(2));
}

#[test]
fn test_sgr_256_fg_then_reset() {
    // Set 256-colour fg, then reset — fg should be Default.
    let (fg, _, attrs) = styled_a(b"\x1b[38;5;100m\x1b[0m");
    assert_eq!(fg, Color::Default);
    assert_eq!(attrs, Attr::empty());
}

#[test]
fn test_sgr_rgb_bg_then_reset() {
    // Set RGB bg, then reset — bg should be Default.
    let (_, bg, _) = styled_a(b"\x1b[48;2;10;20;30m\x1b[0m");
    assert_eq!(bg, Color::Default);
}
