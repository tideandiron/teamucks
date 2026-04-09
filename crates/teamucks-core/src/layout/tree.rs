/// Binary tree layout nodes.
///
/// [`LayoutNode`] is the recursive data structure.  [`LayoutTree`] wraps it
/// with active-pane and zoom state.
use crate::{
    layout::{LayoutError, MIN_PANE_COLS, MIN_PANE_ROWS},
    pane::PaneId,
};

// ---------------------------------------------------------------------------
// Direction
// ---------------------------------------------------------------------------

/// The axis along which a split is oriented.
///
/// - [`Horizontal`][Direction::Horizontal] — the split line runs left-to-right;
///   the two children are stacked **top/bottom**.
/// - [`Vertical`][Direction::Vertical] — the split line runs top-to-bottom;
///   the two children are placed **left/right**.
///
/// # Examples
///
/// ```
/// use teamucks_core::layout::Direction;
/// assert_ne!(Direction::Horizontal, Direction::Vertical);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    /// Split line is horizontal; children are stacked top and bottom.
    Horizontal,
    /// Split line is vertical; children are placed left and right.
    Vertical,
}

// ---------------------------------------------------------------------------
// LayoutNode
// ---------------------------------------------------------------------------

/// A node in the binary layout tree.
///
/// Interior nodes are [`Split`][LayoutNode::Split] (two children plus a
/// direction and ratio).  Leaf nodes are [`Pane`][LayoutNode::Pane] (a single
/// pane ID).
#[derive(Debug)]
pub enum LayoutNode {
    /// An interior node that divides its rectangle between two children.
    Split {
        /// Axis of the split.
        direction: Direction,
        /// Fraction of the rectangle allocated to `first`.  In `[0.0, 1.0]`.
        ratio: f32,
        /// The "first" child (left for Vertical, top for Horizontal).
        first: Box<LayoutNode>,
        /// The "second" child (right for Vertical, bottom for Horizontal).
        second: Box<LayoutNode>,
    },
    /// A leaf node holding a single pane.
    Pane {
        /// The pane's identifier.
        id: PaneId,
    },
}

impl LayoutNode {
    /// Count the number of leaf panes in this subtree.
    #[must_use]
    pub fn pane_count(&self) -> usize {
        match self {
            Self::Pane { .. } => 1,
            Self::Split { first, second, .. } => first.pane_count() + second.pane_count(),
        }
    }

    /// Return `true` if this node is a leaf for `id`.
    #[must_use]
    pub fn is_pane(&self, id: PaneId) -> bool {
        matches!(self, Self::Pane { id: leaf_id } if *leaf_id == id)
    }

    /// Collect all pane IDs into `out` (depth-first).
    pub fn collect_ids(&self, out: &mut Vec<PaneId>) {
        match self {
            Self::Pane { id } => out.push(*id),
            Self::Split { first, second, .. } => {
                first.collect_ids(out);
                second.collect_ids(out);
            }
        }
    }
}

// ---------------------------------------------------------------------------
// CloseResult (internal)
// ---------------------------------------------------------------------------

/// Result of a close operation — used internally to propagate the replacement
/// node up the recursive call.
pub(crate) enum CloseResult {
    /// The target leaf was found and removed; the boxed node is its sibling
    /// (the new replacement for the parent).
    Replaced(Box<LayoutNode>),
    /// The removal completed deeper in the tree; the caller does not need to
    /// take any further action (the in-place replacement is done).
    Done,
    /// The target was not found anywhere in this subtree.
    NotFound,
    /// The target was the root (last pane).
    LastPane,
}

// ---------------------------------------------------------------------------
// LayoutTree
// ---------------------------------------------------------------------------

/// The full layout state for a single window.
///
/// Wraps a [`LayoutNode`] root with active-pane tracking and an optional zoom.
///
/// # Examples
///
/// ```
/// use teamucks_core::layout::{LayoutTree, Direction};
/// use teamucks_core::pane::PaneId;
///
/// let mut tree = LayoutTree::new(PaneId(1));
/// tree.split(PaneId(1), Direction::Vertical, 0.5, PaneId(2))
///     .expect("split must succeed on a large window");
/// assert!(!tree.is_zoomed());
/// ```
#[derive(Debug)]
pub struct LayoutTree {
    pub(crate) root: LayoutNode,
    /// The currently active pane.
    pub active_pane: PaneId,
    /// When `Some`, only this pane renders (full window size).
    pub zoomed_pane: Option<PaneId>,
    /// Window dimensions are not stored here; they are passed to `resolve`.
    /// We keep a hint for validation during split (defaulting to a generous
    /// size; callers that know the real dimensions can validate themselves).
    pub(crate) window_width: u16,
    pub(crate) window_height: u16,
}

