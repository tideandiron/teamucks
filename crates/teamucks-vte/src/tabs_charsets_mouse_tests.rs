// Integration tests for Feature 10: Tab Stops, Character Sets, and Mouse Modes.
//
// Tests are organised into four sections:
//   1. Tab stops (HT, CHT, CBT, HTS, TBC)
//   2. Character sets (ESC(0, ESC(B, ESC)0, ESC)B, SO, SI)
//   3. Mouse mode convenience method
//   4. Integration scenarios

#[cfg(test)]
mod tab_stop_tests {
    use crate::terminal::Terminal;

    // -----------------------------------------------------------------------
    // HT (0x09) — basic tab advancement
    // -----------------------------------------------------------------------

    #[test]
    fn test_default_tab_stops_every_8() {
        // Tab from column 0 → column 8.
        let mut t = Terminal::new(80, 24);
        t.feed(b"\t"); // HT
        assert_eq!(t.grid().cursor_col(), 8);
    }

    #[test]
    fn test_tab_from_col_3() {
        // Tab from column 3 → column 8.
        let mut t = Terminal::new(80, 24);
        t.feed(b"abc"); // cursor at col 3
        t.feed(b"\t");
        assert_eq!(t.grid().cursor_col(), 8);
    }

    #[test]
    fn test_tab_from_col_8() {
        // Tab from column 8 → column 16.
        let mut t = Terminal::new(80, 24);
        // Advance to col 8 with a tab.
        t.feed(b"\t");
        assert_eq!(t.grid().cursor_col(), 8);
        // Another tab: col 8 → col 16.
        t.feed(b"\t");
        assert_eq!(t.grid().cursor_col(), 16);
    }

    #[test]
    fn test_tab_at_end_of_line() {
        // Tab from col 78 in an 80-column terminal clamps to col 79.
        let mut t = Terminal::new(80, 24);
        // Move to col 72 (a tab stop), then move 6 more right to reach 78.
        t.feed(b"\x1b[79G"); // CHA: column 79 (1-indexed) → col 78 (0-indexed)
        assert_eq!(t.grid().cursor_col(), 78);
        t.feed(b"\t");
        // No stop beyond col 78 within the 80-col grid — clamped to 79.
        assert_eq!(t.grid().cursor_col(), 79);
    }

    // -----------------------------------------------------------------------
    // HTS (ESC H) — set tab stop at current column
    // -----------------------------------------------------------------------

    #[test]
    fn test_hts_set_tab_stop() {
        // Move to col 5, set a tab stop, then tab from col 0 → col 5.
        let mut t = Terminal::new(80, 24);
        t.feed(b"\x1b[6G"); // CHA col 5 (0-indexed), 1-indexed = 6
        assert_eq!(t.grid().cursor_col(), 5);
        t.feed(b"\x1bH"); // HTS — set tab stop at col 5
        t.feed(b"\x1b[1G"); // CHA back to col 0 (1-indexed)
        assert_eq!(t.grid().cursor_col(), 0);
        t.feed(b"\t"); // HT — should land at col 5
        assert_eq!(t.grid().cursor_col(), 5);
    }

    // -----------------------------------------------------------------------
    // TBC (CSI g) — clear tab stops
    // -----------------------------------------------------------------------

    #[test]
    fn test_tbc_clear_current() {
        // Set a custom stop at col 5, then clear it; tab from 0 skips to 8.
        let mut t = Terminal::new(80, 24);
        t.feed(b"\x1b[6G\x1bH"); // move to col 5, set HTS
        t.feed(b"\x1b[1G"); // back to col 0
        t.feed(b"\t"); // tab: land at col 5 (custom stop)
        assert_eq!(t.grid().cursor_col(), 5);

        // Go to col 5, clear the stop there.
        t.feed(b"\x1b[6G"); // move to col 5
        t.feed(b"\x1b[0g"); // TBC param 0: clear stop at current col
        t.feed(b"\x1b[1G"); // back to col 0
        t.feed(b"\t"); // tab: col 5 stop is gone, land at col 8
        assert_eq!(t.grid().cursor_col(), 8);
    }

    #[test]
    fn test_tbc_clear_all() {
        // TBC 3 clears all stops; tab from 0 goes to last col (79).
        let mut t = Terminal::new(80, 24);
        t.feed(b"\x1b[3g"); // TBC param 3: clear all tab stops
        t.feed(b"\t"); // no stops → clamp to col 79
        assert_eq!(t.grid().cursor_col(), 79);
    }

    // -----------------------------------------------------------------------
    // CHT (CSI n I) — Cursor Forward Tabulation
    // -----------------------------------------------------------------------

    #[test]
    fn test_cht_forward_tab_two_stops() {
        // CSI 2 I from col 0 → skip 2 tab stops → col 16.
        let mut t = Terminal::new(80, 24);
        t.feed(b"\x1b[2I"); // CHT 2
        assert_eq!(t.grid().cursor_col(), 16);
    }

