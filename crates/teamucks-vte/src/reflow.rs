//! Content reflow helpers for `Grid::resize()`.
//!
//! When the terminal width changes, soft-wrapped lines must be joined back into
//! their logical lines and then re-wrapped at the new width.  Hard-wrapped lines
//! (those with `soft_wrapped == false`) stand alone and are never merged with
//! their neighbours.
//!
//! # Algorithm overview
//!
//! 1. Walk rows from oldest (scrollback back) to newest (visible bottom).
//! 2. Consecutive rows where the *preceding* row has `soft_wrapped == true` are
//!    joined into one logical line (a flat `Vec<Cell>`).
//! 3. Each logical line is re-wrapped to `new_cols` cells per row.
//! 4. The resulting flat list of re-wrapped rows is split: the last `new_rows`
//!    rows become the new visible area; everything above goes to scrollback
//!    (capped at `max_lines`).
//!
//! # Cursor tracking
//!
//! Before reflow the cursor's absolute offset within its logical line is
//! recorded.  After reflow, the cursor is placed at the same cell offset in the
//! re-wrapped output.

use crate::{cell::Cell, row::Row, scrollback::ScrollbackBuffer};

// ---------------------------------------------------------------------------
// Logical-line collection
// ---------------------------------------------------------------------------

/// Collect rows (oldest-first) into logical lines.
///
/// A *logical line* is a contiguous group of rows where every row except the
/// last has `soft_wrapped == true`.  The cells of each row are appended in
/// order; continuation cells (the trailing half of a wide character) are
/// included verbatim so that wide-char geometry is preserved.
///
/// `rows` must be ordered **oldest first** (scrollback back → scrollback front
/// → visible top → visible bottom).
pub(crate) fn collect_logical_lines(rows: &[Row]) -> Vec<Vec<Cell>> {
    let mut lines: Vec<Vec<Cell>> = Vec::new();
    let mut current: Vec<Cell> = Vec::new();
    let mut in_soft_wrap = false;

    for row in rows {
        if !in_soft_wrap {
            // Start a new logical line.
            if !current.is_empty() {
                lines.push(current);
                current = Vec::new();
            }
        }
        // Append this row's cells to the current logical line.
        for cell in row.cells() {
            current.push(cell.snapshot());
        }
        // If this row is soft-wrapped the next row continues the logical line.
        in_soft_wrap = row.is_soft_wrapped();
    }
    // Push the final logical line (always exists even if rows is empty).
    lines.push(current);
    lines
}

// ---------------------------------------------------------------------------
// Re-wrapping
// ---------------------------------------------------------------------------