impl LayoutTree {
    /// Create a new layout tree with a single root pane.
    ///
    /// # Examples
    ///
    /// ```
    /// use teamucks_core::layout::LayoutTree;
    /// use teamucks_core::pane::PaneId;
    ///
    /// let tree = LayoutTree::new(PaneId(1));
    /// ```
    #[must_use]
    pub fn new(root_pane: PaneId) -> Self {
        Self {
            root: LayoutNode::Pane { id: root_pane },
            active_pane: root_pane,
            zoomed_pane: None,
            // Default window hint — split validation uses this.
            window_width: 80,
            window_height: 24,
        }
    }

    /// Create a new layout tree with explicit window dimensions for split
    /// validation.
    #[must_use]
    pub fn with_dimensions(root_pane: PaneId, width: u16, height: u16) -> Self {
        Self {
            root: LayoutNode::Pane { id: root_pane },
            active_pane: root_pane,
            zoomed_pane: None,
            window_width: width,
            window_height: height,
        }
    }

    /// Update the stored window dimensions (used for split validation).
    pub fn set_dimensions(&mut self, width: u16, height: u16) {
        self.window_width = width;
        self.window_height = height;
    }

    // -----------------------------------------------------------------------
    // Split
    // -----------------------------------------------------------------------

    /// Split the pane identified by `target_pane` in `direction`.
    ///
    /// The original pane becomes the **first** child; `new_pane_id` becomes
    /// the **second** child.  `ratio` is the fraction of the parent rectangle
    /// given to the first child.
    ///
    /// # Errors
    ///
    /// - [`LayoutError::PaneNotFound`] — `target_pane` is not in the tree.
    /// - [`LayoutError::BelowMinimum`] — the split would produce a child
    ///   smaller than [`MIN_PANE_COLS`] × [`MIN_PANE_ROWS`].
    /// - [`LayoutError::TooSmallToSplit`] — the window is too small to split.
    ///
    /// # Examples
    ///
    /// ```
    /// use teamucks_core::layout::{LayoutTree, Direction};
    /// use teamucks_core::pane::PaneId;
    ///
    /// let mut tree = LayoutTree::new(PaneId(1));
    /// tree.split(PaneId(1), Direction::Vertical, 0.5, PaneId(2))
    ///     .expect("split must succeed");
    /// ```
    pub fn split(
        &mut self,
        target_pane: PaneId,
        direction: Direction,
        ratio: f32,
        new_pane_id: PaneId,
    ) -> Result<(), LayoutError> {
        // Validate minimum dimensions before modifying the tree.
        self.validate_split(target_pane, direction, ratio)?;

        let replaced =
            replace_leaf_with_split(&mut self.root, target_pane, direction, ratio, new_pane_id);
        if replaced {
            Ok(())
        } else {
            Err(LayoutError::PaneNotFound(target_pane))
        }
    }

