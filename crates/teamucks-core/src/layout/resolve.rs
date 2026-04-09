/// Coordinate resolution: convert a [`LayoutTree`] into concrete pixel
/// rectangles for each pane.
///
/// The top-down recursive walk divides rectangles at each [`Split`] node.
/// A 1-cell border is placed between the two children.  Panes are never
/// smaller than [`MIN_PANE_COLS`] × [`MIN_PANE_ROWS`].
///
/// When the tree is zoomed, only the zoomed pane is returned, filling the
/// entire window.
use crate::{
    layout::{
        tree::{Direction, LayoutNode, LayoutTree},
        MIN_PANE_COLS, MIN_PANE_ROWS,
    },
    pane::PaneId,
};

// ---------------------------------------------------------------------------
// PaneGeometry
// ---------------------------------------------------------------------------

/// The resolved screen rectangle for a single pane.
///
/// Coordinates are 0-based, measured in terminal cells.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PaneGeometry {
    /// The pane this geometry belongs to.
    pub pane_id: PaneId,
    /// Left edge (column index, 0-based).
    pub x: u16,
    /// Top edge (row index, 0-based).
    pub y: u16,
    /// Number of columns (content area, not including borders).
    pub width: u16,
    /// Number of rows (content area, not including borders).
    pub height: u16,
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Resolve a [`LayoutTree`] into [`PaneGeometry`] rectangles.
///
/// When the tree is zoomed, returns a single geometry for the zoomed pane at
/// the full `window_width` × `window_height` size.
///
/// # Examples
///
/// ```
/// use teamucks_core::layout::{LayoutTree, resolve::resolve};
/// use teamucks_core::pane::PaneId;
///
/// let tree = LayoutTree::new(PaneId(1));
/// let geoms = resolve(&tree, 80, 24);
/// assert_eq!(geoms.len(), 1);
/// assert_eq!(geoms[0].width, 80);
/// assert_eq!(geoms[0].height, 24);
/// ```
#[must_use]
pub fn resolve(tree: &LayoutTree, window_width: u16, window_height: u16) -> Vec<PaneGeometry> {
    // Zoom: return only the zoomed pane at full window size.
    if let Some(zoomed) = tree.zoomed_pane {
        return vec![PaneGeometry {
            pane_id: zoomed,
            x: 0,
            y: 0,
            width: window_width,
            height: window_height,
        }];
    }

    let mut out = Vec::new();
    resolve_node(&tree.root, 0, 0, window_width, window_height, &mut out);
    out
}

// ---------------------------------------------------------------------------
// Recursive resolver
// ---------------------------------------------------------------------------

fn resolve_node(
    node: &LayoutNode,
    x: u16,
    y: u16,
    width: u16,
    height: u16,
    out: &mut Vec<PaneGeometry>,
) {
    match node {
        LayoutNode::Pane { id } => {
            // Clamp to minimum dimensions; normally the split validation
            // prevents us from reaching here, but guard defensively.
            let width = width.max(MIN_PANE_COLS);
            let height = height.max(MIN_PANE_ROWS);
            out.push(PaneGeometry { pane_id: *id, x, y, width, height });
        }
        LayoutNode::Split { direction, ratio, first, second } => {
            match direction {
                Direction::Vertical => {
                    // Left/right split.  The border costs 1 column.
                    let usable = width.saturating_sub(1);
                    // f32::from(u16) is lossless (f32 has 24-bit mantissa, u16 is 16-bit).
                    // The product is then truncated to u16; ratio is in [0,1] so the
                    // result is in [0, usable], which always fits in u16.
                    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
                    let first_w = (f32::from(usable) * ratio) as u16;
                    let first_w =
                        first_w.max(MIN_PANE_COLS).min(usable.saturating_sub(MIN_PANE_COLS));
                    let second_w = usable.saturating_sub(first_w).max(MIN_PANE_COLS);

                    // First child: left pane.
                    resolve_node(first, x, y, first_w, height, out);
                    // Border column at x + first_w is implicit (drawn by renderer).
                    // Second child: right pane.
                    resolve_node(second, x + first_w + 1, y, second_w, height, out);
                }
                Direction::Horizontal => {
                    // Top/bottom split.  The border costs 1 row.
                    let usable = height.saturating_sub(1);
                    // f32::from(u16) is lossless; truncation to u16 is safe (see above).
                    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
                    let first_h = (f32::from(usable) * ratio) as u16;
                    let first_h =
                        first_h.max(MIN_PANE_ROWS).min(usable.saturating_sub(MIN_PANE_ROWS));
                    let second_h = usable.saturating_sub(first_h).max(MIN_PANE_ROWS);

                    // First child: top pane.
                    resolve_node(first, x, y, width, first_h, out);
                    // Border row at y + first_h is implicit.
                    // Second child: bottom pane.
                    resolve_node(second, x, y + first_h + 1, width, second_h, out);
                }
            }
        }
    }
}
