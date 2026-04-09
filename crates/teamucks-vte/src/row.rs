use crate::cell::Cell;

/// A single row (line) of terminal cells.
///
/// A row owns exactly `len()` cells and tracks whether it was soft-wrapped
/// (i.e. the line continues on the next row because the content exceeded the
/// terminal width, rather than because a newline was received).
///
/// # Allocation
///
/// `Row` heap-allocates exactly once at construction via [`Row::new`].
/// Subsequent [`Row::resize`] calls resize the `Vec` in place; they do not
/// reallocate unless the new length exceeds the current capacity.
pub struct Row {
    cells: Vec<Cell>,
    soft_wrapped: bool,
}

impl Row {
    /// Create a new row with `cols` cells, each initialised to the default
    /// (a space character with default style).
    ///
    /// # Panics
    ///
    /// Does not panic.  Passing `cols = 0` creates an empty row.
    #[must_use]
    pub fn new(cols: usize) -> Self {
        let mut cells = Vec::with_capacity(cols);
        for _ in 0..cols {
            cells.push(Cell::default());
        }
        Self { cells, soft_wrapped: false }
    }

    /// Return the number of cells in this row.
    #[must_use]
    pub fn len(&self) -> usize {
        self.cells.len()
    }

    /// Return `true` if this row contains no cells.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.cells.is_empty()
    }

    /// Return an immutable reference to the cell at column `col`.
    ///
    /// # Panics
    ///
    /// Panics if `col >= self.len()`.  The caller upholds the invariant that
    /// `col` is within bounds — this is enforced by [`crate::grid::Grid`]
    /// which never exposes out-of-bounds column indices.
    #[must_use]
    pub fn cell(&self, col: usize) -> &Cell {
        &self.cells[col]
    }

    /// Return a mutable reference to the cell at column `col`.
    ///
    /// # Panics
    ///
    /// Panics if `col >= self.len()`.
    pub(crate) fn cell_mut(&mut self, col: usize) -> &mut Cell {
        &mut self.cells[col]
    }

    /// Return an immutable slice of all cells.
    #[must_use]
    pub fn cells(&self) -> &[Cell] {
        &self.cells
    }

    /// Return a mutable slice of all cells.
    // Retained for future internal use (renderers that do in-place mutation).
    #[allow(dead_code)]
    pub(crate) fn cells_mut(&mut self) -> &mut [Cell] {
        &mut self.cells
    }

    /// Return `true` if this row was soft-wrapped (content continues on the
    /// next row because the line width was exceeded).
    #[must_use]
    pub fn is_soft_wrapped(&self) -> bool {
        self.soft_wrapped
    }

    /// Set or clear the soft-wrap flag.
    pub(crate) fn set_soft_wrapped(&mut self, value: bool) {
        self.soft_wrapped = value;
    }

    /// Resize the row to `cols` cells.
    ///
    /// If `cols > self.len()`, new cells are appended at the end, each
    /// initialised to the default (space, default style).  If
    /// `cols < self.len()`, trailing cells are discarded.
    ///
    /// The internal `Vec` is resized in place and does not reallocate unless
    /// the capacity must grow.
    pub(crate) fn resize(&mut self, cols: usize) {
        self.cells.resize_with(cols, Cell::default);
    }

    /// Create an independent copy of this row.
    ///
    /// Used by the alternate screen buffer to snapshot and restore the
    /// primary screen.  The copy is a fresh allocation with the same cell
    /// contents and soft-wrap flag.
    ///
    /// `Row` deliberately does not implement [`Clone`] — it is a hot-path
    /// type subject to the no-cheap-clone policy.  This method makes the
    /// allocation cost explicit at each call site.
    #[must_use]
    pub(crate) fn snapshot(&self) -> Self {
        let mut cells = Vec::with_capacity(self.cells.len());
        for cell in &self.cells {
            cells.push(cell.snapshot());
        }
        Self { cells, soft_wrapped: self.soft_wrapped }
    }

    /// Reset all cells to their default state and clear the soft-wrap flag.
    ///
    /// The row retains its current width.
    pub fn clear(&mut self) {
        for cell in &mut self.cells {
            cell.reset();
        }
        self.soft_wrapped = false;
    }
}

impl std::fmt::Debug for Row {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Row")
            .field("len", &self.len())
            .field("soft_wrapped", &self.soft_wrapped)
            // cells omitted for brevity — would produce very large output
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
