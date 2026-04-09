/// Border rendering: compute box-drawing character cells from resolved pane geometries.
///
/// Given a slice of [`PaneGeometry`] values (produced by the layout engine's
/// [`resolve`][crate::layout::resolve::resolve] function), [`compute_borders`]
/// identifies every terminal cell that lies in the gap between two adjacent
/// panes and selects the correct Unicode box-drawing character based on which
/// of the four cardinal directions connect to another border cell (as opposed to
/// a pane content cell or an out-of-bounds position).
///
/// # Box-drawing character selection
///
/// For each border cell the function checks each of the four cardinal
/// neighbours.  A direction is "connected" when the neighbour is itself a
/// border/gap cell (not a pane content cell and not out-of-bounds).  The four
/// bits are packed into a [`BorderDirs`] bitmask that is matched to select the
/// appropriate box-drawing character:
///
/// | up | down | left | right | Character |
/// |----|------|------|-------|-----------|
/// | F  | F    | T    | T     | ─         |
/// | T  | T    | F    | F     | │         |
/// | F  | T    | F    | T     | ┌         |
/// | F  | T    | T    | F     | ┐         |
/// | T  | F    | F    | T     | └         |
/// | T  | F    | T    | F     | ┘         |
/// | T  | T    | F    | T     | ├         |
/// | T  | T    | T    | F     | ┤         |
/// | F  | T    | T    | T     | ┬         |
/// | T  | F    | T    | T     | ┴         |
/// | T  | T    | T    | T     | ┼         |
///
/// # Active pane highlighting
///
/// A border cell has [`BorderCell::is_active_border`] set to `true` when any
/// of its four in-bounds cardinal neighbours belongs to the active pane's
/// content rectangle.
use crate::{layout::resolve::PaneGeometry, pane::PaneId};

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

