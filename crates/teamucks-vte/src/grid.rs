use unicode_width::UnicodeWidthChar;

use crate::{cell::Cell, modes::ModeFlags, row::Row, style::PackedStyle};

// ---------------------------------------------------------------------------
// Alternate screen
// ---------------------------------------------------------------------------

/// State saved when entering the alternate screen buffer.
///
/// The primary screen's visible rows, cursor, and scroll region are stashed
/// here so they can be restored verbatim when the alternate screen exits.
struct AlternateState {
    /// Saved visible rows of the primary screen.
    visible: Vec<Row>,
    /// Saved cursor of the primary screen.
    cursor: Cursor,
    /// Saved scroll region `(top, bottom)`, both inclusive, 0-indexed.
    scroll_region: (usize, usize),
}

/// Cursor state within a [`Grid`].
///
/// The cursor carries its own style (the current SGR attributes) so that
/// characters written to the grid inherit the correct formatting.
#[derive(Clone, Debug, Default)]
pub struct Cursor {
    /// Column of the cursor (0-based, clamped to `0..=cols-1`).
    pub(crate) col: usize,
    /// Row of the cursor (0-based, clamped to `0..=rows-1`).
    pub(crate) row: usize,
    /// Active text style applied to characters written at this cursor position.
    pub(crate) style: PackedStyle,
    /// Whether the cursor is visible.
    pub(crate) visible: bool,
    /// When `true`, the next printable character written to the grid will first
    /// advance the cursor to the start of the next line (auto-wrap pending).
    pub(crate) wrap_pending: bool,
}

impl Cursor {
    /// Return the cursor column (0-based).
    #[must_use]
    pub fn col(&self) -> usize {
        self.col
    }

    /// Return the cursor row (0-based).
    #[must_use]
    pub fn row(&self) -> usize {
        self.row
    }

    /// Return the active text style at the cursor.
    #[must_use]
    pub fn style(&self) -> &PackedStyle {
        &self.style
    }

    /// Return `true` if the cursor is visible.
    #[must_use]
    pub fn is_visible(&self) -> bool {
        self.visible
    }

    /// Return `true` if a soft-wrap is pending.
    ///
    /// When `wrap_pending` is set, the next printable character written to the
    /// grid will first advance the cursor to the start of the next line.  Any
    /// cursor movement sequence clears this flag.
    #[must_use]
    pub fn wrap_pending(&self) -> bool {
        self.wrap_pending
    }
}

/// The visible grid of terminal cells.
///
/// `Grid` owns the complete two-dimensional array of [`Cell`]s that make up
/// the current terminal screen, plus the cursor and saved-cursor state.
///
/// Scrollback, reflow, and the alternate screen are handled in a higher-level
/// layer (Feature 4+).  `Grid` provides the primitive operations: read/write
/// cells, move the cursor, scroll rows, and resize.
///
/// # Panics
///
/// [`Grid::new`] panics if either dimension is zero — a zero-dimension grid
/// has no valid cursor position and cannot represent any terminal state.
pub struct Grid {
    visible: Vec<Row>,
    cols: usize,
    rows: usize,
    cursor: Cursor,
    saved_cursor: Option<Cursor>,
    /// The active scroll region as `(top, bottom)`, both inclusive, 0-indexed.
    ///
    /// Defaults to `(0, rows - 1)` (the full screen).  Set via DECSTBM.
    scroll_region: (usize, usize),
    /// Terminal mode flags (DECAWM, DECOM, DECTCEM, mouse modes, etc.).
    modes: ModeFlags,
    /// Alternate screen state.
    ///
    /// `Some` only while the alternate screen is active.  `None` on the
    /// primary screen.  We use `Box` so that the `None` case (the common
    /// path) adds only a pointer word to `Grid`'s size.
    alternate: Option<Box<AlternateState>>,
}

impl Grid {
    /// Create a new grid with the given dimensions.
    ///
    /// All cells are initialised to the default (space, default style).
    ///
    /// # Panics
    ///
    /// Panics if `cols == 0` or `rows == 0`.
    #[must_use]
    pub fn new(cols: usize, rows: usize) -> Self {
        assert!(cols > 0, "Grid cols must be > 0");
        assert!(rows > 0, "Grid rows must be > 0");
        let visible = (0..rows).map(|_| Row::new(cols)).collect();
        let modes = ModeFlags::default_modes();
        // Cursor visibility starts as true because CURSOR_VISIBLE is in the
        // default mode set.  Callers may override via `set_modes`.
        let cursor = Cursor { visible: true, ..Cursor::default() };
        Self {
            visible,
            cols,
            rows,
            cursor,
            saved_cursor: None,
            scroll_region: (0, rows - 1),
            modes,
            alternate: None,
        }
    }

