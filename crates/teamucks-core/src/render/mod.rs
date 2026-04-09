/// Terminal renderer: translates protocol frame messages into escape sequences.
///
/// [`TerminalRenderer`] converts [`ServerMessage`] variants (produced by frame
/// diff computation) into escape sequences that can be written to a host
/// terminal to reproduce the pane's visual state.
///
/// # Output structure
///
/// Every rendered output is wrapped in *Synchronized Output* markers
/// (DECSET/DECRST mode 2026) so that the host terminal can batch the update
/// and avoid visible tearing:
///
/// ```text
/// ESC [ ? 2026 h   — begin synchronized update
/// …  cell updates …
/// ESC [ ? 2026 l   — end synchronized update
/// ```
///
/// # SGR optimization
///
/// The renderer tracks the last-emitted style and only emits SGR sequences for
/// attributes that differ from the previous cell.  This reduces output size for
/// runs of cells with the same style.
pub mod borders;
pub mod diff;
pub mod statusbar;

use teamucks_vte::style::Attr;

use crate::protocol::{CellData, ColorData, CursorShape, DiffEntry, ServerMessage};

// ---------------------------------------------------------------------------
// Escape-sequence constants
// ---------------------------------------------------------------------------

/// Begin synchronized output (mode 2026).
const SYNC_START: &[u8] = b"\x1b[?2026h";
/// End synchronized output (mode 2026).
const SYNC_END: &[u8] = b"\x1b[?2026l";
/// DECTCEM show cursor.
const CURSOR_SHOW: &[u8] = b"\x1b[?25h";
/// DECTCEM hide cursor.
const CURSOR_HIDE: &[u8] = b"\x1b[?25l";
/// SGR reset all attributes.
const SGR_RESET: &[u8] = b"\x1b[0m";

// ---------------------------------------------------------------------------
// RenderedStyle
// ---------------------------------------------------------------------------

/// The last-rendered style state tracked by the renderer for SGR optimisation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct RenderedStyle {
    fg: StyleColor,
    bg: StyleColor,
    attrs: u16,
}

impl Default for RenderedStyle {
    fn default() -> Self {
        Self { fg: StyleColor::Default, bg: StyleColor::Default, attrs: 0 }
    }
}

/// Compact representation of a colour for the renderer's style cache.
///
/// This mirrors [`ColorData`] but is `Copy` and `Eq` for cheap comparison.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StyleColor {
    Default,
    Indexed(u8),
    Rgb(u8, u8, u8),
}

impl From<&ColorData> for StyleColor {
    fn from(c: &ColorData) -> Self {
        match c {
            ColorData::Default => Self::Default,
            ColorData::Indexed(idx) => Self::Indexed(*idx),
            ColorData::Rgb(r, g, b) => Self::Rgb(*r, *g, *b),
        }
    }
}

// ---------------------------------------------------------------------------
// TerminalRenderer
// ---------------------------------------------------------------------------

/// Translates protocol frame messages into escape sequences for a host terminal.
///
/// # Examples
///
/// ```
/// use teamucks_core::render::TerminalRenderer;
/// let mut r = TerminalRenderer::new();
/// // r.render_full_frame(&frame) → &[u8] of escape sequences
/// ```
pub struct TerminalRenderer {
    buf: Vec<u8>,
    last_style: RenderedStyle,
}

impl TerminalRenderer {
    /// Create a new renderer with an empty output buffer and reset style state.
    #[must_use]
    pub fn new() -> Self {
        Self { buf: Vec::with_capacity(4096), last_style: RenderedStyle::default() }
    }