/// Re-wrap a logical line (flat cell sequence) to `new_cols` columns.
///
/// Returns a `Vec<Row>` where each row is `new_cols` cells wide.  Intermediate
/// rows have `soft_wrapped = true`; the final row has `soft_wrapped = false`.
///
/// # Wide-character handling
///
/// A wide character that would start at the last column (leaving no room for
/// its continuation) is pushed to the next row: the current row's last cell is
/// replaced with a space placeholder and the wide char begins the next row.
///
/// # Empty input
///
/// An empty `cells` slice produces exactly one blank row of `new_cols` cells.
pub(crate) fn rewrap_line(cells: &[Cell], new_cols: usize) -> Vec<Row> {
    debug_assert!(new_cols > 0, "new_cols must be > 0");

    if cells.is_empty() {
        return vec![Row::new(new_cols)];
    }

    let mut rows: Vec<Row> = Vec::new();
    let mut current = Row::new(new_cols);
    let mut col: usize = 0;

    // Walk the logical line's cells.  We skip continuation cells — they are
    // regenerated automatically when writing the wide leading cell.
    let mut idx = 0;
    while idx < cells.len() {
        let cell = &cells[idx];

        // Skip continuation cells — they're implied by wide-leading cells.
        if cell.is_continuation() {
            idx += 1;
            continue;
        }

        let is_wide = cell.is_wide();

        if is_wide && new_cols == 1 {
            // Wide char cannot fit in a 1-col grid — write a space placeholder.
            current.cell_mut(0).reset();
            col = 1; // Force a row flush below.
            idx += 2; // Skip wide + continuation.
        } else if is_wide && col + 1 >= new_cols {
            // Wide char doesn't fit at the end of the current row (needs 2 cols).
            // Fill the last cell with a space placeholder and start a new row.
            current.cell_mut(col).reset();
            current.set_soft_wrapped(true);
            rows.push(current);
            current = Row::new(new_cols);
            col = 0;
            // Don't advance idx — retry the wide char on the new row.
            continue;
        } else {
            // Write the cell at the current position.
            {
                let dst = current.cell_mut(col);
                *dst = cell.snapshot();
            }
            col += 1;

            if is_wide {
                // Write the continuation cell immediately after.
                if col < new_cols {
                    let dst = current.cell_mut(col);
                    dst.reset();
                    dst.set_continuation(true);
                    col += 1;
                }
                // Skip the original continuation in the input.
                if idx + 1 < cells.len() && cells[idx + 1].is_continuation() {
                    idx += 1;
                }
            }
            idx += 1;
        }

        // If the current row is full, flush it and start the next.
        if col >= new_cols {
            current.set_soft_wrapped(true);
            rows.push(current);
            current = Row::new(new_cols);
            col = 0;
        }
    }

    // Push the last (possibly partial) row only if it has content or if no
    // rows were emitted yet.  When the loop exited because the last cell
    // exactly filled the previous row, `current` is a freshly-allocated empty
    // row and should not be emitted a second time.
    if col > 0 || rows.is_empty() {
        current.set_soft_wrapped(false);
        rows.push(current);
    }

    // The last row of a logical line must never be soft-wrapped, regardless of
    // how it was pushed (e.g. via the mid-loop flush when exactly new_cols
    // cells filled the row).
    if let Some(last) = rows.last_mut() {
        last.set_soft_wrapped(false);
    }

    rows
}

// ---------------------------------------------------------------------------
// Cursor offset tracking
// ---------------------------------------------------------------------------

/// Compute the byte offset of the cursor within its logical line.
///
/// Returns `(line_index, cell_offset)` where `line_index` is the index of the
/// logical line (in the same ordering as `collect_logical_lines`) that contains
/// the cursor, and `cell_offset` is the number of cells from the start of that
/// logical line to the cursor position (counting wide chars as 2 cells).
///
/// `rows` is ordered oldest-first (same as `collect_logical_lines`).
/// `cursor_abs_row` is the row index of the cursor within `rows`.
/// `cursor_col` is the column of the cursor.
pub(crate) fn cursor_offset_in_lines(
    rows: &[Row],
    cursor_abs_row: usize,
    cursor_col: usize,
) -> (usize, usize) {
    // We walk the rows in order, tracking the current logical line index and
    // the offset within that line.
    let mut line_idx = 0;
    let mut offset_in_line: usize = 0;
    let mut in_soft_wrap = false;
    let mut found_line = 0;
    let mut found_offset = 0;

    for (row_idx, row) in rows.iter().enumerate() {
        if !in_soft_wrap {
            // Starting a new logical line.
            if row_idx > 0 {
                line_idx += 1;
            }
            offset_in_line = 0;
        }

        if row_idx == cursor_abs_row {
            // The cursor is in this row.
            found_line = line_idx;
            found_offset = offset_in_line + cursor_col;
        }

        // Advance the offset by the number of cells in this row.
        offset_in_line += row.len();
        in_soft_wrap = row.is_soft_wrapped();
    }

    (found_line, found_offset)
}