    /// Return the number of columns.
    #[must_use]
    pub fn cols(&self) -> usize {
        self.cols
    }

    /// Return the number of rows.
    #[must_use]
    pub fn rows(&self) -> usize {
        self.rows
    }

    /// Return the cursor's current column.
    #[must_use]
    pub fn cursor_col(&self) -> usize {
        self.cursor.col
    }

    /// Return the cursor's current row.
    #[must_use]
    pub fn cursor_row(&self) -> usize {
        self.cursor.row
    }

    /// Return an immutable reference to the cursor.
    #[must_use]
    pub fn cursor(&self) -> &Cursor {
        &self.cursor
    }

    /// Return a mutable reference to the cursor.
    pub fn cursor_mut(&mut self) -> &mut Cursor {
        &mut self.cursor
    }

    /// Return an immutable reference to the cell at `(col, row)`.
    ///
    /// # Panics
    ///
    /// Panics if `col >= cols` or `row >= rows`.
    #[must_use]
    pub fn cell(&self, col: usize, row: usize) -> &Cell {
        self.visible[row].cell(col)
    }

    /// Return a mutable reference to the cell at `(col, row)`.
    ///
    /// # Panics
    ///
    /// Panics if `col >= cols` or `row >= rows`.
    pub fn cell_mut(&mut self, col: usize, row: usize) -> &mut Cell {
        self.visible[row].cell_mut(col)
    }

    /// Return an immutable reference to the row at index `row`.
    ///
    /// # Panics
    ///
    /// Panics if `row >= rows`.
    #[must_use]
    pub fn row(&self, row: usize) -> &Row {
        &self.visible[row]
    }

    /// Return a mutable reference to the row at index `row`.
    ///
    /// # Panics
    ///
    /// Panics if `row >= rows`.
    pub fn row_mut(&mut self, row: usize) -> &mut Row {
        &mut self.visible[row]
    }

    /// Move the cursor to `(col, row)`, clamping to grid bounds.
    ///
    /// Setting the cursor always clears [`Cursor::wrap_pending`].
    pub fn set_cursor(&mut self, col: usize, row: usize) {
        self.cursor.col = col.min(self.cols - 1);
        self.cursor.row = row.min(self.rows - 1);
        self.cursor.wrap_pending = false;
    }

    /// Return the current terminal mode flags.
    #[must_use]
    pub fn modes(&self) -> ModeFlags {
        self.modes
    }

    /// Update the terminal mode flags.
    ///
    /// This method keeps the [`Cursor::visible`] field in sync with
    /// [`ModeFlags::CURSOR_VISIBLE`] so that callers querying the cursor
    /// directly always see the correct visibility state.
    pub fn set_modes(&mut self, modes: ModeFlags) {
        self.modes = modes;
        self.cursor.visible = modes.contains(ModeFlags::CURSOR_VISIBLE);
    }

    /// Position the cursor via a CUP/HVP sequence, honouring DECOM.
    ///
    /// When [`ModeFlags::ORIGIN`] is set, `row` is relative to the top of the
    /// scroll region, and the cursor is clamped to the scroll region rather
    /// than the full grid.  When DECOM is clear, positioning is absolute.
    ///
    /// Both `row` and `col` are 0-indexed at the point of call (the caller is
    /// responsible for converting from 1-indexed VTE parameters).
    pub(crate) fn set_cursor_cup(&mut self, col: usize, row: usize) {
        if self.modes.contains(ModeFlags::ORIGIN) {
            let (region_top, region_bottom) = self.scroll_region;
            let abs_row = (region_top + row).min(region_bottom);
            let abs_col = col.min(self.cols - 1);
            self.cursor.col = abs_col;
            self.cursor.row = abs_row;
            self.cursor.wrap_pending = false;
        } else {
            self.set_cursor(col, row);
        }
    }

    /// Save the current cursor state.
    ///
    /// At most one cursor is saved.  Calling this again overwrites the
    /// previously saved cursor.
    pub fn save_cursor(&mut self) {
        self.saved_cursor = Some(self.cursor.clone());
    }

