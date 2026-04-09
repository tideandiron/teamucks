//! Scrollback buffer for captured terminal rows.
//!
//! When the visible grid scrolls up (content scrolls off the top of the
//! primary screen), rows are captured here rather than discarded.  The buffer
//! is bounded by [`ScrollbackBuffer::max_lines`]; when full, the oldest row is
//! dropped to make room for the newest.
//!
//! # Indexing convention
//!
//! Index `0` always refers to the **most recently captured** row (the one that
//! most recently scrolled off the top of the screen).  Index `n` refers to the
//! row that scrolled off `n` captures ago.
//!
//! # Examples
//!
//! ```
//! use teamucks_vte::{row::Row, scrollback::ScrollbackBuffer};
//!
//! let mut buf = ScrollbackBuffer::new(100);
//! assert!(buf.is_empty());
//!
//! let row = Row::new(80);
//! buf.push(row);
//! assert_eq!(buf.len(), 1);
//! assert!(buf.get(0).is_some());
//! ```

use std::collections::VecDeque;

use crate::row::Row;

/// A bounded, ring-buffer-style store for rows that have scrolled off the top
/// of the visible terminal screen.
///
/// Rows are captured in order of recency: index `0` is the most recently
/// captured row; index `len() - 1` is the oldest row still in the buffer.
/// When capacity is exceeded, the oldest row (highest index) is dropped.
///
/// `ScrollbackBuffer` uses [`VecDeque`] for O(1) push and capacity-bounded
/// eviction.
pub struct ScrollbackBuffer {
    /// Rows stored from most recent (front) to oldest (back).
    rows: VecDeque<Row>,
    /// Hard upper bound on the number of rows retained.
    max_lines: usize,
}

impl ScrollbackBuffer {
    /// Create a new buffer with the given capacity.
    ///
    /// `max_lines` is the maximum number of rows the buffer will retain.
    /// When `push` is called on a full buffer the oldest row is evicted.
    ///
    /// Passing `max_lines == 0` is allowed but produces a buffer that
    /// discards every row immediately on push.
    ///
    /// # Examples
    ///
    /// ```
    /// use teamucks_vte::scrollback::ScrollbackBuffer;
    ///
    /// let buf = ScrollbackBuffer::new(10_000);
    /// assert!(buf.is_empty());
    /// assert_eq!(buf.max_lines(), 10_000);
    /// ```
    #[must_use]
    pub fn new(max_lines: usize) -> Self {
        Self { rows: VecDeque::new(), max_lines }
    }

    /// Append a row to the front of the buffer (making it the new index `0`).
    ///
    /// If the buffer is already at capacity, the oldest row (the one at the
    /// back) is evicted before the new row is prepended.  When `max_lines`
    /// is `0`, the row is immediately discarded.
    ///
    /// # Examples
    ///
    /// ```
    /// use teamucks_vte::{row::Row, scrollback::ScrollbackBuffer};
    ///
    /// let mut buf = ScrollbackBuffer::new(2);
    /// buf.push(Row::new(80));
    /// buf.push(Row::new(80));
    /// assert_eq!(buf.len(), 2);
    ///
    /// // Third push evicts the oldest.
    /// buf.push(Row::new(80));
    /// assert_eq!(buf.len(), 2);
    /// ```
    pub fn push(&mut self, row: Row) {
        if self.max_lines == 0 {
            return;
        }
        // Evict oldest if at capacity.
        if self.rows.len() >= self.max_lines {
            self.rows.pop_back();
        }
        self.rows.push_front(row);
    }

    /// Return the number of rows currently in the buffer.
    ///
    /// # Examples
    ///
    /// ```
    /// use teamucks_vte::{row::Row, scrollback::ScrollbackBuffer};
    ///
    /// let mut buf = ScrollbackBuffer::new(10);
    /// assert_eq!(buf.len(), 0);
    /// buf.push(Row::new(80));
    /// assert_eq!(buf.len(), 1);
    /// ```
    #[must_use]
    pub fn len(&self) -> usize {
        self.rows.len()
    }

