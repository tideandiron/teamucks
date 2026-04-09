/// Frame diff computation.
///
/// Compares two [`FrameSnapshot`]s and produces a list of [`DiffEntry`] items
/// that describe the minimal set of changes needed to bring the old frame up
/// to date with the new one.
///
/// # Diff strategy
///
/// For each row:
/// 1. Count changed cells.
/// 2. If more than 50% of the cells in the row changed, emit a single
///    [`DiffEntry::LineChange`] covering the full row.
/// 3. Otherwise, emit one [`DiffEntry::CellChange`] per changed cell.
///
/// This threshold avoids the overhead of many small cell-change messages when
/// a whole line was redrawn, while keeping diffs small for sparse updates.
use crate::{
    pane::FrameSnapshot,
    protocol::{CellData, DiffEntry},
};

/// Produce a diff list from `prev` to `current`.
///
/// If the dimensions changed, the diff treats all cells as changed (a full
/// redraw is needed).  Callers that detect a dimension change should use
/// [`crate::pane::Pane::full_frame`] instead, but this function handles it
/// gracefully.
#[must_use]
pub(crate) fn compute_diff(prev: &FrameSnapshot, current: &FrameSnapshot) -> Vec<DiffEntry> {
    let cols = current.cols as usize;
    let rows = current.rows as usize;

    // Dimension mismatch → treat as full diff.
    if prev.cols != current.cols || prev.rows != current.rows {
        return full_as_diff(current);
    }

    let mut diffs: Vec<DiffEntry> = Vec::new();

    for row in 0..rows {
        let row_start = row * cols;
        let row_end = row_start + cols;

        let cur_row = &current.cells[row_start..row_end];
        let prev_row = &prev.cells[row_start..row_end];

        let changed: usize =
            cur_row.iter().zip(prev_row.iter()).filter(|(c, p)| !cells_equal(c, p)).count();

        if changed == 0 {
            continue;
        }

        // LineChange threshold: >50% of cells in the row changed.
        // Cast is safe: cols is bounded by the u16 grid dimension.
        #[allow(clippy::cast_possible_truncation)]
        if changed * 2 > cols {
            diffs.push(DiffEntry::LineChange {
                row: row as u16,
                cells: cur_row.iter().map(clone_cell_data).collect(),
            });
        } else {
            for col in 0..cols {
                if !cells_equal(&cur_row[col], &prev_row[col]) {
                    diffs.push(DiffEntry::CellChange {
                        col: col as u16,
                        row: row as u16,
                        cell: clone_cell_data(&cur_row[col]),
                    });
                }
            }
        }
    }

    diffs
}

/// Produce a [`DiffEntry`] list that covers every cell in `snap`.
///
/// Used when there is no previous frame to compare against.
#[must_use]
pub(crate) fn full_as_diff(snap: &FrameSnapshot) -> Vec<DiffEntry> {
    let cols = snap.cols as usize;
    let rows = snap.rows as usize;
    let mut diffs = Vec::with_capacity(rows);
    for row in 0..rows {
        let row_start = row * cols;
        let row_end = row_start + cols;
        diffs.push(DiffEntry::LineChange {
            // row is bounded by snap.rows which is a u16 — cast is safe.
            #[allow(clippy::cast_possible_truncation)]
            row: row as u16,
            cells: snap.cells[row_start..row_end].iter().map(clone_cell_data).collect(),
        });
    }
    diffs
}

// ---------------------------------------------------------------------------
// Private helpers
// ---------------------------------------------------------------------------

/// Compare two [`CellData`] values for equality without allocating.
///
/// This is the inner loop of the diff — keep it branch-minimal and avoid any
/// allocation.
#[inline]
fn cells_equal(a: &CellData, b: &CellData) -> bool {
    a.grapheme == b.grapheme
        && a.fg == b.fg
        && a.bg == b.bg
        && a.attrs == b.attrs
        && a.flags == b.flags
}

/// Clone a [`CellData`].
///
/// `CellData` derives `Clone`, so this is a thin wrapper that makes the
/// allocation explicit at each call site (per the no-implicit-clone guideline).
#[inline]
fn clone_cell_data(c: &CellData) -> CellData {
    c.clone()
}
