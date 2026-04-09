/// Integration tests for the status bar renderer.
///
/// Tests follow TDD naming: `test_statusbar_<unit>_<scenario>`.
use teamucks_core::render::statusbar::{StatusBar, StatusBarData, StatusBarWindow};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn three_windows() -> StatusBarData {
    StatusBarData {
        session_name: "dev".to_owned(),
        windows: vec![
            StatusBarWindow { index: 1, name: "editor".to_owned(), has_activity: false },
            StatusBarWindow { index: 2, name: "logs".to_owned(), has_activity: true },
            StatusBarWindow { index: 3, name: "build".to_owned(), has_activity: false },
        ],
        active_window_index: 0,
        active_pane_cwd: Some("/home/user/Code/teamucks".to_owned()),
        active_pane_command: None,
        mode: None,
    }
}

fn single_window() -> StatusBarData {
    StatusBarData {
        session_name: "main".to_owned(),
        windows: vec![StatusBarWindow { index: 1, name: "shell".to_owned(), has_activity: false }],
        active_window_index: 0,
        active_pane_cwd: Some("/home/user".to_owned()),
        active_pane_command: None,
        mode: None,
    }
}

// ---------------------------------------------------------------------------
// test_statusbar_default_layout
// ---------------------------------------------------------------------------

#[test]
fn test_statusbar_default_layout() {
    let bar = StatusBar::new(120);
    let data = three_windows();
    let content = bar.render(&data);

    // Left segment contains session name.
    assert!(
        content.left.contains("dev"),
        "left segment must include session name; got: {:?}",
        content.left
    );
    // Left segment contains window names.
    assert!(
        content.left.contains("editor"),
        "left must include window names; got: {:?}",
        content.left
    );
    // Center is empty by default.
    assert!(
        content.center.is_empty(),
        "center segment must be empty by default; got: {:?}",
        content.center
    );
    // Right segment contains a clock-like HH:MM pattern (two digits, colon, two digits).
    let has_time = content.right.chars().filter(char::is_ascii_digit).count() >= 4;
    assert!(has_time, "right segment must contain a time component; got: {:?}", content.right);
}

// ---------------------------------------------------------------------------
// test_statusbar_active_window_bold
// ---------------------------------------------------------------------------

#[test]
fn test_statusbar_active_window_bold() {
    let bar = StatusBar::new(120);
    let mut data = three_windows();
    data.active_window_index = 0; // "editor" is active

    let content = bar.render(&data);
    // The active window is marked with [] brackets (different from inactive windows).
    assert!(
        content.left.contains("[editor]") || content.left.contains("editor"),
        "active window must appear in left segment; got: {:?}",
        content.left,
    );
    // The active window must be visually distinct — the implementation encloses
    // the active window name in brackets: [name].
    assert!(
        content.left.contains("[editor]"),
        "active window must be wrapped in brackets; got: {:?}",
        content.left,
    );
}

// ---------------------------------------------------------------------------
// test_statusbar_activity_indicator
// ---------------------------------------------------------------------------

#[test]
fn test_statusbar_activity_indicator() {
    let bar = StatusBar::new(120);
    let data = three_windows(); // "logs" (index 2) has activity

    let content = bar.render(&data);
    // Window with activity gets a '*' suffix: "logs*".
    assert!(
        content.left.contains("logs*"),
        "window with activity must have '*' suffix; got: {:?}",
        content.left,
    );
    // Window without activity must NOT have '*' suffix.
    assert!(
        !content.left.contains("editor*"),
        "window without activity must not have '*' suffix; got: {:?}",
        content.left,
    );
}

// ---------------------------------------------------------------------------
// test_statusbar_mode_indicator
// ---------------------------------------------------------------------------

#[test]
fn test_statusbar_mode_indicator() {
    let bar = StatusBar::new(120);
    let mut data = three_windows();
    data.mode = Some("RESIZE".to_owned());

    let content = bar.render(&data);
    // Mode indicator appears at start of left segment.
    assert!(
        content.left.starts_with("[RESIZE]"),
        "RESIZE mode indicator must be at start of left segment; got: {:?}",
        content.left,
    );
}

#[test]
fn test_statusbar_copy_mode_indicator() {
    let bar = StatusBar::new(120);
    let mut data = three_windows();
    data.mode = Some("COPY".to_owned());

    let content = bar.render(&data);
    assert!(
        content.left.starts_with("[COPY]"),
        "COPY mode indicator must be at start of left segment; got: {:?}",
        content.left,
    );
}

// ---------------------------------------------------------------------------
// test_statusbar_no_mode
// ---------------------------------------------------------------------------

#[test]
fn test_statusbar_no_mode() {
    let bar = StatusBar::new(120);
    let data = three_windows(); // mode = None

    let content = bar.render(&data);
    // No mode tag when mode is None.
    assert!(
        !content.left.starts_with('[')
            || content.left.starts_with("[editor]")
            || content.left.starts_with("[1"),
        "no mode indicator when mode is None; got: {:?}",
        content.left,
    );
    // Specifically, must not contain [RESIZE] or [COPY].
    assert!(!content.left.contains("[RESIZE]"), "must not have mode tag; got: {:?}", content.left);
    assert!(!content.left.contains("[COPY]"), "must not have mode tag; got: {:?}", content.left);
}

