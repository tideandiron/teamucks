//! Prefix key state machine for the input handling system.
//!
//! The [`InputStateMachine`] sits between raw terminal key events and the
//! multiplexer's command dispatcher.  It implements a three-state machine:
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────┐
//! │                        Passthrough                          │
//! │  Any key → ForwardToPane(key)                               │
//! │  Prefix key → Consumed  ──────────────────────────────────► │
//! └──────────────────────────────────────┬──────────────────────┘
//!                                        │ prefix key
//!                                        ▼
//! ┌─────────────────────────────────────────────────────────────┐
//! │                       PrefixActive                          │
//! │  Known binding → ExecuteCommand(cmd) → Passthrough          │
//! │  'r'           → ExecuteCommand(EnterResizeMode) ─────────► │
//! │  Unknown key   → Consumed            → Passthrough          │
//! │  Timeout       → ForwardToPane(prefix_key) → Passthrough    │
//! └──────────────────────────────────────┬──────────────────────┘
//!                                        │ 'r'
//!                                        ▼
//! ┌─────────────────────────────────────────────────────────────┐
//! │                        ResizeMode                           │
//! │  h/j/k/l → ExecuteCommand(Resize*)  (stays in ResizeMode)  │
//! │  '='     → ExecuteCommand(EqualizeSplits) (stays)           │
//! │  Escape  → ExecuteCommand(ExitMode) → Passthrough           │
//! │  Other   → Consumed                 (stays in ResizeMode)   │
//! └─────────────────────────────────────────────────────────────┘
//! ```
//!
//! # Examples
//!
//! ```
//! use std::time::Duration;
//! use teamucks_core::input::{
//!     command::Command,
//!     key::{Key, KeyEvent, Modifiers},
//!     prefix::{InputAction, InputStateMachine},
//! };
//!
//! let prefix = KeyEvent { key: Key::Char(' '), modifiers: Modifiers::CTRL };
//! let mut sm = InputStateMachine::new(prefix.clone(), Duration::from_secs(1));
//!
//! // Normal key forwarded through in passthrough state.
//! let key = KeyEvent { key: Key::Char('a'), modifiers: Modifiers::empty() };
//! assert_eq!(sm.process_key(&key), InputAction::ForwardToPane(key));
//!
//! // Prefix activates the prefix state.
//! assert_eq!(sm.process_key(&prefix), InputAction::Consumed);
//!
//! // Binding decoded.
//! let pipe = KeyEvent { key: Key::Char('|'), modifiers: Modifiers::empty() };
//! assert_eq!(sm.process_key(&pipe), InputAction::ExecuteCommand(Command::SplitVertical));
//! ```

use std::{collections::HashMap, time::Duration};

use super::{
    command::Command,
    key::{Key, KeyEvent, Modifiers},
};

// ── Internal state ────────────────────────────────────────────────────────────

/// The current mode of the [`InputStateMachine`].
#[derive(Debug, Clone, PartialEq, Eq)]
enum InputState {
    /// All keys forwarded directly to the active pane's PTY.
    Passthrough,
    /// The prefix key was pressed; awaiting the command key.
    PrefixActive,
    /// Pane-resize mode; hjkl resize the active pane, Escape exits.
    ResizeMode,
}

// ── Public types ──────────────────────────────────────────────────────────────

/// The decision produced by [`InputStateMachine::process_key`].
///
/// The caller is responsible for acting on the returned action:
/// - `ForwardToPane` — write the key bytes to the active pane's PTY.
/// - `ExecuteCommand` — dispatch the command to the session/window/pane model.
/// - `Consumed` — the key was absorbed by the state machine; take no further action.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InputAction {
    /// Forward this key event to the currently active pane's PTY.
    ForwardToPane(KeyEvent),
    /// Execute the decoded multiplexer command.
    ExecuteCommand(Command),
    /// The key was consumed by the state machine; no further action required.
    Consumed,
}

/// A finite-state machine that decodes raw key events into multiplexer actions.
///
/// Instantiate with a prefix key and timeout, then call [`process_key`] for
/// every incoming key event.
///
/// [`process_key`]: InputStateMachine::process_key
pub struct InputStateMachine {
    state: InputState,
    prefix_key: KeyEvent,
    /// How long the machine waits in [`InputState::PrefixActive`] before
    /// timing out and forwarding the prefix key.  Timeout enforcement is the
    /// caller's responsibility; this field is stored for query purposes.
    #[allow(dead_code)]
    timeout: Duration,
    /// Keybinding table consulted when in [`InputState::PrefixActive`].
    bindings: HashMap<KeyEvent, Command>,
    /// Keybinding table consulted when in [`InputState::ResizeMode`].
    resize_bindings: HashMap<KeyEvent, Command>,
}

