/// Layout engine integration tests.
///
/// These tests exercise the binary tree layout engine end-to-end:
/// split, close, resize, swap, rotate, zoom, coordinate resolution,
/// navigation, and serialization.
use teamucks_core::{
    layout::{
        navigate::navigate,
        resolve::{resolve, PaneGeometry},
        tree::{Direction, LayoutTree},
        LayoutError, MIN_PANE_COLS, MIN_PANE_ROWS,
    },
    pane::PaneId,
};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn geom_for(geoms: &[PaneGeometry], id: PaneId) -> &PaneGeometry {
    geoms.iter().find(|g| g.pane_id == id).unwrap_or_else(|| panic!("no geometry for {id:?}"))
}

// ---------------------------------------------------------------------------
// Single pane
// ---------------------------------------------------------------------------

#[test]
fn test_single_pane_fills_window() {
    let tree = LayoutTree::new(PaneId(1));
    let geoms = resolve(&tree, 80, 24);
    assert_eq!(geoms.len(), 1);
    let g = &geoms[0];
    assert_eq!(g.pane_id, PaneId(1));
    assert_eq!(g.x, 0);
    assert_eq!(g.y, 0);
    assert_eq!(g.width, 80);
    assert_eq!(g.height, 24);
}

// ---------------------------------------------------------------------------
// Split
// ---------------------------------------------------------------------------

#[test]
fn test_split_vertical() {
    let mut tree = LayoutTree::new(PaneId(1));
    tree.split(PaneId(1), Direction::Vertical, 0.5, PaneId(2))
        .expect("vertical split must succeed");
    let geoms = resolve(&tree, 80, 24);
    assert_eq!(geoms.len(), 2);
    let g1 = geom_for(&geoms, PaneId(1));
    let g2 = geom_for(&geoms, PaneId(2));
    // Vertical split: side-by-side. They share a 1-cell border.
    assert_eq!(g1.y, 0);
    assert_eq!(g1.height, 24);
    assert_eq!(g2.y, 0);
    assert_eq!(g2.height, 24);
    assert!(g1.width > 0);
    assert!(g2.width > 0);
    // Together they must cover 80 minus the shared 1-cell border.
    assert_eq!(g1.width + g2.width + 1, 80);
    assert_eq!(g1.x + g1.width + 1, g2.x);
}

#[test]
fn test_split_horizontal() {
    let mut tree = LayoutTree::new(PaneId(1));
    tree.split(PaneId(1), Direction::Horizontal, 0.5, PaneId(2))
        .expect("horizontal split must succeed");
    let geoms = resolve(&tree, 80, 24);
    assert_eq!(geoms.len(), 2);
    let g1 = geom_for(&geoms, PaneId(1));
    let g2 = geom_for(&geoms, PaneId(2));
    // Horizontal split: stacked. They share a 1-cell border.
    assert_eq!(g1.x, 0);
    assert_eq!(g1.width, 80);
    assert_eq!(g2.x, 0);
    assert_eq!(g2.width, 80);
    assert!(g1.height > 0);
    assert!(g2.height > 0);
    assert_eq!(g1.height + g2.height + 1, 24);
    assert_eq!(g1.y + g1.height + 1, g2.y);
}

#[test]
fn test_split_ratio_70_30() {
    let mut tree = LayoutTree::new(PaneId(1));
    tree.split(PaneId(1), Direction::Vertical, 0.7, PaneId(2)).expect("70/30 split must succeed");
    let geoms = resolve(&tree, 100, 24);
    let g1 = geom_for(&geoms, PaneId(1));
    let g2 = geom_for(&geoms, PaneId(2));
    // 100 cols - 1 border = 99 usable. 70% = 69.3 → 69 cols for first.
    // Allow ±1 for rounding.
    assert!((g1.width as i32 - 69).abs() <= 1, "first pane width={}", g1.width);
    assert!(g1.width + g2.width + 1 == 100);
}

