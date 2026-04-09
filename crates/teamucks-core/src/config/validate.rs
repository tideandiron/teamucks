//! Validation of raw TOML-parsed configuration into [`ValidatedConfig`].
//!
//! [`validate`] collects **all** errors in one pass — it does not abort on the
//! first failure.  This means users see every problem in their config file at
//! once rather than iterating fix-restart-fix.
//!
//! # Examples
//!
//! ```
//! use teamucks_core::config::{validate::validate, RawConfig};
//!
//! // Empty raw config produces a valid default configuration.
//! let cfg = validate(RawConfig::default()).expect("empty config is always valid");
//! assert_eq!(cfg.scrollback_lines, 10_000);
//! ```

use std::{collections::HashMap, path::PathBuf};

use crate::{
    input::{
        command::Command,
        key::{Key, KeyEvent, Modifiers},
    },
    pane::ExitBehavior,
};

use super::{
    types::{StatusBarConfig, StatusBarPosition, Theme, ValidatedConfig},
    RawConfig, RawStatusBar, RawTheme,
};

// ── Error type ────────────────────────────────────────────────────────────────

/// A single configuration error found during validation.
///
/// Multiple errors may be produced by [`validate`] in a single call.
///
/// # Examples
///
/// ```
/// use teamucks_core::config::validate::ConfigError;
///
/// let err = ConfigError::InvalidColor { color: "bad".to_owned() };
/// assert!(err.to_string().contains("bad"));
/// ```
#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    /// The prefix key string could not be parsed as a [`KeyEvent`].
    #[error("invalid prefix key '{key}': {reason}")]
    InvalidPrefix {
        /// The raw string that was rejected.
        key: String,
        /// Human-readable reason.
        reason: String,
    },
    /// A hex colour string was not in `#rrggbb` format.
    #[error("invalid color '{color}': expected #rrggbb format")]
    InvalidColor {
        /// The raw string that was rejected.
        color: String,
    },
    /// A keybinding action string does not map to any [`Command`].
    #[error("unknown keybinding action '{action}'")]
    UnknownAction {
        /// The raw action string that was not recognised.
        action: String,
    },
    /// A key sequence string could not be parsed as a [`KeyEvent`].
    #[error("invalid key sequence '{key}': {reason}")]
    InvalidKey {
        /// The raw string that was rejected.
        key: String,
        /// Human-readable reason.
        reason: String,
    },
    /// An exit-behavior string was not one of `close`, `hold`, or `respawn`.
    #[error("unknown exit behavior '{behavior}': expected close, hold, or respawn")]
    UnknownExitBehavior {
        /// The raw string that was rejected.
        behavior: String,
    },
}

// ── Public API ────────────────────────────────────────────────────────────────

