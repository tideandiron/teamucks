/// Layout engine: binary tree pane arrangement.
///
/// The layout engine manages a binary tree of panes.  Each interior node is a
/// [`Split`] that divides a rectangle into two sub-rectangles.  Each leaf is a
/// [`Pane`] leaf holding a [`PaneId`].
///
/// # Operations
///
/// - [`LayoutTree::split`] — divide a leaf pane into two
/// - [`LayoutTree::close`] — remove a leaf, promoting its sibling
/// - [`LayoutTree::resize`] — adjust the ratio of the nearest ancestor split
/// - [`LayoutTree::swap`] — exchange two leaf panes
/// - [`LayoutTree::rotate`] — flip the nearest ancestor split's direction
/// - [`LayoutTree::zoom`] / [`LayoutTree::unzoom`] — full-window overlay
///
/// # Coordinate Resolution
///
/// [`resolve::resolve`] walks the tree top-down, computing each pane's pixel
/// rectangle.  A 1-cell border is placed between adjacent panes.  Panes below
/// [`MIN_PANE_COLS`] × [`MIN_PANE_ROWS`] are never created.
///
/// # Serialization
///
/// Trees serialize to an s-expression string.  Example:
/// `(v 0.70 (h 0.50 [1] [2]) [3])`.
pub mod navigate;
pub mod resolve;
pub mod tree;

pub use tree::{Direction, LayoutTree};

use crate::pane::PaneId;

/// Minimum number of columns a pane may have.
pub const MIN_PANE_COLS: u16 = 5;

/// Minimum number of rows a pane may have.
pub const MIN_PANE_ROWS: u16 = 2;

// ---------------------------------------------------------------------------
// LayoutError
// ---------------------------------------------------------------------------

/// Errors produced by layout engine operations.
///
/// # Examples
///
/// ```
/// use teamucks_core::layout::{LayoutError, LayoutTree};
/// use teamucks_core::pane::PaneId;
///
/// let mut tree = LayoutTree::new(PaneId(1));
/// let err = tree.close(PaneId(1)).unwrap_err();
/// assert!(matches!(err, LayoutError::LastPane));
/// ```
#[derive(Debug, thiserror::Error)]
pub enum LayoutError {
    /// A pane with the given ID does not exist in this layout tree.
    #[error("pane {0} not found in layout")]
    PaneNotFound(PaneId),

    /// A split would create a child pane smaller than the minimum dimensions.
    #[error("split would create pane below minimum dimensions ({cols}x{rows})")]
    BelowMinimum {
        /// The column count that would result.
        cols: u16,
        /// The row count that would result.
        rows: u16,
    },

    /// The only pane is already at or below the minimum size.
    #[error("cannot split: only one pane and it's too small")]
    TooSmallToSplit,

    /// An attempt was made to close the only remaining pane.
    #[error("cannot close last pane")]
    LastPane,

    /// The serialized layout string could not be parsed.
    #[error("layout deserialization error: {0}")]
    ParseError(String),
}
