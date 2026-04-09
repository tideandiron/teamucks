/// Tests for Feature 9: Alternate Screen Buffer (DECSET/DECRST 1049).
///
/// Organised by escape sequence category, following the naming convention:
/// `test_<unit>_<scenario>`.
#[cfg(test)]
mod alternate_screen {
    use crate::terminal::Terminal;

    // -----------------------------------------------------------------------
    // Basic enter / exit
    // -----------------------------------------------------------------------

    /// `CSI ? 1049 h` switches to a blank alternate screen.
    ///
    /// After entering, every row in the visible grid must be empty.
    #[test]
    fn test_enter_alternate_screen() {
        let mut t = Terminal::new(10, 5);
        // Write some content on the primary screen.
        t.feed(b"hello");
        assert_eq!(t.grid().row_text(0), "hello");

        // Enter alternate screen.
        t.feed(b"\x1b[?1049h");

        // The grid must now report being in the alternate screen.
        assert!(t.grid().is_alternate_screen());

        // Every row must be blank.
        for row in 0..t.grid().rows() {
            assert_eq!(
                t.grid().row_text(row),
                "",
                "row {row} must be blank after entering alternate screen"
            );
        }
    }

    /// `CSI ? 1049 l` exits the alternate screen and restores the original
    /// content that was visible before the switch.
    #[test]
    fn test_exit_alternate_screen() {
        let mut t = Terminal::new(10, 5);
        t.feed(b"hello");
        t.feed(b"\x1b[?1049h");
        t.feed(b"\x1b[?1049l");

        // Must no longer be in alternate screen.
        assert!(!t.grid().is_alternate_screen());

        // Original content must be restored.
        assert_eq!(t.grid().row_text(0), "hello");
    }

    /// Content written while in the alternate screen must not appear after
    /// exiting — only the original primary screen content is visible.
    #[test]
    fn test_alternate_screen_content_isolated() {
        let mut t = Terminal::new(20, 5);
        // Write primary content.
        t.feed(b"primary content");
        // Enter alternate screen.
        t.feed(b"\x1b[?1049h");
        // Write alternate content.
        t.feed(b"alternate content");
        // Verify alternate content is visible while in alt screen.
        assert_eq!(t.grid().row_text(0), "alternate content");
        // Exit alternate screen.
        t.feed(b"\x1b[?1049l");
        // Primary content must be back; alternate content must be gone.
        assert_eq!(t.grid().row_text(0), "primary content");
        for row in 1..t.grid().rows() {
            assert_eq!(t.grid().row_text(row), "");
        }
    }

    // -----------------------------------------------------------------------
    // Cursor save/restore across alternate screen
    // -----------------------------------------------------------------------

    /// Entering the alternate screen saves the cursor position; exiting
    /// restores it to where it was before entering.
    #[test]
    fn test_alternate_screen_saves_cursor() {
        let mut t = Terminal::new(20, 10);
        // Move cursor to a known position on the primary screen.
        t.feed(b"\x1b[4;7H"); // row=4, col=7 (1-indexed) → row=3, col=6 (0-indexed)
        assert_eq!(t.grid().cursor_row(), 3);
        assert_eq!(t.grid().cursor_col(), 6);

        // Enter alternate screen (should save cursor at (3,6)).
        t.feed(b"\x1b[?1049h");
        // Move cursor somewhere else in alt screen.
        t.feed(b"\x1b[2;2H");
        assert_eq!(t.grid().cursor_row(), 1);
        assert_eq!(t.grid().cursor_col(), 1);

        // Exit alternate screen — cursor must be restored to (3,6).
        t.feed(b"\x1b[?1049l");
        assert_eq!(t.grid().cursor_row(), 3);
        assert_eq!(t.grid().cursor_col(), 6);
    }