/// Locate the cursor at `cell_offset` cells into the `line_index`-th reflowed
/// logical line within `reflowed_rows`.
///
/// `reflowed_rows` is the flat list of all re-wrapped rows, oldest first.
/// `line_boundaries` contains the starting row index (in `reflowed_rows`) of
/// each logical line in the same order as they were collected.
///
/// Returns `(row, col)` both within `reflowed_rows`.
pub(crate) fn cursor_from_offset(
    reflowed_rows: &[Row],
    line_boundaries: &[usize],
    line_index: usize,
    cell_offset: usize,
) -> (usize, usize) {
    // Find the start row of the target logical line.
    let start_row = if line_index < line_boundaries.len() {
        line_boundaries[line_index]
    } else {
        // Fallback: the last row.
        reflowed_rows.len().saturating_sub(1)
    };

    // Walk from start_row forward through rows of the same logical line,
    // counting cells until we reach cell_offset.
    let mut remaining = cell_offset;
    let mut row_idx = start_row;

    while row_idx < reflowed_rows.len() {
        let row = &reflowed_rows[row_idx];
        let row_len = row.len();

        if remaining < row_len {
            return (row_idx, remaining);
        }
        remaining -= row_len;

        // If this row is the last in its logical line, stop.
        if !row.is_soft_wrapped() {
            // Cursor is at the end of this row.
            return (row_idx, row_len.saturating_sub(1).min(remaining));
        }
        row_idx += 1;
    }

    // Fallback: clamp to last row.
    let last = reflowed_rows.len().saturating_sub(1);
    let last_len = reflowed_rows.get(last).map_or(0, Row::len);
    (last, last_len.saturating_sub(1))
}

// ---------------------------------------------------------------------------
// Main reflow entry point
// ---------------------------------------------------------------------------

