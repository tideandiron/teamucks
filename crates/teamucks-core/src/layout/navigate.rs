/// Spatial navigation between panes.
///
/// [`navigate`] finds the nearest pane in a given direction from the current
/// pane, using resolved geometries.  The direction parameter controls the axis
/// (Vertical = left/right, Horizontal = up/down).
///
/// The algorithm:
/// 1. Locate the source pane geometry.
/// 2. Determine the edge to project from (right edge for moving right, left
///    edge for moving left, etc.).
/// 3. Scan all other panes for the one whose opposing edge is closest to the
///    source edge and overlaps on the perpendicular axis.
///
/// This approach matches the geometry-based navigation used by tmux and zellij,
/// giving intuitive results for complex nested layouts.
use crate::{
    layout::{resolve::PaneGeometry, tree::Direction, tree::LayoutTree},
    pane::PaneId,
};

/// Find the nearest pane from `from` along `direction`.
///
/// - [`Direction::Vertical`] — navigate **left or right** (horizontal axis).
///   Returns the pane whose right edge is closest to `from`'s left edge
///   (going left) OR the pane whose left edge is closest to `from`'s right
///   edge (going right).  In practice we return both candidates and the caller
///   picks, but since the API returns a single `Option<PaneId>`, we return the
///   nearest pane in the positive direction (right for Vertical) unless `from`
///   is already the rightmost pane, in which case we try left.
///
/// - [`Direction::Horizontal`] — navigate **up or down** (vertical axis).
///   Same logic on the vertical axis.
///
/// The function returns the pane that is **spatially adjacent** (its edge
/// is exactly 1 cell away — the border) or the nearest one that shares
/// overlap on the perpendicular axis.
///
/// Returns `None` if there is no pane in the given direction.
///
/// # Examples
///
/// ```
/// use teamucks_core::layout::{LayoutTree, Direction, resolve::resolve, navigate::navigate};
/// use teamucks_core::pane::PaneId;
///
/// let mut tree = LayoutTree::new(PaneId(1));
/// tree.split(PaneId(1), Direction::Vertical, 0.5, PaneId(2)).unwrap();
/// let geoms = resolve(&tree, 80, 24);
/// // Pane 1 is on the left; navigate right finds pane 2.
/// assert_eq!(navigate(&tree, PaneId(1), Direction::Vertical, &geoms), Some(PaneId(2)));
/// ```
#[must_use]
pub fn navigate(
    _tree: &LayoutTree,
    from: PaneId,
    direction: Direction,
    geometries: &[PaneGeometry],
) -> Option<PaneId> {
    let src = geometries.iter().find(|g| g.pane_id == from)?;

    match direction {
        Direction::Vertical => {
            // Navigate left or right.
            // Try right first: find panes whose left edge is just right of src's right edge.
            let right_candidate = find_nearest_right(src, geometries);
            let left_candidate = find_nearest_left(src, geometries);

            // Prefer the direction the source pane is not at the boundary of.
            // If there's a right neighbor, return it; else return left.
            right_candidate.or(left_candidate)
        }
        Direction::Horizontal => {
            // Navigate up or down.
            let down_candidate = find_nearest_down(src, geometries);
            let up_candidate = find_nearest_up(src, geometries);

            down_candidate.or(up_candidate)
        }
    }
}

/// Find the nearest pane to the right of `src`, with perpendicular overlap.
fn find_nearest_right(src: &PaneGeometry, all: &[PaneGeometry]) -> Option<PaneId> {
    let src_right = src.x + src.width; // src right boundary (exclusive)

    all.iter()
        .filter(|g| {
            g.pane_id != src.pane_id
                && g.x > src.x // candidate is to the right
                && overlaps_vertically(src, g)
        })
        .min_by_key(|g| g.x.saturating_sub(src_right))
        .map(|g| g.pane_id)
}

/// Find the nearest pane to the left of `src`, with perpendicular overlap.
fn find_nearest_left(src: &PaneGeometry, all: &[PaneGeometry]) -> Option<PaneId> {
    all.iter()
        .filter(|g| {
            g.pane_id != src.pane_id
                && g.x + g.width < src.x + src.width // candidate left-of-or-at left edge
                && g.x < src.x
                && overlaps_vertically(src, g)
        })
        .max_by_key(|g| g.x + g.width)
        .map(|g| g.pane_id)
}

/// Find the nearest pane below `src`, with perpendicular overlap.
fn find_nearest_down(src: &PaneGeometry, all: &[PaneGeometry]) -> Option<PaneId> {
    let src_bottom = src.y + src.height;

    all.iter()
        .filter(|g| g.pane_id != src.pane_id && g.y > src.y && overlaps_horizontally(src, g))
        .min_by_key(|g| g.y.saturating_sub(src_bottom))
        .map(|g| g.pane_id)
}

/// Find the nearest pane above `src`, with perpendicular overlap.
fn find_nearest_up(src: &PaneGeometry, all: &[PaneGeometry]) -> Option<PaneId> {
    all.iter()
        .filter(|g| {
            g.pane_id != src.pane_id
                && g.y + g.height < src.y + src.height
                && g.y < src.y
                && overlaps_horizontally(src, g)
        })
        .max_by_key(|g| g.y + g.height)
        .map(|g| g.pane_id)
}

/// Do two pane geometries overlap on the vertical (Y) axis?
#[inline]
fn overlaps_vertically(a: &PaneGeometry, b: &PaneGeometry) -> bool {
    a.y < b.y + b.height && b.y < a.y + a.height
}

/// Do two pane geometries overlap on the horizontal (X) axis?
#[inline]
fn overlaps_horizontally(a: &PaneGeometry, b: &PaneGeometry) -> bool {
    a.x < b.x + b.width && b.x < a.x + a.width
}