    /// Validate that a split would not produce sub-minimum panes.
    fn validate_split(
        &self,
        target_pane: PaneId,
        direction: Direction,
        ratio: f32,
    ) -> Result<(), LayoutError> {
        // Compute the rectangle that target_pane currently occupies.
        let geoms = crate::layout::resolve::resolve(self, self.window_width, self.window_height);
        let g = geoms
            .iter()
            .find(|g| g.pane_id == target_pane)
            .ok_or(LayoutError::PaneNotFound(target_pane))?;

        match direction {
            Direction::Vertical => {
                // Split left/right — divides width.
                let usable = g.width.saturating_sub(1); // 1-cell border
                if usable < MIN_PANE_COLS * 2 {
                    // Not enough room for two minimum-size panes.
                    return Err(LayoutError::TooSmallToSplit);
                }
                // f32::from(u16) is lossless; truncation to u16 safe (ratio in [0,1]).
                #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
                let first_w = (f32::from(usable) * ratio) as u16;
                let second_w = usable - first_w;
                if first_w < MIN_PANE_COLS {
                    return Err(LayoutError::BelowMinimum { cols: first_w, rows: g.height });
                }
                if second_w < MIN_PANE_COLS {
                    return Err(LayoutError::BelowMinimum { cols: second_w, rows: g.height });
                }
            }
            Direction::Horizontal => {
                // Split top/bottom — divides height.
                let usable = g.height.saturating_sub(1); // 1-cell border
                if usable < MIN_PANE_ROWS * 2 {
                    return Err(LayoutError::TooSmallToSplit);
                }
                // f32::from(u16) is lossless; truncation to u16 safe (ratio in [0,1]).
                #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
                let first_h = (f32::from(usable) * ratio) as u16;
                let second_h = usable - first_h;
                if first_h < MIN_PANE_ROWS {
                    return Err(LayoutError::BelowMinimum { cols: g.width, rows: first_h });
                }
                if second_h < MIN_PANE_ROWS {
                    return Err(LayoutError::BelowMinimum { cols: g.width, rows: second_h });
                }
            }
        }
        Ok(())
    }

    // -----------------------------------------------------------------------
    // Close
    // -----------------------------------------------------------------------

    /// Remove `pane_id` from the tree, promoting its sibling.
    ///
    /// # Errors
    ///
    /// - [`LayoutError::LastPane`] — `pane_id` is the only pane in the tree.
    /// - [`LayoutError::PaneNotFound`] — `pane_id` is not in the tree.
    ///
    /// # Examples
    ///
    /// ```
    /// use teamucks_core::layout::{LayoutTree, Direction};
    /// use teamucks_core::pane::PaneId;
    ///
    /// let mut tree = LayoutTree::new(PaneId(1));
    /// tree.split(PaneId(1), Direction::Vertical, 0.5, PaneId(2)).unwrap();
    /// tree.close(PaneId(1)).expect("close must succeed");
    /// ```
    pub fn close(&mut self, pane_id: PaneId) -> Result<(), LayoutError> {
        // Special case: root is the target pane.
        if self.root.is_pane(pane_id) {
            return Err(LayoutError::LastPane);
        }

        // Clear zoom if we're closing the zoomed pane.
        if self.zoomed_pane == Some(pane_id) {
            self.zoomed_pane = None;
        }

        let result = close_leaf(&mut self.root, pane_id);
        match result {
            CloseResult::Replaced(new_root) => {
                // The root itself was a Split whose direct child was the target;
                // the returned node is the sibling that now becomes root.
                self.root = *new_root;
                self.fixup_active_pane(pane_id);
                Ok(())
            }
            CloseResult::Done => {
                // Replacement was applied in-place deeper in the tree.
                self.fixup_active_pane(pane_id);
                Ok(())
            }
            CloseResult::NotFound => Err(LayoutError::PaneNotFound(pane_id)),
            CloseResult::LastPane => Err(LayoutError::LastPane),
        }
    }

    /// Update `active_pane` if `closed_pane` was the active one.
    fn fixup_active_pane(&mut self, closed_pane: PaneId) {
        if self.active_pane == closed_pane {
            let mut ids = Vec::new();
            self.root.collect_ids(&mut ids);
            if let Some(&first) = ids.first() {
                self.active_pane = first;
            }
        }
    }

    // -----------------------------------------------------------------------
    // Resize
    // -----------------------------------------------------------------------

    /// Adjust the ratio of the nearest ancestor split in `direction`.
    ///
    /// `delta` is added to the current ratio (clamped so neither child falls
    /// below its minimum dimensions).  Returns `true` if the ratio changed.
    ///
    /// # Examples
    ///
    /// ```
    /// use teamucks_core::layout::{LayoutTree, Direction};
    /// use teamucks_core::pane::PaneId;
    ///
    /// let mut tree = LayoutTree::new(PaneId(1));
    /// tree.split(PaneId(1), Direction::Vertical, 0.5, PaneId(2)).unwrap();
    /// let changed = tree.resize(PaneId(1), Direction::Vertical, 0.1);
    /// assert!(changed);
    /// ```
    pub fn resize(&mut self, pane_id: PaneId, direction: Direction, delta: f32) -> bool {
        let w = self.window_width;
        let h = self.window_height;
        resize_nearest(&mut self.root, pane_id, direction, delta, w, h)
    }

