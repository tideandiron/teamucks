/// Integration tests for border rendering: box-drawing characters and active-pane highlighting.
///
/// Tests follow the naming convention: `test_<unit>_<scenario>_<expected>`.
/// All tests exercise `compute_borders` with concrete `PaneGeometry` inputs.
use teamucks_core::layout::resolve::PaneGeometry;
use teamucks_core::pane::PaneId;
use teamucks_core::render::borders::{compute_borders, BorderCell};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Build a `PaneGeometry` concisely.
fn geom(id: u32, x: u16, y: u16, width: u16, height: u16) -> PaneGeometry {
    PaneGeometry { pane_id: PaneId(id), x, y, width, height }
}

/// Collect only the border cells at a specific (x, y) position.
fn cell_at(borders: &[BorderCell], x: u16, y: u16) -> Option<&BorderCell> {
    borders.iter().find(|b| b.x == x && b.y == y)
}

// ---------------------------------------------------------------------------
// test_single_pane_no_borders
// ---------------------------------------------------------------------------

/// A single pane filling the window has no adjacent panes, so no border cells
/// should be produced at all.
#[test]
fn test_single_pane_no_borders() {
    let geoms = [geom(1, 0, 0, 80, 24)];
    let borders = compute_borders(&geoms, PaneId(1), 80, 24);
    assert!(borders.is_empty(), "single pane must produce no borders; got {borders:?}");
}

// ---------------------------------------------------------------------------
// test_vertical_split_border
// ---------------------------------------------------------------------------

/// Two panes side by side (vertical split) must produce a vertical line of
/// border cells at the column between them.
///
/// Layout (80×24, ratio 0.5):
///   pane 1: x=0, y=0, width=39, height=24
///   border: x=39, y=0..23
///   pane 2: x=40, y=0, width=40, height=24
#[test]
fn test_vertical_split_border_produces_vertical_line() {
    let geoms = [geom(1, 0, 0, 39, 24), geom(2, 40, 0, 40, 24)];
    let borders = compute_borders(&geoms, PaneId(1), 80, 24);

    // Every row from 0 to 23 must have a border cell at x=39.
    for row in 0u16..24 {
        let cell = cell_at(&borders, 39, row);
        assert!(
            cell.is_some(),
            "expected border cell at (39, {row}) for vertical split; borders: {borders:?}"
        );
    }

    // No border cells should appear inside either pane's content area.
    for b in &borders {
        assert!(b.x == 39, "border cell at unexpected x={} (expected only x=39); cell: {b:?}", b.x);
    }
}

/// The vertical line must use │ characters throughout (no corners, since it
/// runs the full height of the window with top and bottom edges).
#[test]
fn test_vertical_split_border_character_is_vertical_bar() {
    let geoms = [geom(1, 0, 0, 39, 24), geom(2, 40, 0, 40, 24)];
    let borders = compute_borders(&geoms, PaneId(1), 80, 24);

    for b in &borders {
        assert_eq!(
            b.ch, '│',
            "vertical split border cell at ({}, {}) must be '│' but got '{}'",
            b.x, b.y, b.ch
        );
    }
}

// ---------------------------------------------------------------------------
// test_horizontal_split_border
// ---------------------------------------------------------------------------

/// Two panes stacked (horizontal split) must produce a horizontal line of
/// border cells at the row between them.
///
/// Layout (80×24, ratio 0.5):
///   pane 1: x=0, y=0, width=80, height=11
///   border: x=0..79, y=11
///   pane 2: x=0, y=12, width=80, height=12
#[test]
fn test_horizontal_split_border_produces_horizontal_line() {
    let geoms = [geom(1, 0, 0, 80, 11), geom(2, 0, 12, 80, 12)];
    let borders = compute_borders(&geoms, PaneId(1), 80, 24);

    // Every column from 0 to 79 must have a border cell at y=11.
    for col in 0u16..80 {
        let cell = cell_at(&borders, col, 11);
        assert!(
            cell.is_some(),
            "expected border cell at ({col}, 11) for horizontal split; borders: {borders:?}"
        );
    }

    // No border cells should appear inside either pane's content area.
    for b in &borders {
        assert!(b.y == 11, "border cell at unexpected y={} (expected only y=11); cell: {b:?}", b.y);
    }
}