    /// Restore the previously saved cursor.
    ///
    /// If no cursor has been saved this is a no-op.
    pub fn restore_cursor(&mut self) {
        if let Some(saved) = self.saved_cursor.take() {
            self.cursor = saved;
        }
    }

    /// Resize the grid to `(cols, rows)`.
    ///
    /// Existing content is preserved where it fits.  New rows or columns are
    /// filled with default cells.  Content that falls outside the new
    /// dimensions is discarded.
    ///
    /// This is a basic resize — reflow (re-wrapping long lines) is implemented
    /// in Feature 12.
    ///
    /// # Panics
    ///
    /// Panics if `cols == 0` or `rows == 0`.
    pub fn resize(&mut self, cols: usize, rows: usize) {
        assert!(cols > 0, "Grid cols must be > 0");
        assert!(rows > 0, "Grid rows must be > 0");

        // Resize each existing row to the new column count.
        for row in &mut self.visible {
            row.resize(cols);
        }

        // Add or remove rows.
        if rows > self.rows {
            for _ in self.rows..rows {
                self.visible.push(Row::new(cols));
            }
        } else {
            self.visible.truncate(rows);
        }

        self.cols = cols;
        self.rows = rows;

        // Clamp cursor to new bounds.
        self.cursor.col = self.cursor.col.min(cols - 1);
        self.cursor.row = self.cursor.row.min(rows - 1);

        // Resize resets the scroll region to the full new screen.
        self.scroll_region = (0, rows - 1);

        // When the alternate screen is active, also resize the saved primary
        // screen so that exiting the alternate screen restores consistent
        // dimensions.  No reflow is performed on the saved primary rows —
        // reflow of the primary screen is Feature 12.
        if let Some(alt) = &mut self.alternate {
            for row in &mut alt.visible {
                row.resize(cols);
            }
            if rows > alt.visible.len() {
                let old_len = alt.visible.len();
                for _ in old_len..rows {
                    alt.visible.push(Row::new(cols));
                }
            } else {
                alt.visible.truncate(rows);
            }
            alt.cursor.col = alt.cursor.col.min(cols - 1);
            alt.cursor.row = alt.cursor.row.min(rows - 1);
            // The saved primary scroll region is also updated to the full new
            // screen so that after exit the region is always valid.
            alt.scroll_region =
                (alt.scroll_region.0.min(rows - 1), alt.scroll_region.1.min(rows - 1));
        }
    }

    /// Clear the entire grid, resetting all cells to the default (space,
    /// default style) and clearing all soft-wrap flags.
    ///
    /// The cursor position and style are not affected.
    pub fn clear(&mut self) {
        for row in &mut self.visible {
            row.clear();
        }
    }