    // -----------------------------------------------------------------------
    // Swap
    // -----------------------------------------------------------------------

    /// Exchange two pane leaves in the tree.
    ///
    /// # Errors
    ///
    /// Returns [`LayoutError::PaneNotFound`] if either pane is not in the tree.
    ///
    /// # Examples
    ///
    /// ```
    /// use teamucks_core::layout::{LayoutTree, Direction};
    /// use teamucks_core::pane::PaneId;
    ///
    /// let mut tree = LayoutTree::new(PaneId(1));
    /// tree.split(PaneId(1), Direction::Vertical, 0.5, PaneId(2)).unwrap();
    /// tree.swap(PaneId(1), PaneId(2)).expect("swap must succeed");
    /// ```
    pub fn swap(&mut self, pane_a: PaneId, pane_b: PaneId) -> Result<(), LayoutError> {
        // Verify both panes exist.
        let mut ids = Vec::new();
        self.root.collect_ids(&mut ids);
        if !ids.contains(&pane_a) {
            return Err(LayoutError::PaneNotFound(pane_a));
        }
        if !ids.contains(&pane_b) {
            return Err(LayoutError::PaneNotFound(pane_b));
        }
        // Swap the IDs in the tree.
        swap_pane_ids(&mut self.root, pane_a, pane_b);
        Ok(())
    }

    // -----------------------------------------------------------------------
    // Rotate
    // -----------------------------------------------------------------------

    /// Flip the direction of the nearest ancestor split of `pane_id`.
    ///
    /// Horizontal becomes Vertical and vice versa.
    ///
    /// # Errors
    ///
    /// Returns [`LayoutError::PaneNotFound`] if `pane_id` is not in the tree.
    ///
    /// # Examples
    ///
    /// ```
    /// use teamucks_core::layout::{LayoutTree, Direction};
    /// use teamucks_core::pane::PaneId;
    ///
    /// let mut tree = LayoutTree::new(PaneId(1));
    /// tree.split(PaneId(1), Direction::Vertical, 0.5, PaneId(2)).unwrap();
    /// tree.rotate(PaneId(1)).expect("rotate must succeed");
    /// ```
    pub fn rotate(&mut self, pane_id: PaneId) -> Result<(), LayoutError> {
        if rotate_nearest(&mut self.root, pane_id) {
            Ok(())
        } else {
            // Check if pane exists — if root is a bare pane, it exists but has
            // no parent split to rotate.
            let mut ids = Vec::new();
            self.root.collect_ids(&mut ids);
            if ids.contains(&pane_id) {
                // Single-pane tree: no split to rotate.
                Ok(())
            } else {
                Err(LayoutError::PaneNotFound(pane_id))
            }
        }
    }

    // -----------------------------------------------------------------------
    // Zoom
    // -----------------------------------------------------------------------

    /// Set the zoomed pane.  When zoomed, [`resolve`][crate::layout::resolve::resolve]
    /// returns only the zoomed pane at full window dimensions.
    ///
    /// # Examples
    ///
    /// ```
    /// use teamucks_core::layout::LayoutTree;
    /// use teamucks_core::pane::PaneId;
    ///
    /// let mut tree = LayoutTree::new(PaneId(1));
    /// tree.zoom(PaneId(1));
    /// assert!(tree.is_zoomed());
    /// ```
    pub fn zoom(&mut self, pane_id: PaneId) {
        self.zoomed_pane = Some(pane_id);
    }

    /// Clear the zoom state.
    ///
    /// # Examples
    ///
    /// ```
    /// use teamucks_core::layout::LayoutTree;
    /// use teamucks_core::pane::PaneId;
    ///
    /// let mut tree = LayoutTree::new(PaneId(1));
    /// tree.zoom(PaneId(1));
    /// tree.unzoom();
    /// assert!(!tree.is_zoomed());
    /// ```
    pub fn unzoom(&mut self) {
        self.zoomed_pane = None;
    }

