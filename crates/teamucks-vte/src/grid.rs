use unicode_width::UnicodeWidthChar;

use crate::{cell::Cell, row::Row, style::PackedStyle};

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
        Self { visible, cols, rows, cursor: Cursor::default(), saved_cursor: None }
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

        self.resolve_pending_wrap();

        // --- Handle wide character at end of line ---
        if width == 2 && self.cursor.col + 1 >= self.cols {
            // Not enough space for a wide char — fill current cell with space
            // and soft-wrap.
            let fill_col = self.cursor.col;
            let fill_row = self.cursor.row;
            self.visible[fill_row].cell_mut(fill_col).reset();

            // If the grid is only 1 column wide the wide char cannot fit at
            // all; write a space placeholder and return without advancing.
            if self.cols == 1 {
                return;
            }

            self.visible[fill_row].set_soft_wrapped(true);
            let next_row = self.cursor.row + 1;
            if next_row >= self.rows {
                self.scroll_up(1);
            } else {
                self.cursor.row = next_row;
            }
            self.cursor.col = 0;
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
        let next_row = self.cursor.row + 1;
        if next_row >= self.rows {
            self.scroll_up(1);
            // cursor.row stays at rows-1 after scroll.
        } else {
            self.cursor.row = next_row;
        }
        self.cursor.col = 0;
        self.cursor.wrap_pending = false;
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

    /// Scroll the visible grid up by `count` rows.
    ///
    /// The top `count` rows are discarded.  New blank rows are appended at the
    /// bottom.  The cursor position is not changed.
    pub fn scroll_up(&mut self, count: usize) {
        let count = count.min(self.rows);
        self.visible.drain(0..count);
        for _ in 0..count {
            self.visible.push(Row::new(self.cols));
        }
    }

    /// Scroll the visible grid down by `count` rows.
    ///
    /// The bottom `count` rows are discarded.  New blank rows are inserted at
    /// the top.  The cursor position is not changed.
    pub fn scroll_down(&mut self, count: usize) {
        let count = count.min(self.rows);
        self.visible.truncate(self.rows - count);
        for _ in 0..count {
            self.visible.insert(0, Row::new(self.cols));
        }
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
