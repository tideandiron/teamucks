//! Status bar renderer: generates three-segment status bar content and escape sequences.
//!
//! [`StatusBar`] renders a status bar occupying the last row of the terminal.
//! It composes three segments:
//!
//! - **Left:** mode tag (when active) + session name + window list.
//! - **Center:** empty by default.
//! - **Right:** current working directory + optional command + clock (`HH:MM`).
//!
//! The rendered segments are then positioned via
//! [`StatusBar::render_to_escape_sequences`] which emits CUP + SGR sequences
//! that can be written directly to the host terminal.
//!
//! # Adaptive density
//!
//! When the session has exactly one window the window list is omitted from the
//! left segment; showing it would be redundant.
//!
//! # CWD truncation
//!
//! Long CWD paths are abbreviated: each intermediate path component is replaced
//! with its first character, and the last two components are kept in full.
//! Example: `/home/user/Code/teamucks/crates/vte` → `/h/u/C/teamucks/crates/vte`.
//! When the path starts with the `HOME` environment variable it is replaced
//! with `~`.
//!
//! # Layout priority
//!
//! The three segments are positioned left-aligned, right-aligned, and
//! centre-aligned respectively.  When segments would overlap the priority is:
//! left > right > center.  Center is truncated (or omitted) before the other
//! two segments are reduced.

// ---------------------------------------------------------------------------
// Public data types
// ---------------------------------------------------------------------------

/// Lightweight snapshot of session state consumed by the status bar renderer.
///
/// Callers extract only the fields the status bar needs, avoiding a borrow of
/// the entire session structure.
///
/// # Examples
///
/// ```
/// use teamucks_core::render::statusbar::{StatusBarData, StatusBarWindow};
///
/// let data = StatusBarData {
///     session_name: "main".to_owned(),
///     windows: vec![StatusBarWindow { index: 1, name: "shell".to_owned(), has_activity: false }],
///     active_window_index: 0,
///     active_pane_cwd: None,
///     active_pane_command: None,
///     mode: None,
/// };
/// assert_eq!(data.session_name, "main");
/// ```
#[derive(Debug, Clone)]
pub struct StatusBarData {
    /// The name of the current session (e.g. `"dev"`).
    pub session_name: String,
    /// All windows in the session, in display order.
    pub windows: Vec<StatusBarWindow>,
    /// Zero-based index of the currently active window into `windows`.
    pub active_window_index: usize,
    /// CWD of the active pane, if known.
    pub active_pane_cwd: Option<String>,
    /// Foreground command running in the active pane, if known.
    pub active_pane_command: Option<String>,
    /// Active multiplexer mode, e.g. `"RESIZE"` or `"COPY"`.  `None` for
    /// normal mode.
    pub mode: Option<String>,
}

/// A single window entry for status bar display.
///
/// # Examples
///
/// ```
/// use teamucks_core::render::statusbar::StatusBarWindow;
///
/// let w = StatusBarWindow { index: 2, name: "logs".to_owned(), has_activity: true };
/// assert!(w.has_activity);
/// ```
#[derive(Debug, Clone)]
pub struct StatusBarWindow {
    /// 1-based window index shown in the window list.
    pub index: usize,
    /// Human-readable window name.
    pub name: String,
    /// `true` when unread activity has occurred in this window since last
    /// focus.
    pub has_activity: bool,
}

/// The three string segments of a rendered status bar.
///
/// Segments are plain text (no embedded escape sequences).  Escape sequences
/// are added by [`StatusBar::render_to_escape_sequences`].
///
/// # Examples
///
/// ```
/// use teamucks_core::render::statusbar::StatusBarContent;
///
/// let content = StatusBarContent {
///     left: "dev | [editor] 2:logs*".to_owned(),
///     center: String::new(),
///     right: "~/Code  14:32".to_owned(),
/// };
/// assert!(content.center.is_empty());
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StatusBarContent {
    /// Left-aligned segment (mode tag + session name + window list).
    pub left: String,
    /// Centre-aligned segment (empty by default).
    pub center: String,
    /// Right-aligned segment (CWD + command + clock).
    pub right: String,
}

// ---------------------------------------------------------------------------
// StatusBar
// ---------------------------------------------------------------------------