    /// Return `true` if a pane is currently zoomed.
    ///
    /// # Examples
    ///
    /// ```
    /// use teamucks_core::layout::LayoutTree;
    /// use teamucks_core::pane::PaneId;
    ///
    /// let tree = LayoutTree::new(PaneId(1));
    /// assert!(!tree.is_zoomed());
    /// ```
    #[must_use]
    pub fn is_zoomed(&self) -> bool {
        self.zoomed_pane.is_some()
    }

    // -----------------------------------------------------------------------
    // Serialization
    // -----------------------------------------------------------------------

    /// Serialize the layout tree to an s-expression string.
    ///
    /// Format: `(v 0.50 [1] [2])` for a vertical split with ratio 0.50,
    /// first child pane 1 and second child pane 2.
    ///
    /// Single pane: `[1]`.
    ///
    /// # Examples
    ///
    /// ```
    /// use teamucks_core::layout::LayoutTree;
    /// use teamucks_core::pane::PaneId;
    ///
    /// let tree = LayoutTree::new(PaneId(1));
    /// let s = tree.serialize();
    /// assert!(s.contains('1'));
    /// ```
    #[must_use]
    pub fn serialize(&self) -> String {
        serialize_node(&self.root)
    }

    /// Deserialize a layout tree from an s-expression string.
    ///
    /// The `active_pane` is set to the first pane found during deserialization.
    ///
    /// # Errors
    ///
    /// Returns [`LayoutError::ParseError`] if the string is malformed.
    ///
    /// # Examples
    ///
    /// ```
    /// use teamucks_core::layout::LayoutTree;
    ///
    /// let tree = LayoutTree::deserialize("[42]").expect("single pane");
    /// ```
    pub fn deserialize(s: &str) -> Result<Self, LayoutError> {
        let s = s.trim();
        let (node, rest) = parse_node(s)?;
        let rest = rest.trim();
        if !rest.is_empty() {
            return Err(LayoutError::ParseError(format!("unexpected trailing input: {rest:?}")));
        }
        let mut ids = Vec::new();
        node.collect_ids(&mut ids);
        let active_pane =
            ids.first().copied().ok_or_else(|| LayoutError::ParseError("empty tree".into()))?;
        Ok(Self { root: node, active_pane, zoomed_pane: None, window_width: 80, window_height: 24 })
    }
}

// ---------------------------------------------------------------------------
// Tree mutation helpers (free functions operating on &mut LayoutNode)
// ---------------------------------------------------------------------------

/// Replace the leaf `target` with a Split node.  Returns `true` on success.
fn replace_leaf_with_split(
    node: &mut LayoutNode,
    target: PaneId,
    direction: Direction,
    ratio: f32,
    new_id: PaneId,
) -> bool {
    match node {
        LayoutNode::Pane { id } if *id == target => {
            // Replace this leaf.
            let original_id = *id;
            *node = LayoutNode::Split {
                direction,
                ratio,
                first: Box::new(LayoutNode::Pane { id: original_id }),
                second: Box::new(LayoutNode::Pane { id: new_id }),
            };
            true
        }
        LayoutNode::Pane { .. } => false,
        LayoutNode::Split { first, second, .. } => {
            replace_leaf_with_split(first, target, direction, ratio, new_id)
                || replace_leaf_with_split(second, target, direction, ratio, new_id)
        }
    }
}

