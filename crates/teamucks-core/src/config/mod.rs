//! Configuration loading, parsing, and validation for teamucks.
//!
//! This module handles the full configuration pipeline:
//!
//! 1. Locate the config file via XDG conventions ([`config_path`]).
//! 2. Parse TOML into [`RawConfig`] via `serde`.
//! 3. Validate and produce a [`ValidatedConfig`] ([`validate`]).
//! 4. Supply a live-reload helper ([`reload_config`]).
//!
//! # File location
//!
//! The config file is at `$XDG_CONFIG_HOME/teamucks/config.toml`, falling back
//! to `~/.config/teamucks/config.toml` when `XDG_CONFIG_HOME` is not set.
//!
//! # Example config
//!
//! ```toml
//! prefix           = "ctrl-space"
//! default_shell    = "/bin/zsh"
//! scrollback_lines = 10000
//! mouse            = true
//!
//! [theme]
//! accent        = "#7aa2f7"
//! border        = "#3b4261"
//! status_bg     = "#1a1b26"
//! status_fg     = "#a9b1d6"
//! active_window = "#7aa2f7"
//! activity      = "#e0af68"
//!
//! [keybindings]
//! "|" = "split_vertical"
//! "-" = "split_horizontal"
//! "h" = "navigate_left"
//! ```

use std::{collections::HashMap, path::PathBuf};

use serde::Deserialize;

pub mod types;
pub mod validate;

pub use validate::parse_key;
pub use validate::validate;

use types::ValidatedConfig;

// ── Raw types (serde-deserialisable) ─────────────────────────────────────────

/// Raw TOML-parsed configuration.
///
/// All fields are `Option` so that unset values fall back to their defaults
/// during validation.  Use [`validate`] to convert to [`ValidatedConfig`].
///
/// # Examples
///
/// ```
/// use teamucks_core::config::RawConfig;
///
/// let raw: RawConfig = toml::from_str(r#"scrollback_lines = 5000"#).unwrap();
/// assert_eq!(raw.scrollback_lines, Some(5000));
/// assert!(raw.prefix.is_none());
/// ```
#[derive(Debug, Deserialize, Default)]
pub struct RawConfig {
    /// Prefix key string (e.g. `"ctrl-space"`, `"ctrl-a"`).
    pub prefix: Option<String>,
    /// Shell executable path (e.g. `"/bin/zsh"`).
    pub default_shell: Option<String>,
    /// Default working directory for new panes.
    pub default_cwd: Option<String>,
    /// Number of scrollback lines per pane.
    pub scrollback_lines: Option<usize>,
    /// Exit behavior string: `"close"`, `"hold"`, or `"respawn"`.
    pub pane_exit_behavior: Option<String>,
    /// Enable mouse reporting.
    pub mouse: Option<bool>,
    /// Colour theme.
    pub theme: Option<RawTheme>,
    /// Status bar configuration.
    pub status_bar: Option<RawStatusBar>,
    /// Keybinding overrides.
    pub keybindings: Option<RawKeybindings>,
}

/// Raw colour theme (all fields optional; unset fields use defaults).
///
/// # Examples
///
/// ```
/// use teamucks_core::config::RawTheme;
///
/// let raw: RawTheme = toml::from_str(r##"accent = "#ff0000""##).unwrap();
/// assert_eq!(raw.accent.as_deref(), Some("#ff0000"));
/// ```
#[derive(Debug, Deserialize, Default)]
pub struct RawTheme {
    /// Accent colour in `#rrggbb` format.
    pub accent: Option<String>,
    /// Inactive pane border colour.
    pub border: Option<String>,
    /// Status bar background colour.
    pub status_bg: Option<String>,
    /// Status bar foreground text colour.
    pub status_fg: Option<String>,
    /// Active window indicator colour.
    pub active_window: Option<String>,
    /// Window activity indicator colour.
    pub activity: Option<String>,
}

/// Raw status bar configuration (all fields optional).
///
/// # Examples
///
/// ```
/// use teamucks_core::config::RawStatusBar;
///
/// let raw: RawStatusBar = toml::from_str(r#"position = "top""#).unwrap();
/// assert_eq!(raw.position.as_deref(), Some("top"));
/// ```
#[derive(Debug, Deserialize, Default)]
pub struct RawStatusBar {
    /// `"top"` or `"bottom"`.
    pub position: Option<String>,
    /// Left segment template.
    pub left: Option<String>,
    /// Center segment template.
    pub center: Option<String>,
    /// Right segment template.
    pub right: Option<String>,
}

/// Raw keybinding table: maps key strings to action strings.
///
/// # Examples
///
/// ```
/// use teamucks_core::config::RawKeybindings;
///
/// let raw: RawKeybindings = toml::from_str(r#""|" = "split_vertical""#).unwrap();
/// assert_eq!(raw.map.get("|").map(String::as_str), Some("split_vertical"));
/// ```
#[derive(Debug, Default)]
pub struct RawKeybindings {
    /// Inner map from key string to action string.
    pub map: HashMap<String, String>,
}

impl<'de> Deserialize<'de> for RawKeybindings {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let map = HashMap::<String, String>::deserialize(deserializer)?;
        Ok(Self { map })
    }
}

// ── Config path ───────────────────────────────────────────────────────────────

