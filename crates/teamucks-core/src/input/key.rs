//! Key event representation for the input handling system.
//!
//! Provides [`KeyEvent`], [`Key`], and [`Modifiers`] — the fundamental types used
//! throughout the multiplexer's input pipeline.
//!
//! # Examples
//!
//! ```
//! use teamucks_core::input::key::{Key, KeyEvent, Modifiers};
//!
//! // Ctrl-Space — the default prefix key
//! let prefix = KeyEvent {
//!     key: Key::Char(' '),
//!     modifiers: Modifiers::CTRL,
//! };
//! assert!(prefix.modifiers.contains(Modifiers::CTRL));
//! ```

use bitflags::bitflags;

bitflags! {
    /// Keyboard modifier flags.
    ///
    /// Multiple modifiers are combined with `|`:
    ///
    /// ```
    /// use teamucks_core::input::key::Modifiers;
    /// let ctrl_shift = Modifiers::CTRL | Modifiers::SHIFT;
    /// assert!(ctrl_shift.contains(Modifiers::CTRL));
    /// assert!(ctrl_shift.contains(Modifiers::SHIFT));
    /// assert!(!ctrl_shift.contains(Modifiers::ALT));
    /// ```
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
    pub struct Modifiers: u8 {
        /// Shift key held.
        const SHIFT = 0b0001;
        /// Alt / Meta key held.
        const ALT   = 0b0010;
        /// Control key held.
        const CTRL  = 0b0100;
        /// Super (Win / Cmd) key held.
        const SUPER = 0b1000;
    }
}

/// The logical key pressed, independent of modifiers.
///
/// # Examples
///
/// ```
/// use teamucks_core::input::key::Key;
///
/// let f1 = Key::F(1);
/// let enter = Key::Enter;
/// assert_ne!(f1, enter);
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Key {
    /// A printable Unicode character.
    Char(char),
    /// Enter / Return.
    Enter,
    /// Escape.
    Escape,
    /// Backspace.
    Backspace,
    /// Horizontal tab.
    Tab,
    /// Arrow up.
    Up,
    /// Arrow down.
    Down,
    /// Arrow left.
    Left,
    /// Arrow right.
    Right,
    /// Home key.
    Home,
    /// End key.
    End,
    /// Page Up.
    PageUp,
    /// Page Down.
    PageDown,
    /// Delete (forward delete).
    Delete,
    /// Insert.
    Insert,
    /// Function key F1–F12.
    F(u8),
}

/// A complete key event: a logical key plus any active modifier keys.
///
/// `KeyEvent` implements `Eq` and `Hash` so it can be used as a `HashMap` key
/// for keybinding tables.
///
/// # Examples
///
/// ```
/// use teamucks_core::input::key::{Key, KeyEvent, Modifiers};
/// use std::collections::HashMap;
///
/// let mut map: HashMap<KeyEvent, &str> = HashMap::new();
/// map.insert(
///     KeyEvent { key: Key::Char('q'), modifiers: Modifiers::CTRL },
///     "quit",
/// );
/// assert_eq!(
///     map[&KeyEvent { key: Key::Char('q'), modifiers: Modifiers::CTRL }],
///     "quit"
/// );
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct KeyEvent {
    /// The logical key that was pressed.
    pub key: Key,
    /// Active modifier keys at the time of the event.
    pub modifiers: Modifiers,
}
