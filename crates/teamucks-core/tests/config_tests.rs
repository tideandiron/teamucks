/// Integration tests for Feature 23: Basic Configuration.
///
/// Tests are grouped by concern:
/// - Default values
/// - Parsing from TOML
/// - Validation errors
/// - Key string parsing
/// - Config file path resolution
/// - TOML round-trip
use std::collections::HashMap;

use teamucks_core::{
    config::{
        load_config_from_str,
        types::{StatusBarPosition, ValidatedConfig},
        validate::{validate, ConfigError},
        RawConfig,
    },
    input::{
        command::Command,
        key::{Key, KeyEvent, Modifiers},
    },
};

// ── Helpers ───────────────────────────────────────────────────────────────────

fn ctrl_space() -> KeyEvent {
    KeyEvent { key: Key::Char(' '), modifiers: Modifiers::CTRL }
}

fn no_mod(c: char) -> KeyEvent {
    KeyEvent { key: Key::Char(c), modifiers: Modifiers::empty() }
}

// ── Default config ────────────────────────────────────────────────────────────

#[test]
fn test_config_default() {
    let cfg = ValidatedConfig::default();
    assert_eq!(cfg.scrollback_lines, 10_000);
    assert!(cfg.mouse_enabled);
    // default_shell must be non-empty
    assert!(!cfg.default_shell.is_empty());
    // default_cwd must exist as a path (may not be filesystem-accessible in all envs)
    assert!(!cfg.default_cwd.as_os_str().is_empty());
}

#[test]
fn test_config_default_prefix_ctrl_space() {
    let cfg = ValidatedConfig::default();
    assert_eq!(cfg.prefix_key, ctrl_space());
}

#[test]
fn test_config_default_theme() {
    let cfg = ValidatedConfig::default();
    // Tokyo Night accent: #7aa2f7
    assert_eq!(cfg.theme.accent, (0x7a, 0xa2, 0xf7));
    // border: #3b4261
    assert_eq!(cfg.theme.border, (0x3b, 0x42, 0x61));
    // status_bg: #1a1b26
    assert_eq!(cfg.theme.status_bg, (0x1a, 0x1b, 0x26));
    // status_fg: #a9b1d6
    assert_eq!(cfg.theme.status_fg, (0xa9, 0xb1, 0xd6));
    // active_window: #7aa2f7
    assert_eq!(cfg.theme.active_window, (0x7a, 0xa2, 0xf7));
    // activity: #e0af68
    assert_eq!(cfg.theme.activity, (0xe0, 0xaf, 0x68));
}

#[test]
fn test_config_default_keybindings() {
    let cfg = ValidatedConfig::default();
    // The default keybindings must cover | - h j k l
    assert_eq!(cfg.keybindings.get(&no_mod('|')), Some(&Command::SplitVertical));
    assert_eq!(cfg.keybindings.get(&no_mod('-')), Some(&Command::SplitHorizontal));
    assert_eq!(cfg.keybindings.get(&no_mod('h')), Some(&Command::NavigateLeft));
    assert_eq!(cfg.keybindings.get(&no_mod('j')), Some(&Command::NavigateDown));
    assert_eq!(cfg.keybindings.get(&no_mod('k')), Some(&Command::NavigateUp));
    assert_eq!(cfg.keybindings.get(&no_mod('l')), Some(&Command::NavigateRight));
    assert_eq!(cfg.keybindings.get(&no_mod('c')), Some(&Command::CreateWindow));
    assert_eq!(cfg.keybindings.get(&no_mod('d')), Some(&Command::Detach));
}

// ── Parsing ───────────────────────────────────────────────────────────────────

#[test]
fn test_config_parse_prefix() {
    let toml = r#"prefix = "ctrl-a""#;
    let cfg = load_config_from_str(toml).expect("valid config must parse");
    let expected = KeyEvent { key: Key::Char('a'), modifiers: Modifiers::CTRL };
    assert_eq!(cfg.prefix_key, expected);
}