/// Remove the leaf `target` from the tree.  Returns a [`CloseResult`].
///
/// When a Split has `target` as one of its children, the other child is
/// "promoted" — i.e. returned as [`CloseResult::Replaced`] — and the caller
/// replaces the Split with it.
fn close_leaf(node: &mut LayoutNode, target: PaneId) -> CloseResult {
    match node {
        LayoutNode::Pane { id } => {
            if *id == target {
                CloseResult::LastPane
            } else {
                CloseResult::NotFound
            }
        }
        LayoutNode::Split { first, second, .. } => {
            // Check if either child is the target leaf.
            if first.is_pane(target) {
                // Promote second.
                // We need to take second out of the split.  We'll signal the
                // caller to replace this node with second.
                // Temporarily swap second with a dummy to move it.
                let dummy = Box::new(LayoutNode::Pane { id: PaneId(u32::MAX) });
                let sibling = std::mem::replace(second, dummy);
                CloseResult::Replaced(sibling)
            } else if second.is_pane(target) {
                let dummy = Box::new(LayoutNode::Pane { id: PaneId(u32::MAX) });
                let sibling = std::mem::replace(first, dummy);
                CloseResult::Replaced(sibling)
            } else {
                // Recurse into first.
                let res = close_leaf(first, target);
                match res {
                    CloseResult::Replaced(new_first) => {
                        *first = new_first;
                        CloseResult::Done // replacement applied in-place; signal done
                    }
                    CloseResult::Done => CloseResult::Done,
                    CloseResult::NotFound => {
                        // Try second.
                        let res2 = close_leaf(second, target);
                        match res2 {
                            CloseResult::Replaced(new_second) => {
                                *second = new_second;
                                CloseResult::Done
                            }
                            other @ (CloseResult::Done
                            | CloseResult::NotFound
                            | CloseResult::LastPane) => other,
                        }
                    }
                    other @ CloseResult::LastPane => other,
                }
            }
        }
    }
}

/// Walk up from `pane_id` and adjust the ratio of the nearest ancestor split
/// matching `direction`.  Returns `true` if a change was made.
fn resize_nearest(
    node: &mut LayoutNode,
    pane_id: PaneId,
    direction: Direction,
    delta: f32,
    window_width: u16,
    window_height: u16,
) -> bool {
    match node {
        LayoutNode::Pane { .. } => false,
        LayoutNode::Split { direction: split_dir, ratio, first, second } => {
            // Try to find pane_id in our subtree first.
            let mut ids = Vec::new();
            first.collect_ids(&mut ids);
            second.collect_ids(&mut ids);
            if !ids.contains(&pane_id) {
                // Not in this subtree.
                return false;
            }

            // If this split matches the desired direction, adjust it.
            if *split_dir == direction {
                let new_ratio = (*ratio + delta).clamp(0.0, 1.0);

                // Enforce minimums on both children.
                // Compute what dimensions each child would get.
                let clamped =
                    clamp_ratio_for_minimums(new_ratio, *split_dir, window_width, window_height);
                if (clamped - *ratio).abs() > f32::EPSILON {
                    *ratio = clamped;
                    return true;
                }
                return false;
            }

            // Otherwise recurse.
            let in_first = {
                let mut first_ids = Vec::new();
                first.collect_ids(&mut first_ids);
                first_ids.contains(&pane_id)
            };

            if in_first {
                resize_nearest(first, pane_id, direction, delta, window_width, window_height)
            } else {
                resize_nearest(second, pane_id, direction, delta, window_width, window_height)
            }
        }
    }
}

/// Clamp a ratio so neither child falls below the minimum dimensions.
///
/// This is a best-effort clamp on the root split; it doesn't account for the
/// full tree geometry (which would require a full resolve pass).  For most
/// cases it's sufficient.
fn clamp_ratio_for_minimums(ratio: f32, direction: Direction, width: u16, height: u16) -> f32 {
    match direction {
        Direction::Vertical => {
            // f32::from(u16) is lossless.
            let usable = f32::from(width.saturating_sub(1));
            if usable <= 0.0 {
                return 0.5;
            }
            let min_ratio = f32::from(MIN_PANE_COLS) / usable;
            let max_ratio = 1.0 - min_ratio;
            ratio.clamp(min_ratio, max_ratio)
        }
        Direction::Horizontal => {
            // f32::from(u16) is lossless.
            let usable = f32::from(height.saturating_sub(1));
            if usable <= 0.0 {
                return 0.5;
            }
            let min_ratio = f32::from(MIN_PANE_ROWS) / usable;
            let max_ratio = 1.0 - min_ratio;
            ratio.clamp(min_ratio, max_ratio)
        }
    }
}

/// Swap the IDs of two leaf panes in the tree.
fn swap_pane_ids(node: &mut LayoutNode, a: PaneId, b: PaneId) {
    match node {
        LayoutNode::Pane { id } => {
            if *id == a {
                *id = b;
            } else if *id == b {
                *id = a;
            }
        }
        LayoutNode::Split { first, second, .. } => {
            swap_pane_ids(first, a, b);
            swap_pane_ids(second, a, b);
        }
    }
}