    /// Render a [`ServerMessage::FullFrame`] to escape sequences.
    ///
    /// The returned slice is valid until the next call to any `render_*` method.
    ///
    /// # Panics
    ///
    /// Panics if `frame` is not a `ServerMessage::FullFrame`.
    pub fn render_full_frame<'a>(&'a mut self, frame: &ServerMessage) -> &'a [u8] {
        let ServerMessage::FullFrame { cols, rows, cells, .. } = frame else {
            panic!("render_full_frame requires a FullFrame message");
        };

        self.buf.clear();
        self.last_style = RenderedStyle::default();

        self.buf.extend_from_slice(SYNC_START);
        // Reset SGR state.
        self.buf.extend_from_slice(SGR_RESET);

        let cols = *cols as usize;
        let rows = *rows as usize;

        for row in 0..rows {
            for col in 0..cols {
                let idx = row * cols + col;
                let cell = &cells[idx];
                // Skip continuation cells (second half of wide chars).
                if cell.flags & 0x02 != 0 {
                    continue;
                }
                self.emit_cup(col, row);
                self.emit_sgr(cell);
                self.buf.extend_from_slice(cell.grapheme.as_bytes());
            }
        }

        // Reset SGR at the end to leave the terminal in a clean state.
        self.buf.extend_from_slice(SGR_RESET);
        self.last_style = RenderedStyle::default();

        self.buf.extend_from_slice(SYNC_END);

        &self.buf
    }

    /// Render a [`ServerMessage::FrameDiff`] to escape sequences.
    ///
    /// The returned slice is valid until the next call to any `render_*` method.
    ///
    /// # Panics
    ///
    /// Panics if `diff` is not a `ServerMessage::FrameDiff`.
    pub fn render_diff<'a>(&'a mut self, diff: &ServerMessage) -> &'a [u8] {
        let ServerMessage::FrameDiff { diffs, .. } = diff else {
            panic!("render_diff requires a FrameDiff message");
        };

        self.buf.clear();
        self.last_style = RenderedStyle::default();

        self.buf.extend_from_slice(SYNC_START);

        for entry in diffs {
            match entry {
                DiffEntry::CellChange { col, row, cell } => {
                    self.emit_cup(*col as usize, *row as usize);
                    self.emit_sgr(cell);
                    self.buf.extend_from_slice(cell.grapheme.as_bytes());
                }
                DiffEntry::LineChange { row, cells } => {
                    // Position at the start of the row.
                    self.emit_cup(0, *row as usize);
                    for cell in cells {
                        // Skip continuation cells.
                        if cell.flags & 0x02 != 0 {
                            continue;
                        }
                        self.emit_sgr(cell);
                        self.buf.extend_from_slice(cell.grapheme.as_bytes());
                    }
                }
                DiffEntry::RegionScroll { top, bottom, count } => {
                    // Set scroll region then scroll.
                    self.emit_decstbm(*top as usize, *bottom as usize);
                    if *count > 0 {
                        // Scroll up.
                        write_buf(&mut self.buf, format_args!("\x1b[{count}S"));
                    } else if *count < 0 {
                        let down = -count;
                        // Scroll down.
                        write_buf(&mut self.buf, format_args!("\x1b[{down}T"));
                    }
                    // Reset scroll region to full screen (not stored here;
                    // caller is responsible for re-establishing it).
                }
            }
        }

        // Reset SGR.
        self.buf.extend_from_slice(SGR_RESET);
        self.last_style = RenderedStyle::default();

        self.buf.extend_from_slice(SYNC_END);

        &self.buf
    }

    /// Render a [`ServerMessage::CursorUpdate`] to escape sequences.
    ///
    /// Emits a CUP sequence to position the cursor and DECTCEM to
    /// show/hide it.
    ///
    /// The returned slice is valid until the next call to any `render_*` method.
    ///
    /// # Panics
    ///
    /// Panics if `update` is not a `ServerMessage::CursorUpdate`.
    pub fn render_cursor<'a>(&'a mut self, update: &ServerMessage) -> &'a [u8] {
        let ServerMessage::CursorUpdate { col, row, visible, shape, .. } = update else {
            panic!("render_cursor requires a CursorUpdate message");
        };

        self.buf.clear();

        // Position the cursor.
        self.emit_cup(*col as usize, *row as usize);

        // Cursor shape sequence (DECSCUSR — CSI Ps SP q).
        let shape_code: u8 = match shape {
            CursorShape::Block => 2,     // steady block
            CursorShape::Underline => 4, // steady underline
            CursorShape::Bar => 6,       // steady bar
        };
        write_buf(&mut self.buf, format_args!("\x1b[{shape_code} q"));

        // Visibility.
        if *visible {
            self.buf.extend_from_slice(CURSOR_SHOW);
        } else {
            self.buf.extend_from_slice(CURSOR_HIDE);
        }

        &self.buf
    }

    /// Render a slice of [`BorderCell`][borders::BorderCell] values to escape sequences.
    ///
    /// Emits a CUP + SGR color + character for each border cell.  Active border
    /// cells use `accent_color`; inactive cells use `muted_color`.  Colors are
    /// expressed as hex strings (`"#rrggbb"`).  If a color string cannot be
    /// parsed it falls back to the terminal's default foreground color.
    ///
    /// The returned slice is valid until the next call to any `render_*` method.
    ///
    /// # Examples
    ///
    /// ```
    /// use teamucks_core::render::TerminalRenderer;
    /// use teamucks_core::render::borders::BorderCell;
    ///
    /// let mut r = TerminalRenderer::new();
    /// let cells = [BorderCell { x: 5, y: 2, ch: '│', is_active_border: true }];
    /// let out = r.render_borders(&cells, "#00aaff", "#555555");
    /// assert!(!out.is_empty());
    /// ```
    pub fn render_borders<'a>(
        &'a mut self,
        borders: &[borders::BorderCell],
        accent_color: &str,
        muted_color: &str,
    ) -> &'a [u8] {
        self.buf.clear();

        let accent = parse_hex_color(accent_color);
        let muted = parse_hex_color(muted_color);

        for cell in borders {
            // CUP: position cursor at (col, row), 1-indexed.
            self.emit_cup(usize::from(cell.x), usize::from(cell.y));

            // SGR: foreground color — accent for active borders, muted for others.
            let (r, g, b) = if cell.is_active_border { accent } else { muted };
            write_buf(&mut self.buf, format_args!("\x1b[38;2;{r};{g};{b}m"));

            // Encode the box-drawing character as UTF-8.
            let mut char_buf = [0u8; 4];
            let encoded = cell.ch.encode_utf8(&mut char_buf);
            self.buf.extend_from_slice(encoded.as_bytes());
        }

        // Reset SGR to leave the terminal in a clean state.
        if !borders.is_empty() {
            self.buf.extend_from_slice(SGR_RESET);
        }

        &self.buf
    }

    // ---------------------------------------------------------------------------
    // Private helpers
    // ---------------------------------------------------------------------------

    /// Emit a CUP (Cursor Position) sequence: `ESC [ row+1 ; col+1 H`.
    ///
    /// Terminal coordinates are 1-indexed; the parameters are 0-indexed.
    #[inline]
    fn emit_cup(&mut self, col: usize, row: usize) {
        write_buf(&mut self.buf, format_args!("\x1b[{};{}H", row + 1, col + 1));
    }

    /// Emit DECSTBM (set top and bottom margins): `ESC [ top+1 ; bottom+1 r`.
    #[inline]
    fn emit_decstbm(&mut self, top: usize, bottom: usize) {
        write_buf(&mut self.buf, format_args!("\x1b[{};{}r", top + 1, bottom + 1));
    }

    /// Emit SGR sequences for `cell`'s style, but only for attributes that
    /// differ from `last_style`.
    ///
    /// This is the SGR optimisation: avoid re-emitting sequences that are
    /// already active on the host terminal.
    fn emit_sgr(&mut self, cell: &CellData) {
        let target = RenderedStyle {
            fg: StyleColor::from(&cell.fg),
            bg: StyleColor::from(&cell.bg),
            attrs: cell.attrs,
        };

        let fg_changed = target.fg != self.last_style.fg;
        let bg_changed = target.bg != self.last_style.bg;
        let attrs_changed = target.attrs != self.last_style.attrs;

        if !fg_changed && !bg_changed && !attrs_changed {
            return;
        }

        // If attributes changed AND we need to clear bits, emit SGR reset first
        // then re-apply everything.  This is simpler than tracking individual
        // cleared attributes and is correct.
        let attrs_cleared = (self.last_style.attrs & !target.attrs) != 0;

        if attrs_cleared {
            // Reset and re-emit all.
            self.buf.extend_from_slice(SGR_RESET);
            self.last_style = RenderedStyle::default();
            // Now emit everything from scratch.
            self.emit_sgr_full(target.fg, target.bg, target.attrs);
        } else {
            // Only emit changed components.
            if attrs_changed {
                self.emit_sgr_attrs_added(self.last_style.attrs, target.attrs);
            }
            if fg_changed {
                self.emit_sgr_color(target.fg, ColorTarget::Foreground);
            }
            if bg_changed {
                self.emit_sgr_color(target.bg, ColorTarget::Background);
            }
        }

        self.last_style = target;
    }

    /// Emit a full SGR sequence for all style components (called after a reset).
    fn emit_sgr_full(&mut self, fg: StyleColor, bg: StyleColor, attrs: u16) {
        self.emit_sgr_attrs_added(0, attrs);
        self.emit_sgr_color(fg, ColorTarget::Foreground);
        self.emit_sgr_color(bg, ColorTarget::Background);
    }

    /// Emit SGR codes for newly-added attribute bits (bits in `new` that are
    /// not in `old`).
    fn emit_sgr_attrs_added(&mut self, old: u16, new: u16) {
        let added = new & !old;
        if added == 0 {
            return;
        }

        // Map Attr bits to SGR codes.
        let attr_map: &[(u16, u8)] = &[
            (Attr::BOLD.bits(), 1),
            (Attr::DIM.bits(), 2),
            (Attr::ITALIC.bits(), 3),
            (Attr::UNDERLINE.bits(), 4),
            (Attr::BLINK.bits(), 5),
            (Attr::INVERSE.bits(), 7),
            (Attr::HIDDEN.bits(), 8),
            (Attr::STRIKETHROUGH.bits(), 9),
            (Attr::CURLY_UNDERLINE.bits(), 21),
        ];

        for &(bit, code) in attr_map {
            if added & bit != 0 {
                write_buf(&mut self.buf, format_args!("\x1b[{code}m"));
            }
        }
    }

    /// Emit a foreground or background colour SGR sequence.
    fn emit_sgr_color(&mut self, color: StyleColor, target: ColorTarget) {
        match (color, target) {
            (StyleColor::Default, ColorTarget::Foreground) => {
                self.buf.extend_from_slice(b"\x1b[39m");
            }
            (StyleColor::Default, ColorTarget::Background) => {
                self.buf.extend_from_slice(b"\x1b[49m");
            }
            (StyleColor::Indexed(idx), ColorTarget::Foreground) => {
                write_buf(&mut self.buf, format_args!("\x1b[38;5;{idx}m"));
            }
            (StyleColor::Indexed(idx), ColorTarget::Background) => {
                write_buf(&mut self.buf, format_args!("\x1b[48;5;{idx}m"));
            }
            (StyleColor::Rgb(r, g, b), ColorTarget::Foreground) => {
                write_buf(&mut self.buf, format_args!("\x1b[38;2;{r};{g};{b}m"));
            }
            (StyleColor::Rgb(r, g, b), ColorTarget::Background) => {
                write_buf(&mut self.buf, format_args!("\x1b[48;2;{r};{g};{b}m"));
            }
        }
    }
}

