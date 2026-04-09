//! Mouse event parsing, hit-testing, and action dispatch.
//!
//! This module connects raw terminal mouse events (in SGR format) to the
//! multiplexer's layout and pane model.
//!
//! # Pipeline
//!
//! ```text
//! Raw bytes ──► parse_sgr_mouse ──► MouseEvent
//!                                        │
//!                                   hit_test (layout)
//!                                        │
//!                                   MouseTarget
//!                                        │
//!                                  dispatch_mouse
//!                                        │
//!                                   MouseAction
//! ```

use crate::{
    input::key::Modifiers,
    layout::{resolve::PaneGeometry, Direction},
    pane::PaneId,
};

// ---------------------------------------------------------------------------
// MouseButton
// ---------------------------------------------------------------------------

/// A mouse button identifier.
///
/// # Examples
///
/// ```
/// use teamucks_core::input::mouse::MouseButton;
/// assert_ne!(MouseButton::Left, MouseButton::Right);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MouseButton {
    /// The primary (left) mouse button.
    Left,
    /// The middle mouse button (scroll wheel click).
    Middle,
    /// The secondary (right) mouse button.
    Right,
}

// ---------------------------------------------------------------------------
// MouseEventKind
// ---------------------------------------------------------------------------

/// The kind of mouse event.
///
/// # Examples
///
/// ```
/// use teamucks_core::input::mouse::{MouseButton, MouseEventKind};
/// let kind = MouseEventKind::Press(MouseButton::Left);
/// assert!(matches!(kind, MouseEventKind::Press(_)));
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MouseEventKind {
    /// A button was pressed.
    Press(MouseButton),
    /// A button was released.
    Release(MouseButton),
    /// The pointer moved (with or without a button held).
    Move,
    /// The scroll wheel moved up.
    ScrollUp,
    /// The scroll wheel moved down.
    ScrollDown,
}

// ---------------------------------------------------------------------------
// MouseEvent
// ---------------------------------------------------------------------------

/// A parsed mouse event with position and modifier state.
///
/// Coordinates are 0-based terminal cell indices.
///
/// # Examples
///
/// ```
/// use teamucks_core::input::{
///     key::Modifiers,
///     mouse::{MouseButton, MouseEvent, MouseEventKind},
/// };
///
/// let ev = MouseEvent {
///     kind: MouseEventKind::Press(MouseButton::Left),
///     col: 0,
///     row: 0,
///     modifiers: Modifiers::empty(),
/// };
/// assert_eq!(ev.col, 0);
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MouseEvent {
    /// What happened.
    pub kind: MouseEventKind,
    /// Column index (0-based).
    pub col: u16,
    /// Row index (0-based).
    pub row: u16,
    /// Active modifier keys at event time.
    pub modifiers: Modifiers,
}

// ---------------------------------------------------------------------------
// SGR mouse parsing
// ---------------------------------------------------------------------------