#[test]
fn test_split_default_ratio() {
    let mut tree = LayoutTree::new(PaneId(1));
    tree.split(PaneId(1), Direction::Vertical, 0.5, PaneId(2)).expect("default split must succeed");
    let geoms = resolve(&tree, 80, 24);
    let g1 = geom_for(&geoms, PaneId(1));
    let g2 = geom_for(&geoms, PaneId(2));
    // 80 - 1 = 79, split evenly: 39 and 40 (or 40 and 39).
    let diff = (g1.width as i32 - g2.width as i32).abs();
    assert!(diff <= 1, "widths should be nearly equal: {} vs {}", g1.width, g2.width);
}

#[test]
fn test_split_rejected_too_small() {
    // Use a tree configured with a tiny window so splits violate minimums.
    // MIN_PANE_COLS = 5. A 10-col window with ratio 0.3:
    //   usable = 10 - 1 = 9 cols, first = floor(9 * 0.3) = 2 cols → below MIN_PANE_COLS.
    let mut tree = LayoutTree::with_dimensions(PaneId(1), 10, 24);
    let err = tree.split(PaneId(1), Direction::Vertical, 0.3, PaneId(2));
    assert!(
        matches!(err, Err(LayoutError::BelowMinimum { .. }))
            || matches!(err, Err(LayoutError::TooSmallToSplit)),
        "expected rejection for tiny first pane, got: {err:?}"
    );

    // Also test: window too small to split at all (< 2 * MIN_PANE_COLS + 1 border).
    // 2 * 5 + 1 = 11 → window of 10 cols is too small even at ratio 0.5.
    let mut tree2 = LayoutTree::with_dimensions(PaneId(10), 10, 24);
    let err2 = tree2.split(PaneId(10), Direction::Vertical, 0.5, PaneId(11));
    assert!(
        matches!(err2, Err(LayoutError::BelowMinimum { .. }))
            || matches!(err2, Err(LayoutError::TooSmallToSplit)),
        "expected rejection for too-small window, got: {err2:?}"
    );
}

// ---------------------------------------------------------------------------
// Close
// ---------------------------------------------------------------------------

#[test]
fn test_close_pane_promotes_sibling() {
    let mut tree = LayoutTree::new(PaneId(1));
    tree.split(PaneId(1), Direction::Vertical, 0.5, PaneId(2)).expect("split");
    tree.close(PaneId(1)).expect("close must succeed");
    let geoms = resolve(&tree, 80, 24);
    assert_eq!(geoms.len(), 1);
    let g = &geoms[0];
    assert_eq!(g.pane_id, PaneId(2));
    assert_eq!(g.width, 80);
    assert_eq!(g.height, 24);
}

#[test]
fn test_close_last_pane() {
    let mut tree = LayoutTree::new(PaneId(1));
    let err = tree.close(PaneId(1));
    assert!(matches!(err, Err(LayoutError::LastPane)));
}

// ---------------------------------------------------------------------------
// Resize
// ---------------------------------------------------------------------------

#[test]
fn test_resize_adjusts_ratio() {
    let mut tree = LayoutTree::new(PaneId(1));
    tree.split(PaneId(1), Direction::Vertical, 0.5, PaneId(2)).expect("split");

    let geoms_before = resolve(&tree, 80, 24);
    let w1_before = geom_for(&geoms_before, PaneId(1)).width;

    // Resize: move boundary 5 cells to the right → first pane gets larger.
    let changed = tree.resize(PaneId(1), Direction::Vertical, 0.1);
    assert!(changed, "resize should succeed and change the ratio");

    let geoms_after = resolve(&tree, 80, 24);
    let w1_after = geom_for(&geoms_after, PaneId(1)).width;
    assert!(w1_after > w1_before, "first pane should be wider after resize");
}