/// Perform a full reflow.
///
/// `scrollback` rows are ordered most-recent-first (index 0 = most recent).
/// `visible` rows are ordered top-to-bottom.
/// `cursor_visible_row` and `cursor_col` identify the cursor in the visible
/// area.
///
/// Returns `(new_scrollback, new_visible, new_cursor_row, new_cursor_col)`.
///
/// `new_scrollback` rows are most-recent-first (ready to replace the buffer).
/// `new_visible` rows are top-to-bottom.
pub(crate) fn reflow(
    scrollback_buf: &ScrollbackBuffer,
    visible: &[Row],
    cursor_visible_row: usize,
    cursor_col: usize,
    new_cols: usize,
    new_rows: usize,
) -> (Vec<Row>, Vec<Row>, usize, usize) {
    // -----------------------------------------------------------------------
    // Step 1: Build an oldest-first slice of all rows.
    // -----------------------------------------------------------------------
    // Scrollback is stored most-recent-first (index 0 = top of scrollback,
    // i.e. the row most recently scrolled off the bottom of scrollback = oldest
    // is at the back).  Actually: index 0 = most recently scrolled off, index
    // len-1 = oldest.  For oldest-first we reverse the scrollback.

    let sb_len = scrollback_buf.len();
    // Collect scrollback rows oldest-first.
    let mut all_rows: Vec<Row> = Vec::with_capacity(sb_len + visible.len());

    // Scrollback oldest-first: scrollback index len-1..=0.
    for i in (0..sb_len).rev() {
        if let Some(row) = scrollback_buf.get(i) {
            all_rows.push(row.snapshot());
        }
    }
    // Visible top-to-bottom.
    for row in visible {
        all_rows.push(row.snapshot());
    }

    // -----------------------------------------------------------------------
    // Step 2: Record cursor position as (logical-line-index, cell-offset).
    // -----------------------------------------------------------------------
    // The cursor is at `cursor_visible_row` in the visible area, which maps to
    // `sb_len + cursor_visible_row` in `all_rows`.
    let cursor_abs_row = sb_len + cursor_visible_row;
    let (cursor_line_idx, cursor_cell_offset) =
        cursor_offset_in_lines(&all_rows, cursor_abs_row, cursor_col);

    // -----------------------------------------------------------------------
    // Step 3: Collect logical lines.
    // -----------------------------------------------------------------------
    let logical_lines = collect_logical_lines(&all_rows);
    // Drop all_rows to free memory early.
    drop(all_rows);

    // -----------------------------------------------------------------------
    // Step 4: Re-wrap each logical line to new_cols.
    // -----------------------------------------------------------------------
    // Track where each logical line starts in the reflowed output (for cursor
    // relocation).
    let mut reflowed: Vec<Row> = Vec::new();
    let mut line_start_rows: Vec<usize> = Vec::with_capacity(logical_lines.len());

    for line_cells in &logical_lines {
        line_start_rows.push(reflowed.len());
        // Trim trailing blank non-wide cells from the logical line before
        // rewrapping.  This avoids ghost whitespace rows after widening.
        let trimmed = trim_trailing_blank_cells(line_cells);
        let wrapped = rewrap_line(trimmed, new_cols);
        reflowed.extend(wrapped);
    }

    // -----------------------------------------------------------------------
    // Step 5: Locate the cursor in the reflowed output.
    // -----------------------------------------------------------------------
    let (new_cursor_abs_row, new_cursor_col) =
        cursor_from_offset(&reflowed, &line_start_rows, cursor_line_idx, cursor_cell_offset);

    // -----------------------------------------------------------------------
    // Step 6: Split reflowed rows into scrollback and visible.
    // -----------------------------------------------------------------------
    // Trim trailing blank rows from the reflowed output.  Blank rows at the
    // bottom of the reflowed sequence come from original blank visible-area
    // rows (padding below the actual content).  Keeping them would cause
    // content rows to be pushed into scrollback unnecessarily.  We discard
    // these trailing blanks and re-add them as padding when building the
    // visible area.
    let content_len = {
        let mut n = reflowed.len();
        while n > 0 {
            let row = &reflowed[n - 1];
            let is_blank = row
                .cells()
                .iter()
                .all(|c| c.grapheme() == " " && !c.is_wide() && !c.is_continuation());
            if is_blank && !row.is_soft_wrapped() {
                n -= 1;
            } else {
                break;
            }
        }
        n
    };

    let (new_scrollback_rows, new_visible): (Vec<Row>, Vec<Row>) = if content_len <= new_rows {
        // All content rows fit in the visible area.  Pad with blank rows.
        let mut vis: Vec<Row> = reflowed.drain(..content_len).collect();
        while vis.len() < new_rows {
            vis.push(Row::new(new_cols));
        }
        (Vec::new(), vis)
    } else {
        let split_at = content_len - new_rows;
        let sb_rows: Vec<Row> = reflowed[..split_at].iter().map(Row::snapshot).collect();
        let vis_rows: Vec<Row> =
            reflowed[split_at..content_len].iter().map(Row::snapshot).collect();
        // Pad visible if needed (shouldn't happen but defensive).
        let mut vis = vis_rows;
        while vis.len() < new_rows {
            vis.push(Row::new(new_cols));
        }
        (sb_rows, vis)
    };

    // -----------------------------------------------------------------------
    // Step 7: Map new_cursor_abs_row to visible coordinates.
    // -----------------------------------------------------------------------
    let sb_new_len = new_scrollback_rows.len();
    let (new_cursor_vis_row, final_cursor_col) = if new_cursor_abs_row >= sb_new_len {
        let vis_row = new_cursor_abs_row - sb_new_len;
        // Clamp to visible area.
        let vis_row = vis_row.min(new_rows - 1);
        // Clamp col to new width.
        let col = new_cursor_col.min(new_cols - 1);
        (vis_row, col)
    } else {
        // Cursor ended up in scrollback — put it at the top of visible.
        (0, 0)
    };

    (new_scrollback_rows, new_visible, new_cursor_vis_row, final_cursor_col)
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Trim trailing blank (space, no flags, no wide) cells from a logical line.
///
/// A cell is considered blank when it holds a single ASCII space, is not wide,
/// and is not a continuation.  Trailing blank cells within a logical line come
/// from the padding that fills the original row up to the terminal width.
/// Removing them avoids spurious blank rows in the reflowed output and prevents
/// content rows from being pushed into scrollback by padding inflation.
fn trim_trailing_blank_cells(cells: &[Cell]) -> &[Cell] {
    let mut end = cells.len();
    while end > 0 {
        let c = &cells[end - 1];
        if c.grapheme() == " " && !c.is_wide() && !c.is_continuation() {
            end -= 1;
        } else {
            break;
        }
    }
    &cells[..end]
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::row::Row;

    fn make_row(text: &str, soft_wrapped: bool) -> Row {
        let cols = text.chars().count().max(1);
        let mut row = Row::new(cols);
        for (i, ch) in text.chars().enumerate() {
            row.cell_mut(i).set_grapheme(&ch.to_string());
        }
        row.set_soft_wrapped(soft_wrapped);
        row
    }

    fn row_text(row: &Row) -> String {
        let mut s = String::new();
        for c in row.cells() {
            if c.is_continuation() {
                continue;
            }
            s.push_str(c.grapheme());
        }
        s.trim_end_matches(' ').to_owned()
    }

    // ---------------------------------------------------------------------------
    // collect_logical_lines tests
    // ---------------------------------------------------------------------------

    #[test]
    fn test_collect_logical_lines_single_hard() {
        let rows = vec![make_row("ABCD", false)];
        let lines = collect_logical_lines(&rows);
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].len(), 4);
    }

    #[test]
    fn test_collect_logical_lines_two_hard() {
        let rows = vec![make_row("AAAA", false), make_row("BBBB", false)];
        let lines = collect_logical_lines(&rows);
        assert_eq!(lines.len(), 2);
    }

    #[test]
    fn test_collect_logical_lines_soft_then_hard() {
        let rows = vec![make_row("AAAA", true), make_row("BBBB", false)];
        let lines = collect_logical_lines(&rows);
        // Two rows joined into one logical line: 8 cells.
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].len(), 8);
    }

    #[test]
    fn test_collect_logical_lines_hard_then_soft_then_hard() {
        let rows = vec![make_row("XXXX", false), make_row("AAAA", true), make_row("BBBB", false)];
        let lines = collect_logical_lines(&rows);
        // "XXXX" = line 0 (4 cells), "AAAABBBB" = line 1 (8 cells).
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0].len(), 4);
        assert_eq!(lines[1].len(), 8);
    }

    // ---------------------------------------------------------------------------
    // rewrap_line tests
    // ---------------------------------------------------------------------------

    #[test]
    fn test_rewrap_line_fits_in_one_row() {
        let mut row = Row::new(4);
        for (i, ch) in "ABCD".chars().enumerate() {
            row.cell_mut(i).set_grapheme(&ch.to_string());
        }
        let cells: Vec<Cell> = row.cells().iter().map(Cell::snapshot).collect();
        let wrapped = rewrap_line(&cells, 8);
        assert_eq!(wrapped.len(), 1);
        assert_eq!(row_text(&wrapped[0]), "ABCD");
        assert!(!wrapped[0].is_soft_wrapped());
    }

    #[test]
    fn test_rewrap_line_wraps_at_width() {
        let mut row = Row::new(8);
        for (i, ch) in "ABCDEFGH".chars().enumerate() {
            row.cell_mut(i).set_grapheme(&ch.to_string());
        }
        let cells: Vec<Cell> = row.cells().iter().map(Cell::snapshot).collect();
        let wrapped = rewrap_line(&cells, 4);
        assert_eq!(wrapped.len(), 2);
        assert_eq!(row_text(&wrapped[0]), "ABCD");
        assert!(wrapped[0].is_soft_wrapped());
        assert_eq!(row_text(&wrapped[1]), "EFGH");
        assert!(!wrapped[1].is_soft_wrapped());
    }

    #[test]
    fn test_rewrap_empty_produces_one_blank_row() {
        let wrapped = rewrap_line(&[], 5);
        assert_eq!(wrapped.len(), 1);
        assert!(!wrapped[0].is_soft_wrapped());
    }

    // ---------------------------------------------------------------------------
    // cursor_offset_in_lines tests
    // ---------------------------------------------------------------------------

    #[test]
    fn test_cursor_offset_single_row() {
        let rows = vec![make_row("ABCDE", false)];
        let (line, off) = cursor_offset_in_lines(&rows, 0, 3);
        assert_eq!(line, 0);
        assert_eq!(off, 3);
    }

    #[test]
    fn test_cursor_offset_second_logical_line() {
        let rows = vec![make_row("AAAA", false), make_row("BBBB", false)];
        let (line, off) = cursor_offset_in_lines(&rows, 1, 2);
        assert_eq!(line, 1);
        assert_eq!(off, 2);
    }

    #[test]
    fn test_cursor_offset_in_soft_wrapped_second_row() {
        let rows = vec![make_row("AAAA", true), make_row("BBBB", false)];
        // Cursor at row 1, col 1 → logical line 0, offset 4+1=5.
        let (line, off) = cursor_offset_in_lines(&rows, 1, 1);
        assert_eq!(line, 0);
        assert_eq!(off, 5);
    }
}
