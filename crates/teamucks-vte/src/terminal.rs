//! [`Terminal`] — the top-level terminal emulator struct.
//!
//! `Terminal` wraps [`Parser`] and [`Grid`] and implements the [`Performer`]
//! trait so that parsed VTE events mutate the grid correctly.
//!
//! # Design
//!
//! The borrow checker requires that `Parser::advance` receives `&mut Parser`
//! and `&mut impl Performer` simultaneously.  Because `Terminal` owns both the
//! parser and the grid/state, a two-struct split is used:
//!
//! - [`TerminalState`] holds the [`Grid`], the current title, and any mutable
//!   state that `Performer` callbacks need to modify.
//! - [`Terminal`] holds a [`Parser`] and a [`TerminalState`] as two separate
//!   fields.  `parser.advance(&mut self.state, input)` borrows them
//!   independently, satisfying the borrow checker.

use crate::{
    grid::Grid,
    parser::{Parser, Performer},
    style::{Attr, Color, PackedStyle},
};

// ---------------------------------------------------------------------------
// TerminalState
// ---------------------------------------------------------------------------

/// Mutable state of the terminal, separate from the parser.
///
/// Holds the grid and all other state that [`Performer`] callbacks need to
/// mutate.  This is kept separate from [`Terminal`] so that the parser can
/// borrow it mutably while the caller still has access to the parser.
pub struct TerminalState {
    grid: Grid,
    title: String,
}

impl TerminalState {
    fn new(cols: usize, rows: usize) -> Self {
        Self { grid: Grid::new(cols, rows), title: String::new() }
    }

    /// Handle SGR (Select Graphic Rendition) — CSI … m.
    ///
    /// Processes the parameter list left-to-right, consuming sub-parameters
    /// for extended colours (38/48) as needed.
    fn apply_sgr(&mut self, params: &[u16]) {
        // An empty parameter list is equivalent to a single zero (reset).
        if params.is_empty() {
            self.grid.cursor_mut().style = PackedStyle::default();
            return;
        }

        let mut i = 0usize;
        while i < params.len() {
            let p = params[i];
            match p {
                // ── Reset ─────────────────────────────────────────────────
                0 => {
                    self.grid.cursor_mut().style = PackedStyle::default();
                }

                // ── Attribute on ──────────────────────────────────────────
                1 => self.grid.cursor_mut().style.set_attr(Attr::BOLD),
                2 => self.grid.cursor_mut().style.set_attr(Attr::DIM),
                3 => self.grid.cursor_mut().style.set_attr(Attr::ITALIC),
                4 => self.grid.cursor_mut().style.set_attr(Attr::UNDERLINE),
                5 => self.grid.cursor_mut().style.set_attr(Attr::BLINK),
                7 => self.grid.cursor_mut().style.set_attr(Attr::INVERSE),
                8 => self.grid.cursor_mut().style.set_attr(Attr::HIDDEN),
                9 => self.grid.cursor_mut().style.set_attr(Attr::STRIKETHROUGH),
                // SGR 21: curly underline (used by Kitty / iTerm).
                21 => self.grid.cursor_mut().style.set_attr(Attr::CURLY_UNDERLINE),

                // ── Attribute off ─────────────────────────────────────────
                // 22 clears both BOLD and DIM (same intensity reset).
                22 => {
                    self.grid.cursor_mut().style.clear_attr(Attr::BOLD);
                    self.grid.cursor_mut().style.clear_attr(Attr::DIM);
                }
                23 => self.grid.cursor_mut().style.clear_attr(Attr::ITALIC),
                // 24 clears all underline variants.
                24 => {
                    self.grid.cursor_mut().style.clear_attr(Attr::UNDERLINE);
                    self.grid.cursor_mut().style.clear_attr(Attr::CURLY_UNDERLINE);
                    self.grid.cursor_mut().style.clear_attr(Attr::DOTTED_UNDERLINE);
                    self.grid.cursor_mut().style.clear_attr(Attr::DASHED_UNDERLINE);
                }
                25 => self.grid.cursor_mut().style.clear_attr(Attr::BLINK),
                27 => self.grid.cursor_mut().style.clear_attr(Attr::INVERSE),
                28 => self.grid.cursor_mut().style.clear_attr(Attr::HIDDEN),
                29 => self.grid.cursor_mut().style.clear_attr(Attr::STRIKETHROUGH),

                // ── Named foreground colours (30–37) ──────────────────────
                30..=37 => {
                    // Truncation: p is in 30..=37 so p - 30 is in 0..=7, fits u8.
                    #[allow(clippy::cast_possible_truncation)]
                    let idx = (p - 30) as u8;
                    self.grid.cursor_mut().style.set_foreground(Color::Named(idx));
                }

                // ── Extended foreground colour (38) ───────────────────────
                38 => {
                    if let Some(color) = Self::parse_extended_color(params, &mut i) {
                        self.grid.cursor_mut().style.set_foreground(color);
                    }
                    // If parsing failed (missing sub-params), the index has not
                    // advanced past what was consumed; the while loop's i += 1
                    // will move past code 38 itself.
                }

                // ── Default foreground ────────────────────────────────────
                39 => self.grid.cursor_mut().style.set_foreground(Color::Default),

                // ── Named background colours (40–47) ──────────────────────
                40..=47 => {
                    #[allow(clippy::cast_possible_truncation)]
                    let idx = (p - 40) as u8;
                    self.grid.cursor_mut().style.set_background(Color::Named(idx));
                }

                // ── Extended background colour (48) ───────────────────────
                48 => {
                    if let Some(color) = Self::parse_extended_color(params, &mut i) {
                        self.grid.cursor_mut().style.set_background(color);
                    }
                }

                // ── Default background ────────────────────────────────────
                49 => self.grid.cursor_mut().style.set_background(Color::Default),

                // ── Bright foreground colours (90–97) ─────────────────────
                90..=97 => {
                    #[allow(clippy::cast_possible_truncation)]
                    let idx = (p - 90 + 8) as u8;
                    self.grid.cursor_mut().style.set_foreground(Color::Named(idx));
                }

                // ── Bright background colours (100–107) ───────────────────
                100..=107 => {
                    #[allow(clippy::cast_possible_truncation)]
                    let idx = (p - 100 + 8) as u8;
                    self.grid.cursor_mut().style.set_background(Color::Named(idx));
                }

                // ── Unknown code — silently ignore ────────────────────────
                _ => {}
            }

            i += 1;
        }
    }