    /// Write a single character at the current cursor position, applying the
    /// cursor's active style, then advance the cursor.
    ///
    /// # Width handling
    ///
    /// - **Zero-width characters** (combining diacritics, ZWJ, etc.): the
    ///   character is combined with the grapheme in the previous cell.  The
    ///   cursor does not advance.
    /// - **Full-width characters** (CJK, some emoji): occupy two columns.  If
    ///   the cursor is at the last column, the current cell is filled with a
    ///   space and the cursor soft-wraps before writing.  If the grid is only
    ///   1 column wide, the character is replaced with a space placeholder.
    /// - **Normal characters**: occupy one column.
    ///
    /// # Wrap-pending semantics
    ///
    /// Writing to the last column does not immediately move to the next row.
    /// Instead, `wrap_pending` is set on the cursor.  The next call to
    /// `write_char` with a non-zero-width character triggers the actual wrap.
    pub fn write_char(&mut self, c: char) {
        let width = UnicodeWidthChar::width(c).unwrap_or(0);

        if width == 0 {
            // Zero-width: combine with the preceding cell's grapheme.
            self.combine_with_previous(c);
            return;
        }

        // Resolve a pending soft-wrap only when DECAWM (auto-wrap) is enabled.
        // When DECAWM is off and wrap_pending is set, the cursor stays at the
        // last column and the next character overwrites that cell in-place.
        if self.cursor.wrap_pending {
            if self.modes.contains(ModeFlags::AUTO_WRAP) {
                self.resolve_pending_wrap();
            } else {
                // DECAWM off: clear the pending flag (no wrap) and position the
                // cursor at the last column so the write targets that cell.
                self.cursor.wrap_pending = false;
                self.cursor.col = self.cols - 1;
            }
        }

        // --- Handle wide character at end of line ---
        if width == 2 && self.cursor.col + 1 >= self.cols {
            // Not enough space for a wide char — fill current cell with space.
            let fill_col = self.cursor.col;
            let fill_row = self.cursor.row;
            self.visible[fill_row].cell_mut(fill_col).reset();

            // If the grid is only 1 column wide the wide char cannot fit at
            // all; write a space placeholder and return without advancing.
            if self.cols == 1 {
                return;
            }

            if self.modes.contains(ModeFlags::AUTO_WRAP) {
                // DECAWM on: soft-wrap and continue writing on the next line.
                self.visible[fill_row].set_soft_wrapped(true);
                self.advance_cursor_row_for_wrap();
                self.cursor.col = 0;
            } else {
                // DECAWM off: stay on this line; the wide char cannot be
                // written — the space placeholder stands and we return.
                return;
            }
        }

        // --- Clean up any existing wide character at the target position ---
        let target_col = self.cursor.col;
        let target_row = self.cursor.row;
        self.clear_wide_at(target_col, target_row);

        // If writing a wide char also clean up the second column.
        if width == 2 {
            let next_col = target_col + 1;
            if next_col < self.cols {
                self.clear_wide_at(next_col, target_row);
            }
        }

        // --- Write the grapheme ---
        {
            let cell = self.visible[target_row].cell_mut(target_col);
            cell.set_grapheme_char(c);
            *cell.style_mut() = self.cursor.style;
            cell.set_wide(width == 2);
            cell.set_continuation(false);
        }

        // For wide characters, write the continuation placeholder.
        // Guard with cols check: on a 1-col grid the wide char already
        // returned above, but guard defensively.
        if width == 2 {
            let cont_col = target_col + 1;
            if cont_col < self.cols {
                let cell = self.visible[target_row].cell_mut(cont_col);
                cell.reset();
                *cell.style_mut() = self.cursor.style;
                cell.set_continuation(true);
            }
        }

        // --- Advance cursor ---
        let new_col = self.cursor.col + width;
        if new_col >= self.cols {
            // Cursor lands past the end — park at last col and set wrap_pending.
            self.cursor.col = self.cols - 1;
            self.cursor.wrap_pending = true;
        } else {
            self.cursor.col = new_col;
        }
    }

    /// Resolve a pending soft-wrap: mark the current row as soft-wrapped and
    /// advance the cursor to the start of the next line, scrolling if needed.
    fn resolve_pending_wrap(&mut self) {
        if !self.cursor.wrap_pending {
            return;
        }
        let wrap_row = self.cursor.row;
        self.visible[wrap_row].set_soft_wrapped(true);
        self.advance_cursor_row_for_wrap();
        self.cursor.col = 0;
        self.cursor.wrap_pending = false;
    }

    /// Advance the cursor one row downward for a wrap or LF-style operation.
    ///
    /// - If the cursor is at the bottom of the scroll region, scroll up within
    ///   the region (cursor stays at the bottom row).
    /// - If the cursor is below the scroll region or at the screen bottom
    ///   without being in the region, just move down without scrolling.
    /// - Otherwise, move down by one row.
    fn advance_cursor_row_for_wrap(&mut self) {
        let (region_top, region_bottom) = self.scroll_region;
        if self.cursor.row == region_bottom {
            // At the bottom of the scroll region — scroll up within it.
            self.scroll_up_in_region(1, region_top, region_bottom);
            // cursor.row stays at region_bottom (the now-blank bottom row).
        } else if self.cursor.row + 1 < self.rows {
            self.cursor.row += 1;
        }
        // If cursor.row == rows-1 and not in the region (already at screen
        // bottom outside the region), do nothing (no advance, no scroll).
    }

    /// Combine `c` with the grapheme in the cell immediately before the cursor.
    ///
    /// If the cursor is at the start of the grid (0, 0), the character is
    /// silently discarded — there is no preceding cell.
    fn combine_with_previous(&mut self, c: char) {
        let (prev_col, prev_row) = if self.cursor.col > 0 {
            (self.cursor.col - 1, self.cursor.row)
        } else if self.cursor.row > 0 {
            (self.cols - 1, self.cursor.row - 1)
        } else {
            // No previous cell; discard.
            return;
        };

        let existing = self.visible[prev_row].cell(prev_col).grapheme().to_owned();
        let mut combined = existing;
        combined.push(c);
        self.visible[prev_row].cell_mut(prev_col).set_grapheme(&combined);
    }

