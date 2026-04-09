/// Tests for Feature 8: Terminal Modes (DECSET/DECRST).
///
/// Organised by escape sequence category, following the naming convention:
/// `test_<unit>_<scenario>`.
#[cfg(test)]
mod modes {
    use crate::{modes::ModeFlags, terminal::Terminal};

    // -----------------------------------------------------------------------
    // DECAWM — Auto-wrap mode
    // -----------------------------------------------------------------------

    /// Default terminal (DECAWM on): writing characters past the last column
    /// wraps to the beginning of the next row.
    #[test]
    fn test_decawm_on_wraps() {
        let mut t = Terminal::new(5, 4);
        // Default modes: AUTO_WRAP set.
        assert!(t.modes().contains(ModeFlags::AUTO_WRAP));
        // Write 6 characters to a 5-column terminal — the 6th should appear on row 1.
        t.feed(b"ABCDEF");
        assert_eq!(t.grid().row_text(0), "ABCDE");
        assert_eq!(t.grid().row_text(1), "F");
        assert_eq!(t.grid().cursor_row(), 1);
        assert_eq!(t.grid().cursor_col(), 1);
    }

    /// Disable DECAWM via `CSI ? 7 l`: writing past the last column overwrites
    /// the last cell in-place and the cursor stays at the final column.
    #[test]
    fn test_decawm_off_no_wrap() {
        let mut t = Terminal::new(5, 4);
        t.feed(b"\x1b[?7l"); // disable DECAWM
        assert!(!t.modes().contains(ModeFlags::AUTO_WRAP));
        // Write 7 characters — excess should overwrite the last cell.
        t.feed(b"ABCDEFG");
        // Row 0 should show "ABCDG" (D overwritten by E, F, G).
        assert_eq!(t.grid().row_text(0), "ABCDG");
        // Cursor stays at the last column.
        assert_eq!(t.grid().cursor_row(), 0);
        assert_eq!(t.grid().cursor_col(), 4);
    }

    /// Disable then re-enable DECAWM: wrapping resumes after `CSI ? 7 h`.
    #[test]
    fn test_decawm_off_then_on() {
        let mut t = Terminal::new(5, 4);
        t.feed(b"\x1b[?7l"); // disable
        t.feed(b"ABCDE"); // fills row 0 (no wrap)
        t.feed(b"\x1b[?7h"); // re-enable
        t.feed(b"FG"); // F should wrap to row 1, G lands at col 1
        assert_eq!(t.grid().row_text(0), "ABCDF");
        assert_eq!(t.grid().row_text(1), "G");
    }

    // -----------------------------------------------------------------------
    // DECOM — Origin mode
    // -----------------------------------------------------------------------

    /// With DECOM enabled, CUP row 1 col 1 positions the cursor at the top of
    /// the scroll region, not at (0,0) of the screen.
    #[test]
    fn test_decom_on_cup_relative() {
        let mut t = Terminal::new(80, 24);
        // Set scroll region rows 5-20 (1-indexed: CSI 5 ; 20 r).
        t.feed(b"\x1b[5;20r");
        // Enable DECOM.
        t.feed(b"\x1b[?6h");
        assert!(t.modes().contains(ModeFlags::ORIGIN));
        // CUP 1;1 — with DECOM on, row 1 means scroll-region top (row 4, 0-indexed).
        t.feed(b"\x1b[1;1H");
        assert_eq!(t.grid().cursor_row(), 4); // row 5 (1-idx) = row 4 (0-idx)
        assert_eq!(t.grid().cursor_col(), 0);
    }

    /// With DECOM on, CUP coordinates beyond the region bottom are clamped to
    /// the region bottom.
    #[test]
    fn test_decom_on_cup_clamped() {
        let mut t = Terminal::new(80, 24);
        t.feed(b"\x1b[5;10r"); // region rows 5-10
        t.feed(b"\x1b[?6h"); // enable DECOM
                             // CUP 20;1 — region is only 6 rows (5-10).  Should clamp to region bottom.
        t.feed(b"\x1b[20;1H");
        assert_eq!(t.grid().cursor_row(), 9); // row 10 (1-idx) = row 9 (0-idx)
    }

    /// Without DECOM (default), CUP uses absolute screen coordinates.
    #[test]
    fn test_decom_off_cup_absolute() {
        let mut t = Terminal::new(80, 24);
        t.feed(b"\x1b[5;20r"); // set scroll region (should not affect CUP)
                               // DECOM is off by default.
        assert!(!t.modes().contains(ModeFlags::ORIGIN));
        // CUP 3;5 → absolute row 2 col 4 (0-indexed).
        t.feed(b"\x1b[3;5H");
        assert_eq!(t.grid().cursor_row(), 2);
        assert_eq!(t.grid().cursor_col(), 4);
    }