impl Default for TerminalRenderer {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Private types
// ---------------------------------------------------------------------------

#[derive(Clone, Copy)]
enum ColorTarget {
    Foreground,
    Background,
}

// ---------------------------------------------------------------------------
// Private helpers
// ---------------------------------------------------------------------------

/// Write a formatted string to `buf` without allocating a temporary `String`.
#[inline]
fn write_buf(buf: &mut Vec<u8>, args: std::fmt::Arguments<'_>) {
    use std::io::Write as _;
    // Vec<u8> implements Write — write! never fails here.
    let _ = write!(buf, "{args}");
}

/// Parse a `#rrggbb` hex color string into `(r, g, b)` components.
///
/// Returns `(255, 255, 255)` (white) if the string cannot be parsed, so the
/// renderer always emits a valid color sequence.
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

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::{ColorData, CursorShape, DiffEntry, ServerMessage};

    fn make_cell(grapheme: &str) -> CellData {
        CellData {
            grapheme: grapheme.to_owned(),
            fg: ColorData::Default,
            bg: ColorData::Default,
            attrs: 0,
            flags: 0,
        }
    }

    #[test]
    fn test_render_new_creates_renderer() {
        let r = TerminalRenderer::new();
        assert_eq!(r.last_style, RenderedStyle::default());
        assert!(r.buf.is_empty());
    }