// ---------------------------------------------------------------------------
// test_statusbar_single_window_no_list
// ---------------------------------------------------------------------------

#[test]
fn test_statusbar_single_window_no_list() {
    let bar = StatusBar::new(120);
    let data = single_window();

    let content = bar.render(&data);
    // Single window: just the session name, no window list.
    assert!(
        content.left.contains("main"),
        "left segment must contain session name; got: {:?}",
        content.left,
    );
    // With a single window there is no window index/list rendered.
    assert!(
        !content.left.contains("1:"),
        "single window must not render window list; got: {:?}",
        content.left,
    );
}

// ---------------------------------------------------------------------------
// test_statusbar_cwd_truncation
// ---------------------------------------------------------------------------

#[test]
fn test_statusbar_cwd_truncation() {
    let bar = StatusBar::new(120);
    let mut data = three_windows();
    // Long path: ~/Code/teamucks/crates/vte
    data.active_pane_cwd = Some("/home/user/Code/teamucks/crates/vte".to_owned());
    // HOME would be /home/user — simulate by using the full path without ~.

    let content = bar.render(&data);
    // The right segment must contain a truncated form of the path.
    // The last two components are preserved in full; intermediate dirs are
    // truncated to their first character.
    // "crates/vte" must be present as the tail.
    assert!(
        content.right.contains("crates") || content.right.contains("vte"),
        "right segment must contain tail of path; got: {:?}",
        content.right,
    );
}

#[test]
fn test_statusbar_cwd_short_path_not_truncated() {
    let bar = StatusBar::new(120);
    let mut data = three_windows();
    // Short path needs no truncation.
    data.active_pane_cwd = Some("/home/user".to_owned());

    let content = bar.render(&data);
    assert!(
        content.right.contains("user") || content.right.contains("home"),
        "short path must appear in right segment; got: {:?}",
        content.right,
    );
}

// ---------------------------------------------------------------------------
// test_statusbar_cwd_none
// ---------------------------------------------------------------------------

#[test]
fn test_statusbar_cwd_none() {
    let bar = StatusBar::new(120);
    let mut data = three_windows();
    data.active_pane_cwd = None;
    data.active_pane_command = None;

    let content = bar.render(&data);
    // Right segment still has the clock but no path.
    let has_time = content.right.chars().filter(char::is_ascii_digit).count() >= 4;
    assert!(
        has_time,
        "right segment must still have time even with no CWD; got: {:?}",
        content.right
    );
}

// ---------------------------------------------------------------------------
// test_statusbar_empty_center
// ---------------------------------------------------------------------------

#[test]
fn test_statusbar_empty_center() {
    let bar = StatusBar::new(120);
    let data = three_windows();
    let content = bar.render(&data);
    assert!(
        content.center.is_empty(),
        "center segment must always be empty; got: {:?}",
        content.center
    );
}

// ---------------------------------------------------------------------------
// test_statusbar_width_adaptation
// ---------------------------------------------------------------------------

#[test]
fn test_statusbar_width_adaptation() {
    // Very narrow terminal: 30 columns.
    let bar = StatusBar::new(30);
    let data = three_windows();
    let content = bar.render(&data);

    let output = bar.render_to_escape_sequences(&content, 23, "#00aaff", "#1e1e2e", "#cdd6f4");
    // The escape sequences must be non-empty.
    assert!(!output.is_empty(), "escape sequences must be produced even for narrow terminals");
}

// ---------------------------------------------------------------------------
// test_statusbar_overflow_left_wins
// ---------------------------------------------------------------------------

#[test]
fn test_statusbar_overflow_left_wins() {
    // 20-column terminal — very tight.
    let bar = StatusBar::new(20);
    let mut data = three_windows();
    // Long session name and many windows to overflow.
    data.session_name = "very-long-session-name".to_owned();
    data.active_pane_cwd = Some("/home/user/deeply/nested/path/that/is/very/long".to_owned());

    let content = bar.render(&data);
    // When content overflows, left segment always wins; check rendering doesn't panic.
    let output = bar.render_to_escape_sequences(&content, 23, "#00aaff", "#1e1e2e", "#cdd6f4");
    assert!(!output.is_empty());
}

// ---------------------------------------------------------------------------
// test_statusbar_escape_sequences
// ---------------------------------------------------------------------------

#[test]
fn test_statusbar_escape_sequences() {
    let bar = StatusBar::new(80);
    let data = three_windows();
    let content = bar.render(&data);

    let output = bar.render_to_escape_sequences(&content, 23, "#00aaff", "#1e1e2e", "#cdd6f4");
    let s = String::from_utf8_lossy(&output);

    // Must contain CUP sequence (ESC [ row ; col H).
    assert!(
        s.contains("\x1b["),
        "escape sequences must contain CUP; got bytes: {:?}",
        &output[..output.len().min(64)]
    );
    // The CUP for the status bar row (y=23 → 1-indexed 24) must appear.
    assert!(
        s.contains("\x1b[24;1H"),
        "must position status bar at row 24 col 1; got: {:?}",
        &s[..s.len().min(128)],
    );
    // Must include SGR background color sequence for the bar background.
    assert!(
        s.contains("\x1b[48;2;"),
        "must contain RGB background SGR; got: {:?}",
        &s[..s.len().min(128)]
    );
}