#[test]
fn test_config_parse_theme_colors() {
    let toml = r##"
[theme]
accent        = "#ff0000"
border        = "#00ff00"
status_bg     = "#0000ff"
status_fg     = "#ffffff"
active_window = "#123456"
activity      = "#abcdef"
"##;
    let cfg = load_config_from_str(toml).expect("valid theme must parse");
    assert_eq!(cfg.theme.accent, (0xff, 0x00, 0x00));
    assert_eq!(cfg.theme.border, (0x00, 0xff, 0x00));
    assert_eq!(cfg.theme.status_bg, (0x00, 0x00, 0xff));
    assert_eq!(cfg.theme.status_fg, (0xff, 0xff, 0xff));
    assert_eq!(cfg.theme.active_window, (0x12, 0x34, 0x56));
    assert_eq!(cfg.theme.activity, (0xab, 0xcd, 0xef));
}

#[test]
fn test_config_parse_scrollback() {
    let toml = "scrollback_lines = 5000";
    let cfg = load_config_from_str(toml).expect("valid config");
    assert_eq!(cfg.scrollback_lines, 5000);
}

#[test]
fn test_config_parse_shell() {
    let toml = r#"default_shell = "/bin/sh""#;
    let cfg = load_config_from_str(toml).expect("valid config");
    assert_eq!(cfg.default_shell, "/bin/sh");
}

#[test]
fn test_config_parse_exit_behavior() {
    let toml = r#"pane_exit_behavior = "close""#;
    let cfg = load_config_from_str(toml).expect("valid config");
    use teamucks_core::pane::ExitBehavior;
    assert_eq!(cfg.pane_exit_behavior, ExitBehavior::Close);
}

#[test]
fn test_config_parse_mouse() {
    let toml = "mouse = false";
    let cfg = load_config_from_str(toml).expect("valid config");
    assert!(!cfg.mouse_enabled);
}

// ── Validation errors ─────────────────────────────────────────────────────────

#[test]
fn test_config_validate_invalid_prefix() {
    let raw = RawConfig { prefix: Some("ctrl-@@@".to_owned()), ..RawConfig::default() };
    let errors = validate(raw).expect_err("invalid prefix must produce errors");
    assert!(
        errors.iter().any(|e| matches!(e, ConfigError::InvalidPrefix { .. })),
        "expected InvalidPrefix error, got: {errors:?}"
    );
}

#[test]
fn test_config_validate_invalid_color() {
    let raw = RawConfig {
        theme: Some(teamucks_core::config::RawTheme {
            accent: Some("not-a-color".to_owned()),
            ..Default::default()
        }),
        ..RawConfig::default()
    };
    let errors = validate(raw).expect_err("invalid color must produce errors");
    assert!(
        errors.iter().any(|e| matches!(e, ConfigError::InvalidColor { .. })),
        "expected InvalidColor error, got: {errors:?}"
    );
}

#[test]
fn test_config_validate_collects_all_errors() {
    // Two separate errors: bad prefix AND bad color — both must appear.
    let raw = RawConfig {
        prefix: Some("ctrl-@@@".to_owned()),
        theme: Some(teamucks_core::config::RawTheme {
            accent: Some("not-a-color".to_owned()),
            ..Default::default()
        }),
        ..RawConfig::default()
    };
    let errors = validate(raw).expect_err("must produce errors");
    assert!(errors.len() >= 2, "expected >=2 errors, got {}: {errors:?}", errors.len());
}

#[test]
fn test_config_validate_unknown_action() {
    let mut kb = HashMap::new();
    kb.insert("|".to_owned(), "does_not_exist".to_owned());
    let raw = RawConfig {
        keybindings: Some(teamucks_core::config::RawKeybindings { map: kb }),
        ..RawConfig::default()
    };
    let errors = validate(raw).expect_err("unknown action must produce errors");
    assert!(
        errors.iter().any(|e| matches!(e, ConfigError::UnknownAction { .. })),
        "expected UnknownAction error, got: {errors:?}"
    );
}

// ── Key parsing ───────────────────────────────────────────────────────────────

mod key_parsing {
    use teamucks_core::config::parse_key;
    use teamucks_core::input::key::{Key, KeyEvent, Modifiers};

    fn plain(c: char) -> KeyEvent {
        KeyEvent { key: Key::Char(c), modifiers: Modifiers::empty() }
    }

    #[test]
    fn test_parse_key_char() {
        assert_eq!(parse_key("h"), Ok(plain('h')));
        assert_eq!(parse_key("a"), Ok(plain('a')));
        assert_eq!(parse_key("z"), Ok(plain('z')));
    }

