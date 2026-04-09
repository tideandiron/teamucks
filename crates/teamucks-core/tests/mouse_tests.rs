/// Integration tests for Feature 22: Mouse Support
///
/// Covers:
/// 1. SGR mouse event parsing
/// 2. Hit testing against layout geometries
/// 3. Mouse action dispatch
use teamucks_core::{
    input::{
        key::Modifiers,
        mouse::{
            dispatch_mouse, hit_test, parse_sgr_mouse, BorderHit, MouseAction, MouseButton,
            MouseEvent, MouseEventKind, MouseState, MouseTarget,
        },
    },
    layout::{resolve::PaneGeometry, Direction},
    pane::PaneId,
};

// ── Helpers ───────────────────────────────────────────────────────────────────

/// Build a SGR mouse escape sequence.
fn sgr(button: u16, col: u16, row: u16, release: bool) -> Vec<u8> {
    let terminator = if release { b'm' } else { b'M' };
    format!("\x1b[<{button};{col};{row}{}", terminator as char).into_bytes()
}

/// A simple two-pane left/right geometry at 80×24.
///
/// Pane 1: cols 0-38 (width 39), rows 0-22 (height 23)
/// Border col: 39
/// Pane 2: cols 40-79 (width 40), rows 0-22 (height 23)
/// Status row: 23
fn two_pane_geoms() -> Vec<PaneGeometry> {
    vec![
        PaneGeometry { pane_id: PaneId(1), x: 0, y: 0, width: 39, height: 23 },
        PaneGeometry { pane_id: PaneId(2), x: 40, y: 0, width: 40, height: 23 },
    ]
}

// ── SGR mouse parsing ─────────────────────────────────────────────────────────

#[test]
fn test_parse_sgr_left_press() {
    // \x1b[<0;10;5M → Press(Left), col=9, row=4 (0-indexed from 1-indexed)
    let data = sgr(0, 10, 5, false);
    let ev = parse_sgr_mouse(&data).expect("must parse");
    assert_eq!(ev.kind, MouseEventKind::Press(MouseButton::Left));
    assert_eq!(ev.col, 9);
    assert_eq!(ev.row, 4);
    assert_eq!(ev.modifiers, Modifiers::empty());
}

#[test]
fn test_parse_sgr_right_press() {
    // \x1b[<2;10;5M → Press(Right)
    let data = sgr(2, 10, 5, false);
    let ev = parse_sgr_mouse(&data).expect("must parse");
    assert_eq!(ev.kind, MouseEventKind::Press(MouseButton::Right));
}

#[test]
fn test_parse_sgr_middle_press() {
    // \x1b[<1;10;5M → Press(Middle)
    let data = sgr(1, 10, 5, false);
    let ev = parse_sgr_mouse(&data).expect("must parse");
    assert_eq!(ev.kind, MouseEventKind::Press(MouseButton::Middle));
}

#[test]
fn test_parse_sgr_release() {
    // \x1b[<0;10;5m → Release(Left) (lowercase m)
    let data = sgr(0, 10, 5, true);
    let ev = parse_sgr_mouse(&data).expect("must parse");
    assert_eq!(ev.kind, MouseEventKind::Release(MouseButton::Left));
    assert_eq!(ev.col, 9);
    assert_eq!(ev.row, 4);
}

#[test]
fn test_parse_sgr_scroll_up() {
    // \x1b[<64;10;5M → ScrollUp
    let data = sgr(64, 10, 5, false);
    let ev = parse_sgr_mouse(&data).expect("must parse");
    assert_eq!(ev.kind, MouseEventKind::ScrollUp);
}

#[test]
fn test_parse_sgr_scroll_down() {
    // \x1b[<65;10;5M → ScrollDown
    let data = sgr(65, 10, 5, false);
    let ev = parse_sgr_mouse(&data).expect("must parse");
    assert_eq!(ev.kind, MouseEventKind::ScrollDown);
}

#[test]
fn test_parse_sgr_motion() {
    // \x1b[<32;10;5M → Move (32 = 0+32, left button held during motion)
    let data = sgr(32, 10, 5, false);
    let ev = parse_sgr_mouse(&data).expect("must parse");
    assert_eq!(ev.kind, MouseEventKind::Move);
}

#[test]
fn test_parse_sgr_motion_right_held() {
    // \x1b[<34;10;5M → Move (32+2 = right button held)
    let data = sgr(34, 10, 5, false);
    let ev = parse_sgr_mouse(&data).expect("must parse");
    assert_eq!(ev.kind, MouseEventKind::Move);
}