    /// Return `true` if the buffer contains no rows.
    ///
    /// # Examples
    ///
    /// ```
    /// use teamucks_vte::scrollback::ScrollbackBuffer;
    ///
    /// let buf = ScrollbackBuffer::new(10);
    /// assert!(buf.is_empty());
    /// ```
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }

    /// Return a reference to the row at `index`, where `0` is the most
    /// recently captured row and `len() - 1` is the oldest.
    ///
    /// Returns `None` if `index >= len()`.
    ///
    /// # Examples
    ///
    /// ```
    /// use teamucks_vte::{row::Row, scrollback::ScrollbackBuffer};
    ///
    /// let mut buf = ScrollbackBuffer::new(10);
    /// buf.push(Row::new(80));
    /// assert!(buf.get(0).is_some());
    /// assert!(buf.get(1).is_none());
    /// ```
    #[must_use]
    pub fn get(&self, index: usize) -> Option<&Row> {
        self.rows.get(index)
    }

    /// Return an iterator over the rows from most recent (`0`) to oldest
    /// (`len() - 1`).
    ///
    /// # Examples
    ///
    /// ```
    /// use teamucks_vte::{row::Row, scrollback::ScrollbackBuffer};
    ///
    /// let mut buf = ScrollbackBuffer::new(10);
    /// buf.push(Row::new(80));
    /// buf.push(Row::new(80));
    /// assert_eq!(buf.iter().count(), 2);
    /// ```
    pub fn iter(&self) -> impl Iterator<Item = &Row> {
        self.rows.iter()
    }

    /// Remove all rows from the buffer.
    ///
    /// The `max_lines` capacity is not changed.
    ///
    /// # Examples
    ///
    /// ```
    /// use teamucks_vte::{row::Row, scrollback::ScrollbackBuffer};
    ///
    /// let mut buf = ScrollbackBuffer::new(10);
    /// buf.push(Row::new(80));
    /// buf.clear();
    /// assert!(buf.is_empty());
    /// ```
    pub fn clear(&mut self) {
        self.rows.clear();
    }

    /// Return the maximum number of rows the buffer will retain.
    ///
    /// # Examples
    ///
    /// ```
    /// use teamucks_vte::scrollback::ScrollbackBuffer;
    ///
    /// let buf = ScrollbackBuffer::new(500);
    /// assert_eq!(buf.max_lines(), 500);
    /// ```
    #[must_use]
    pub fn max_lines(&self) -> usize {
        self.max_lines
    }

    /// Change the capacity to `max_lines`, dropping the oldest rows if the
    /// current length exceeds the new limit.
    ///
    /// Setting `max_lines` to `0` clears the buffer entirely and causes all
    /// subsequent pushes to be discarded.
    ///
    /// # Examples
    ///
    /// ```
    /// use teamucks_vte::{row::Row, scrollback::ScrollbackBuffer};
    ///
    /// let mut buf = ScrollbackBuffer::new(10);
    /// for _ in 0..10 {
    ///     buf.push(Row::new(80));
    /// }
    /// assert_eq!(buf.len(), 10);
    ///
    /// buf.set_max_lines(5);
    /// assert_eq!(buf.max_lines(), 5);
    /// assert_eq!(buf.len(), 5);
    /// ```
    pub fn set_max_lines(&mut self, max_lines: usize) {
        self.max_lines = max_lines;
        // Evict oldest rows until within the new limit.
        while self.rows.len() > self.max_lines {
            self.rows.pop_back();
        }
    }

    /// Return the plain-text content of the row at `index` with trailing
    /// spaces trimmed, or `None` if `index >= len()`.
    ///
    /// `index` follows the same convention as [`get`](Self::get): `0` is
    /// the most recent row.
    ///
    /// # Examples
    ///
    /// ```
    /// use teamucks_vte::{row::Row, scrollback::ScrollbackBuffer};
    ///
    /// let mut buf = ScrollbackBuffer::new(10);
    /// // A blank row produces an empty trimmed string.
    /// buf.push(Row::new(80));
    /// assert_eq!(buf.text(0).as_deref(), Some(""));
    /// assert_eq!(buf.text(1), None);
    /// ```
    #[must_use]
    pub fn text(&self, index: usize) -> Option<String> {
        let row = self.rows.get(index)?;
        let mut text = String::new();
        for cell in row.cells() {
            if cell.is_continuation() {
                continue;
            }
            text.push_str(cell.grapheme());
        }
        let trimmed_len = text.trim_end_matches(' ').len();
        text.truncate(trimmed_len);
        Some(text)
    }
}

/// The default `ScrollbackBuffer` has a capacity of 10 000 lines, matching the
/// conventional scrollback limit used by major terminal emulators.
///
/// # Examples
///
/// ```
/// use teamucks_vte::scrollback::ScrollbackBuffer;
///
/// let buf = ScrollbackBuffer::default();
/// assert_eq!(buf.max_lines(), 10_000);
/// ```
impl Default for ScrollbackBuffer {
    fn default() -> Self {
        Self::new(10_000)
    }
}