    /// Clear wide-char metadata at `(col, row)`.
    ///
    /// - If the cell at `(col, row)` is a **leading wide cell**: clear the
    ///   trailing continuation cell at `(col+1, row)`.
    /// - If the cell at `(col, row)` is a **continuation cell**: clear the
    ///   leading wide cell at `(col-1, row)`.
    ///
    /// In both cases the affected cells have their grapheme reset to a space
    /// and their flags cleared.
    fn clear_wide_at(&mut self, col: usize, row: usize) {
        let cell = self.visible[row].cell(col);
        if cell.is_wide() {
            // Clear the trailing continuation cell.
            let cont_col = col + 1;
            if cont_col < self.cols {
                self.visible[row].cell_mut(cont_col).reset();
            }
            self.visible[row].cell_mut(col).set_wide(false);
        } else if cell.is_continuation() {
            // Clear the leading wide cell.
            if col > 0 {
                let lead_col = col - 1;
                self.visible[row].cell_mut(lead_col).reset();
            }
            self.visible[row].cell_mut(col).set_continuation(false);
        }
    }

    // -----------------------------------------------------------------------
    // Scroll region
    // -----------------------------------------------------------------------

    /// Return the current scroll region as `(top, bottom)`, both inclusive,
    /// 0-indexed.
    #[must_use]
    pub fn scroll_region(&self) -> (usize, usize) {
        self.scroll_region
    }

    /// Set the scroll region.
    ///
    /// `top` and `bottom` are 0-indexed and inclusive.  The caller is
    /// responsible for validating that `top < bottom` and that both values are
    /// within `0..rows`.
    pub(crate) fn set_scroll_region(&mut self, top: usize, bottom: usize) {
        self.scroll_region = (top, bottom);
    }

    // -----------------------------------------------------------------------
    // Alternate screen
    // -----------------------------------------------------------------------

    /// Return `true` when the alternate screen buffer is currently active.
    ///
    /// The alternate screen is entered via `CSI ? 1049 h` and exited via
    /// `CSI ? 1049 l`.
    ///
    /// # Examples
    ///
    /// ```
    /// use teamucks_vte::terminal::Terminal;
    ///
    /// let mut t = Terminal::new(80, 24);
    /// assert!(!t.grid().is_alternate_screen());
    /// t.feed(b"\x1b[?1049h"); // enter alternate screen
    /// assert!(t.grid().is_alternate_screen());
    /// t.feed(b"\x1b[?1049l"); // exit alternate screen
    /// assert!(!t.grid().is_alternate_screen());
    /// ```
    #[must_use]
    pub fn is_alternate_screen(&self) -> bool {
        self.alternate.is_some()
    }

    /// Switch to the alternate screen buffer.
    ///
    /// Steps performed:
    /// 1. Save the current visible rows, cursor, and scroll region into
    ///    [`AlternateState`].
    /// 2. Replace the visible buffer with a fresh blank grid of the same
    ///    dimensions.
    /// 3. Reset the scroll region to the full screen.
    /// 4. Move the cursor to `(0, 0)` and set `cursor.visible = true`.
    ///
    /// If the alternate screen is already active this is a no-op (no stacking).
    pub(crate) fn enter_alternate_screen(&mut self) {
        // No-op if already in alternate screen — do not stack.
        if self.alternate.is_some() {
            return;
        }

        // Save primary screen state.  Row and Cell are hot-path types that
        // forbid Clone; use snapshot() which makes the allocation cost explicit.
        let saved_visible = self.visible.iter().map(Row::snapshot).collect();
        let saved = AlternateState {
            visible: saved_visible,
            cursor: self.cursor.clone(),
            scroll_region: self.scroll_region,
        };
        self.alternate = Some(Box::new(saved));

        // Replace visible with a fresh blank grid.
        self.visible = (0..self.rows).map(|_| Row::new(self.cols)).collect();

        // Reset scroll region to the full screen.
        self.scroll_region = (0, self.rows - 1);

        // Cursor goes to origin and is made visible.
        self.cursor = Cursor { visible: true, ..Cursor::default() };
    }