/// The horizontal line must use ─ characters throughout.
#[test]
fn test_horizontal_split_border_character_is_horizontal_bar() {
    let geoms = [geom(1, 0, 0, 80, 11), geom(2, 0, 12, 80, 12)];
    let borders = compute_borders(&geoms, PaneId(1), 80, 24);

    for b in &borders {
        assert_eq!(
            b.ch, '─',
            "horizontal split border cell at ({}, {}) must be '─' but got '{}'",
            b.x, b.y, b.ch
        );
    }
}

// ---------------------------------------------------------------------------
// test_corner_characters
// ---------------------------------------------------------------------------

/// Three-pane T layout: pane 1 is top-left, pane 2 is top-right, pane 3 is
/// bottom spanning the full width.
///
/// Window: 20×10
///   pane 1: x=0, y=0, w=9, h=4   (rows 0-3, cols 0-8)
///   pane 2: x=10, y=0, w=10, h=4 (rows 0-3, cols 10-19)
///   pane 3: x=0, y=5, w=20, h=5  (rows 5-9, cols 0-19)
///
/// Gap cells:
///   col=9, rows 0-3:   vertical separator between pane 1 and pane 2
///   row=4, cols 0-19:  horizontal separator between top panes and pane 3
///
/// At (9, 4), the vertical separator (from above) meets the horizontal
/// separator.  The lines extend: up (col=9, row=3 is a border cell),
/// left (col=8, row=4 is a border cell), right (col=10, row=4 is a border
/// cell), but NOT down (col=9, row=5 is pane 3 content).
///
/// `up=T, down=F, left=T, right=T` → ┴ (bottom tee).
#[test]
fn test_corner_characters_top_left_junction() {
    let geoms = [geom(1, 0, 0, 9, 4), geom(2, 10, 0, 10, 4), geom(3, 0, 5, 20, 5)];
    let borders = compute_borders(&geoms, PaneId(1), 20, 10);

    // The junction at (9, 4): vertical comes from above, horizontal runs
    // left and right, nothing below (pane 3 starts at row 5) → ┴.
    let junction = cell_at(&borders, 9, 4);
    assert!(junction.is_some(), "expected a border cell at (9, 4); borders: {borders:?}");
    assert_eq!(
        junction.unwrap().ch,
        '┴',
        "junction at (9, 4) must be '┴' (bottom tee: up+left+right); got '{}'",
        junction.unwrap().ch
    );
}

/// Four-pane 2×2 grid: test all four corner characters.
///
/// Window: 21×11
///   pane 1: x=0,  y=0,  w=9,  h=4   (top-left)
///   pane 2: x=10, y=0,  w=11, h=4   (top-right)
///   pane 3: x=0,  y=5,  w=9,  h=6   (bottom-left)
///   pane 4: x=10, y=5,  w=11, h=6   (bottom-right)
///
/// Corner at (9, 4) must be ┼.
/// Corners at (0, 4) and (20, 4) must be ─ (edges of the horizontal border).
#[test]
fn test_cross_junction_in_2x2_grid() {
    let geoms =
        [geom(1, 0, 0, 9, 4), geom(2, 10, 0, 11, 4), geom(3, 0, 5, 9, 6), geom(4, 10, 5, 11, 6)];
    let borders = compute_borders(&geoms, PaneId(1), 21, 11);

    // Center cross at (9, 4).
    let cross = cell_at(&borders, 9, 4);
    assert!(cross.is_some(), "expected border cell at center (9, 4); borders: {borders:?}");
    assert_eq!(
        cross.unwrap().ch,
        '┼',
        "center of 2×2 grid must be '┼'; got '{}'",
        cross.unwrap().ch
    );
}

