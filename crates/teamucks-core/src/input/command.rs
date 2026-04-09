//! Multiplexer commands produced by the input state machine.
//!
//! A [`Command`] represents a high-level user intent — split a pane, navigate,
//! detach — decoded from a keybinding by [`crate::input::prefix::InputStateMachine`].
//!
//! The command enum deliberately uses closed variants so the compiler can verify
//! exhaustive handling throughout the dispatch layer.
//!
//! # Examples
//!
//! ```
//! use teamucks_core::input::command::Command;
//!
//! let cmd = Command::GoToWindow(3);
//! assert_eq!(cmd, Command::GoToWindow(3));
//! assert_ne!(cmd, Command::GoToWindow(0));
//! ```

/// A high-level multiplexer command, decoded from a keybinding sequence.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    // ── Pane management ──────────────────────────────────────────────────────
    /// Open a new pane by splitting the active pane along the vertical axis
    /// (side by side).
    SplitVertical,
    /// Open a new pane by splitting the active pane along the horizontal axis
    /// (top and bottom).
    SplitHorizontal,
    /// Close the currently active pane.
    ClosePane,
    /// Toggle full-screen zoom on the currently active pane.
    ZoomPane,
    /// Move focus to the pane to the left of the active pane.
    NavigateLeft,
    /// Move focus to the pane below the active pane.
    NavigateDown,
    /// Move focus to the pane above the active pane.
    NavigateUp,
    /// Move focus to the pane to the right of the active pane.
    NavigateRight,

    // ── Window management ─────────────────────────────────────────────────────
    /// Create a new window in the current session.
    CreateWindow,
    /// Switch to the next window (wraps at the end).
    NextWindow,
    /// Switch to the previous window (wraps at the start).
    PrevWindow,
    /// Switch to the window at a specific index (0–9).
    GoToWindow(u8),
    /// Enter rename mode for the current window.
    RenameWindow,
    /// Close the current window.
    CloseWindow,

    // ── Session management ────────────────────────────────────────────────────
    /// Detach from the current session, leaving it running in the background.
    Detach,
    /// Open the interactive session picker overlay.
    SessionPicker,
    /// Enter rename mode for the current session.
    RenameSession,

    // ── Mode transitions ──────────────────────────────────────────────────────
    /// Enter resize mode, allowing pane borders to be moved with hjkl.
    EnterResizeMode,
    /// Enter copy mode for scrollback selection and search.
    EnterCopyMode,

    // ── Resize mode actions ───────────────────────────────────────────────────
    /// In resize mode: grow the active pane leftward.
    ResizeLeft,
    /// In resize mode: grow the active pane downward.
    ResizeDown,
    /// In resize mode: grow the active pane upward.
    ResizeUp,
    /// In resize mode: grow the active pane rightward.
    ResizeRight,
    /// In resize mode: make all panes equal size.
    EqualizeSplits,

    // ── Mode exit ─────────────────────────────────────────────────────────────
    /// Exit the current non-passthrough mode (resize, copy, …).
    ExitMode,
}