/// Validate a [`RawConfig`] into a [`ValidatedConfig`].
///
/// Validation runs in full-pass mode: all errors are collected and returned
/// together so the user can see every problem at once.
///
/// An empty [`RawConfig`] (all fields `None`) always succeeds and produces
/// [`ValidatedConfig::default`].
///
/// # Errors
///
/// Returns `Err(Vec<ConfigError>)` with at least one element when the raw
/// config contains invalid values.
///
/// # Examples
///
/// ```
/// use teamucks_core::config::{validate::validate, RawConfig};
///
/// let errors = validate(RawConfig { prefix: Some("ctrl-@@@".to_owned()), ..RawConfig::default() })
///     .expect_err("invalid prefix must fail");
/// assert!(!errors.is_empty());
/// ```
pub fn validate(raw: RawConfig) -> Result<ValidatedConfig, Vec<ConfigError>> {
    let mut errors: Vec<ConfigError> = Vec::new();

    // Start with defaults and override field by field, collecting errors.
    let defaults = ValidatedConfig::default();

    // ── prefix ────────────────────────────────────────────────────────────────
    let prefix_key = match raw.prefix {
        None => defaults.prefix_key.clone(),
        Some(ref s) => match parse_key(s) {
            Ok(k) => k,
            Err(reason) => {
                errors.push(ConfigError::InvalidPrefix { key: s.clone(), reason });
                defaults.prefix_key.clone()
            }
        },
    };

    // ── default_shell ─────────────────────────────────────────────────────────
    let default_shell = raw.default_shell.unwrap_or(defaults.default_shell);

    // ── default_cwd ───────────────────────────────────────────────────────────
    let default_cwd = raw.default_cwd.map(PathBuf::from).unwrap_or(defaults.default_cwd);

    // ── scrollback_lines ──────────────────────────────────────────────────────
    let scrollback_lines = raw.scrollback_lines.unwrap_or(defaults.scrollback_lines);

    // ── pane_exit_behavior ────────────────────────────────────────────────────
    let pane_exit_behavior = match raw.pane_exit_behavior {
        None => defaults.pane_exit_behavior,
        Some(ref s) => {
            if let Ok(b) = parse_exit_behavior(s) {
                b
            } else {
                errors.push(ConfigError::UnknownExitBehavior { behavior: s.clone() });
                defaults.pane_exit_behavior
            }
        }
    };

    // ── mouse ─────────────────────────────────────────────────────────────────
    let mouse_enabled = raw.mouse.unwrap_or(defaults.mouse_enabled);

    // ── theme ─────────────────────────────────────────────────────────────────
    let theme = validate_theme(raw.theme, defaults.theme, &mut errors);

    // ── status_bar ────────────────────────────────────────────────────────────
    let status_bar = validate_status_bar(raw.status_bar, defaults.status_bar);

    // ── keybindings ───────────────────────────────────────────────────────────
    let (keybindings, resize_bindings) = validate_keybindings(
        raw.keybindings,
        defaults.keybindings,
        defaults.resize_bindings,
        &mut errors,
    );

    if errors.is_empty() {
        Ok(ValidatedConfig {
            prefix_key,
            default_shell,
            default_cwd,
            scrollback_lines,
            pane_exit_behavior,
            mouse_enabled,
            theme,
            status_bar,
            keybindings,
            resize_bindings,
        })
    } else {
        Err(errors)
    }
}

// ── Internal helpers ──────────────────────────────────────────────────────────

fn parse_exit_behavior(s: &str) -> Result<ExitBehavior, ()> {
    match s {
        "close" => Ok(ExitBehavior::Close),
        "hold" => Ok(ExitBehavior::Hold),
        "respawn" => Ok(ExitBehavior::Respawn),
        _ => Err(()),
    }
}

/// Parse a `#rrggbb` hex string into an `(r, g, b)` triple.
pub(crate) fn parse_hex_color(s: &str) -> Result<(u8, u8, u8), ()> {
    let s = s.trim();
    if s.len() != 7 || !s.starts_with('#') {
        return Err(());
    }
    let r = u8::from_str_radix(&s[1..3], 16).map_err(|_| ())?;
    let g = u8::from_str_radix(&s[3..5], 16).map_err(|_| ())?;
    let b = u8::from_str_radix(&s[5..7], 16).map_err(|_| ())?;
    Ok((r, g, b))
}

fn validate_theme(raw: Option<RawTheme>, defaults: Theme, errors: &mut Vec<ConfigError>) -> Theme {
    let Some(raw) = raw else {
        return defaults;
    };

    macro_rules! color_field {
        ($field:ident, $default:expr) => {
            match raw.$field {
                None => $default,
                Some(ref s) => match parse_hex_color(s) {
                    Ok(c) => c,
                    Err(()) => {
                        errors.push(ConfigError::InvalidColor { color: s.clone() });
                        $default
                    }
                },
            }
        };
    }

    Theme {
        accent: color_field!(accent, defaults.accent),
        border: color_field!(border, defaults.border),
        status_bg: color_field!(status_bg, defaults.status_bg),
        status_fg: color_field!(status_fg, defaults.status_fg),
        active_window: color_field!(active_window, defaults.active_window),
        activity: color_field!(activity, defaults.activity),
    }
}

fn validate_status_bar(raw: Option<RawStatusBar>, defaults: StatusBarConfig) -> StatusBarConfig {
    let Some(raw) = raw else {
        return defaults;
    };

    let position = match raw.position.as_deref() {
        Some("top") => StatusBarPosition::Top,
        Some("bottom") | None => StatusBarPosition::Bottom,
        Some(_) => defaults.position,
    };

    StatusBarConfig {
        position,
        left: raw.left.unwrap_or(defaults.left),
        center: raw.center.unwrap_or(defaults.center),
        right: raw.right.unwrap_or(defaults.right),
    }
}