    /// Enabling DECOM moves the cursor to the home position of the scroll region.
    #[test]
    fn test_decom_set_homes_cursor() {
        let mut t = Terminal::new(80, 24);
        t.feed(b"\x1b[5;20r"); // scroll region 5-20
                               // Move cursor somewhere.
        t.feed(b"\x1b[10;10H");
        // Enable DECOM — cursor should jump to region home.
        t.feed(b"\x1b[?6h");
        assert_eq!(t.grid().cursor_row(), 4); // region top (0-idx)
        assert_eq!(t.grid().cursor_col(), 0);
    }

    /// Disabling DECOM moves the cursor to screen home (0,0).
    #[test]
    fn test_decom_reset_homes_cursor() {
        let mut t = Terminal::new(80, 24);
        t.feed(b"\x1b[5;20r");
        t.feed(b"\x1b[?6h"); // enable DECOM (cursor at row 4)
        t.feed(b"\x1b[?6l"); // disable DECOM — cursor should go to (0,0)
        assert!(!t.modes().contains(ModeFlags::ORIGIN));
        assert_eq!(t.grid().cursor_row(), 0);
        assert_eq!(t.grid().cursor_col(), 0);
    }

    // -----------------------------------------------------------------------
    // DECTCEM — Cursor visibility
    // -----------------------------------------------------------------------

    /// `CSI ? 25 l` hides the cursor.
    #[test]
    fn test_dectcem_hide_cursor() {
        let mut t = Terminal::new(80, 24);
        t.feed(b"\x1b[?25l");
        assert!(!t.modes().contains(ModeFlags::CURSOR_VISIBLE));
        assert!(!t.grid().cursor().is_visible());
    }

    /// `CSI ? 25 h` makes the cursor visible.
    #[test]
    fn test_dectcem_show_cursor() {
        let mut t = Terminal::new(80, 24);
        t.feed(b"\x1b[?25l"); // hide first
        t.feed(b"\x1b[?25h"); // then show
        assert!(t.modes().contains(ModeFlags::CURSOR_VISIBLE));
        assert!(t.grid().cursor().is_visible());
    }

    /// The cursor is visible by default.
    #[test]
    fn test_dectcem_default_visible() {
        let t = Terminal::new(80, 24);
        assert!(t.modes().contains(ModeFlags::CURSOR_VISIBLE));
        assert!(t.grid().cursor().is_visible());
    }

    // -----------------------------------------------------------------------
    // DECCKM — Application cursor keys
    // -----------------------------------------------------------------------

    /// `CSI ? 1 h` sets application cursor key mode.
    #[test]
    fn test_decckm_set() {
        let mut t = Terminal::new(80, 24);
        t.feed(b"\x1b[?1h");
        assert!(t.modes().contains(ModeFlags::CURSOR_KEYS_APPLICATION));
    }

    /// `CSI ? 1 l` clears application cursor key mode.
    #[test]
    fn test_decckm_reset() {
        let mut t = Terminal::new(80, 24);
        t.feed(b"\x1b[?1h");
        t.feed(b"\x1b[?1l");
        assert!(!t.modes().contains(ModeFlags::CURSOR_KEYS_APPLICATION));
    }

    // -----------------------------------------------------------------------
    // Bracketed paste
    // -----------------------------------------------------------------------

    /// `CSI ? 2004 h` enables bracketed paste; `CSI ? 2004 l` disables it.
    #[test]
    fn test_bracketed_paste_mode() {
        let mut t = Terminal::new(80, 24);
        t.feed(b"\x1b[?2004h");
        assert!(t.modes().contains(ModeFlags::BRACKETED_PASTE));
        t.feed(b"\x1b[?2004l");
        assert!(!t.modes().contains(ModeFlags::BRACKETED_PASTE));
    }

    // -----------------------------------------------------------------------
    // Focus events
    // -----------------------------------------------------------------------

    /// `CSI ? 1004 h` enables focus event reporting.
    #[test]
    fn test_focus_events_mode() {
        let mut t = Terminal::new(80, 24);
        t.feed(b"\x1b[?1004h");
        assert!(t.modes().contains(ModeFlags::FOCUS_EVENTS));
    }

    // -----------------------------------------------------------------------
    // Mouse modes
    // -----------------------------------------------------------------------

    /// `CSI ? 1000 h` enables basic click mouse reporting.
    #[test]
    fn test_mouse_mode_1000() {
        let mut t = Terminal::new(80, 24);
        t.feed(b"\x1b[?1000h");
        assert!(t.modes().contains(ModeFlags::MOUSE_REPORT_CLICK));
    }