    #[test]
    fn test_render_cup_is_1_indexed() {
        // CUP at (col=0, row=0) → ESC[1;1H.
        let diff = ServerMessage::FrameDiff {
            pane_id: 1,
            diffs: vec![DiffEntry::CellChange { col: 0, row: 0, cell: make_cell("A") }],
        };
        let mut r = TerminalRenderer::new();
        let out = String::from_utf8_lossy(r.render_diff(&diff));
        assert!(out.contains("\x1b[1;1H"), "got: {out:?}");
    }

    #[test]
    fn test_render_cursor_position_cup() {
        let update = ServerMessage::CursorUpdate {
            pane_id: 1,
            col: 4,
            row: 2,
            visible: true,
            shape: CursorShape::Block,
        };
        let mut r = TerminalRenderer::new();
        let out = String::from_utf8_lossy(r.render_cursor(&update));
        // row=2 → 3, col=4 → 5 (1-indexed).
        assert!(out.contains("\x1b[3;5H"), "got: {out:?}");
    }

    #[test]
    fn test_render_cursor_show() {
        let update = ServerMessage::CursorUpdate {
            pane_id: 1,
            col: 0,
            row: 0,
            visible: true,
            shape: CursorShape::Block,
        };
        let mut r = TerminalRenderer::new();
        let out = String::from_utf8_lossy(r.render_cursor(&update));
        assert!(out.contains("\x1b[?25h"), "got: {out:?}");
    }