    /// Parse an extended colour sub-sequence starting *after* the 38 or 48
    /// code.
    ///
    /// - `38;5;N`   → `Color::Indexed(N)`
    /// - `38;2;R;G;B` → `Color::Rgb(R, G, B)`
    ///
    /// `i` is the index of the **38/48** code itself in `params`.  If parsing
    /// succeeds, `i` is advanced so that the outer loop's `i += 1` lands on
    /// the first param *after* the consumed sub-sequence.
    ///
    /// Returns `None` without advancing `i` if sub-params are missing or the
    /// sub-type is unrecognised.
    fn parse_extended_color(params: &[u16], i: &mut usize) -> Option<Color> {
        // Need at least one more param (the sub-type).
        let sub = *params.get(*i + 1)?;

        match sub {
            // 256-colour indexed: consume sub-type + index.
            5 => {
                let idx_raw = *params.get(*i + 2)?;
                // idx_raw is a u16 parameter; clamp to u8 for the index.
                #[allow(clippy::cast_possible_truncation)]
                let idx = idx_raw.min(255) as u8;
                *i += 2; // skip sub-type + index
                Some(Color::Indexed(idx))
            }
            // RGB true colour: consume sub-type + R + G + B.
            2 => {
                let r_raw = *params.get(*i + 2)?;
                let g_raw = *params.get(*i + 3)?;
                let b_raw = *params.get(*i + 4)?;
                #[allow(clippy::cast_possible_truncation)]
                let r = r_raw.min(255) as u8;
                #[allow(clippy::cast_possible_truncation)]
                let g = g_raw.min(255) as u8;
                #[allow(clippy::cast_possible_truncation)]
                let b = b_raw.min(255) as u8;
                *i += 4; // skip sub-type + R + G + B
                Some(Color::Rgb(r, g, b))
            }
            // Unknown sub-type — silently ignore the whole extended sequence.
            _ => None,
        }
    }
}

impl Performer for TerminalState {
    fn print(&mut self, c: char) {
        self.grid.write_char(c);
    }