/// A single rendered border cell.
///
/// # Examples
///
/// ```
/// use teamucks_core::render::borders::BorderCell;
///
/// let cell = BorderCell { x: 5, y: 3, ch: '│', is_active_border: true };
/// assert_eq!(cell.ch, '│');
/// assert!(cell.is_active_border);
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BorderCell {
    /// Column position of this border cell (0-based).
    pub x: u16,
    /// Row position of this border cell (0-based).
    pub y: u16,
    /// The box-drawing character to display at this position.
    pub ch: char,
    /// `true` when this border cell is adjacent to the active pane.
    pub is_active_border: bool,
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Compute border cells from a slice of resolved pane geometries.
///
/// Returns a [`Vec<BorderCell>`] containing one entry per terminal cell that
/// lies in the gap between panes.  When `geometries` contains zero or one
/// entries an empty vector is returned.
///
/// # Arguments
///
/// * `geometries` — resolved pane rectangles from the layout engine.
/// * `active_pane` — the currently focused pane; drives
///   [`BorderCell::is_active_border`].
/// * `window_width` — total terminal width in columns.
/// * `window_height` — total terminal height in rows.
///
/// # Examples
///
/// ```
/// use teamucks_core::layout::resolve::PaneGeometry;
/// use teamucks_core::pane::PaneId;
/// use teamucks_core::render::borders::compute_borders;
///
/// // Single pane fills the window — no borders produced.
/// let geoms = vec![PaneGeometry { pane_id: PaneId(1), x: 0, y: 0, width: 80, height: 24 }];
/// let borders = compute_borders(&geoms, PaneId(1), 80, 24);
/// assert!(borders.is_empty());
/// ```
#[must_use]
pub fn compute_borders(
    geometries: &[PaneGeometry],
    active_pane: PaneId,
    window_width: u16,
    window_height: u16,
) -> Vec<BorderCell> {
    if geometries.len() <= 1 {
        return Vec::new();
    }

    let w = usize::from(window_width);
    let h = usize::from(window_height);
    let total = w * h;

    // Flat array: cell_pane[row * w + col] = pane id, or 0 for border/empty.
    // 0 is never a valid PaneId in practice; we use it to mean "no pane".
    let mut cell_pane: Vec<u32> = vec![0; total];

    for g in geometries {
        for row in g.y..g.y + g.height {
            for col in g.x..g.x + g.width {
                let col_u = usize::from(col);
                let row_u = usize::from(row);
                if col_u < w && row_u < h {
                    cell_pane[row_u * w + col_u] = g.pane_id.0;
                }
            }
        }
    }

    let active_id = active_pane.0;
    let mut borders = Vec::new();

    for row in 0..window_height {
        for col in 0..window_width {
            let col_u = usize::from(col);
            let row_u = usize::from(row);
            let idx = row_u * w + col_u;

            // Only process gap (border) cells.
            if cell_pane[idx] != 0 {
                continue;
            }

            // Connectivity: a direction is connected when the in-bounds
            // neighbour in that direction is *also a border/gap cell* (cell_pane == 0).
            // Pane content neighbours (cell_pane != 0) and out-of-bounds positions
            // terminate the line in that direction.
            let dirs = BorderDirs::from_neighbours(&cell_pane, col_u, row_u, w, h);
            let ch = box_char(dirs);

            // Active-border detection: true when any in-bounds cardinal
            // neighbour is owned by the active pane.
            let is_active_border = cardinal_neighbour_indices(col_u, row_u, w, h)
                .into_iter()
                .flatten()
                .any(|ni| cell_pane[ni] == active_id);

            borders.push(BorderCell { x: col, y: row, ch, is_active_border });
        }
    }

    borders
}

// ---------------------------------------------------------------------------
// Private types and helpers
// ---------------------------------------------------------------------------

/// Packed bitmask of the four cardinal directions in which a border cell
/// connects to another border/gap cell.
///
/// Using a `u8` bitmask avoids clippy's `struct_excessive_bools` and
/// `fn_params_excessive_bools` lints while keeping bit-level efficiency.
#[derive(Clone, Copy, PartialEq, Eq)]
struct BorderDirs(u8);

impl BorderDirs {
    const UP: u8 = 0b0001;
    const DOWN: u8 = 0b0010;
    const LEFT: u8 = 0b0100;
    const RIGHT: u8 = 0b1000;

    /// Compute the connectivity mask for the border cell at `(col, row)`.
    ///
    /// A direction bit is set when the in-bounds neighbour in that direction
    /// also has `cell_pane == 0` (i.e. it is another border cell, not pane
    /// content).
    #[inline]
    fn from_neighbours(cell_pane: &[u32], col: usize, row: usize, w: usize, h: usize) -> Self {
        let mut bits: u8 = 0;
        if row > 0 && cell_pane[(row - 1) * w + col] == 0 {
            bits |= Self::UP;
        }
        if row + 1 < h && cell_pane[(row + 1) * w + col] == 0 {
            bits |= Self::DOWN;
        }
        if col > 0 && cell_pane[row * w + (col - 1)] == 0 {
            bits |= Self::LEFT;
        }
        if col + 1 < w && cell_pane[row * w + (col + 1)] == 0 {
            bits |= Self::RIGHT;
        }
        Self(bits)
    }

    #[inline]
    fn has(self, bit: u8) -> bool {
        self.0 & bit != 0
    }
}

/// Return the flat indices of all in-bounds cardinal neighbours of `(col, row)`
/// in a grid of size `width × height`.
///
/// The returned array has four slots; `None` values indicate out-of-bounds
/// directions.  Callers iterate with `into_iter().flatten()`.
#[inline]
fn cardinal_neighbour_indices(
    col: usize,
    row: usize,
    width: usize,
    height: usize,
) -> [Option<usize>; 4] {
    [
        row.checked_sub(1).map(|r| r * width + col),
        (row + 1 < height).then(|| (row + 1) * width + col),
        col.checked_sub(1).map(|c| row * width + c),
        (col + 1 < width).then(|| row * width + (col + 1)),
    ]
}

/// Map a [`BorderDirs`] bitmask to a Unicode box-drawing character.
///
/// The mapping covers the standard single-line box-drawing block (U+2500–U+257F).
#[inline]
#[must_use]
fn box_char(dirs: BorderDirs) -> char {
    let u = dirs.has(BorderDirs::UP);
    let d = dirs.has(BorderDirs::DOWN);
    let l = dirs.has(BorderDirs::LEFT);
    let r = dirs.has(BorderDirs::RIGHT);

    match (u, d, l, r) {
        // Pure vertical line and single-direction vertical stubs.
        // Clippy unnested_or_patterns: (true|false, true, false, false) nests the first two
        // patterns; the third (false, true, false, false) remains separate.
        (true | false, true, false, false) | (true, false, false, false) => '│',
        // Corners.
        (false, true, false, true) => '┌',
        (false, true, true, false) => '┐',
        (true, false, false, true) => '└',
        (true, false, true, false) => '┘',
        // T-junctions.
        (true, true, false, true) => '├',
        (true, true, true, false) => '┤',
        (false, true, true, true) => '┬',
        (true, false, true, true) => '┴',
        // Cross.
        (true, true, true, true) => '┼',
        // Horizontal line, single-direction horizontal stubs, and all other
        // combinations (degenerate fallback).
        _ => '─',
    }
}

// ---------------------------------------------------------------------------
// Unit tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn geom(id: u32, x: u16, y: u16, width: u16, height: u16) -> PaneGeometry {
        PaneGeometry { pane_id: PaneId(id), x, y, width, height }
    }

    // Aliases for the bitmask constants to keep test assertions readable.
    const U: u8 = BorderDirs::UP;
    const D: u8 = BorderDirs::DOWN;
    const L: u8 = BorderDirs::LEFT;
    const R: u8 = BorderDirs::RIGHT;

    fn bd(bits: u8) -> BorderDirs {
        BorderDirs(bits)
    }

    #[test]
    fn test_box_char_horizontal() {
        assert_eq!(box_char(bd(L | R)), '─');
    }

    #[test]
    fn test_box_char_vertical() {
        assert_eq!(box_char(bd(U | D)), '│');
    }

    #[test]
    fn test_box_char_corners() {
        assert_eq!(box_char(bd(D | R)), '┌');
        assert_eq!(box_char(bd(D | L)), '┐');
        assert_eq!(box_char(bd(U | R)), '└');
        assert_eq!(box_char(bd(U | L)), '┘');
    }

    #[test]
    fn test_box_char_tees() {
        assert_eq!(box_char(bd(U | D | R)), '├');
        assert_eq!(box_char(bd(U | D | L)), '┤');
        assert_eq!(box_char(bd(D | L | R)), '┬');
        assert_eq!(box_char(bd(U | L | R)), '┴');
    }

    #[test]
    fn test_box_char_cross() {
        assert_eq!(box_char(bd(U | D | L | R)), '┼');
    }

    #[test]
    fn test_compute_borders_single_pane_returns_empty() {
        let geoms = [geom(1, 0, 0, 80, 24)];
        let borders = compute_borders(&geoms, PaneId(1), 80, 24);
        assert!(borders.is_empty());
    }

    #[test]
    fn test_compute_borders_vertical_split_correct_count() {
        // 80 wide, split: pane1 w=39, gap at col=39, pane2 x=40 w=40.
        // 24 rows × 1 gap column = 24 border cells.
        let geoms = [geom(1, 0, 0, 39, 24), geom(2, 40, 0, 40, 24)];
        let borders = compute_borders(&geoms, PaneId(1), 80, 24);
        assert_eq!(borders.len(), 24, "vertical split must produce 24 border cells");
    }

    #[test]
    fn test_compute_borders_horizontal_split_correct_count() {
        // 80 wide, 24 tall: pane1 h=11, gap at row=11, pane2 y=12 h=12.
        // 1 gap row × 80 cols = 80 border cells.
        let geoms = [geom(1, 0, 0, 80, 11), geom(2, 0, 12, 80, 12)];
        let borders = compute_borders(&geoms, PaneId(1), 80, 24);
        assert_eq!(borders.len(), 80, "horizontal split must produce 80 border cells");
    }
}