#[test]
fn test_resize_clamps_at_minimum() {
    let mut tree = LayoutTree::new(PaneId(1));
    tree.split(PaneId(1), Direction::Vertical, 0.5, PaneId(2)).expect("split");

    // Try to resize so far right that the second pane would be below minimum.
    let changed = tree.resize(PaneId(1), Direction::Vertical, 1.0);
    // Should clamp rather than going below MIN_PANE_COLS.
    if changed {
        let geoms = resolve(&tree, 80, 24);
        let g2 = geom_for(&geoms, PaneId(2));
        assert!(g2.width >= MIN_PANE_COLS, "second pane must stay >= MIN_PANE_COLS");
    }
}

// ---------------------------------------------------------------------------
// Swap
// ---------------------------------------------------------------------------

#[test]
fn test_swap_panes() {
    let mut tree = LayoutTree::new(PaneId(1));
    tree.split(PaneId(1), Direction::Vertical, 0.5, PaneId(2)).expect("split");

    let geoms_before = resolve(&tree, 80, 24);
    let x1_before = geom_for(&geoms_before, PaneId(1)).x;
    let x2_before = geom_for(&geoms_before, PaneId(2)).x;

    tree.swap(PaneId(1), PaneId(2)).expect("swap must succeed");

    let geoms_after = resolve(&tree, 80, 24);
    let x1_after = geom_for(&geoms_after, PaneId(1)).x;
    let x2_after = geom_for(&geoms_after, PaneId(2)).x;

    // After swap, pane 1 should be where pane 2 was and vice versa.
    assert_eq!(x1_after, x2_before);
    assert_eq!(x2_after, x1_before);
}

// ---------------------------------------------------------------------------
// Rotate
// ---------------------------------------------------------------------------

#[test]
fn test_rotate_split() {
    let mut tree = LayoutTree::new(PaneId(1));
    tree.split(PaneId(1), Direction::Vertical, 0.5, PaneId(2)).expect("split");

    let geoms_v = resolve(&tree, 80, 24);
    // Vertical split: panes are side by side.
    let g1v = geom_for(&geoms_v, PaneId(1));
    let _g2v = geom_for(&geoms_v, PaneId(2));
    assert_eq!(g1v.height, 24); // full height

    tree.rotate(PaneId(1)).expect("rotate must succeed");

    let geoms_h = resolve(&tree, 80, 24);
    // After rotate, split is now Horizontal: panes are stacked.
    let g1h = geom_for(&geoms_h, PaneId(1));
    let g2h = geom_for(&geoms_h, PaneId(2));
    assert_eq!(g1h.width, 80); // full width
    assert_eq!(g2h.width, 80); // full width
    assert!(g1h.height < 24);
}

// ---------------------------------------------------------------------------
// Zoom
// ---------------------------------------------------------------------------

#[test]
fn test_zoom_fills_window() {
    let mut tree = LayoutTree::new(PaneId(1));
    tree.split(PaneId(1), Direction::Vertical, 0.5, PaneId(2)).expect("split");

    tree.zoom(PaneId(1));
    assert!(tree.is_zoomed());

    let geoms = resolve(&tree, 80, 24);
    assert_eq!(geoms.len(), 1, "zoomed: only one geometry returned");
    let g = &geoms[0];
    assert_eq!(g.pane_id, PaneId(1));
    assert_eq!(g.x, 0);
    assert_eq!(g.y, 0);
    assert_eq!(g.width, 80);
    assert_eq!(g.height, 24);
}

#[test]
fn test_unzoom_restores() {
    let mut tree = LayoutTree::new(PaneId(1));
    tree.split(PaneId(1), Direction::Vertical, 0.5, PaneId(2)).expect("split");

    let geoms_normal = resolve(&tree, 80, 24);

    tree.zoom(PaneId(1));
    tree.unzoom();
    assert!(!tree.is_zoomed());

    let geoms_restored = resolve(&tree, 80, 24);
    // Same panes and same widths restored.
    assert_eq!(geoms_restored.len(), geoms_normal.len());
    let g1_n = geom_for(&geoms_normal, PaneId(1));
    let g1_r = geom_for(&geoms_restored, PaneId(1));
    assert_eq!(g1_n.width, g1_r.width);
    assert_eq!(g1_n.height, g1_r.height);
}