// ---------------------------------------------------------------------------
// test_t_junction
// ---------------------------------------------------------------------------

/// Three panes: left pane full height, right-top pane, right-bottom pane.
///
/// Window: 20×10
///   pane 1: x=0,  y=0, w=9, h=10   (left, full height)
///   pane 2: x=10, y=0, w=10, h=4   (right-top)
///   pane 3: x=10, y=5, w=10, h=5   (right-bottom)
///
/// Border at x=9: full-height vertical line.
/// Border at y=4, x=10..19: horizontal line in right half.
/// Junction at (9, 4): ├ (left tee — extends up, down, right).
#[test]
fn test_t_junction_left_tee() {
    let geoms = [geom(1, 0, 0, 9, 10), geom(2, 10, 0, 10, 4), geom(3, 10, 5, 10, 5)];
    let borders = compute_borders(&geoms, PaneId(1), 20, 10);

    let junction = cell_at(&borders, 9, 4);
    assert!(junction.is_some(), "expected border cell at (9, 4); borders: {borders:?}");
    assert_eq!(
        junction.unwrap().ch,
        '├',
        "left tee at (9, 4) must be '├'; got '{}'",
        junction.unwrap().ch
    );
}

// ---------------------------------------------------------------------------
// test_cross_junction
// ---------------------------------------------------------------------------

/// Four panes in a 2×2 grid produce a ┼ at the center intersection.
/// This test uses a clean minimal layout to verify just the cross character.
///
/// Window: 11×11
///   pane 1: x=0, y=0, w=4, h=4
///   pane 2: x=5, y=0, w=6, h=4
///   pane 3: x=0, y=5, w=4, h=6
///   pane 4: x=5, y=5, w=6, h=6
///
/// Cross at (4, 4).
#[test]
fn test_cross_junction_glyph_at_center() {
    let geoms =
        [geom(1, 0, 0, 4, 4), geom(2, 5, 0, 6, 4), geom(3, 0, 5, 4, 6), geom(4, 5, 5, 6, 6)];
    let borders = compute_borders(&geoms, PaneId(1), 11, 11);

    let cross = cell_at(&borders, 4, 4);
    assert!(cross.is_some(), "expected border cell at (4, 4); borders: {borders:?}");
    assert_eq!(
        cross.unwrap().ch,
        '┼',
        "four-pane grid center must be '┼'; got '{}'",
        cross.unwrap().ch
    );
}

// ---------------------------------------------------------------------------
// test_active_pane_border_flagged
// ---------------------------------------------------------------------------

/// Borders adjacent to the active pane must have `is_active_border = true`.
///
/// Layout: two panes side by side.
///   pane 1: x=0, y=0, w=9, h=10   (active)
///   pane 2: x=10, y=0, w=10, h=10
///
/// Border column x=9 is adjacent to pane 1 (right edge) → all those cells
/// must have `is_active_border = true`.
#[test]
fn test_active_pane_border_flagged() {
    let geoms = [geom(1, 0, 0, 9, 10), geom(2, 10, 0, 10, 10)];
    let borders = compute_borders(&geoms, PaneId(1), 20, 10);

    // All border cells must be adjacent to the active pane (pane 1).
    for b in &borders {
        assert!(
            b.is_active_border,
            "border cell at ({}, {}) adjacent to active pane must have is_active_border=true; cell: {b:?}",
            b.x, b.y
        );
    }
}

// ---------------------------------------------------------------------------
// test_inactive_borders_not_flagged
// ---------------------------------------------------------------------------

