//! Validated, runtime configuration types.
//!
//! These types represent the canonical configuration used by the running
//! multiplexer.  They are produced by [`super::validate::validate`] and
//! carry only well-formed values — every field is guaranteed correct by the
//! time a [`ValidatedConfig`] is constructed.
//!
//! # Examples
//!
//! ```
//! use teamucks_core::config::types::ValidatedConfig;
//!
//! let cfg = ValidatedConfig::default();
//! // Default prefix is Ctrl-Space.
//! use teamucks_core::input::key::{Key, Modifiers};
//! assert_eq!(cfg.prefix_key.key, Key::Char(' '));
//! assert!(cfg.prefix_key.modifiers.contains(Modifiers::CTRL));
//! ```

use std::{collections::HashMap, path::PathBuf};

use crate::{
    input::{
        command::Command,
        key::{Key, KeyEvent, Modifiers},
        prefix::{default_bindings, default_resize_bindings},
    },
    pane::ExitBehavior,
};

// ── Theme ─────────────────────────────────────────────────────────────────────

/// Terminal colour theme.
///
/// Each colour is an RGB triple `(r, g, b)` in the range `0..=255`.
///
/// # Examples
///
/// ```
/// use teamucks_core::config::types::Theme;
///
/// let theme = Theme::default();
/// // accent is the Tokyo Night blue
/// assert_eq!(theme.accent, (0x7a, 0xa2, 0xf7));
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Theme {
    /// Accent colour — used for active pane borders and highlights.
    pub accent: (u8, u8, u8),
    /// Border colour for inactive pane borders.
    pub border: (u8, u8, u8),
    /// Status bar background.
    pub status_bg: (u8, u8, u8),
    /// Status bar foreground text.
    pub status_fg: (u8, u8, u8),
    /// Active window indicator in the status bar.
    pub active_window: (u8, u8, u8),
    /// Window activity indicator colour.
    pub activity: (u8, u8, u8),
}

impl Default for Theme {
    /// Tokyo Night-inspired defaults.
    fn default() -> Self {
        Self {
            accent: (0x7a, 0xa2, 0xf7),        // #7aa2f7
            border: (0x3b, 0x42, 0x61),        // #3b4261
            status_bg: (0x1a, 0x1b, 0x26),     // #1a1b26
            status_fg: (0xa9, 0xb1, 0xd6),     // #a9b1d6
            active_window: (0x7a, 0xa2, 0xf7), // #7aa2f7
            activity: (0xe0, 0xaf, 0x68),      // #e0af68
        }
    }
}

// ── StatusBarPosition ─────────────────────────────────────────────────────────

/// Where the status bar is rendered on-screen.
///
/// # Examples
///
/// ```
/// use teamucks_core::config::types::StatusBarPosition;
///
/// assert!(matches!(StatusBarPosition::default(), StatusBarPosition::Bottom));
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum StatusBarPosition {
    /// Status bar at the top of the screen.
    Top,
    /// Status bar at the bottom of the screen (default).
    #[default]
    Bottom,
}

// ── StatusBarConfig ───────────────────────────────────────────────────────────

/// Configuration for the status bar layout and content templates.
///
/// The `left`, `center`, and `right` fields are format strings that may contain
/// placeholder tokens such as `{session}`, `{windows}`, `{cwd}`, `{command}`,
/// and `{clock}`.
///
/// # Examples
///
/// ```
/// use teamucks_core::config::types::StatusBarConfig;
///
/// let cfg = StatusBarConfig::default();
/// assert!(cfg.right.contains("{clock}"));
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StatusBarConfig {
    /// Screen position.
    pub position: StatusBarPosition,
    /// Left segment template.
    pub left: String,
    /// Center segment template.
    pub center: String,
    /// Right segment template.
    pub right: String,
}

impl Default for StatusBarConfig {
    fn default() -> Self {
        Self {
            position: StatusBarPosition::Bottom,
            left: "{session} {windows}".to_owned(),
            center: String::new(),
            right: "{cwd} {command} {clock}".to_owned(),
        }
    }
}

// ── ValidatedConfig ───────────────────────────────────────────────────────────

/// The complete, validated runtime configuration.
///
/// Constructed from a [`RawConfig`](super::RawConfig) by
/// [`validate`](super::validate::validate).  All fields are guaranteed correct;
/// there are no `Option` wrappers here.
///
/// # Examples
///
/// ```
/// use teamucks_core::config::types::ValidatedConfig;
///
/// let cfg = ValidatedConfig::default();
/// assert_eq!(cfg.scrollback_lines, 10_000);
/// assert!(cfg.mouse_enabled);
/// ```
#[derive(Debug, Clone)]
pub struct ValidatedConfig {
    /// The prefix key that activates the multiplexer command mode.
    pub prefix_key: KeyEvent,
    /// Shell executable used when spawning new panes.
    pub default_shell: String,
    /// Default working directory for new panes.
    pub default_cwd: PathBuf,
    /// Number of scrollback lines per pane.
    pub scrollback_lines: usize,
    /// What to do when a pane's child process exits.
    pub pane_exit_behavior: ExitBehavior,
    /// Whether mouse reporting is enabled.
    pub mouse_enabled: bool,
    /// Visual colour theme.
    pub theme: Theme,
    /// Status bar display configuration.
    pub status_bar: StatusBarConfig,
    /// Post-prefix keybindings: key → command.
    pub keybindings: HashMap<KeyEvent, Command>,
    /// Resize-mode keybindings: key → command.
    pub resize_bindings: HashMap<KeyEvent, Command>,
}

impl Default for ValidatedConfig {
    fn default() -> Self {
        let default_shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".to_owned());
        let default_cwd = std::env::var("HOME").map_or_else(|_| PathBuf::from("/"), PathBuf::from);

        Self {
            prefix_key: KeyEvent { key: Key::Char(' '), modifiers: Modifiers::CTRL },
            default_shell,
            default_cwd,
            scrollback_lines: 10_000,
            pane_exit_behavior: ExitBehavior::Hold,
            mouse_enabled: true,
            theme: Theme::default(),
            status_bar: StatusBarConfig::default(),
            keybindings: default_bindings(),
            resize_bindings: default_resize_bindings(),
        }
    }
}