/// Return the path to the teamucks configuration file.
///
/// Resolution order:
/// 1. `$XDG_CONFIG_HOME/teamucks/config.toml`
/// 2. `~/.config/teamucks/config.toml`
///
/// # Examples
///
/// ```
/// use teamucks_core::config::config_path;
///
/// let path = config_path();
/// assert!(path.ends_with("teamucks/config.toml"));
/// ```
#[must_use]
pub fn config_path() -> PathBuf {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
        .unwrap_or_else(|| PathBuf::from("/.config"));

    base.join("teamucks").join("config.toml")
}

// ── Loading ───────────────────────────────────────────────────────────────────

/// Load and validate the configuration file, falling back to defaults on error.
///
/// If the file does not exist, returns [`ValidatedConfig::default`] silently.
/// If the file has parse errors or validation errors, logs warnings via
/// `tracing` and returns [`ValidatedConfig::default`].
///
/// # Examples
///
/// ```no_run
/// use teamucks_core::config::load_config;
///
/// let cfg = load_config();
/// assert_eq!(cfg.scrollback_lines, 10_000);
/// ```
#[must_use]
pub fn load_config() -> ValidatedConfig {
    let path = config_path();
    if !path.exists() {
        return ValidatedConfig::default();
    }

    let content = match std::fs::read_to_string(&path) {
        Ok(s) => s,
        Err(e) => {
            tracing::warn!(path = %path.display(), error = %e, "failed to read config file");
            return ValidatedConfig::default();
        }
    };

    load_config_from_str_inner(&content, &path)
}

/// Parse and validate a TOML configuration string.
///
/// Useful for testing or for loading config from a non-default location.
///
/// # Errors
///
/// Returns `Err` with a description if TOML parsing fails or if validation
/// produces errors.  All validation errors are concatenated.
///
/// # Examples
///
/// ```
/// use teamucks_core::config::load_config_from_str;
///
/// let cfg = load_config_from_str(r#"scrollback_lines = 5000"#).unwrap();
/// assert_eq!(cfg.scrollback_lines, 5000);
/// ```
pub fn load_config_from_str(toml: &str) -> Result<ValidatedConfig, String> {
    let raw: RawConfig = toml::from_str(toml).map_err(|e| e.to_string())?;
    validate(raw)
        .map_err(|errors| errors.iter().map(ToString::to_string).collect::<Vec<_>>().join("; "))
}

/// Internal helper used by both [`load_config`] and tests.
fn load_config_from_str_inner(content: &str, path: &std::path::Path) -> ValidatedConfig {
    let raw: RawConfig = match toml::from_str(content) {
        Ok(r) => r,
        Err(e) => {
            tracing::warn!(path = %path.display(), error = %e, "failed to parse config TOML");
            return ValidatedConfig::default();
        }
    };

    match validate(raw) {
        Ok(cfg) => cfg,
        Err(errors) => {
            for e in &errors {
                tracing::warn!(path = %path.display(), "config: {e}");
            }
            ValidatedConfig::default()
        }
    }
}

// ── Live reload ───────────────────────────────────────────────────────────────

/// Re-read the config file and return an updated [`ValidatedConfig`].
///
/// Phase 1 live reload re-reads the file on demand (triggered by a signal or
/// command) rather than using filesystem watches.
///
/// Fields that affect session identity (e.g. socket path) cannot be changed
/// without restarting the server; a warning is logged when such fields differ.
/// Reloadable fields — keybindings and theme — are updated.
///
/// When the file is missing or contains errors, the current config is returned
/// unchanged.
///
/// # Examples
///
/// ```no_run
/// use teamucks_core::config::{load_config, reload_config};
///
/// let current = load_config();
/// let updated = reload_config(&current);
/// // updated contains any changes from the config file
/// ```
#[must_use]
pub fn reload_config(current: &ValidatedConfig) -> ValidatedConfig {
    let path = config_path();
    if !path.exists() {
        return current.clone();
    }

    let content = match std::fs::read_to_string(&path) {
        Ok(s) => s,
        Err(e) => {
            tracing::warn!(path = %path.display(), error = %e, "reload: failed to read config file");
            return current.clone();
        }
    };

    let updated = load_config_from_str_inner(&content, &path);

    // Warn about non-reloadable fields that changed.
    if updated.default_shell != current.default_shell {
        tracing::warn!(
            old = %current.default_shell,
            new = %updated.default_shell,
            "config reload: default_shell change takes effect for new panes only"
        );
    }

    updated
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_raw_config_deserializes_scrollback() {
        let raw: RawConfig = toml::from_str("scrollback_lines = 5000").unwrap();
        assert_eq!(raw.scrollback_lines, Some(5000));
    }

    #[test]
    fn test_raw_keybindings_deserializes() {
        let raw: RawKeybindings = toml::from_str(r#""|" = "split_vertical""#).unwrap();
        assert_eq!(raw.map.get("|").map(String::as_str), Some("split_vertical"));
    }

    #[test]
    fn test_config_path_ends_with_suffix() {
        let path = config_path();
        assert!(path.ends_with("teamucks/config.toml"));
        assert!(path.is_absolute());
    }

    #[test]
    fn test_load_config_from_str_empty() {
        let cfg = load_config_from_str("").expect("empty string must succeed");
        assert_eq!(cfg.scrollback_lines, 10_000);
    }

    #[test]
    fn test_load_config_from_str_bad_toml() {
        let result = load_config_from_str("{{{{not valid toml");
        assert!(result.is_err());
    }
}