#[test]
fn test_parse_sgr_motion_no_button() {
    // \x1b[<35;10;5M → Move (32+3 = no button held, pure motion)
    let data = sgr(35, 10, 5, false);
    let ev = parse_sgr_mouse(&data).expect("must parse");
    assert_eq!(ev.kind, MouseEventKind::Move);
}

#[test]
fn test_parse_sgr_with_modifiers_shift() {
    // Shift modifier: button bits 0b0000_0100 = 4 → Shift
    // raw button = 0 (left) | 4 (shift) = 4
    let data = sgr(4, 10, 5, false);
    let ev = parse_sgr_mouse(&data).expect("must parse");
    assert!(ev.modifiers.contains(Modifiers::SHIFT), "expected Shift modifier");
    assert_eq!(ev.kind, MouseEventKind::Press(MouseButton::Left));
}

#[test]
fn test_parse_sgr_with_modifiers_alt() {
    // Alt modifier: bit 3 (value 8) → ALT
    // raw button = 0 (left) | 8 (alt) = 8
    let data = sgr(8, 10, 5, false);
    let ev = parse_sgr_mouse(&data).expect("must parse");
    assert!(ev.modifiers.contains(Modifiers::ALT), "expected Alt modifier");
}

#[test]
fn test_parse_sgr_with_modifiers_ctrl() {
    // Ctrl modifier: bit 4 (value 16) → CTRL
    // raw button = 0 (left) | 16 (ctrl) = 16
    let data = sgr(16, 10, 5, false);
    let ev = parse_sgr_mouse(&data).expect("must parse");
    assert!(ev.modifiers.contains(Modifiers::CTRL), "expected Ctrl modifier");
}

#[test]
fn test_parse_sgr_col_row_conversion() {
    // SGR uses 1-based; output must be 0-based.
    let data = sgr(0, 1, 1, false);
    let ev = parse_sgr_mouse(&data).expect("must parse");
    assert_eq!(ev.col, 0);
    assert_eq!(ev.row, 0);
}

#[test]
fn test_parse_sgr_invalid_empty() {
    assert!(parse_sgr_mouse(&[]).is_none());
}

#[test]
fn test_parse_sgr_invalid_no_escape() {
    assert!(parse_sgr_mouse(b"no escape here").is_none());
}

#[test]
fn test_parse_sgr_invalid_truncated() {
    // Missing terminator M or m
    assert!(parse_sgr_mouse(b"\x1b[<0;10;5").is_none());
}

#[test]
fn test_parse_sgr_invalid_bad_numbers() {
    // Non-numeric in button position
    assert!(parse_sgr_mouse(b"\x1b[<abc;10;5M").is_none());
}

#[test]
fn test_parse_sgr_invalid_missing_prefix() {
    // Has CSI but missing '<'
    assert!(parse_sgr_mouse(b"\x1b[0;10;5M").is_none());
}

// ── Hit testing ───────────────────────────────────────────────────────────────

#[test]
fn test_hit_pane_content_left_pane() {
    let geoms = two_pane_geoms();
    // Click in left pane (col=5, row=10 — well inside pane 1)
    let target = hit_test(5, 10, &geoms, 24);
    assert_eq!(target, MouseTarget::Pane(PaneId(1)));
}

#[test]
fn test_hit_pane_content_right_pane() {
    let geoms = two_pane_geoms();
    // Click in right pane (col=50, row=10)
    let target = hit_test(50, 10, &geoms, 24);
    assert_eq!(target, MouseTarget::Pane(PaneId(2)));
}

#[test]
fn test_hit_pane_top_left_corner() {
    let geoms = two_pane_geoms();
    let target = hit_test(0, 0, &geoms, 24);
    assert_eq!(target, MouseTarget::Pane(PaneId(1)));
}

#[test]
fn test_hit_pane_bottom_right_of_left_pane() {
    let geoms = two_pane_geoms();
    // Right edge of left pane content: col=38, row=22
    let target = hit_test(38, 22, &geoms, 24);
    assert_eq!(target, MouseTarget::Pane(PaneId(1)));
}

#[test]
fn test_hit_border_vertical() {
    let geoms = two_pane_geoms();
    // The border is at col=39 (between pane 1 and pane 2)
    let target = hit_test(39, 10, &geoms, 24);
    assert!(
        matches!(
            target,
            MouseTarget::Border(BorderHit { split_direction: Direction::Vertical, position: 39 })
        ),
        "expected vertical border at col 39, got {target:?}"
    );
}