// ── Construction ──────────────────────────────────────────────────────────────

impl InputStateMachine {
    /// Create a new state machine with the given prefix key and timeout.
    ///
    /// The machine starts in [`Passthrough`] state and uses the built-in
    /// vim-style default keybindings.
    ///
    /// [`Passthrough`]: InputState::Passthrough
    ///
    /// # Examples
    ///
    /// ```
    /// use std::time::Duration;
    /// use teamucks_core::input::{
    ///     key::{Key, KeyEvent, Modifiers},
    ///     prefix::InputStateMachine,
    /// };
    ///
    /// let prefix = KeyEvent { key: Key::Char(' '), modifiers: Modifiers::CTRL };
    /// let sm = InputStateMachine::new(prefix, Duration::from_secs(1));
    /// assert!(sm.is_passthrough());
    /// ```
    #[must_use]
    pub fn new(prefix_key: KeyEvent, timeout: Duration) -> Self {
        Self {
            state: InputState::Passthrough,
            prefix_key,
            timeout,
            bindings: default_bindings(),
            resize_bindings: default_resize_bindings(),
        }
    }

    // ── State queries ─────────────────────────────────────────────────────────

    /// Returns `true` when the machine is in `Passthrough` state.
    #[must_use]
    pub fn is_passthrough(&self) -> bool {
        self.state == InputState::Passthrough
    }

    /// Returns `true` when the machine is in `PrefixActive` state.
    #[must_use]
    pub fn is_prefix_active(&self) -> bool {
        self.state == InputState::PrefixActive
    }

    /// Returns `true` when the machine is in `ResizeMode` state.
    #[must_use]
    pub fn is_resize_active(&self) -> bool {
        self.state == InputState::ResizeMode
    }

    // ── Key processing ────────────────────────────────────────────────────────

    /// Process a single key event and return the appropriate [`InputAction`].
    ///
    /// This is the primary interface for the input loop.  Call this method for
    /// every raw key event received from the terminal.
    ///
    /// # State transitions
    ///
    /// See the module-level documentation for the full state diagram.
    pub fn process_key(&mut self, key: &KeyEvent) -> InputAction {
        match self.state {
            InputState::Passthrough => self.process_passthrough(key),
            InputState::PrefixActive => self.process_prefix_active(key),
            InputState::ResizeMode => self.process_resize_mode(key),
        }
    }

    /// Notify the state machine that the prefix-active timeout has elapsed.
    ///
    /// Returns the prefix key that should be forwarded to the active pane, and
    /// transitions back to `Passthrough`.
    pub fn on_prefix_timeout(&mut self) -> InputAction {
        let key = self.prefix_key.clone();
        self.state = InputState::Passthrough;
        InputAction::ForwardToPane(key)
    }

    // ── State-specific handlers ───────────────────────────────────────────────

    fn process_passthrough(&mut self, key: &KeyEvent) -> InputAction {
        if key == &self.prefix_key {
            self.state = InputState::PrefixActive;
            InputAction::Consumed
        } else {
            InputAction::ForwardToPane(key.clone())
        }
    }

    fn process_prefix_active(&mut self, key: &KeyEvent) -> InputAction {
        if let Some(cmd) = self.bindings.get(key).cloned() {
            // A recognised binding was pressed.  Transition depends on the command.
            if cmd == Command::EnterResizeMode {
                self.state = InputState::ResizeMode;
            } else {
                self.state = InputState::Passthrough;
            }
            InputAction::ExecuteCommand(cmd)
        } else {
            // Unknown binding — drop the key and return to passthrough.
            self.state = InputState::Passthrough;
            InputAction::Consumed
        }
    }

    fn process_resize_mode(&mut self, key: &KeyEvent) -> InputAction {
        if let Some(cmd) = self.resize_bindings.get(key).cloned() {
            if cmd == Command::ExitMode {
                self.state = InputState::Passthrough;
            }
            // All other resize commands keep the machine in ResizeMode.
            InputAction::ExecuteCommand(cmd)
        } else {
            // Unknown key in resize mode: consume and stay in resize mode.
            InputAction::Consumed
        }
    }
}

// ── Default keybinding tables ─────────────────────────────────────────────────