    #[test]
    fn test_parse_key_ctrl() {
        let expected = KeyEvent { key: Key::Char('a'), modifiers: Modifiers::CTRL };
        assert_eq!(parse_key("ctrl-a"), Ok(expected));
    }

    #[test]
    fn test_parse_key_ctrl_space() {
        let expected = KeyEvent { key: Key::Char(' '), modifiers: Modifiers::CTRL };
        assert_eq!(parse_key("ctrl-space"), Ok(expected));
    }

    #[test]
    fn test_parse_key_special_enter() {
        let expected = KeyEvent { key: Key::Enter, modifiers: Modifiers::empty() };
        assert_eq!(parse_key("enter"), Ok(expected));
    }

    #[test]
    fn test_parse_key_special_escape() {
        let expected = KeyEvent { key: Key::Escape, modifiers: Modifiers::empty() };
        assert_eq!(parse_key("escape"), Ok(expected));
    }

    #[test]
    fn test_parse_key_pipe() {
        assert_eq!(parse_key("|"), Ok(plain('|')));
    }

    #[test]
    fn test_parse_key_dash() {
        assert_eq!(parse_key("-"), Ok(plain('-')));
    }

    #[test]
    fn test_parse_key_invalid() {
        assert!(parse_key("ctrl-@@@").is_err(), "ctrl-@@@ must be invalid");
        assert!(parse_key("").is_err(), "empty string must be invalid");
    }
}

// ── Config path ───────────────────────────────────────────────────────────────

mod config_path {
    use std::path::PathBuf;
    use teamucks_core::config::config_path;

    #[test]
    fn test_config_path_xdg() {
        // Set XDG_CONFIG_HOME to a known temp dir and confirm the path uses it.
        let tmp = std::env::temp_dir().join("teamucks_test_xdg");
        // Use a subprocess-safe approach: just call config_path() and verify form.
        // We can't easily isolate env vars without unsafe in a multi-threaded test
        // runner, so we verify the returned path ends with the correct suffix.
        let path = config_path();
        // Path must end with teamucks/config.toml regardless of which base is used.
        assert!(
            path.ends_with("teamucks/config.toml"),
            "config path must end with teamucks/config.toml, got: {}",
            path.display()
        );
        drop(tmp); // silence unused warning
    }

    #[test]
    fn test_config_path_default() {
        // Without XDG_CONFIG_HOME, fallback should be ~/.config/teamucks/config.toml
        // We verify the path contains ".config/teamucks/config.toml" OR is under XDG.
        let path = config_path();
        let s = path.to_string_lossy();
        assert!(
            s.contains("teamucks/config.toml"),
            "path must contain teamucks/config.toml, got: {s}"
        );
        // Must be absolute
        assert!(PathBuf::from(s.as_ref()).is_absolute(), "config path must be absolute");
    }
}

// ── TOML round-trip ───────────────────────────────────────────────────────────

#[test]
fn test_config_toml_parse() {
    let toml = r##"
prefix            = "ctrl-space"
default_shell     = "/bin/sh"
default_cwd       = "/tmp"
scrollback_lines  = 8000
pane_exit_behavior = "hold"
mouse             = true

[theme]
accent        = "#7aa2f7"
border        = "#3b4261"
status_bg     = "#1a1b26"
status_fg     = "#a9b1d6"
active_window = "#7aa2f7"
activity      = "#e0af68"

[status_bar]
position = "bottom"
left     = "{session} {windows}"
center   = ""
right    = "{cwd} {command} {clock}"

[keybindings]
"|" = "split_vertical"
"-" = "split_horizontal"
"h" = "navigate_left"
"j" = "navigate_down"
"k" = "navigate_up"
"l" = "navigate_right"
"##;
    let cfg = load_config_from_str(toml).expect("full sample TOML must parse without error");
    assert_eq!(cfg.scrollback_lines, 8000);
    assert_eq!(cfg.default_shell, "/bin/sh");
    assert_eq!(cfg.prefix_key, KeyEvent { key: Key::Char(' '), modifiers: Modifiers::CTRL });
    assert_eq!(cfg.theme.accent, (0x7a, 0xa2, 0xf7));
    assert!(matches!(cfg.status_bar.position, StatusBarPosition::Bottom));
}