    /// Entering the alternate screen must reset the cursor to (0, 0).
    #[test]
    fn test_alternate_screen_cursor_at_origin() {
        let mut t = Terminal::new(20, 10);
        // Position cursor away from origin.
        t.feed(b"\x1b[5;10H");
        assert_eq!(t.grid().cursor_row(), 4);
        assert_eq!(t.grid().cursor_col(), 9);

        // Enter alternate screen.
        t.feed(b"\x1b[?1049h");

        // Cursor must be at origin.
        assert_eq!(t.grid().cursor_row(), 0);
        assert_eq!(t.grid().cursor_col(), 0);
    }

    // -----------------------------------------------------------------------
    // Nested DECSET 1049 (no stacking)
    // -----------------------------------------------------------------------

    /// Sending DECSET 1049 while already in the alternate screen is a no-op —
    /// the state does not stack.  A single DECRST 1049 exits completely.
    #[test]
    fn test_nested_alternate_ignored() {
        let mut t = Terminal::new(10, 5);
        t.feed(b"primary");
        // Enter alternate screen.
        t.feed(b"\x1b[?1049h");
        t.feed(b"first alt");
        // "Enter" again — must be a no-op, should not save the alt screen state.
        t.feed(b"\x1b[?1049h");
        t.feed(b"second alt");
        // A single exit must restore the original primary content.
        t.feed(b"\x1b[?1049l");
        assert!(!t.grid().is_alternate_screen());
        assert_eq!(t.grid().row_text(0), "primary");
    }

    // -----------------------------------------------------------------------
    // Exit without prior enter — must be a no-op
    // -----------------------------------------------------------------------

    /// DECRST 1049 without a preceding DECSET 1049 must be a no-op.
    ///
    /// The grid content, cursor, and mode flags must remain unchanged.
    #[test]
    fn test_exit_without_enter_noop() {
        let mut t = Terminal::new(10, 5);
        t.feed(b"hello");
        let row0_before = t.grid().row_text(0).clone();
        let cur_row_before = t.grid().cursor_row();
        let cur_col_before = t.grid().cursor_col();

        // Exit alternate screen without having entered it.
        t.feed(b"\x1b[?1049l");

        assert!(!t.grid().is_alternate_screen());
        assert_eq!(t.grid().row_text(0), row0_before);
        assert_eq!(t.grid().cursor_row(), cur_row_before);
        assert_eq!(t.grid().cursor_col(), cur_col_before);
    }

    // -----------------------------------------------------------------------
    // Scroll region reset on enter
    // -----------------------------------------------------------------------

    /// After entering the alternate screen the scroll region must cover the
    /// full screen (0, rows-1), regardless of what was set on the primary.
    #[test]
    fn test_alternate_screen_scroll_region_reset() {
        let mut t = Terminal::new(20, 10);
        // Set a non-default scroll region on the primary screen.
        t.feed(b"\x1b[3;8r"); // rows 3-8 (1-indexed) → (2,7) 0-indexed
        assert_eq!(t.grid().scroll_region(), (2, 7));

        // Enter alternate screen — scroll region must reset to full screen.
        t.feed(b"\x1b[?1049h");
        assert_eq!(t.grid().scroll_region(), (0, t.grid().rows() - 1));
    }

    // -----------------------------------------------------------------------
    // Resize while in alternate screen
    // -----------------------------------------------------------------------

    /// Resizing while in the alternate screen must adjust the grid dimensions
    /// without performing any reflow.  The alternate screen contents must
    /// reflect the new dimensions, and exiting must restore the primary content.
    #[test]
    fn test_alternate_screen_resize() {
        let mut t = Terminal::new(10, 5);
        t.feed(b"primary");
        t.feed(b"\x1b[?1049h");
        t.feed(b"alt text");

        // Resize while in alt screen.
        t.resize(20, 10);

        // Alt screen dimensions must update.
        assert_eq!(t.grid().cols(), 20);
        assert_eq!(t.grid().rows(), 10);
        assert!(t.grid().is_alternate_screen());

        // Exit — primary content must be restored (primary was also resized).
        t.feed(b"\x1b[?1049l");
        assert_eq!(t.grid().row_text(0), "primary");
        assert_eq!(t.grid().cols(), 20);
        assert_eq!(t.grid().rows(), 10);
    }