    #[test]
    fn test_cht_default_is_one_stop() {
        // CSI I (no param) = CHT 1 = next tab stop.
        let mut t = Terminal::new(80, 24);
        t.feed(b"\x1b[I");
        assert_eq!(t.grid().cursor_col(), 8);
    }

    #[test]
    fn test_cht_clamps_at_last_column() {
        // CHT with a large n should clamp at the last column.
        let mut t = Terminal::new(80, 24);
        t.feed(b"\x1b[100I"); // way more tabs than stops
        assert_eq!(t.grid().cursor_col(), 79);
    }

    // -----------------------------------------------------------------------
    // CBT (CSI n Z) — Cursor Backward Tabulation
    // -----------------------------------------------------------------------

    #[test]
    fn test_cbt_backward_tab() {
        // CSI Z from col 20 → previous stop at col 16.
        let mut t = Terminal::new(80, 24);
        t.feed(b"\x1b[21G"); // CHA to col 20 (1-indexed = 21)
        assert_eq!(t.grid().cursor_col(), 20);
        t.feed(b"\x1b[Z"); // CBT 1
        assert_eq!(t.grid().cursor_col(), 16);
    }

    #[test]
    fn test_cbt_two_stops_backward() {
        // CSI 2 Z from col 20 → col 8 (two stops back).
        let mut t = Terminal::new(80, 24);
        t.feed(b"\x1b[21G"); // col 20
        t.feed(b"\x1b[2Z"); // CBT 2
        assert_eq!(t.grid().cursor_col(), 8);
    }

    #[test]
    fn test_cbt_clamps_at_column_zero() {
        // CBT from col 3 should end at col 0 (clamped).
        let mut t = Terminal::new(80, 24);
        t.feed(b"\x1b[4G"); // col 3 (1-indexed=4)
        t.feed(b"\x1b[Z"); // CBT 1
                           // Previous stop before col 3 is col 0.
        assert_eq!(t.grid().cursor_col(), 0);
    }

    // -----------------------------------------------------------------------
    // Integration: tab then write
    // -----------------------------------------------------------------------

    #[test]
    fn test_tab_and_write() {
        let mut t = Terminal::new(80, 24);
        t.feed(b"\t"); // advance to col 8
        t.feed(b"hi");
        // "hi" starts at col 8.
        assert_eq!(t.grid().cell(8, 0).grapheme(), "h");
        assert_eq!(t.grid().cell(9, 0).grapheme(), "i");
        assert_eq!(t.grid().cursor_col(), 10);
    }
}

#[cfg(test)]
mod charset_tests {
    use crate::terminal::Terminal;

    // -----------------------------------------------------------------------
    // G0 character set selection via ESC ( designator
    // -----------------------------------------------------------------------

    #[test]
    fn test_charset_g0_line_drawing_q() {
        // ESC ( 0 → set G0 to DEC Special Graphics. 'q' → '─' (horizontal line).
        let mut t = Terminal::new(80, 24);
        t.feed(b"\x1b(0"); // designate G0 = DEC Special Graphics
        t.feed(b"q"); // 'q' in DEC graphics = '─'
        assert_eq!(t.grid().cell(0, 0).grapheme(), "─");
    }

    #[test]
    fn test_charset_g0_ascii_restore() {
        // ESC ( B restores G0 to ASCII; 'q' should then be 'q'.
        let mut t = Terminal::new(80, 24);
        t.feed(b"\x1b(0"); // G0 = DEC Special Graphics
        t.feed(b"\x1b(B"); // G0 = ASCII
        t.feed(b"q");
        assert_eq!(t.grid().cell(0, 0).grapheme(), "q");
    }

    #[test]
    fn test_charset_g0_unmapped_passthrough() {
        // ESC ( 0 then 'A' — 'A' has no DEC graphics mapping, passes through.
        let mut t = Terminal::new(80, 24);
        t.feed(b"\x1b(0");
        t.feed(b"A");
        assert_eq!(t.grid().cell(0, 0).grapheme(), "A");
    }

    // -----------------------------------------------------------------------
    // G1 character set selection via ESC ) designator + SO/SI
    // -----------------------------------------------------------------------

    #[test]
    fn test_charset_g1_line_drawing_via_so() {
        // ESC ) 0 sets G1; SO (0x0E) activates G1; 'q' → '─'.
        let mut t = Terminal::new(80, 24);
        t.feed(b"\x1b)0"); // designate G1 = DEC Special Graphics
        t.feed(b"\x0e"); // SO — shift to G1
        t.feed(b"q");
        assert_eq!(t.grid().cell(0, 0).grapheme(), "─");
    }