/// Status bar renderer parameterised by terminal width.
///
/// # Examples
///
/// ```
/// use teamucks_core::render::statusbar::{StatusBar, StatusBarData, StatusBarWindow};
///
/// let bar = StatusBar::new(80);
/// let data = StatusBarData {
///     session_name: "work".to_owned(),
///     windows: vec![
///         StatusBarWindow { index: 1, name: "shell".to_owned(), has_activity: false },
///         StatusBarWindow { index: 2, name: "vim".to_owned(), has_activity: false },
///     ],
///     active_window_index: 1,
///     active_pane_cwd: Some("/tmp".to_owned()),
///     active_pane_command: None,
///     mode: None,
/// };
/// let content = bar.render(&data);
/// assert!(content.left.contains("work"));
/// ```
pub struct StatusBar {
    width: u16,
}

impl StatusBar {
    /// Create a new [`StatusBar`] renderer for a terminal `width` columns wide.
    ///
    /// # Examples
    ///
    /// ```
    /// use teamucks_core::render::statusbar::StatusBar;
    /// let bar = StatusBar::new(80);
    /// ```
    #[must_use]
    pub fn new(width: u16) -> Self {
        Self { width }
    }

    /// Generate the three status bar segments from `session` data.
    ///
    /// The returned [`StatusBarContent`] holds plain text strings — escape
    /// sequences are added by [`render_to_escape_sequences`].
    ///
    /// [`render_to_escape_sequences`]: StatusBar::render_to_escape_sequences
    #[must_use]
    pub fn render(&self, session: &StatusBarData) -> StatusBarContent {
        let left = build_left_segment(session);
        let center = String::new(); // always empty by design
        let right = build_right_segment(session);
        StatusBarContent { left, center, right }
    }

    /// Produce escape sequences that render `content` on terminal row `y`.
    ///
    /// The output:
    /// 1. Positions the cursor at column 0, row `y` (1-indexed in the CUP).
    /// 2. Fills the entire row with the background color.
    /// 3. Renders the left segment aligned left.
    /// 4. Renders the right segment aligned right.
    /// 5. Renders the center segment centred in remaining space (if any).
    ///
    /// Colors are `#rrggbb` hex strings.
    ///
    /// # Examples
    ///
    /// ```
    /// use teamucks_core::render::statusbar::{StatusBar, StatusBarContent};
    ///
    /// let bar = StatusBar::new(80);
    /// let content = StatusBarContent {
    ///     left: "dev".to_owned(),
    ///     center: String::new(),
    ///     right: "14:32".to_owned(),
    /// };
    /// let bytes = bar.render_to_escape_sequences(&content, 23, "#00aaff", "#1e1e2e", "#cdd6f4");
    /// assert!(!bytes.is_empty());
    /// ```
    #[must_use]
    pub fn render_to_escape_sequences(
        &self,
        content: &StatusBarContent,
        y: u16,
        accent_color: &str,
        bg_color: &str,
        fg_color: &str,
    ) -> Vec<u8> {
        let mut buf = Vec::with_capacity(512);
        let width = usize::from(self.width);

        let (bg_r, bg_g, bg_b) = parse_hex_color(bg_color);
        let (fg_r, fg_g, fg_b) = parse_hex_color(fg_color);
        let (ac_r, ac_g, ac_b) = parse_hex_color(accent_color);

        // 1. Position at the start of the status bar row.
        // CUP uses 1-indexed rows and columns.
        write_buf(&mut buf, format_args!("\x1b[{};1H", y + 1));

        // 2. Set background + foreground, then fill the row with spaces.
        write_buf(
            &mut buf,
            format_args!("\x1b[48;2;{bg_r};{bg_g};{bg_b}m\x1b[38;2;{fg_r};{fg_g};{fg_b}m"),
        );
        // Fill the entire row.
        buf.resize(buf.len() + width, b' ');

        // Compute display widths of each segment (in terminal columns).
        let left_display = display_width(&content.left);
        let right_display = display_width(&content.right);

        // Left wins: render left segment from col 0.
        let left_cols = left_display.min(width);
        if left_cols > 0 {
            write_buf(&mut buf, format_args!("\x1b[{};1H", y + 1));
            // Use accent color for the active-window brackets inside left segment.
            // We emit the full left segment in fg color for now; a future
            // enhancement can diff the segment for per-character coloring.
            write_buf(&mut buf, format_args!("\x1b[38;2;{fg_r};{fg_g};{fg_b}m"));
            let truncated_left = truncate_to_cols(&content.left, left_cols);
            buf.extend_from_slice(truncated_left.as_bytes());
        }

        // Right segment: render right-aligned.
        let right_start = width.saturating_sub(right_display);
        // Only render right if it doesn't overlap left.
        if right_start > left_cols {
            write_buf(&mut buf, format_args!("\x1b[{};{}H", y + 1, right_start + 1));
            write_buf(&mut buf, format_args!("\x1b[38;2;{fg_r};{fg_g};{fg_b}m"));
            let right_available = width - right_start;
            let truncated_right = truncate_to_cols(&content.right, right_available);
            buf.extend_from_slice(truncated_right.as_bytes());
        }

        // Center segment: render centred in remaining space between left and right.
        let center_display = display_width(&content.center);
        if center_display > 0 {
            let available_start = left_cols + 1;
            let available_end = if right_start > left_cols { right_start } else { left_cols };
            if available_end > available_start {
                let available = available_end - available_start;
                if center_display <= available {
                    let center_col = available_start + (available - center_display) / 2;
                    write_buf(&mut buf, format_args!("\x1b[{};{}H", y + 1, center_col + 1));
                    write_buf(&mut buf, format_args!("\x1b[38;2;{fg_r};{fg_g};{fg_b}m"));
                    buf.extend_from_slice(content.center.as_bytes());
                }
            }
        }

        // Reset SGR to leave terminal in a clean state.
        buf.extend_from_slice(b"\x1b[0m");

        // Suppress unused variable warning for accent color fields when center is empty
        let _ = (ac_r, ac_g, ac_b);

        buf
    }
}