    // -----------------------------------------------------------------------
    // Write in alternate screen, verify isolation
    // -----------------------------------------------------------------------

    /// Write text in the alternate screen, verify it appears there, then exit
    /// and verify the primary screen is completely unaffected.
    #[test]
    fn test_write_in_alternate() {
        let mut t = Terminal::new(20, 5);
        // Set up known primary content.
        t.feed(b"\x1b[1;1H"); // cursor to (0,0)
        t.feed(b"line one");
        t.feed(b"\r\n");
        t.feed(b"line two");

        assert_eq!(t.grid().row_text(0), "line one");
        assert_eq!(t.grid().row_text(1), "line two");

        // Enter alternate screen.
        t.feed(b"\x1b[?1049h");

        // Alt screen is blank.
        assert_eq!(t.grid().row_text(0), "");
        assert_eq!(t.grid().row_text(1), "");

        // Write to alternate screen.
        t.feed(b"alt line A");
        t.feed(b"\r\n");
        t.feed(b"alt line B");

        assert_eq!(t.grid().row_text(0), "alt line A");
        assert_eq!(t.grid().row_text(1), "alt line B");

        // Exit alternate screen.
        t.feed(b"\x1b[?1049l");

        // Primary content must be intact, alternate content must be gone.
        assert_eq!(t.grid().row_text(0), "line one");
        assert_eq!(t.grid().row_text(1), "line two");
    }

    // -----------------------------------------------------------------------
    // Cursor visibility on enter
    // -----------------------------------------------------------------------

    /// Entering the alternate screen sets cursor visibility to true, even if
    /// the cursor was hidden on the primary screen.
    #[test]
    fn test_alternate_screen_cursor_visible_on_enter() {
        let mut t = Terminal::new(20, 5);
        // Hide the cursor on the primary screen.
        t.feed(b"\x1b[?25l");
        assert!(!t.grid().cursor().is_visible());

        // Enter alternate screen — cursor must become visible.
        t.feed(b"\x1b[?1049h");
        assert!(t.grid().cursor().is_visible());
    }

    // -----------------------------------------------------------------------
    // Scroll region restore on exit
    // -----------------------------------------------------------------------

    /// The primary screen's scroll region must be restored correctly when
    /// exiting the alternate screen, even if it differed from the default.
    #[test]
    fn test_alternate_screen_restores_scroll_region() {
        let mut t = Terminal::new(20, 10);
        // Set a non-default scroll region on the primary screen.
        t.feed(b"\x1b[3;8r"); // rows 3-8 (1-indexed) → (2,7) 0-indexed
        assert_eq!(t.grid().scroll_region(), (2, 7));

        // Enter alternate screen.
        t.feed(b"\x1b[?1049h");
        assert_eq!(t.grid().scroll_region(), (0, t.grid().rows() - 1));

        // Modify scroll region in alt screen.
        t.feed(b"\x1b[2;5r");
        assert_eq!(t.grid().scroll_region(), (1, 4));

        // Exit — primary's scroll region must be restored.
        t.feed(b"\x1b[?1049l");
        assert_eq!(t.grid().scroll_region(), (2, 7));
    }

    // -----------------------------------------------------------------------
    // is_alternate_screen predicate
    // -----------------------------------------------------------------------

    /// `is_alternate_screen` returns false on a fresh terminal.
    #[test]
    fn test_is_alternate_screen_default_false() {
        let t = Terminal::new(80, 24);
        assert!(!t.grid().is_alternate_screen());
    }

    /// `is_alternate_screen` returns true only while in the alternate screen.
    #[test]
    fn test_is_alternate_screen_true_only_while_active() {
        let mut t = Terminal::new(80, 24);
        assert!(!t.grid().is_alternate_screen());
        t.feed(b"\x1b[?1049h");
        assert!(t.grid().is_alternate_screen());
        t.feed(b"\x1b[?1049l");
        assert!(!t.grid().is_alternate_screen());
    }
}