/// Flip the direction of the nearest ancestor split that contains `pane_id`.
/// Returns `true` if a rotation was performed.
fn rotate_nearest(node: &mut LayoutNode, pane_id: PaneId) -> bool {
    match node {
        LayoutNode::Pane { .. } => false,
        LayoutNode::Split { direction, first, second, .. } => {
            // Check if pane_id is a direct child leaf (nearest ancestor is self).
            if first.is_pane(pane_id) || second.is_pane(pane_id) {
                *direction = match *direction {
                    Direction::Horizontal => Direction::Vertical,
                    Direction::Vertical => Direction::Horizontal,
                };
                return true;
            }

            // Try to find it deeper.
            let in_first = {
                let mut ids = Vec::new();
                first.collect_ids(&mut ids);
                ids.contains(&pane_id)
            };

            if in_first {
                if rotate_nearest(first, pane_id) {
                    return true;
                }
                // If the subtree rotation succeeded by operating on a deeper
                // split, the "nearest" ancestor containing pane_id as a direct
                // child is self. Try this level.
                // Actually: if first contains pane_id (not as direct leaf), we
                // already recursed. If recursion returned false, pane not found.
                false
            } else {
                rotate_nearest(second, pane_id)
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Serialization helpers
// ---------------------------------------------------------------------------

fn serialize_node(node: &LayoutNode) -> String {
    match node {
        LayoutNode::Pane { id } => format!("[{}]", id.0),
        LayoutNode::Split { direction, ratio, first, second } => {
            let dir_char = match direction {
                Direction::Vertical => 'v',
                Direction::Horizontal => 'h',
            };
            format!("({dir_char} {ratio:.2} {} {})", serialize_node(first), serialize_node(second))
        }
    }
}

/// Parse a node from the start of `s`.  Returns `(node, remaining_str)`.
fn parse_node(s: &str) -> Result<(LayoutNode, &str), LayoutError> {
    let s = s.trim_start();
    if let Some(s) = s.strip_prefix('[') {
        // Pane leaf: `[<id>]`.
        let end = s
            .find(']')
            .ok_or_else(|| LayoutError::ParseError(format!("unclosed '[' in: {s:?}")))?;
        let id_str = s[..end].trim();
        let id: u32 = id_str
            .parse()
            .map_err(|_| LayoutError::ParseError(format!("invalid pane id {id_str:?}")))?;
        Ok((LayoutNode::Pane { id: PaneId(id) }, &s[end + 1..]))
    } else if let Some(s) = s.strip_prefix('(') {
        // Split: `(<dir> <ratio> <first> <second>)`.
        let s = s.trim_start();

        // Direction character.
        let (dir_char, s) = s
            .split_once(char::is_whitespace)
            .ok_or_else(|| LayoutError::ParseError(format!("expected direction in: {s:?}")))?;
        let direction = match dir_char.trim() {
            "v" => Direction::Vertical,
            "h" => Direction::Horizontal,
            other => return Err(LayoutError::ParseError(format!("unknown direction {other:?}"))),
        };

        let s = s.trim_start();
        // Ratio: next whitespace-delimited token.
        let (ratio_str, s) = s
            .split_once(char::is_whitespace)
            .ok_or_else(|| LayoutError::ParseError(format!("expected ratio in: {s:?}")))?;
        let ratio: f32 = ratio_str
            .trim()
            .parse()
            .map_err(|_| LayoutError::ParseError(format!("invalid ratio {ratio_str:?}")))?;

        let s = s.trim_start();
        let (first_node, s) = parse_node(s)?;
        let s = s.trim_start();
        let (second_node, s) = parse_node(s)?;
        let s = s.trim_start();

        // Consume closing ')'.
        let s = s
            .strip_prefix(')')
            .ok_or_else(|| LayoutError::ParseError(format!("expected ')' but found: {s:?}")))?;

        Ok((
            LayoutNode::Split {
                direction,
                ratio,
                first: Box::new(first_node),
                second: Box::new(second_node),
            },
            s,
        ))
    } else {
        Err(LayoutError::ParseError(format!("expected '[' or '(' but found: {s:?}")))
    }
}