/// Parse a SGR mouse escape sequence from raw bytes.
///
/// The SGR format is: `ESC [ < button ; col ; row M` (press) or `m` (release).
///
/// Coordinates are converted from 1-based (SGR) to 0-based.
///
/// Returns `None` if `data` is not a valid SGR mouse sequence.
///
/// # Examples
///
/// ```
/// use teamucks_core::input::mouse::{parse_sgr_mouse, MouseButton, MouseEventKind};
///
/// // \x1b[<0;10;5M — left press at (col=9, row=4)
/// let ev = parse_sgr_mouse(b"\x1b[<0;10;5M").expect("valid SGR sequence");
/// assert_eq!(ev.kind, MouseEventKind::Press(MouseButton::Left));
/// assert_eq!(ev.col, 9);
/// assert_eq!(ev.row, 4);
/// ```
#[must_use]
pub fn parse_sgr_mouse(data: &[u8]) -> Option<MouseEvent> {
    // Minimum: ESC [ < 0 ; 1 ; 1 M  = 9 bytes
    if data.len() < 9 {
        return None;
    }

    // Must start with ESC [ <
    if !data.starts_with(b"\x1b[<") {
        return None;
    }

    // Find the terminator: 'M' (press) or 'm' (release).
    let rest = &data[3..]; // skip ESC [ <
    let term_pos = rest.iter().position(|&b| b == b'M' || b == b'm')?;
    let is_release = rest[term_pos] == b'm';
    let params = std::str::from_utf8(&rest[..term_pos]).ok()?;

    // Split on ';' into exactly three parts: button, col, row.
    let mut iter = params.splitn(3, ';');
    let button_str = iter.next()?;
    let col_str = iter.next()?;
    let row_str = iter.next()?;

    let raw_button: u16 = button_str.parse().ok()?;
    let col_1based: u16 = col_str.parse().ok()?;
    let row_1based: u16 = row_str.parse().ok()?;

    // Convert to 0-based; underflow protection (SGR always ≥ 1, but guard).
    let col = col_1based.saturating_sub(1);
    let row = row_1based.saturating_sub(1);

    // Extract modifiers from button bits 2-4.
    // Bit 2 (mask 0x04) → Shift
    // Bit 3 (mask 0x08) → Alt
    // Bit 4 (mask 0x10) → Ctrl
    let mut modifiers = Modifiers::empty();
    if raw_button & 0x04 != 0 {
        modifiers |= Modifiers::SHIFT;
    }
    if raw_button & 0x08 != 0 {
        modifiers |= Modifiers::ALT;
    }
    if raw_button & 0x10 != 0 {
        modifiers |= Modifiers::CTRL;
    }

    // Strip modifier bits to get the base button code.
    let base = raw_button & !0x1C; // mask off bits 2, 3, 4

    let kind = if base >= 64 {
        // Scroll events (bit 6 set).
        match base {
            64 => MouseEventKind::ScrollUp,
            65 => MouseEventKind::ScrollDown,
            _ => return None, // unknown scroll variant
        }
    } else if base >= 32 {
        // Motion event (bit 5 set).
        MouseEventKind::Move
    } else {
        // Button event (bits 0-1 select the button).
        let button = match base & 0x03 {
            0 => MouseButton::Left,
            1 => MouseButton::Middle,
            2 => MouseButton::Right,
            _ => return None, // unreachable: mask guarantees 0-3
        };
        if is_release {
            MouseEventKind::Release(button)
        } else {
            MouseEventKind::Press(button)
        }
    };

    Some(MouseEvent { kind, col, row, modifiers })
}

// ---------------------------------------------------------------------------
// Hit testing
// ---------------------------------------------------------------------------

/// A border between two adjacent panes.
///
/// The `position` is the column (for `Vertical` splits) or row (for
/// `Horizontal` splits) of the 1-cell-wide border line.
///
/// # Examples
///
/// ```
/// use teamucks_core::input::mouse::BorderHit;
/// use teamucks_core::layout::Direction;
/// let hit = BorderHit { split_direction: Direction::Vertical, position: 39 };
/// assert_eq!(hit.position, 39);
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BorderHit {
    /// Whether the border line runs vertically (left/right split) or
    /// horizontally (top/bottom split).
    pub split_direction: Direction,
    /// Column index for `Vertical` borders; row index for `Horizontal` borders.
    pub position: u16,
}

/// The UI element under the mouse cursor.
///
/// Produced by [`hit_test`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MouseTarget {
    /// The click landed inside a pane's content area.
    Pane(PaneId),
    /// The click landed on a border between two panes.
    Border(BorderHit),
    /// The click landed on the status bar (the last terminal row).
    StatusBar,
}