#[test]
fn test_zoom_returns_single_geometry() {
    let mut tree = LayoutTree::new(PaneId(1));
    tree.split(PaneId(1), Direction::Vertical, 0.5, PaneId(2)).expect("split");
    tree.split(PaneId(2), Direction::Horizontal, 0.5, PaneId(3)).expect("split");

    tree.zoom(PaneId(3));
    let geoms = resolve(&tree, 80, 24);
    assert_eq!(geoms.len(), 1);
    assert_eq!(geoms[0].pane_id, PaneId(3));
    assert_eq!(geoms[0].width, 80);
    assert_eq!(geoms[0].height, 24);
}

// ---------------------------------------------------------------------------
// Resolve
// ---------------------------------------------------------------------------

#[test]
fn test_resolve_single_pane() {
    let tree = LayoutTree::new(PaneId(1));
    let geoms = resolve(&tree, 80, 24);
    assert_eq!(geoms.len(), 1);
    assert_eq!(geoms[0].x, 0);
    assert_eq!(geoms[0].y, 0);
    assert_eq!(geoms[0].width, 80);
    assert_eq!(geoms[0].height, 24);
}

#[test]
fn test_resolve_two_panes_vertical() {
    let mut tree = LayoutTree::new(PaneId(1));
    tree.split(PaneId(1), Direction::Vertical, 0.5, PaneId(2)).expect("split");
    let geoms = resolve(&tree, 80, 24);
    assert_eq!(geoms.len(), 2);

    let g1 = geom_for(&geoms, PaneId(1));
    let g2 = geom_for(&geoms, PaneId(2));

    assert_eq!(g1.x, 0);
    assert_eq!(g1.y, 0);
    assert_eq!(g1.height, 24);
    assert_eq!(g2.y, 0);
    assert_eq!(g2.height, 24);
    // Border is at g1.x + g1.width; g2 starts at g1.x + g1.width + 1.
    assert_eq!(g1.x + g1.width + 1 + g2.width, 80);
}

#[test]
fn test_resolve_two_panes_horizontal() {
    let mut tree = LayoutTree::new(PaneId(1));
    tree.split(PaneId(1), Direction::Horizontal, 0.5, PaneId(2)).expect("split");
    let geoms = resolve(&tree, 80, 24);
    assert_eq!(geoms.len(), 2);

    let g1 = geom_for(&geoms, PaneId(1));
    let g2 = geom_for(&geoms, PaneId(2));

    assert_eq!(g1.x, 0);
    assert_eq!(g1.y, 0);
    assert_eq!(g1.width, 80);
    assert_eq!(g2.x, 0);
    assert_eq!(g2.width, 80);
    assert_eq!(g1.y + g1.height + 1 + g2.height, 24);
}

#[test]
fn test_resolve_deep_tree() {
    // Build: split 1 into [1,2] vertical, then split 2 into [2,3] horizontal,
    // then split 3 into [3,4] vertical. 4 panes total.
    let mut tree = LayoutTree::new(PaneId(1));
    tree.split(PaneId(1), Direction::Vertical, 0.5, PaneId(2)).expect("split 1->2");
    tree.split(PaneId(2), Direction::Horizontal, 0.5, PaneId(3)).expect("split 2->3");
    tree.split(PaneId(3), Direction::Vertical, 0.5, PaneId(4)).expect("split 3->4");

    let geoms = resolve(&tree, 80, 24);
    assert_eq!(geoms.len(), 4);

    // All panes must be within window bounds.
    for g in &geoms {
        assert!(g.x + g.width <= 80, "pane {:?} exceeds width", g.pane_id);
        assert!(g.y + g.height <= 24, "pane {:?} exceeds height", g.pane_id);
        assert!(g.width >= MIN_PANE_COLS, "pane {:?} below min cols", g.pane_id);
        assert!(g.height >= MIN_PANE_ROWS, "pane {:?} below min rows", g.pane_id);
    }
}