#[test]
fn test_hit_status_bar() {
    let geoms = two_pane_geoms();
    // Status bar is at the last row (window_height - 1 = 23)
    let target = hit_test(20, 23, &geoms, 24);
    assert_eq!(target, MouseTarget::StatusBar);
}

#[test]
fn test_hit_no_panes_returns_status_bar() {
    // Empty geometry list — no panes, clicks land as StatusBar/Ignore
    let target = hit_test(5, 5, &[], 24);
    assert_eq!(target, MouseTarget::StatusBar);
}

#[test]
fn test_hit_horizontal_border() {
    // Set up a top/bottom split:
    // Pane 1: rows 0-9, Pane 2: rows 11-22, border at row 10
    let geoms = vec![
        PaneGeometry { pane_id: PaneId(1), x: 0, y: 0, width: 80, height: 10 },
        PaneGeometry { pane_id: PaneId(2), x: 0, y: 11, width: 80, height: 12 },
    ];
    let target = hit_test(40, 10, &geoms, 24);
    assert!(
        matches!(
            target,
            MouseTarget::Border(BorderHit { split_direction: Direction::Horizontal, position: 10 })
        ),
        "expected horizontal border at row 10, got {target:?}"
    );
}

// ── Mouse action dispatch ─────────────────────────────────────────────────────

fn make_press(col: u16, row: u16) -> MouseEvent {
    MouseEvent {
        kind: MouseEventKind::Press(MouseButton::Left),
        col,
        row,
        modifiers: Modifiers::empty(),
    }
}

fn make_scroll_up(col: u16, row: u16) -> MouseEvent {
    MouseEvent { kind: MouseEventKind::ScrollUp, col, row, modifiers: Modifiers::empty() }
}

fn make_scroll_down(col: u16, row: u16) -> MouseEvent {
    MouseEvent { kind: MouseEventKind::ScrollDown, col, row, modifiers: Modifiers::empty() }
}

fn make_move(col: u16, row: u16) -> MouseEvent {
    MouseEvent { kind: MouseEventKind::Move, col, row, modifiers: Modifiers::empty() }
}

fn make_release(col: u16, row: u16) -> MouseEvent {
    MouseEvent {
        kind: MouseEventKind::Release(MouseButton::Left),
        col,
        row,
        modifiers: Modifiers::empty(),
    }
}

#[test]
fn test_mouse_click_focuses_pane() {
    // Active pane is 1; click on pane 2 → FocusPane(2)
    let geoms = two_pane_geoms();
    let mut state = MouseState::new();
    let active_pane = PaneId(1);
    let pane_has_mouse_mode = false;

    let ev = make_press(50, 10); // col=50 is in pane 2
    let action = dispatch_mouse(&ev, &geoms, 24, active_pane, pane_has_mouse_mode, &mut state);
    assert_eq!(action, MouseAction::FocusPane(PaneId(2)));
}

#[test]
fn test_mouse_click_on_active_pane_no_mouse_mode() {
    // Active pane is 1, no mouse mode → Ignore
    let geoms = two_pane_geoms();
    let mut state = MouseState::new();
    let active_pane = PaneId(1);
    let pane_has_mouse_mode = false;

    let ev = make_press(5, 10); // col=5 is in pane 1
    let action = dispatch_mouse(&ev, &geoms, 24, active_pane, pane_has_mouse_mode, &mut state);
    assert_eq!(action, MouseAction::Ignore);
}

#[test]
fn test_mouse_click_forwarded_with_mouse_mode() {
    // Active pane is 1, has mouse mode → ForwardToPane
    let geoms = two_pane_geoms();
    let mut state = MouseState::new();
    let active_pane = PaneId(1);
    let pane_has_mouse_mode = true;

    let ev = make_press(5, 10);
    let action = dispatch_mouse(&ev, &geoms, 24, active_pane, pane_has_mouse_mode, &mut state);
    assert!(
        matches!(action, MouseAction::ForwardToPane(_)),
        "expected ForwardToPane, got {action:?}"
    );
}

#[test]
fn test_mouse_scroll_up() {
    let geoms = two_pane_geoms();
    let mut state = MouseState::new();
    let active_pane = PaneId(1);

    // Scroll up on pane 1
    let ev = make_scroll_up(5, 10);
    let action = dispatch_mouse(&ev, &geoms, 24, active_pane, false, &mut state);
    assert_eq!(action, MouseAction::ScrollUp(PaneId(1)));
}