    #[test]
    fn test_render_cursor_hide() {
        let update = ServerMessage::CursorUpdate {
            pane_id: 1,
            col: 0,
            row: 0,
            visible: false,
            shape: CursorShape::Block,
        };
        let mut r = TerminalRenderer::new();
        let out = String::from_utf8_lossy(r.render_cursor(&update));
        assert!(out.contains("\x1b[?25l"), "got: {out:?}");
    }

    #[test]
    fn test_render_sgr_bold() {
        let cell = CellData {
            grapheme: "X".to_owned(),
            fg: ColorData::Default,
            bg: ColorData::Default,
            attrs: Attr::BOLD.bits(),
            flags: 0,
        };
        let diff = ServerMessage::FrameDiff {
            pane_id: 1,
            diffs: vec![DiffEntry::CellChange { col: 0, row: 0, cell }],
        };
        let mut r = TerminalRenderer::new();
        let out = String::from_utf8_lossy(r.render_diff(&diff));
        assert!(out.contains("\x1b[1m"), "got: {out:?}");
    }

    #[test]
    fn test_render_sgr_not_repeated_for_same_style() {
        let cell_a = CellData {
            grapheme: "A".to_owned(),
            fg: ColorData::Indexed(2),
            bg: ColorData::Default,
            attrs: 0,
            flags: 0,
        };
        let cell_b = CellData {
            grapheme: "B".to_owned(),
            fg: ColorData::Indexed(2),
            bg: ColorData::Default,
            attrs: 0,
            flags: 0,
        };
        let diff = ServerMessage::FrameDiff {
            pane_id: 1,
            diffs: vec![
                DiffEntry::CellChange { col: 0, row: 0, cell: cell_a },
                DiffEntry::CellChange { col: 1, row: 0, cell: cell_b },
            ],
        };
        let mut r = TerminalRenderer::new();
        let out = String::from_utf8_lossy(r.render_diff(&diff)).to_string();
        // SGR for index 2: ESC[38;5;2m — should appear exactly once.
        let count = out.matches("\x1b[38;5;2m").count();
        assert_eq!(count, 1, "SGR should be emitted once for repeated style; got: {out:?}");
    }

    #[test]
    fn test_render_sync_markers_present_in_full_frame() {
        let frame = ServerMessage::FullFrame {
            pane_id: 1,
            cols: 2,
            rows: 1,
            cells: vec![make_cell("A"), make_cell("B")],
        };
        let mut r = TerminalRenderer::new();
        let out = String::from_utf8_lossy(r.render_full_frame(&frame));
        assert!(out.contains("\x1b[?2026h"), "got: {out:?}");
        assert!(out.contains("\x1b[?2026l"), "got: {out:?}");
    }
}