    /// `CSI ? 1002 h` enables button-event mouse tracking.
    #[test]
    fn test_mouse_mode_1002() {
        let mut t = Terminal::new(80, 24);
        t.feed(b"\x1b[?1002h");
        assert!(t.modes().contains(ModeFlags::MOUSE_REPORT_BUTTON));
    }

    /// `CSI ? 1003 h` enables all-motion mouse tracking.
    #[test]
    fn test_mouse_mode_1003() {
        let mut t = Terminal::new(80, 24);
        t.feed(b"\x1b[?1003h");
        assert!(t.modes().contains(ModeFlags::MOUSE_REPORT_ALL));
    }

    /// `CSI ? 1006 h` enables SGR mouse format.
    #[test]
    fn test_mouse_sgr_format() {
        let mut t = Terminal::new(80, 24);
        t.feed(b"\x1b[?1006h");
        assert!(t.modes().contains(ModeFlags::MOUSE_SGR_FORMAT));
    }

    /// `CSI ? 1000 l` disables basic click mouse reporting.
    #[test]
    fn test_mouse_mode_reset() {
        let mut t = Terminal::new(80, 24);
        t.feed(b"\x1b[?1000h");
        t.feed(b"\x1b[?1000l");
        assert!(!t.modes().contains(ModeFlags::MOUSE_REPORT_CLICK));
    }

    // -----------------------------------------------------------------------
    // Multiple modes in one sequence
    // -----------------------------------------------------------------------

    /// `CSI ? 25 ; 1 h` sets both DECTCEM and DECCKM in a single sequence.
    #[test]
    fn test_multiple_modes() {
        let mut t = Terminal::new(80, 24);
        // First clear DECTCEM so we can confirm the sequence re-sets it.
        t.feed(b"\x1b[?25l");
        t.feed(b"\x1b[?25;1h");
        assert!(t.modes().contains(ModeFlags::CURSOR_VISIBLE));
        assert!(t.modes().contains(ModeFlags::CURSOR_KEYS_APPLICATION));
    }

    // -----------------------------------------------------------------------
    // Unknown mode — must not crash and must not alter any known flag
    // -----------------------------------------------------------------------

    /// An unknown private mode number is silently ignored — no panic, no flag
    /// change.
    #[test]
    fn test_unknown_mode_ignored() {
        let mut t = Terminal::new(80, 24);
        let before = t.modes();
        t.feed(b"\x1b[?9999h");
        t.feed(b"\x1b[?9999l");
        assert_eq!(t.modes(), before);
    }

    // -----------------------------------------------------------------------
    // Integration: DECAWM off with wide characters
    // -----------------------------------------------------------------------

    /// Wide character at the end of a line with DECAWM off must not wrap;
    /// the cell at the last valid position is overwritten with a space
    /// placeholder and the cursor stays at the last column.
    #[test]
    fn test_decawm_off_with_wide_char() {
        // 5 columns: positions 0-4.  A wide char needs 2 cols.
        // Writing "ABC" puts cursor at col 3.  Then writing a wide char "Ａ"
        // (U+FF21 FULLWIDTH LATIN CAPITAL LETTER A, width 2) would need cols
        // 3 and 4 — that fits.  Writing a second wide char would need cols 4
        // and 5 — 5 is out of range.  With DECAWM off it should not wrap.
        let mut t = Terminal::new(6, 4);
        t.feed(b"\x1b[?7l"); // disable DECAWM
                             // Write "ABCD\xef\xbc\xa1" — that's ABCD + Ａ (fullwidth, 2 cols).
                             // Grid: A B C D Ａ▶  (Ａ occupies cols 4 and 5 — but col 5 is past
                             // the end on a 6-col grid? No — 6 cols are 0..5.  Ａ at col 4 needs
                             // cols 4+5, which is exactly the end.  Now write a second fullwidth:
                             // cursor would be at col 5 (last), which is also past needed 5+6 → no wrap.
        t.feed(b"ABCD");
        // cursor is now at col 4.
        // Write fullwidth 'Ａ' (U+FF21) — fits at cols 4-5.
        t.feed("\u{FF21}".as_bytes()); // Ａ
                                       // cursor is now at col 5, wrap_pending = true (would wrap if DECAWM were on).
                                       // Write another fullwidth — with DECAWM off, no wrap occurs; the last
                                       // cell is overwritten with a space/placeholder.
        t.feed("\u{FF22}".as_bytes()); // Ｂ
        assert_eq!(t.grid().cursor_row(), 0, "cursor must not wrap to next row");
    }
}