/// Build the default post-prefix keybinding table (vim-style).
///
/// Returns the standard set of post-prefix keybindings used by [`InputStateMachine`]
/// and the default [`ValidatedConfig`](crate::config::types::ValidatedConfig).
///
/// # Panics
///
/// Never panics in practice — the digit-to-char conversion is infallible for
/// values 0–9.
#[must_use]
pub fn default_bindings() -> HashMap<KeyEvent, Command> {
    let no_mod = Modifiers::empty();

    let mut map = HashMap::new();

    // ── Pane ──────────────────────────────────────────────────────────────────
    map.insert(char_key('|', no_mod), Command::SplitVertical);
    map.insert(char_key('-', no_mod), Command::SplitHorizontal);
    map.insert(char_key('x', no_mod), Command::ClosePane);
    map.insert(char_key('z', no_mod), Command::ZoomPane);
    map.insert(char_key('h', no_mod), Command::NavigateLeft);
    map.insert(char_key('j', no_mod), Command::NavigateDown);
    map.insert(char_key('k', no_mod), Command::NavigateUp);
    map.insert(char_key('l', no_mod), Command::NavigateRight);

    // ── Window ────────────────────────────────────────────────────────────────
    map.insert(char_key('c', no_mod), Command::CreateWindow);
    map.insert(char_key('n', no_mod), Command::NextWindow);
    map.insert(char_key('p', no_mod), Command::PrevWindow);
    map.insert(char_key(',', no_mod), Command::RenameWindow);
    map.insert(char_key('&', no_mod), Command::CloseWindow);

    // Digit shortcuts: '0'–'9' → GoToWindow(0)–GoToWindow(9)
    for digit in 0u8..=9 {
        let c =
            char::from_digit(u32::from(digit), 10).expect("digit 0-9 always maps to a valid char");
        map.insert(char_key(c, no_mod), Command::GoToWindow(digit));
    }

    // ── Session ───────────────────────────────────────────────────────────────
    map.insert(char_key('d', no_mod), Command::Detach);
    map.insert(char_key('s', no_mod), Command::SessionPicker);
    map.insert(char_key('$', no_mod), Command::RenameSession);

    // ── Mode transitions ──────────────────────────────────────────────────────
    map.insert(char_key('r', no_mod), Command::EnterResizeMode);
    map.insert(char_key('[', no_mod), Command::EnterCopyMode);

    map
}

/// Build the resize-mode keybinding table.
///
/// Returns the standard resize-mode keybindings used by [`InputStateMachine`]
/// and the default [`ValidatedConfig`](crate::config::types::ValidatedConfig).
#[must_use]
pub fn default_resize_bindings() -> HashMap<KeyEvent, Command> {
    let no_mod = Modifiers::empty();

    let mut map = HashMap::new();
    map.insert(char_key('h', no_mod), Command::ResizeLeft);
    map.insert(char_key('j', no_mod), Command::ResizeDown);
    map.insert(char_key('k', no_mod), Command::ResizeUp);
    map.insert(char_key('l', no_mod), Command::ResizeRight);
    map.insert(char_key('=', no_mod), Command::EqualizeSplits);
    map.insert(KeyEvent { key: Key::Escape, modifiers: no_mod }, Command::ExitMode);
    map
}

// ── Convenience constructor ───────────────────────────────────────────────────

/// Shorthand for creating a [`KeyEvent`] from a character and modifier set.
#[inline]
fn char_key(c: char, modifiers: Modifiers) -> KeyEvent {
    KeyEvent { key: Key::Char(c), modifiers }
}

// ── Unit tests ────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn ctrl_space() -> KeyEvent {
        KeyEvent { key: Key::Char(' '), modifiers: Modifiers::CTRL }
    }

    fn plain(c: char) -> KeyEvent {
        KeyEvent { key: Key::Char(c), modifiers: Modifiers::empty() }
    }

    fn default_sm() -> InputStateMachine {
        InputStateMachine::new(ctrl_space(), Duration::from_secs(1))
    }

    #[test]
    fn test_initial_state_is_passthrough() {
        let sm = default_sm();
        assert!(sm.is_passthrough());
        assert!(!sm.is_prefix_active());
        assert!(!sm.is_resize_active());
    }

    #[test]
    fn test_on_prefix_timeout_forwards_prefix_and_resets() {
        let mut sm = default_sm();
        // Activate prefix
        sm.process_key(&ctrl_space());
        assert!(sm.is_prefix_active());

        // Simulate timeout
        let action = sm.on_prefix_timeout();
        assert_eq!(action, InputAction::ForwardToPane(ctrl_space()));
        assert!(sm.is_passthrough());
    }

    #[test]
    fn test_default_bindings_cover_all_digits() {
        for n in 0u8..=9 {
            let mut sm = default_sm();
            sm.process_key(&ctrl_space());
            let c = char::from_digit(u32::from(n), 10).unwrap();
            let action = sm.process_key(&plain(c));
            assert_eq!(action, InputAction::ExecuteCommand(Command::GoToWindow(n)));
        }
    }
}