    /// Exit the alternate screen buffer and restore the primary screen.
    ///
    /// Steps performed:
    /// 1. Restore the saved visible rows, cursor, and scroll region from
    ///    [`AlternateState`].
    /// 2. Drop the alternate state.
    ///
    /// If the alternate screen is not active this is a no-op.
    pub(crate) fn exit_alternate_screen(&mut self) {
        let Some(saved) = self.alternate.take() else {
            return;
        };

        self.visible = saved.visible;
        self.cursor = saved.cursor;
        self.scroll_region = saved.scroll_region;
    }

    // -----------------------------------------------------------------------
    // Scroll operations
    // -----------------------------------------------------------------------

    /// Scroll upward within a specific row range by `count` rows.
    ///
    /// Only rows in `top..=bottom` participate.  The row at `top` is removed
    /// and a blank row is inserted at `bottom`.  Rows outside `top..=bottom`
    /// are untouched.  The cursor position is not changed.
    pub(crate) fn scroll_up_in_region(&mut self, count: usize, top: usize, bottom: usize) {
        let region_height = bottom - top + 1;
        let count = count.min(region_height);
        // Drain the top `count` rows of the region (indices top..top+count).
        self.visible.drain(top..top + count);
        // Insert `count` blank rows at position `bottom - count + 1` (= old
        // `bottom` position after drain), i.e. at `bottom + 1 - count`.
        let insert_at = bottom + 1 - count;
        for _ in 0..count {
            self.visible.insert(insert_at, Row::new(self.cols));
        }
    }

    /// Scroll downward within a specific row range by `count` rows.
    ///
    /// Only rows in `top..=bottom` participate.  The row at `bottom` is
    /// removed and a blank row is inserted at `top`.  Rows outside
    /// `top..=bottom` are untouched.  The cursor position is not changed.
    pub(crate) fn scroll_down_in_region(&mut self, count: usize, top: usize, bottom: usize) {
        let region_height = bottom - top + 1;
        let count = count.min(region_height);
        // Remove the bottom `count` rows of the region.
        self.visible.drain(bottom + 1 - count..=bottom);
        // Insert `count` blank rows at `top`.
        for _ in 0..count {
            self.visible.insert(top, Row::new(self.cols));
        }
    }

    /// Scroll the visible grid up by `count` rows within the current scroll
    /// region.
    ///
    /// The top `count` rows of the region are discarded.  New blank rows are
    /// appended at the bottom of the region.  The cursor position is not
    /// changed.
    pub fn scroll_up(&mut self, count: usize) {
        let (top, bottom) = self.scroll_region;
        self.scroll_up_in_region(count, top, bottom);
    }

    /// Scroll the visible grid down by `count` rows within the current scroll
    /// region.
    ///
    /// The bottom `count` rows of the region are discarded.  New blank rows
    /// are inserted at the top of the region.  The cursor position is not
    /// changed.
    pub fn scroll_down(&mut self, count: usize) {
        let (top, bottom) = self.scroll_region;
        self.scroll_down_in_region(count, top, bottom);
    }

    // -----------------------------------------------------------------------
    // Erase operations
    // -----------------------------------------------------------------------

    /// Erase every cell in `col_start..=col_end` on `row`, handling wide-char
    /// cleanup at both boundaries.
    ///
    /// Wide-character pairs that straddle the boundaries are fully cleared: if
    /// the leftmost erased column is a continuation half the leading half is
    /// also cleared; if the rightmost erased column is a wide leading half the
    /// trailing continuation is also cleared.
    ///
    /// # Panics
    ///
    /// Panics if `row >= rows` or `col_end >= cols`.
    fn erase_range(&mut self, row: usize, col_start: usize, col_end: usize) {
        if col_start > col_end {
            return;
        }
        // Handle wide-char cleanup at the left boundary: if the first cell we
        // are erasing is a continuation, clear the leading cell to the left.
        if col_start > 0 && self.visible[row].cell(col_start).is_continuation() {
            self.visible[row].cell_mut(col_start - 1).reset();
        }
        // Handle wide-char cleanup at the right boundary: if the last cell we
        // are erasing is a wide leading half, clear the continuation to the right.
        if col_end + 1 < self.cols && self.visible[row].cell(col_end).is_wide() {
            self.visible[row].cell_mut(col_end + 1).reset();
        }
        // Reset every cell in the range.
        for col in col_start..=col_end {
            self.visible[row].cell_mut(col).reset();
        }
    }