/// Hit-test a (col, row) coordinate against the resolved pane geometries.
///
/// `window_height` is used to detect when the cursor is on the status bar
/// (the last terminal row, i.e. `row == window_height - 1`).
///
/// Returns [`MouseTarget::StatusBar`] when no pane or border matches.
///
/// # Examples
///
/// ```
/// use teamucks_core::input::mouse::{hit_test, MouseTarget};
/// use teamucks_core::layout::resolve::PaneGeometry;
/// use teamucks_core::pane::PaneId;
///
/// let geoms = vec![
///     PaneGeometry { pane_id: PaneId(1), x: 0, y: 0, width: 40, height: 23 },
///     PaneGeometry { pane_id: PaneId(2), x: 41, y: 0, width: 39, height: 23 },
/// ];
/// assert_eq!(hit_test(5, 5, &geoms, 24), MouseTarget::Pane(PaneId(1)));
/// assert_eq!(hit_test(20, 23, &geoms, 24), MouseTarget::StatusBar);
/// ```
#[must_use]
pub fn hit_test(
    col: u16,
    row: u16,
    geometries: &[PaneGeometry],
    window_height: u16,
) -> MouseTarget {
    // Status bar occupies the last terminal row.
    if window_height > 0 && row >= window_height - 1 {
        return MouseTarget::StatusBar;
    }

    // Check each pane's content rectangle.
    for g in geometries {
        if col >= g.x && col < g.x + g.width && row >= g.y && row < g.y + g.height {
            return MouseTarget::Pane(g.pane_id);
        }
    }

    // Not inside any pane — check if we are on a border.
    // A border exists between two panes that are adjacent.
    // For a vertical split: panes share the same y/height range and the
    // right edge of one (x + width) + 1 = left edge of the other.
    // The border cell is at col = left_pane.x + left_pane.width.
    // For a horizontal split: panes share the same x/width range and the
    // bottom edge of one (y + height) + 1 = top edge of the other.
    // The border cell is at row = top_pane.y + top_pane.height.

    for a in geometries {
        for b in geometries {
            if a.pane_id == b.pane_id {
                continue;
            }
            // Vertical border: a is to the left of b.
            // a.x + a.width == border_col, b.x == border_col + 1
            let a_right_border = a.x + a.width; // border column
            if a_right_border == b.x.saturating_sub(1) || (b.x > 0 && a_right_border == b.x - 1) {
                // Overlapping row ranges?
                let a_row_end = a.y + a.height;
                let b_row_end = b.y + b.height;
                let overlap_start = a.y.max(b.y);
                let overlap_end = a_row_end.min(b_row_end);
                if row >= overlap_start && row < overlap_end && col == a_right_border {
                    return MouseTarget::Border(BorderHit {
                        split_direction: Direction::Vertical,
                        position: col,
                    });
                }
            }

            // Horizontal border: a is above b.
            let a_bottom_border = a.y + a.height; // border row
            if b.y > 0 && a_bottom_border == b.y - 1 {
                // Overlapping col ranges?
                let a_col_end = a.x + a.width;
                let b_col_end = b.x + b.width;
                let overlap_start = a.x.max(b.x);
                let overlap_end = a_col_end.min(b_col_end);
                if col >= overlap_start && col < overlap_end && row == a_bottom_border {
                    return MouseTarget::Border(BorderHit {
                        split_direction: Direction::Horizontal,
                        position: row,
                    });
                }
            }
        }
    }

    MouseTarget::StatusBar
}

// ---------------------------------------------------------------------------
// Mouse action dispatch
// ---------------------------------------------------------------------------

/// The multiplexer-level action to take in response to a mouse event.
///
/// Produced by [`dispatch_mouse`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MouseAction {
    /// Focus a different pane (the one clicked).
    FocusPane(PaneId),
    /// Forward the mouse event to the active pane's PTY (mouse mode active).
    ForwardToPane(MouseEvent),
    /// Begin dragging a pane border (resize).
    StartBorderDrag(BorderHit),
    /// Continue a border drag (mouse moved during drag).
    ContinueBorderDrag {
        /// New column position.
        col: u16,
        /// New row position.
        row: u16,
    },
    /// End a border drag (button released).
    EndBorderDrag,
    /// Scroll the pane's scrollback buffer up.
    ScrollUp(PaneId),
    /// Scroll the pane's scrollback buffer down.
    ScrollDown(PaneId),
    /// Take no action (status bar click, unhandled release, etc.).
    Ignore,
}

// ---------------------------------------------------------------------------
// MouseState
// ---------------------------------------------------------------------------

/// Per-client mouse interaction state.
///
/// Tracks whether a border drag is in progress.
///
/// # Examples
///
/// ```
/// use teamucks_core::input::mouse::MouseState;
///
/// let mut state = MouseState::new();
/// assert!(!state.is_dragging());
/// ```
#[derive(Debug, Default)]
pub struct MouseState {
    dragging: Option<BorderHit>,
}

impl MouseState {
    /// Create a new, idle mouse state.
    #[must_use]
    pub fn new() -> Self {
        Self { dragging: None }
    }

    /// Return `true` if a border drag is in progress.
    #[must_use]
    pub fn is_dragging(&self) -> bool {
        self.dragging.is_some()
    }
}

// ---------------------------------------------------------------------------
// Dispatch
// ---------------------------------------------------------------------------