    fn execute(&mut self, byte: u8) {
        match byte {
            // BS — move cursor left one column, clamp at column 0.
            0x08 => {
                let col = self.grid.cursor_col();
                let row = self.grid.cursor_row();
                if col > 0 {
                    self.grid.set_cursor(col - 1, row);
                }
            }

            // HT — advance to the next tab stop (every 8 columns by default).
            0x09 => {
                let col = self.grid.cursor_col();
                let row = self.grid.cursor_row();
                let cols = self.grid.cols();
                // Next tab stop is at the smallest multiple of 8 that is > col.
                let next_stop = (col / 8 + 1) * 8;
                // Clamp to the last column (not past it).
                let new_col = next_stop.min(cols - 1);
                self.grid.set_cursor(new_col, row);
            }

            // LF / VT / FF — move cursor down one row, scroll if at the bottom.
            0x0A..=0x0C => {
                let col = self.grid.cursor_col();
                let row = self.grid.cursor_row();
                let rows = self.grid.rows();
                if row + 1 >= rows {
                    // At the last row — scroll the grid up by one.
                    self.grid.scroll_up(1);
                    // Cursor stays at the (now blank) last row.
                    self.grid.set_cursor(col, row);
                } else {
                    self.grid.set_cursor(col, row + 1);
                }
            }

            // CR — move cursor to column 0 of the current row.
            0x0D => {
                let row = self.grid.cursor_row();
                self.grid.set_cursor(0, row);
            }

            // All other C0 controls are silently ignored.
            // This includes BEL (0x07): audio bell is a display-layer concern,
            // not a grid mutation.  Features 5+ will handle additional controls.
            _ => {}
        }
    }

    fn csi_dispatch(&mut self, params: &[u16], _intermediates: &[u8], action: u8) {
        // Only SGR (action b'm') is handled in Feature 4.
        // Cursor movement, erase, and scroll are handled in Features 5–7.
        if action == b'm' {
            self.apply_sgr(params);
        }
    }

    fn esc_dispatch(&mut self, _intermediates: &[u8], _action: u8) {
        // ESC sequences are handled in Feature 5+.
    }

    fn osc_dispatch(&mut self, params: &[&[u8]]) {
        // OSC 0 and OSC 2: set terminal title.
        //
        // params[0] = command number as ASCII bytes, params[1] = title.
        let command = params.first().copied().unwrap_or(b"");
        if command == b"0" || command == b"2" {
            if let Some(title_bytes) = params.get(1) {
                // The title is arbitrary UTF-8; replace invalid bytes with U+FFFD.
                self.title = String::from_utf8_lossy(title_bytes).into_owned();
            }
        }
    }

    fn dcs_dispatch(&mut self, _params: &[u16], _intermediates: &[u8], _action: u8, _data: &[u8]) {
        // DCS sequences are not handled in Feature 4.
    }
}

// ---------------------------------------------------------------------------
// Terminal
// ---------------------------------------------------------------------------

/// High-level terminal emulator.
///
/// `Terminal` owns a [`Parser`] and a [`TerminalState`] (which holds the
/// [`Grid`] and title).  Feeding bytes via [`Terminal::feed`] advances the
/// parser, which calls back into `TerminalState` as a [`Performer`].
///
/// # Examples
///
/// ```
/// use teamucks_vte::terminal::Terminal;
///
/// let mut t = Terminal::new(80, 24);
/// t.feed(b"hello");
/// assert_eq!(t.grid().row_text(0), "hello");
/// ```
pub struct Terminal {
    parser: Parser,
    state: TerminalState,
}

impl Terminal {
    /// Create a new terminal with the given dimensions.
    ///
    /// The grid is initialised to all-blank cells with default style.  The
    /// cursor starts at (0, 0) and the title is empty.
    ///
    /// # Panics
    ///
    /// Panics if `cols == 0` or `rows == 0`.
    #[must_use]
    pub fn new(cols: usize, rows: usize) -> Self {
        Self { parser: Parser::new(), state: TerminalState::new(cols, rows) }
    }

    /// Feed raw bytes to the parser.
    ///
    /// The parser advances its state machine and calls the appropriate
    /// [`Performer`] methods on the internal [`TerminalState`].
    pub fn feed(&mut self, input: &[u8]) {
        self.parser.advance(&mut self.state, input);
    }

    /// Return an immutable reference to the grid.
    #[must_use]
    pub fn grid(&self) -> &Grid {
        &self.state.grid
    }

    /// Return a mutable reference to the grid.
    pub fn grid_mut(&mut self) -> &mut Grid {
        &mut self.state.grid
    }

    /// Return the current terminal title.
    ///
    /// The title is set by OSC 0 or OSC 2 sequences.  It is empty until a
    /// title sequence is received.
    #[must_use]
    pub fn title(&self) -> &str {
        &self.state.title
    }

    /// Resize the terminal to the given dimensions.
    ///
    /// Existing content is preserved where it fits.  The cursor is clamped to
    /// the new bounds.
    ///
    /// # Panics
    ///
    /// Panics if `cols == 0` or `rows == 0`.
    pub fn resize(&mut self, cols: usize, rows: usize) {
        self.state.grid.resize(cols, rows);
    }
}