#[test]
fn test_resolve_no_gaps() {
    // Two pane vertical split. Total coverage = g1.width + 1 (border) + g2.width = 80.
    let mut tree = LayoutTree::new(PaneId(1));
    tree.split(PaneId(1), Direction::Vertical, 0.5, PaneId(2)).expect("split");
    let geoms = resolve(&tree, 80, 24);
    let g1 = geom_for(&geoms, PaneId(1));
    let g2 = geom_for(&geoms, PaneId(2));
    // Border = 1 cell.
    assert_eq!(g1.width + 1 + g2.width, 80, "no gap: total must equal window width");
    assert_eq!(g1.height, 24);
    assert_eq!(g2.height, 24);
}

#[test]
fn test_resolve_borders_between_panes() {
    // Pane 2 starts exactly 1 col after pane 1 ends.
    let mut tree = LayoutTree::new(PaneId(1));
    tree.split(PaneId(1), Direction::Vertical, 0.5, PaneId(2)).expect("split");
    let geoms = resolve(&tree, 80, 24);
    let g1 = geom_for(&geoms, PaneId(1));
    let g2 = geom_for(&geoms, PaneId(2));
    assert_eq!(g2.x, g1.x + g1.width + 1, "1-cell border between panes");
}

// ---------------------------------------------------------------------------
// Navigate
// ---------------------------------------------------------------------------

#[test]
fn test_navigate_right() {
    let mut tree = LayoutTree::new(PaneId(1));
    tree.split(PaneId(1), Direction::Vertical, 0.5, PaneId(2)).expect("split");
    let geoms = resolve(&tree, 80, 24);
    let result = navigate(&tree, PaneId(1), Direction::Vertical, &geoms);
    assert_eq!(result, Some(PaneId(2)));
}

#[test]
fn test_navigate_left() {
    let mut tree = LayoutTree::new(PaneId(1));
    tree.split(PaneId(1), Direction::Vertical, 0.5, PaneId(2)).expect("split");
    let geoms = resolve(&tree, 80, 24);
    // Navigate left from pane 2.
    let result = navigate(&tree, PaneId(2), Direction::Vertical, &geoms);
    // Direction::Vertical with negative delta means left — but we use
    // a separate function parameter. Let's use the direction + from/to logic.
    // The navigate function uses direction to determine axis. We need a way
    // to go left vs right. We'll test using the geometry:
    // From pane 2 (right pane), moving "left" means looking for pane whose
    // right edge aligns with pane 2's left edge.
    // The navigate API uses Direction to determine axis, and finds the nearest
    // pane in each direction. We need to test both directions separately.
    // Since the API finds "nearest in direction" we test: from right pane,
    // navigate left → find left pane.
    // Re-read the navigate signature: navigate(tree, from, direction, geoms)
    // Direction::Vertical means navigate horizontally (left or right).
    // The function should find closest pane whose right edge ≤ our left edge.
    // For now assert it finds pane 1.
    assert_eq!(result, Some(PaneId(1)));
}

#[test]
fn test_navigate_down() {
    let mut tree = LayoutTree::new(PaneId(1));
    tree.split(PaneId(1), Direction::Horizontal, 0.5, PaneId(2)).expect("split");
    let geoms = resolve(&tree, 80, 24);
    let result = navigate(&tree, PaneId(1), Direction::Horizontal, &geoms);
    assert_eq!(result, Some(PaneId(2)));
}

#[test]
fn test_navigate_up() {
    let mut tree = LayoutTree::new(PaneId(1));
    tree.split(PaneId(1), Direction::Horizontal, 0.5, PaneId(2)).expect("split");
    let geoms = resolve(&tree, 80, 24);
    let result = navigate(&tree, PaneId(2), Direction::Horizontal, &geoms);
    assert_eq!(result, Some(PaneId(1)));
}