/// Map an action string to a [`Command`].
fn action_to_command(s: &str) -> Option<Command> {
    match s {
        "split_vertical" => Some(Command::SplitVertical),
        "split_horizontal" => Some(Command::SplitHorizontal),
        "close_pane" => Some(Command::ClosePane),
        "zoom_pane" => Some(Command::ZoomPane),
        "navigate_left" => Some(Command::NavigateLeft),
        "navigate_down" => Some(Command::NavigateDown),
        "navigate_up" => Some(Command::NavigateUp),
        "navigate_right" => Some(Command::NavigateRight),
        "create_window" => Some(Command::CreateWindow),
        "next_window" => Some(Command::NextWindow),
        "prev_window" => Some(Command::PrevWindow),
        "rename_window" => Some(Command::RenameWindow),
        "close_window" => Some(Command::CloseWindow),
        "detach" => Some(Command::Detach),
        "session_picker" => Some(Command::SessionPicker),
        "rename_session" => Some(Command::RenameSession),
        "enter_resize_mode" => Some(Command::EnterResizeMode),
        "enter_copy_mode" => Some(Command::EnterCopyMode),
        "resize_left" => Some(Command::ResizeLeft),
        "resize_down" => Some(Command::ResizeDown),
        "resize_up" => Some(Command::ResizeUp),
        "resize_right" => Some(Command::ResizeRight),
        "equalize_splits" => Some(Command::EqualizeSplits),
        "exit_mode" => Some(Command::ExitMode),
        _ => None,
    }
}

fn validate_keybindings(
    raw: Option<super::RawKeybindings>,
    default_kb: HashMap<KeyEvent, Command>,
    default_resize: HashMap<KeyEvent, Command>,
    errors: &mut Vec<ConfigError>,
) -> (HashMap<KeyEvent, Command>, HashMap<KeyEvent, Command>) {
    let Some(raw) = raw else {
        return (default_kb, default_resize);
    };

    // Merge user keybindings on top of defaults.
    let mut keybindings = default_kb;
    let resize_bindings = default_resize;

    for (key_str, action_str) in raw.map {
        let key = match parse_key(&key_str) {
            Ok(k) => k,
            Err(reason) => {
                errors.push(ConfigError::InvalidKey { key: key_str, reason });
                continue;
            }
        };
        let Some(cmd) = action_to_command(&action_str) else {
            errors.push(ConfigError::UnknownAction { action: action_str });
            continue;
        };
        keybindings.insert(key, cmd);
    }

    (keybindings, resize_bindings)
}

// ── parse_key ─────────────────────────────────────────────────────────────────

/// Parse a key string such as `"ctrl-a"`, `"enter"`, `"|"`, or `"h"` into a
/// [`KeyEvent`].
///
/// # Supported formats
///
/// - Single printable character: `"h"`, `"|"`, `"-"`, `"a"`, etc.
/// - `ctrl-<key>`: `"ctrl-a"`, `"ctrl-space"`, `"ctrl-["`, etc.
/// - Special names: `"enter"`, `"escape"`, `"backspace"`, `"tab"`,
///   `"up"`, `"down"`, `"left"`, `"right"`, `"home"`, `"end"`,
///   `"pageup"`, `"pagedown"`, `"delete"`, `"insert"`.
///
/// Key names are case-insensitive.
///
/// # Errors
///
/// Returns `Err(String)` with a human-readable description when the string
/// cannot be parsed.
///
/// # Examples
///
/// ```
/// use teamucks_core::config::parse_key;
/// use teamucks_core::input::key::{Key, KeyEvent, Modifiers};
///
/// let k = parse_key("ctrl-space").expect("ctrl-space must parse");
/// assert_eq!(k.key, Key::Char(' '));
/// assert!(k.modifiers.contains(Modifiers::CTRL));
///
/// let k = parse_key("enter").expect("enter must parse");
/// assert_eq!(k.key, Key::Enter);
/// ```
pub fn parse_key(s: &str) -> Result<KeyEvent, String> {
    if s.is_empty() {
        return Err("key string must not be empty".to_owned());
    }

    let lower = s.to_ascii_lowercase();

    // ctrl-<something>
    if let Some(rest) = lower.strip_prefix("ctrl-") {
        let (key, extra_mod) = parse_bare_key(rest)?;
        let modifiers = Modifiers::CTRL | extra_mod;
        return Ok(KeyEvent { key, modifiers });
    }

    // alt-<something>
    if let Some(rest) = lower.strip_prefix("alt-") {
        let (key, extra_mod) = parse_bare_key(rest)?;
        let modifiers = Modifiers::ALT | extra_mod;
        return Ok(KeyEvent { key, modifiers });
    }

    // shift-<something>
    if let Some(rest) = lower.strip_prefix("shift-") {
        let (key, extra_mod) = parse_bare_key(rest)?;
        let modifiers = Modifiers::SHIFT | extra_mod;
        return Ok(KeyEvent { key, modifiers });
    }

    // Bare key (no modifier prefix)
    let (key, modifiers) = parse_bare_key(&lower)?;
    Ok(KeyEvent { key, modifiers })
}