    /// Erase from cursor to end of screen (ED 0).
    ///
    /// Clears from the cursor position to the end of the cursor's row, then
    /// all rows below the cursor row.  The cursor position is not changed.
    pub(crate) fn erase_below(&mut self) {
        let cur_col = self.cursor.col;
        let cur_row = self.cursor.row;
        let last_col = self.cols - 1;
        let last_row = self.rows - 1;

        // Erase from cursor to end of the current line.
        self.erase_range(cur_row, cur_col, last_col);

        // Erase all rows below the cursor row.
        for row in (cur_row + 1)..=last_row {
            self.erase_range(row, 0, last_col);
        }
    }

    /// Erase from start of screen to cursor (ED 1).
    ///
    /// Clears all rows above the cursor row, then from the start of the cursor
    /// row to and including the cursor position.  The cursor position is not
    /// changed.
    pub(crate) fn erase_above(&mut self) {
        let cur_col = self.cursor.col;
        let cur_row = self.cursor.row;
        let last_col = self.cols - 1;

        // Erase all rows above the cursor row.
        for row in 0..cur_row {
            self.erase_range(row, 0, last_col);
        }

        // Erase from start of current line to cursor position (inclusive).
        if cur_col > 0 {
            self.erase_range(cur_row, 0, cur_col);
        } else {
            self.erase_range(cur_row, 0, 0);
        }
    }

    /// Erase the entire visible screen (ED 2).
    ///
    /// All cells are reset to the default.  The cursor position is not changed.
    pub(crate) fn erase_all(&mut self) {
        let last_col = self.cols - 1;
        let last_row = self.rows - 1;
        for row in 0..=last_row {
            self.erase_range(row, 0, last_col);
        }
    }

    /// Erase from cursor to end of the current line (EL 0).
    ///
    /// The cursor position is not changed.
    pub(crate) fn erase_line_right(&mut self) {
        let cur_col = self.cursor.col;
        let cur_row = self.cursor.row;
        let last_col = self.cols - 1;
        self.erase_range(cur_row, cur_col, last_col);
    }

    /// Erase from start of the current line to cursor (EL 1).
    ///
    /// The cursor position is not changed.
    pub(crate) fn erase_line_left(&mut self) {
        let cur_col = self.cursor.col;
        let cur_row = self.cursor.row;
        self.erase_range(cur_row, 0, cur_col);
    }

    /// Erase the entire current line (EL 2).
    ///
    /// The cursor position is not changed.
    pub(crate) fn erase_line_all(&mut self) {
        let cur_row = self.cursor.row;
        let last_col = self.cols - 1;
        self.erase_range(cur_row, 0, last_col);
    }

    /// Erase `count` characters starting at the cursor position (ECH).
    ///
    /// Erased cells are reset to the default.  The erase range is clamped to
    /// the end of the current line — it does not wrap to the next line.  The
    /// cursor position is not changed.
    ///
    /// Wide characters that overlap the erase boundaries are fully cleaned up.
    pub(crate) fn erase_chars(&mut self, count: usize) {
        if count == 0 {
            return;
        }
        let cur_col = self.cursor.col;
        let cur_row = self.cursor.row;
        let last_col = self.cols - 1;
        // Clamp: erase at most up to the last column of the current line.
        let end_col = (cur_col + count - 1).min(last_col);
        self.erase_range(cur_row, cur_col, end_col);
    }

    /// Return the plain-text content of row `row` with trailing spaces trimmed.
    ///
    /// Continuation cells (the second half of a wide character) are skipped so
    /// that wide characters appear only once in the output.
    ///
    /// # Panics
    ///
    /// Panics if `row >= rows`.
    #[must_use]
    pub fn row_text(&self, row: usize) -> String {
        let row = &self.visible[row];
        let mut text = String::new();
        for cell in row.cells() {
            if cell.is_continuation() {
                continue;
            }
            text.push_str(cell.grapheme());
        }
        // Trim trailing spaces.
        let trimmed_len = text.trim_end_matches(' ').len();
        text.truncate(trimmed_len);
        text
    }
}

impl std::fmt::Debug for Grid {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Grid")
            .field("cols", &self.cols)
            .field("rows", &self.rows)
            .field("cursor", &self.cursor)
            .field("saved_cursor", &self.saved_cursor)
            // visible rows omitted for brevity — would produce very large output
            .finish_non_exhaustive()
    }
}