#[test]
fn test_mouse_scroll_down() {
    let geoms = two_pane_geoms();
    let mut state = MouseState::new();
    let active_pane = PaneId(1);

    let ev = make_scroll_down(5, 10);
    let action = dispatch_mouse(&ev, &geoms, 24, active_pane, false, &mut state);
    assert_eq!(action, MouseAction::ScrollDown(PaneId(1)));
}

#[test]
fn test_mouse_scroll_on_inactive_pane() {
    let geoms = two_pane_geoms();
    let mut state = MouseState::new();
    let active_pane = PaneId(1);

    // Scroll up on pane 2 (inactive)
    let ev = make_scroll_up(50, 10);
    let action = dispatch_mouse(&ev, &geoms, 24, active_pane, false, &mut state);
    assert_eq!(action, MouseAction::ScrollUp(PaneId(2)));
}

#[test]
fn test_mouse_border_drag_start() {
    let geoms = two_pane_geoms();
    let mut state = MouseState::new();
    let active_pane = PaneId(1);

    // Press on the border at col=39
    let ev = make_press(39, 10);
    let action = dispatch_mouse(&ev, &geoms, 24, active_pane, false, &mut state);
    assert!(
        matches!(action, MouseAction::StartBorderDrag(_)),
        "expected StartBorderDrag, got {action:?}"
    );
    assert!(state.is_dragging(), "state must be dragging after StartBorderDrag");
}

#[test]
fn test_mouse_border_always_multiplexer_even_with_mouse_mode() {
    // Even when the active pane has mouse mode, border clicks are ours
    let geoms = two_pane_geoms();
    let mut state = MouseState::new();
    let active_pane = PaneId(1);
    let pane_has_mouse_mode = true;

    let ev = make_press(39, 10); // border col
    let action = dispatch_mouse(&ev, &geoms, 24, active_pane, pane_has_mouse_mode, &mut state);
    assert!(
        matches!(action, MouseAction::StartBorderDrag(_)),
        "border click must not be forwarded even with mouse mode; got {action:?}"
    );
}

#[test]
fn test_mouse_border_drag_continue() {
    let geoms = two_pane_geoms();
    let mut state = MouseState::new();
    let active_pane = PaneId(1);

    // Start drag
    let press = make_press(39, 10);
    dispatch_mouse(&press, &geoms, 24, active_pane, false, &mut state);
    assert!(state.is_dragging());

    // Move during drag
    let mv = make_move(42, 10);
    let action = dispatch_mouse(&mv, &geoms, 24, active_pane, false, &mut state);
    assert!(
        matches!(action, MouseAction::ContinueBorderDrag { col: 42, row: 10 }),
        "expected ContinueBorderDrag, got {action:?}"
    );
}

#[test]
fn test_mouse_border_drag_end() {
    let geoms = two_pane_geoms();
    let mut state = MouseState::new();
    let active_pane = PaneId(1);

    // Start drag
    let press = make_press(39, 10);
    dispatch_mouse(&press, &geoms, 24, active_pane, false, &mut state);

    // Release ends drag
    let release = make_release(42, 10);
    let action = dispatch_mouse(&release, &geoms, 24, active_pane, false, &mut state);
    assert_eq!(action, MouseAction::EndBorderDrag);
    assert!(!state.is_dragging(), "state must not be dragging after EndBorderDrag");
}

#[test]
fn test_mouse_status_bar_ignored() {
    let geoms = two_pane_geoms();
    let mut state = MouseState::new();
    let active_pane = PaneId(1);

    // Click on the status bar row (row=23 with window_height=24)
    let ev = make_press(20, 23);
    let action = dispatch_mouse(&ev, &geoms, 24, active_pane, false, &mut state);
    assert_eq!(action, MouseAction::Ignore);
}

#[test]
fn test_mouse_release_without_drag_ignored() {
    // A release without a prior drag should be Ignore (no drag to end)
    let geoms = two_pane_geoms();
    let mut state = MouseState::new();
    let active_pane = PaneId(1);

    let release = make_release(5, 10);
    let action = dispatch_mouse(&release, &geoms, 24, active_pane, false, &mut state);
    assert_eq!(action, MouseAction::Ignore);
}

#[test]
fn test_mouse_move_without_drag_ignored() {
    // A move without an active drag should be Ignore
    let geoms = two_pane_geoms();
    let mut state = MouseState::new();
    let active_pane = PaneId(1);

    let mv = make_move(5, 10);
    let action = dispatch_mouse(&mv, &geoms, 24, active_pane, false, &mut state);
    assert_eq!(action, MouseAction::Ignore);
}