#[test]
fn test_navigate_no_neighbor() {
    // Single pane: no neighbor in any direction.
    let tree = LayoutTree::new(PaneId(1));
    let geoms = resolve(&tree, 80, 24);
    let r1 = navigate(&tree, PaneId(1), Direction::Vertical, &geoms);
    let r2 = navigate(&tree, PaneId(1), Direction::Horizontal, &geoms);
    assert_eq!(r1, None);
    assert_eq!(r2, None);
}

// ---------------------------------------------------------------------------
// Serialization
// ---------------------------------------------------------------------------

#[test]
fn test_serialize_single() {
    let tree = LayoutTree::new(PaneId(1));
    let s = tree.serialize();
    assert!(s.contains('1'), "serialized single pane should contain id 1: {s}");
}

#[test]
fn test_serialize_nested() {
    let mut tree = LayoutTree::new(PaneId(1));
    tree.split(PaneId(1), Direction::Vertical, 0.5, PaneId(2)).expect("split");
    let s = tree.serialize();
    let tree2 = LayoutTree::deserialize(&s).expect("must deserialize");
    let geoms1 = resolve(&tree, 80, 24);
    let geoms2 = resolve(&tree2, 80, 24);
    assert_eq!(geoms1.len(), geoms2.len());
    // Same pane ids and geometry.
    for g in &geoms1 {
        let g2 = geom_for(&geoms2, g.pane_id);
        assert_eq!(g.width, g2.width);
        assert_eq!(g.height, g2.height);
    }
}

#[test]
fn test_deserialize_invalid() {
    let result = LayoutTree::deserialize("((((invalid");
    assert!(
        matches!(result, Err(LayoutError::ParseError(_))),
        "expected ParseError, got: {result:?}"
    );
}

// ---------------------------------------------------------------------------
// Property tests
// ---------------------------------------------------------------------------

use proptest::prelude::*;

proptest! {
    #[test]
    fn prop_test_layout_invariants(ops in prop::collection::vec(
        (0u32..10u32, prop::bool::ANY, 0usize..5usize),
        0..20
    )) {
        // Start with a single pane.
        let mut tree = LayoutTree::new(PaneId(1));
        let mut next_id = 2u32;
        let mut pane_ids: Vec<PaneId> = vec![PaneId(1)];

        for (idx, is_split, target_idx) in ops {
            if pane_ids.is_empty() {
                break;
            }
            let target = pane_ids[target_idx % pane_ids.len()];
            if is_split && pane_ids.len() < 8 {
                let new_id = PaneId(next_id);
                next_id += 1;
                let dir = if idx % 2 == 0 { Direction::Vertical } else { Direction::Horizontal };
                let ratio = 0.5;
                if tree.split(target, dir, ratio, new_id).is_ok() {
                    pane_ids.push(new_id);
                }
            } else if pane_ids.len() > 1 {
                // Close the target pane.
                if tree.close(target).is_ok() {
                    pane_ids.retain(|&id| id != target);
                }
            }

            // Invariant: resolve must produce geometries for all pane ids.
            if !pane_ids.is_empty() {
                let geoms = resolve(&tree, 80, 24);
                prop_assert_eq!(geoms.len(), pane_ids.len(),
                    "geometry count must match pane count");

                // All geometries must be within window bounds.
                for g in &geoms {
                    prop_assert!(g.x + g.width <= 80,
                        "pane {:?} exceeds window width", g.pane_id);
                    prop_assert!(g.y + g.height <= 24,
                        "pane {:?} exceeds window height", g.pane_id);
                    prop_assert!(g.width >= MIN_PANE_COLS,
                        "pane {:?} below min cols", g.pane_id);
                    prop_assert!(g.height >= MIN_PANE_ROWS,
                        "pane {:?} below min rows", g.pane_id);
                }
            }
        }
    }
}
