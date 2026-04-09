/// Tab stop configuration for a terminal grid row.
///
/// Tracks which columns have tab stops set. By default tab stops are placed
/// every 8 columns (columns 0, 8, 16, …). Applications may set custom stops
/// via HTS (ESC H) and clear them via TBC (CSI g).
///
/// # Examples
///
/// ```
/// use teamucks_vte::tabstops::TabStops;
///
/// let mut tabs = TabStops::new(80);
/// // Default stop at column 8.
/// assert_eq!(tabs.next_stop(0), 8);
/// assert_eq!(tabs.next_stop(7), 8);
/// // Past last stop clamps to last column.
/// assert_eq!(tabs.next_stop(78), 79);
/// ```
pub struct TabStops {
    /// One entry per column; `true` means a tab stop is active at that column.
    stops: Vec<bool>,
    /// Total number of columns this instance tracks.
    cols: usize,
}

impl TabStops {
    /// Create a new `TabStops` for a terminal with `cols` columns.
    ///
    /// Default tab stops are placed at every 8th column: 0, 8, 16, 24, …
    /// Column 0 is intentionally included so that `prev_stop` at column 1–7
    /// can return 0 as the leftmost stop.
    ///
    /// # Panics
    ///
    /// Panics if `cols == 0`.
    #[must_use]
    pub fn new(cols: usize) -> Self {
        assert!(cols > 0, "TabStops cols must be > 0");
        let stops = (0..cols).map(|c| c % 8 == 0).collect();
        Self { stops, cols }
    }

    /// Return the column of the next tab stop strictly after `col`.
    ///
    /// If there is no tab stop after `col`, returns the last column
    /// (`cols - 1`), ensuring the cursor never advances past the grid boundary.
    #[must_use]
    pub fn next_stop(&self, col: usize) -> usize {
        // Search for the first stop at a column strictly greater than `col`.
        for c in (col + 1)..self.cols {
            if self.stops[c] {
                return c;
            }
        }
        // No stop found — clamp to the last column.
        self.cols - 1
    }

    /// Return the column of the previous tab stop strictly before `col`.
    ///
    /// If there is no tab stop before `col`, returns 0.
    #[must_use]
    pub fn prev_stop(&self, col: usize) -> usize {
        if col == 0 {
            return 0;
        }
        // Search backwards for the first stop strictly before `col`.
        for c in (0..col).rev() {
            if self.stops[c] {
                return c;
            }
        }
        0
    }

    /// Set a tab stop at `col`.
    ///
    /// Has no effect if `col >= cols`.
    pub fn set(&mut self, col: usize) {
        if col < self.cols {
            self.stops[col] = true;
        }
    }

    /// Clear the tab stop at `col` (TBC parameter 0).
    ///
    /// Has no effect if `col >= cols` or no stop exists there.
    pub fn clear(&mut self, col: usize) {
        if col < self.cols {
            self.stops[col] = false;
        }
    }

    /// Clear all tab stops (TBC parameter 3).
    pub fn clear_all(&mut self) {
        for stop in &mut self.stops {
            *stop = false;
        }
    }

    /// Resize to `new_cols` columns.
    ///
    /// Existing stops are preserved. New columns beyond the old width receive
    /// default stops (every 8 columns). Columns that are removed are dropped.
    ///
    /// # Panics
    ///
    /// Panics if `new_cols == 0`.
    pub fn resize(&mut self, new_cols: usize) {
        assert!(new_cols > 0, "TabStops cols must be > 0");
        if new_cols > self.cols {
            // Extend with default stops for new columns.
            for c in self.cols..new_cols {
                self.stops.push(c % 8 == 0);
            }
        } else {
            self.stops.truncate(new_cols);
        }
        self.cols = new_cols;
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::TabStops;

    #[test]
    fn test_default_tab_stops_every_8() {
        let tabs = TabStops::new(80);
        // Tab from column 0 advances to the next stop, which is column 8.
        assert_eq!(tabs.next_stop(0), 8);
    }

    #[test]
    fn test_tab_from_col_3() {
        let tabs = TabStops::new(80);
        // From column 3 the next stop is still column 8.
        assert_eq!(tabs.next_stop(3), 8);
    }

    #[test]
    fn test_tab_from_col_8() {
        let tabs = TabStops::new(80);
        // From column 8 the next stop is column 16.
        assert_eq!(tabs.next_stop(8), 16);
    }

    #[test]
    fn test_tab_at_end_of_line() {
        // 80-column terminal: last column index is 79.
        let tabs = TabStops::new(80);
        // Column 78 has no stop to the right within bounds, so we get 79.
        assert_eq!(tabs.next_stop(78), 79);
    }

    #[test]
    fn test_hts_set_tab_stop_custom_position() {
        let mut tabs = TabStops::new(80);
        // Clear the default stop at 8 to verify our custom one wins.
        tabs.clear(8);
        tabs.set(5);
        // From column 0 the next stop is 5.
        assert_eq!(tabs.next_stop(0), 5);
        // From column 5 the next stop is 16 (default stays at 16).
        assert_eq!(tabs.next_stop(5), 16);
    }

    #[test]
    fn test_tbc_clear_current() {
        let mut tabs = TabStops::new(80);
        tabs.set(5);
        // Clear the stop at column 5.
        tabs.clear(5);
        // From column 0, stop at 5 is gone — next is 8.
        assert_eq!(tabs.next_stop(0), 8);
    }

    #[test]
    fn test_tbc_clear_all() {
        let mut tabs = TabStops::new(80);
        tabs.clear_all();
        // With no stops at all, next_stop clamps to the last column (79).
        assert_eq!(tabs.next_stop(0), 79);
    }

    #[test]
    fn test_prev_stop_from_col_20() {
        let tabs = TabStops::new(80);
        // From column 20 the previous stop is 16.
        assert_eq!(tabs.prev_stop(20), 16);
    }

    #[test]
    fn test_prev_stop_at_zero() {
        let tabs = TabStops::new(80);
        assert_eq!(tabs.prev_stop(0), 0);
    }

    #[test]
    fn test_resize_adds_default_stops() {
        let mut tabs = TabStops::new(16);
        // Only columns 0, 8 have stops.
        tabs.resize(24);
        // Column 16 should now have a default stop.
        assert_eq!(tabs.next_stop(15), 16);
    }

    #[test]
    fn test_resize_truncates_stops() {
        let mut tabs = TabStops::new(80);
        tabs.resize(16);
        // Only 16 columns now; last column is 15.
        assert_eq!(tabs.next_stop(14), 15);
    }

    #[test]
    fn test_set_out_of_bounds_is_noop() {
        let mut tabs = TabStops::new(16);
        // Should not panic.
        tabs.set(100);
        // Nothing changed.
        assert_eq!(tabs.next_stop(14), 15);
    }

    #[test]
    fn test_clear_out_of_bounds_is_noop() {
        let mut tabs = TabStops::new(16);
        // Should not panic.
        tabs.clear(100);
    }
}