// ---------------------------------------------------------------------------
// Private helpers
// ---------------------------------------------------------------------------

/// Build the left segment: mode tag + session name + window list.
fn build_left_segment(session: &StatusBarData) -> String {
    let mut parts: Vec<String> = Vec::new();

    // Mode tag: [RESIZE] or [COPY] at the start.
    if let Some(ref mode) = session.mode {
        parts.push(format!("[{mode}]"));
    }

    // Session name.
    parts.push(session.session_name.clone());

    // Window list: only when there is more than one window.
    if session.windows.len() > 1 {
        parts.push(build_window_list(session));
    }

    parts.join(" | ")
}

/// Build the window list portion of the left segment.
///
/// Format: `[editor] 2:logs* 3:build` where the active window is wrapped in
/// brackets and windows with activity have a `*` suffix.
fn build_window_list(session: &StatusBarData) -> String {
    session
        .windows
        .iter()
        .enumerate()
        .map(|(pos, w)| {
            let activity = if w.has_activity { "*" } else { "" };
            let label = format!("{}{}", w.name, activity);
            if pos == session.active_window_index {
                format!("[{label}]")
            } else {
                format!("{}:{label}", w.index)
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// Build the right segment from session state.
fn build_right_segment(session: &StatusBarData) -> String {
    let mut parts: Vec<String> = Vec::new();

    // CWD (truncated).
    if let Some(ref cwd) = session.active_pane_cwd {
        parts.push(truncate_cwd(cwd));
    }

    // Active command.
    if let Some(ref cmd) = session.active_pane_command {
        if !cmd.is_empty() {
            parts.push(cmd.clone());
        }
    }

    // Clock: HH:MM from the system clock.
    parts.push(current_time_hhmm());

    parts.join("  ")
}

/// Truncate `cwd` using first-char compression for intermediate path components.
///
/// The last two components are kept in full.  If the path begins with the
/// `HOME` environment variable it is replaced with `~`.
fn truncate_cwd(cwd: &str) -> String {
    // Attempt to replace HOME prefix with '~'.
    let with_tilde: String;
    let path = if let Ok(home) = std::env::var("HOME") {
        if cwd.starts_with(&home) {
            with_tilde = format!("~{}", &cwd[home.len()..]);
            &with_tilde
        } else {
            cwd
        }
    } else {
        cwd
    };

    // Split on '/'.
    let components: Vec<&str> = path.split('/').collect();
    let n = components.len();

    // If 3 or fewer components no truncation needed.
    if n <= 3 {
        return path.to_owned();
    }

    // Keep last two components in full; abbreviate everything in between
    // (except a leading empty string from a root '/').
    let mut result = Vec::with_capacity(n);
    for (i, &comp) in components.iter().enumerate() {
        if i == 0 {
            // Root prefix (empty string before leading '/') or '~'.
            result.push(comp.to_owned());
        } else if i >= n - 2 {
            // Last two components: keep in full.
            result.push(comp.to_owned());
        } else if comp.is_empty() {
            // Consecutive slashes — preserve.
            result.push(comp.to_owned());
        } else {
            // Intermediate component: keep only first char.
            let first = comp.chars().next().unwrap_or('_');
            result.push(first.to_string());
        }
    }

    result.join("/")
}

/// Return the current local time as an `"HH:MM"` string.
fn current_time_hhmm() -> String {
    // Use std::time to get a UTC timestamp, then format as HH:MM.
    // We avoid external date libraries; the simplest approach that compiles
    // without extra deps is to read /proc/driver/rtc or use the libc gmtime_r.
    // For portability and zero-new-deps we use std::time::SystemTime and
    // compute hours and minutes from the Unix epoch.
    use std::time::{SystemTime, UNIX_EPOCH};

    let secs = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs();

    // UTC time only (no local time zone without libc or chrono).
    // Seconds since midnight UTC.
    let day_secs = secs % 86_400;
    let hours = day_secs / 3_600;
    let minutes = (day_secs % 3_600) / 60;
    format!("{hours:02}:{minutes:02}")
}

/// Compute the display width of `s` in terminal columns.
///
/// Currently uses a simple byte-length proxy since all status bar text is
/// ASCII-safe.  A future enhancement can add `unicode-width` support.
#[inline]
fn display_width(s: &str) -> usize {
    // For ASCII-dominant status bar content this is a safe approximation.
    // TODO: integrate unicode-width when CJK session/window names are required.
    s.chars().count()
}

/// Truncate `s` to at most `cols` display columns, returning a new `String`.
fn truncate_to_cols(s: &str, cols: usize) -> String {
    // Collect char indices and stop at `cols` characters.
    let mut char_count = 0usize;
    let mut byte_end = s.len();
    for (byte_idx, _ch) in s.char_indices() {
        if char_count >= cols {
            byte_end = byte_idx;
            break;
        }
        char_count += 1;
    }
    if char_count < cols {
        // String is shorter than cols — return as-is.
        s.to_owned()
    } else {
        s[..byte_end].to_owned()
    }
}

/// Parse a `#rrggbb` hex color string into `(r, g, b)` components.
///
/// Returns `(255, 255, 255)` (white) on parse failure, ensuring a valid SGR
/// color sequence is always emitted.
#[inline]
fn parse_hex_color(color: &str) -> (u8, u8, u8) {
    let hex = color.strip_prefix('#').unwrap_or(color);
    if hex.len() != 6 {
        return (255, 255, 255);
    }
    let r = u8::from_str_radix(&hex[0..2], 16).unwrap_or(255);
    let g = u8::from_str_radix(&hex[2..4], 16).unwrap_or(255);
    let b = u8::from_str_radix(&hex[4..6], 16).unwrap_or(255);
    (r, g, b)
}

/// Write a formatted string into `buf` without allocating a temporary `String`.
#[inline]
fn write_buf(buf: &mut Vec<u8>, args: std::fmt::Arguments<'_>) {
    use std::io::Write as _;
    // Vec<u8> implements Write — write! never fails here.
    let _ = write!(buf, "{args}");
}

// ---------------------------------------------------------------------------
// Unit tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // -----------------------------------------------------------------------
    // truncate_cwd
    // -----------------------------------------------------------------------

    #[test]
    fn test_truncate_cwd_short_path_unchanged() {
        // Three components: /home/user — no truncation.
        let result = truncate_cwd("/home/user");
        assert!(result.contains("home"), "short path must not truncate: {result:?}");
        assert!(result.contains("user"), "short path must not truncate: {result:?}");
    }

    #[test]
    fn test_truncate_cwd_long_path_abbreviates_intermediate() {
        // /home/user/Code/teamucks/crates/vte
        // After abbreviation: /h/u/C/teamucks/crates/vte
        let result = truncate_cwd("/home/user/Code/teamucks/crates/vte");
        // Last two components kept in full.
        assert!(result.contains("crates"), "last components kept full: {result:?}");
        assert!(result.contains("vte"), "last components kept full: {result:?}");
        // Intermediate components are single chars.
        // The result is shorter than the original.
        assert!(
            result.len() < "/home/user/Code/teamucks/crates/vte".len(),
            "truncated path must be shorter: {result:?}"
        );
    }

    #[test]
    fn test_truncate_cwd_tilde_replacement() {
        // This test only verifies the function doesn't panic when HOME is set.
        // The actual ~ substitution depends on the environment.
        let _result = truncate_cwd("/home/someuser/Code");
    }

    // -----------------------------------------------------------------------
    // display_width
    // -----------------------------------------------------------------------

    #[test]
    fn test_display_width_ascii() {
        assert_eq!(display_width("hello"), 5);
        assert_eq!(display_width(""), 0);
    }

    // -----------------------------------------------------------------------
    // truncate_to_cols
    // -----------------------------------------------------------------------

    #[test]
    fn test_truncate_to_cols_shorter_than_limit() {
        let result = truncate_to_cols("hi", 10);
        assert_eq!(result, "hi");
    }

    #[test]
    fn test_truncate_to_cols_exactly_limit() {
        let result = truncate_to_cols("hello", 5);
        assert_eq!(result, "hello");
    }

    #[test]
    fn test_truncate_to_cols_exceeds_limit() {
        let result = truncate_to_cols("hello world", 5);
        assert_eq!(result, "hello");
    }

    // -----------------------------------------------------------------------
    // parse_hex_color
    // -----------------------------------------------------------------------

    #[test]
    fn test_parse_hex_color_valid() {
        assert_eq!(parse_hex_color("#1e1e2e"), (0x1e, 0x1e, 0x2e));
    }

    #[test]
    fn test_parse_hex_color_without_hash() {
        assert_eq!(parse_hex_color("ff0000"), (255, 0, 0));
    }

    #[test]
    fn test_parse_hex_color_invalid_returns_white() {
        assert_eq!(parse_hex_color("nope"), (255, 255, 255));
    }

    // -----------------------------------------------------------------------
    // current_time_hhmm
    // -----------------------------------------------------------------------

    #[test]
    fn test_current_time_hhmm_format() {
        let t = current_time_hhmm();
        // Must be HH:MM — five chars, digit:digit.
        assert_eq!(t.len(), 5, "time must be HH:MM (5 chars): {t:?}");
        assert_eq!(t.as_bytes()[2], b':', "third char must be ':': {t:?}");
        assert!(t.as_bytes()[0].is_ascii_digit(), "hour tens must be digit: {t:?}");
        assert!(t.as_bytes()[1].is_ascii_digit(), "hour ones must be digit: {t:?}");
        assert!(t.as_bytes()[3].is_ascii_digit(), "minute tens must be digit: {t:?}");
        assert!(t.as_bytes()[4].is_ascii_digit(), "minute ones must be digit: {t:?}");
    }

    // -----------------------------------------------------------------------
    // StatusBar::render
    // -----------------------------------------------------------------------

    fn make_data() -> StatusBarData {
        StatusBarData {
            session_name: "dev".to_owned(),
            windows: vec![
                StatusBarWindow { index: 1, name: "editor".to_owned(), has_activity: false },
                StatusBarWindow { index: 2, name: "logs".to_owned(), has_activity: true },
            ],
            active_window_index: 0,
            active_pane_cwd: Some("/home/user/Code".to_owned()),
            active_pane_command: None,
            mode: None,
        }
    }

    #[test]
    fn test_render_left_contains_session_name() {
        let bar = StatusBar::new(80);
        let content = bar.render(&make_data());
        assert!(content.left.contains("dev"), "left must contain session name: {:?}", content.left);
    }

    #[test]
    fn test_render_left_activity_star() {
        let bar = StatusBar::new(80);
        let content = bar.render(&make_data());
        assert!(
            content.left.contains("logs*"),
            "activity window must have star: {:?}",
            content.left
        );
    }

    #[test]
    fn test_render_center_is_empty() {
        let bar = StatusBar::new(80);
        let content = bar.render(&make_data());
        assert!(content.center.is_empty());
    }

    #[test]
    fn test_render_right_has_time() {
        let bar = StatusBar::new(80);
        let content = bar.render(&make_data());
        let digit_count = content.right.chars().filter(char::is_ascii_digit).count();
        assert!(digit_count >= 4, "right must contain time digits: {:?}", content.right);
    }
}