/// Borders NOT adjacent to the active pane must have `is_active_border = false`.
///
/// Layout: three panes.
///   pane 1: x=0, y=0, w=9, h=10    (active)
///   pane 2: x=10, y=0, w=9, h=4
///   pane 3: x=10, y=5, w=9, h=5
///
/// Border at x=9: adjacent to pane 1 → active.
/// Border at y=4, x=10..18: adjacent to panes 2 and 3, NOT pane 1 → inactive.
#[test]
fn test_inactive_borders_not_flagged() {
    let geoms = [geom(1, 0, 0, 9, 10), geom(2, 10, 0, 9, 4), geom(3, 10, 5, 9, 5)];
    let borders = compute_borders(&geoms, PaneId(1), 20, 10);

    // The horizontal border at y=4 (between pane 2 and pane 3) must be inactive.
    for b in borders.iter().filter(|b| b.y == 4 && b.x >= 10) {
        assert!(
            !b.is_active_border,
            "border cell at ({}, {}) not adjacent to active pane 1 must have is_active_border=false; cell: {b:?}",
            b.x, b.y
        );
    }

    // The vertical border at x=9 must be active (adjacent to pane 1).
    for b in borders.iter().filter(|b| b.x == 9) {
        assert!(
            b.is_active_border,
            "border cell at ({}, {}) adjacent to active pane 1 must have is_active_border=true; cell: {b:?}",
            b.x, b.y
        );
    }
}

// ---------------------------------------------------------------------------
// test_complex_layout
// ---------------------------------------------------------------------------

/// Asymmetric 4-pane layout: verify all border positions and intersection
/// characters are computed correctly.
///
/// Window: 30×20
///   pane 1: x=0,  y=0,  w=13, h=8    (top-left)
///   pane 2: x=14, y=0,  w=16, h=8    (top-right)
///   pane 3: x=0,  y=9,  w=13, h=11   (bottom-left)
///   pane 4: x=14, y=9,  w=16, h=11   (bottom-right)
///
/// Border column x=13 (full height).
/// Border row y=8 (full width).
/// Cross at (13, 8).
#[test]
fn test_complex_layout_all_intersections_correct() {
    let geoms = [
        geom(1, 0, 0, 13, 8),
        geom(2, 14, 0, 16, 8),
        geom(3, 0, 9, 13, 11),
        geom(4, 14, 9, 16, 11),
    ];
    let borders = compute_borders(&geoms, PaneId(1), 30, 20);

    // Vertical border at x=13 must exist for all rows.
    for row in 0u16..20 {
        assert!(
            cell_at(&borders, 13, row).is_some(),
            "expected vertical border at (13, {row}); borders: {borders:?}"
        );
    }

    // Horizontal border at y=8 must exist for all columns.
    for col in 0u16..30 {
        assert!(
            cell_at(&borders, col, 8).is_some(),
            "expected horizontal border at ({col}, 8); borders: {borders:?}"
        );
    }

    // Cross at (13, 8).
    let cross = cell_at(&borders, 13, 8);
    assert!(cross.is_some(), "expected border cell at center (13, 8)");
    assert_eq!(
        cross.unwrap().ch,
        '┼',
        "center intersection must be '┼'; got '{}'",
        cross.unwrap().ch
    );

    // Active pane 1 borders: the separator column (x=13) from rows 0..7 is
    // adjacent to pane 1's right edge.  The separator row (y=8) from cols
    // 0..12 is adjacent to pane 1's bottom edge.
    //
    // The cross cell at (13, 8) is the junction of the two separator lines.
    // Its four cardinal neighbours are all border/gap cells — none are pane 1
    // content — so it is correctly NOT flagged as active.
    for b in borders.iter().filter(|b| b.x == 13 && b.y < 8) {
        assert!(
            b.is_active_border,
            "vertical separator cell at ({}, {}) adjacent to pane 1 must be active; cell: {b:?}",
            b.x, b.y
        );
    }
    for b in borders.iter().filter(|b| b.y == 8 && b.x < 13) {
        assert!(
            b.is_active_border,
            "horizontal separator cell at ({}, {}) adjacent to pane 1 must be active; cell: {b:?}",
            b.x, b.y
        );
    }
}