/// Parse a bare key name (no modifier prefix) into a `(Key, Modifiers)` pair.
///
/// The `Modifiers` component is always empty for bare keys; it is returned so
/// callers can OR in any prefix-derived modifiers.
fn parse_bare_key(s: &str) -> Result<(Key, Modifiers), String> {
    // Special names first.
    let key = match s {
        "enter" | "return" => Key::Enter,
        "escape" | "esc" => Key::Escape,
        "backspace" | "bs" => Key::Backspace,
        "tab" => Key::Tab,
        "up" => Key::Up,
        "down" => Key::Down,
        "left" => Key::Left,
        "right" => Key::Right,
        "home" => Key::Home,
        "end" => Key::End,
        "pageup" | "pgup" => Key::PageUp,
        "pagedown" | "pgdn" => Key::PageDown,
        "delete" | "del" => Key::Delete,
        "insert" | "ins" => Key::Insert,
        "space" => Key::Char(' '),
        // Function keys: f1–f12
        _ if s.starts_with('f') && s.len() <= 3 => {
            let n: u8 = s[1..].parse().map_err(|_| format!("unknown key '{s}'"))?;
            if n == 0 || n > 12 {
                return Err(format!("function key out of range: '{s}'"));
            }
            Key::F(n)
        }
        // Single printable ASCII character
        _ => {
            // We normalised to lowercase above, but the original input may have
            // been a single char — we want to preserve the original character.
            // At this point `s` is already lowercased, so single-char keys will
            // be lowercase; that is intentional.
            let mut chars = s.chars();
            let c = chars.next().ok_or_else(|| "key string is empty".to_owned())?;
            if chars.next().is_some() {
                return Err(format!("unknown key name '{s}'"));
            }
            // Only printable ASCII is accepted as a bare character key.
            if c.is_ascii() && !c.is_ascii_control() {
                Key::Char(c)
            } else {
                return Err(format!("unsupported character in key '{s}'"));
            }
        }
    };

    Ok((key, Modifiers::empty()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_hex_color_valid() {
        assert_eq!(parse_hex_color("#7aa2f7"), Ok((0x7a, 0xa2, 0xf7)));
        assert_eq!(parse_hex_color("#000000"), Ok((0, 0, 0)));
        assert_eq!(parse_hex_color("#ffffff"), Ok((255, 255, 255)));
    }

    #[test]
    fn test_parse_hex_color_invalid() {
        assert!(parse_hex_color("not-a-color").is_err());
        assert!(parse_hex_color("#gg0000").is_err());
        assert!(parse_hex_color("#fff").is_err());
        assert!(parse_hex_color("").is_err());
    }

    #[test]
    fn test_validate_empty_is_ok() {
        let result = validate(RawConfig::default());
        assert!(result.is_ok());
    }

    #[test]
    fn test_parse_exit_behavior() {
        assert_eq!(parse_exit_behavior("close"), Ok(ExitBehavior::Close));
        assert_eq!(parse_exit_behavior("hold"), Ok(ExitBehavior::Hold));
        assert_eq!(parse_exit_behavior("respawn"), Ok(ExitBehavior::Respawn));
        assert!(parse_exit_behavior("unknown").is_err());
    }

    #[test]
    fn test_action_to_command_known() {
        assert_eq!(action_to_command("split_vertical"), Some(Command::SplitVertical));
        assert_eq!(action_to_command("detach"), Some(Command::Detach));
        assert_eq!(action_to_command("navigate_left"), Some(Command::NavigateLeft));
    }

    #[test]
    fn test_action_to_command_unknown() {
        assert!(action_to_command("does_not_exist").is_none());
    }
}