/// Dispatch a [`MouseEvent`] to the appropriate [`MouseAction`].
///
/// Decision rules (in priority order):
///
/// 1. If a border drag is in progress:
///    - Move → [`MouseAction::ContinueBorderDrag`]
///    - Release → [`MouseAction::EndBorderDrag`]
///    - Other events → [`MouseAction::Ignore`] (shouldn't happen normally)
/// 2. Hit-test the event coordinate:
///    - [`MouseTarget::StatusBar`] → [`MouseAction::Ignore`]
///    - [`MouseTarget::Border`] → [`MouseAction::StartBorderDrag`] (overrides mouse mode)
///    - [`MouseTarget::Pane`]:
///      - Scroll events → [`MouseAction::ScrollUp`] / [`MouseAction::ScrollDown`]
///      - Click on *inactive* pane → [`MouseAction::FocusPane`]
///      - Click on *active* pane + mouse mode → [`MouseAction::ForwardToPane`]
///      - Click on *active* pane + no mouse mode → [`MouseAction::Ignore`]
///
/// # Examples
///
/// ```
/// use teamucks_core::input::{
///     key::Modifiers,
///     mouse::{
///         dispatch_mouse, MouseAction, MouseButton, MouseEvent, MouseEventKind, MouseState,
///     },
/// };
/// use teamucks_core::layout::resolve::PaneGeometry;
/// use teamucks_core::pane::PaneId;
///
/// let geoms = vec![PaneGeometry { pane_id: PaneId(1), x: 0, y: 0, width: 80, height: 23 }];
/// let mut state = MouseState::new();
/// let ev = MouseEvent {
///     kind: MouseEventKind::ScrollUp,
///     col: 5,
///     row: 5,
///     modifiers: Modifiers::empty(),
/// };
/// let action = dispatch_mouse(&ev, &geoms, 24, PaneId(1), false, &mut state);
/// assert_eq!(action, MouseAction::ScrollUp(PaneId(1)));
/// ```
pub fn dispatch_mouse(
    event: &MouseEvent,
    geometries: &[PaneGeometry],
    window_height: u16,
    active_pane: PaneId,
    pane_has_mouse_mode: bool,
    state: &mut MouseState,
) -> MouseAction {
    // ── Drag in progress ────────────────────────────────────────────────────
    if state.is_dragging() {
        return match &event.kind {
            MouseEventKind::Move => {
                MouseAction::ContinueBorderDrag { col: event.col, row: event.row }
            }
            MouseEventKind::Release(_) => {
                state.dragging = None;
                MouseAction::EndBorderDrag
            }
            // Press or scroll during a drag: ignore (shouldn't happen).
            _ => MouseAction::Ignore,
        };
    }

    // ── Release without drag ─────────────────────────────────────────────────
    if matches!(event.kind, MouseEventKind::Release(_)) {
        return MouseAction::Ignore;
    }

    // ── Move without drag ────────────────────────────────────────────────────
    if matches!(event.kind, MouseEventKind::Move) {
        return MouseAction::Ignore;
    }

    // ── Hit test ────────────────────────────────────────────────────────────
    let target = hit_test(event.col, event.row, geometries, window_height);

    match target {
        MouseTarget::StatusBar => MouseAction::Ignore,

        MouseTarget::Border(hit) => {
            // Border drags are always handled by the multiplexer, regardless
            // of mouse mode.
            if matches!(event.kind, MouseEventKind::Press(_)) {
                state.dragging = Some(hit.clone());
                MouseAction::StartBorderDrag(hit)
            } else {
                // Scroll on a border: ignore.
                MouseAction::Ignore
            }
        }

        MouseTarget::Pane(pane_id) => match &event.kind {
            MouseEventKind::ScrollUp => MouseAction::ScrollUp(pane_id),
            MouseEventKind::ScrollDown => MouseAction::ScrollDown(pane_id),
            MouseEventKind::Press(_) => {
                if pane_id != active_pane {
                    MouseAction::FocusPane(pane_id)
                } else if pane_has_mouse_mode {
                    MouseAction::ForwardToPane(event.clone())
                } else {
                    MouseAction::Ignore
                }
            }
            // Move/Release already handled above; this arm is unreachable.
            _ => MouseAction::Ignore,
        },
    }
}