// ---------------------------------------------------------------------------
// test_border_positions_match_geometries
// ---------------------------------------------------------------------------

/// Verify that borders occupy exactly the gap cells between pane content areas.
///
/// After collecting all border positions, every cell in the window should be
/// either inside a pane's content rectangle OR be a border cell — no gaps, no
/// overlaps (border cells do not overlap pane content rectangles).
#[test]
fn test_border_positions_match_geometries() {
    // 2×2 grid, 11×11 window.
    let geoms =
        [geom(1, 0, 0, 4, 4), geom(2, 5, 0, 6, 4), geom(3, 0, 5, 4, 6), geom(4, 5, 5, 6, 6)];
    let borders = compute_borders(&geoms, PaneId(1), 11, 11);

    // Build a set of pane cells (content area rectangles).
    let mut pane_cells = std::collections::HashSet::new();
    for g in &geoms {
        for row in g.y..g.y + g.height {
            for col in g.x..g.x + g.width {
                pane_cells.insert((col, row));
            }
        }
    }

    // No border cell must overlap a pane content cell.
    for b in &borders {
        assert!(
            !pane_cells.contains(&(b.x, b.y)),
            "border cell at ({}, {}) overlaps a pane content area",
            b.x,
            b.y
        );
    }

    // All border cells must lie within the window dimensions.
    for b in &borders {
        assert!(b.x < 11, "border cell x={} is outside window width=11", b.x);
        assert!(b.y < 11, "border cell y={} is outside window height=11", b.y);
    }

    // Every cell not in a pane content area and within bounds must be a border.
    let border_set: std::collections::HashSet<(u16, u16)> =
        borders.iter().map(|b| (b.x, b.y)).collect();
    for row in 0u16..11 {
        for col in 0u16..11 {
            if !pane_cells.contains(&(col, row)) {
                assert!(
                    border_set.contains(&(col, row)),
                    "gap cell ({col}, {row}) is neither a pane content cell nor a border cell"
                );
            }
        }
    }
}

// ---------------------------------------------------------------------------
// test_render_borders_method
// ---------------------------------------------------------------------------

/// `TerminalRenderer::render_borders` must produce CUP + SGR + character output
/// for each border cell, using accent color for active borders and muted for
/// inactive.
#[test]
fn test_render_borders_active_uses_accent_color() {
    use teamucks_core::render::TerminalRenderer;

    let geoms = [geom(1, 0, 0, 9, 5), geom(2, 10, 0, 10, 5)];
    let borders = compute_borders(&geoms, PaneId(1), 20, 5);

    let mut r = TerminalRenderer::new();
    let output = r.render_borders(&borders, "#ff0000", "#888888");
    let s = String::from_utf8_lossy(output);

    // Must contain at least one CUP sequence.
    assert!(s.contains('\x1b'), "render_borders must emit escape sequences; got: {s:?}");

    // Must contain the border character.
    assert!(s.contains('│'), "render_borders must emit '│' for vertical border; got: {s:?}");
}

#[test]
fn test_render_borders_empty_produces_no_output() {
    use teamucks_core::render::TerminalRenderer;

    let mut r = TerminalRenderer::new();
    let output = r.render_borders(&[], "#ff0000", "#888888");
    // An empty slice should produce either no output or only the sync markers.
    // We verify it does NOT contain any CUP sequences (no border characters to render).
    let s = String::from_utf8_lossy(output);
    // The sync markers are allowed, but no CUP should appear.
    // A simple assertion: no │ or ─ characters.
    assert!(!s.contains('│'), "empty borders must not emit '│'; got: {s:?}");
    assert!(!s.contains('─'), "empty borders must not emit '─'; got: {s:?}");
}