    #[test]
    fn test_charset_si_activates_g0() {
        // SO shifts to G1, SI shifts back to G0.
        let mut t = Terminal::new(80, 24);
        t.feed(b"\x1b)0"); // G1 = DEC Special Graphics
        t.feed(b"\x0e"); // SO: activate G1
        t.feed(b"q"); // '─'
        t.feed(b"\x0f"); // SI: activate G0 (ASCII)
        t.feed(b"q"); // 'q' (ASCII)
        assert_eq!(t.grid().cell(0, 0).grapheme(), "─");
        assert_eq!(t.grid().cell(1, 0).grapheme(), "q");
    }

    #[test]
    fn test_charset_g1_ascii_restore() {
        // ESC ) B restores G1 to ASCII.
        let mut t = Terminal::new(80, 24);
        t.feed(b"\x1b)0"); // G1 = DEC Special Graphics
        t.feed(b"\x1b)B"); // G1 = ASCII
        t.feed(b"\x0e"); // SO: activate G1
        t.feed(b"q"); // Should be 'q' (ASCII), not '─'.
        assert_eq!(t.grid().cell(0, 0).grapheme(), "q");
    }

    // -----------------------------------------------------------------------
    // Integration: draw a simple box using line-drawing characters
    // -----------------------------------------------------------------------

    #[test]
    fn test_line_drawing_box() {
        // Draw: ┌─┐
        //       │ │
        //       └─┘
        let mut t = Terminal::new(80, 24);
        t.feed(b"\x1b(0"); // G0 = DEC Special Graphics

        // Row 0: ┌─┐
        t.feed(b"lqk");
        // Move to row 1, col 0.
        t.feed(b"\r\n");
        // Row 1: │ │
        t.feed(b"x x");
        t.feed(b"\r\n");
        // Row 2: └─┘
        t.feed(b"mqj");

        assert_eq!(t.grid().cell(0, 0).grapheme(), "┌");
        assert_eq!(t.grid().cell(1, 0).grapheme(), "─");
        assert_eq!(t.grid().cell(2, 0).grapheme(), "┐");
        assert_eq!(t.grid().cell(0, 1).grapheme(), "│");
        assert_eq!(t.grid().cell(1, 1).grapheme(), " ");
        assert_eq!(t.grid().cell(2, 1).grapheme(), "│");
        assert_eq!(t.grid().cell(0, 2).grapheme(), "└");
        assert_eq!(t.grid().cell(1, 2).grapheme(), "─");
        assert_eq!(t.grid().cell(2, 2).grapheme(), "┘");
    }
}

#[cfg(test)]
mod mouse_mode_tests {
    use crate::terminal::{MouseMode, Terminal};

    #[test]
    fn test_mouse_mode_none_by_default() {
        let t = Terminal::new(80, 24);
        assert_eq!(t.mouse_mode(), MouseMode::None);
    }

    #[test]
    fn test_mouse_mode_click() {
        let mut t = Terminal::new(80, 24);
        t.feed(b"\x1b[?1000h"); // set mode 1000
        assert_eq!(t.mouse_mode(), MouseMode::Click);
    }

    #[test]
    fn test_mouse_mode_button() {
        let mut t = Terminal::new(80, 24);
        t.feed(b"\x1b[?1002h"); // set mode 1002
        assert_eq!(t.mouse_mode(), MouseMode::ButtonEvent);
    }

    #[test]
    fn test_mouse_mode_all() {
        let mut t = Terminal::new(80, 24);
        t.feed(b"\x1b[?1003h"); // set mode 1003
        assert_eq!(t.mouse_mode(), MouseMode::AllMotion);
    }

    #[test]
    fn test_mouse_mode_button_overrides_click() {
        // When both 1000 and 1002 are set, ButtonEvent (1002) takes priority.
        let mut t = Terminal::new(80, 24);
        t.feed(b"\x1b[?1000h");
        t.feed(b"\x1b[?1002h");
        assert_eq!(t.mouse_mode(), MouseMode::ButtonEvent);
    }

    #[test]
    fn test_mouse_mode_all_overrides_button() {
        let mut t = Terminal::new(80, 24);
        t.feed(b"\x1b[?1002h");
        t.feed(b"\x1b[?1003h");
        assert_eq!(t.mouse_mode(), MouseMode::AllMotion);
    }

    #[test]
    fn test_mouse_mode_reset() {
        let mut t = Terminal::new(80, 24);
        t.feed(b"\x1b[?1000h");
        assert_eq!(t.mouse_mode(), MouseMode::Click);
        t.feed(b"\x1b[?1000l"); // reset mode 1000
        assert_eq!(t.mouse_mode(), MouseMode::None);
    }

    #[test]
    fn test_mouse_mode_all_reset_to_button() {
        // Resetting 1003 when 1002 is still active → ButtonEvent.
        let mut t = Terminal::new(80, 24);
        t.feed(b"\x1b[?1002h");
        t.feed(b"\x1b[?1003h");
        t.feed(b"\x1b[?1003l"); // reset 1003
        assert_eq!(t.mouse_mode(), MouseMode::ButtonEvent);
    }
}