impl std::fmt::Debug for ScrollbackBuffer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ScrollbackBuffer")
            .field("len", &self.rows.len())
            .field("max_lines", &self.max_lines)
            .finish()
    }
}

// ---------------------------------------------------------------------------
// Unit tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::row::Row;

    /// Helper: build a Row whose first cell contains the given string.
    fn row_with_text(cols: usize, text: &str) -> Row {
        let mut row = Row::new(cols);
        for (i, ch) in text.chars().enumerate() {
            if i >= cols {
                break;
            }
            row.cell_mut(i).set_grapheme(&ch.to_string());
        }
        row
    }

    #[test]
    fn test_scrollback_new_empty() {
        let buf = ScrollbackBuffer::new(100);
        assert_eq!(buf.len(), 0);
        assert!(buf.is_empty());
    }

    #[test]
    fn test_scrollback_push() {
        let mut buf = ScrollbackBuffer::new(100);
        buf.push(Row::new(80));
        assert_eq!(buf.len(), 1);
        assert!(!buf.is_empty());
    }

    #[test]
    fn test_scrollback_capacity() {
        let mut buf = ScrollbackBuffer::new(3);
        buf.push(Row::new(80));
        buf.push(Row::new(80));
        buf.push(Row::new(80));
        assert_eq!(buf.len(), 3);
        // Fourth push exceeds capacity — oldest is dropped.
        buf.push(Row::new(80));
        assert_eq!(buf.len(), 3);
    }

    #[test]
    fn test_scrollback_get_most_recent() {
        let mut buf = ScrollbackBuffer::new(100);
        let row_a = row_with_text(80, "first");
        let row_b = row_with_text(80, "second");
        buf.push(row_a);
        buf.push(row_b);
        // index 0 should be the last-pushed row ("second").
        let text = buf.text(0).expect("row 0 should exist");
        assert_eq!(text, "second");
    }

    #[test]
    fn test_scrollback_get_oldest() {
        let mut buf = ScrollbackBuffer::new(100);
        let row_a = row_with_text(80, "first");
        let row_b = row_with_text(80, "second");
        buf.push(row_a);
        buf.push(row_b);
        // index 1 (len-1) should be the first-pushed row ("first").
        let text = buf.text(1).expect("row 1 should exist");
        assert_eq!(text, "first");
    }

    #[test]
    fn test_scrollback_get_out_of_bounds() {
        let mut buf = ScrollbackBuffer::new(100);
        buf.push(Row::new(80));
        assert!(buf.get(1).is_none());
        assert!(buf.text(1).is_none());
    }

    #[test]
    fn test_scrollback_clear() {
        let mut buf = ScrollbackBuffer::new(100);
        buf.push(Row::new(80));
        buf.push(Row::new(80));
        buf.clear();
        assert_eq!(buf.len(), 0);
        assert!(buf.is_empty());
    }

    #[test]
    fn test_scrollback_set_max_lines() {
        let mut buf = ScrollbackBuffer::new(10);
        for _ in 0..10 {
            buf.push(Row::new(80));
        }
        assert_eq!(buf.len(), 10);
        // Reduce capacity — oldest rows should be dropped.
        buf.set_max_lines(5);
        assert_eq!(buf.max_lines(), 5);
        assert_eq!(buf.len(), 5);
    }

    #[test]
    fn test_scrollback_iter() {
        let mut buf = ScrollbackBuffer::new(10);
        let row_a = row_with_text(80, "row_a");
        let row_b = row_with_text(80, "row_b");
        let row_c = row_with_text(80, "row_c");
        // Push in order a, b, c — most recent is c (index 0).
        buf.push(row_a);
        buf.push(row_b);
        buf.push(row_c);

        let texts: Vec<String> = buf
            .iter()
            .map(|row| {
                let mut t = String::new();
                for cell in row.cells() {
                    if !cell.is_continuation() {
                        t.push_str(cell.grapheme());
                    }
                }
                t.trim_end_matches(' ').to_owned()
            })
            .collect();
        // Iterator order: most recent first → c, b, a.
        assert_eq!(texts, vec!["row_c", "row_b", "row_a"]);
    }

    #[test]
    fn test_scrollback_default_capacity() {
        let buf = ScrollbackBuffer::default();
        assert_eq!(buf.max_lines(), 10_000);
    }

    #[test]
    fn test_scrollback_overflow() {
        let mut buf = ScrollbackBuffer::new(10_000);
        // Push 10 001 rows — oldest should be evicted.
        for _ in 0..10_001 {
            buf.push(Row::new(80));
        }
        assert_eq!(buf.len(), 10_000);
    }
}
