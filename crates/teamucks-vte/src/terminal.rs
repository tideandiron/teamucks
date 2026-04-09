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
    modes::ModeFlags,
    parser::{Parser, Performer},
    style::{Attr, Color, PackedStyle},
};

// ---------------------------------------------------------------------------
// CSI parameter helpers
// ---------------------------------------------------------------------------

/// Extract parameter `idx` from a VTE parameter slice, substituting
/// `default` when the slot is absent or when the value is `0` (which VTE
/// uses to mean "use the default").
///
/// This matches the ECMA-48 rule that a sub-parameter of zero is treated as
/// the default value for that position.
#[inline]
fn param(params: &[u16], idx: usize, default: u16) -> u16 {
    params.get(idx).copied().filter(|&v| v != 0).unwrap_or(default)
}

// ---------------------------------------------------------------------------
// TerminalState
// ---------------------------------------------------------------------------

/// Mutable state of the terminal, separate from the parser.
///
/// Holds the grid and all other state that [`Performer`] callbacks need to
/// mutate.  This is kept separate from [`Terminal`] so that the parser can
/// borrow it mutably while the caller still has access to the parser.
pub(crate) struct TerminalState {
    grid: Grid,
    title: String,
    modes: ModeFlags,
}

impl TerminalState {
    fn new(cols: usize, rows: usize) -> Self {
        let grid = Grid::new(cols, rows);
        // Grid::new initialises modes to ModeFlags::default_modes().  Mirror
        // that into TerminalState so both are always in sync.
        let modes = grid.modes();
        Self { grid, title: String::new(), modes }
    }

    /// Handle SGR (Select Graphic Rendition) — CSI … m.
    ///
    /// Processes the parameter list left-to-right, consuming sub-parameters
    /// for extended colours (38/48) as needed.
    // This is a flat SGR dispatch table analogous to `perform_action` in the
    // parser; the length is inherent to the standard, not a design smell.
    #[allow(clippy::too_many_lines)]
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
                    // On failure, parse_extended_color still advances `i` past
                    // the sub-type byte, so the outer `i += 1` lands correctly.
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
    /// - `38;5;N`     → `Color::Indexed(N)`
    /// - `38;2;R;G;B` → `Color::Rgb(R, G, B)`
    ///
    /// `i` is the index of the **38/48** code itself in `params`.  On return
    /// (success *or* failure), `i` is advanced past every sub-parameter that
    /// was inspected, so that the outer loop's `i += 1` lands on the first
    /// param after the consumed sub-sequence and no byte is misread as a
    /// standalone SGR code.
    ///
    /// Returns `None` if sub-params are missing or the sub-type is
    /// unrecognised; `i` is still advanced past the sub-type byte in that
    /// case.
    fn parse_extended_color(params: &[u16], i: &mut usize) -> Option<Color> {
        // Need at least one more param (the sub-type).
        let sub = *params.get(*i + 1)?;

        // Always advance past the sub-type byte.  This ensures that even when
        // the remaining params are absent (malformed sequence), the sub-type
        // is not re-interpreted as a standalone SGR code by the outer loop.
        *i += 1;

        match sub {
            // 256-colour indexed: consume index.
            5 => {
                let idx_raw = *params.get(*i + 1)?;
                // idx_raw is a u16 parameter; clamp to u8 for the index.
                #[allow(clippy::cast_possible_truncation)]
                let idx = idx_raw.min(255) as u8;
                *i += 1; // skip index
                Some(Color::Indexed(idx))
            }
            // RGB true colour: consume R + G + B.
            2 => {
                let r_raw = *params.get(*i + 1)?;
                let g_raw = *params.get(*i + 2)?;
                let b_raw = *params.get(*i + 3)?;
                #[allow(clippy::cast_possible_truncation)]
                let r = r_raw.min(255) as u8;
                #[allow(clippy::cast_possible_truncation)]
                let g = g_raw.min(255) as u8;
                #[allow(clippy::cast_possible_truncation)]
                let b = b_raw.min(255) as u8;
                *i += 3; // skip R + G + B
                Some(Color::Rgb(r, g, b))
            }
            // Unknown sub-type — silently ignore the whole extended sequence.
            // `i` has already been advanced past the sub-type above.
            _ => None,
        }
    }

    /// Handle cursor movement CSI sequences (CUU, CUD, CUF, CUB, CNL, CPL,
    /// CHA, CUP/HVP, VPA).
    fn apply_cursor_movement(&mut self, params: &[u16], action: u8) {
        match action {
            // CUU — Cursor Up: move up n rows, clamp at row 0.
            b'A' => {
                let n = param(params, 0, 1) as usize;
                let row = self.grid.cursor_row();
                self.grid.set_cursor(self.grid.cursor_col(), row.saturating_sub(n));
            }

            // CUD — Cursor Down: move down n rows, clamp at last row.
            b'B' => {
                let n = param(params, 0, 1) as usize;
                let row = self.grid.cursor_row();
                let new_row = (row + n).min(self.grid.rows() - 1);
                self.grid.set_cursor(self.grid.cursor_col(), new_row);
            }

            // CUF — Cursor Forward: move right n cols, clamp at last col.
            b'C' => {
                let n = param(params, 0, 1) as usize;
                let col = self.grid.cursor_col();
                let new_col = (col + n).min(self.grid.cols() - 1);
                self.grid.set_cursor(new_col, self.grid.cursor_row());
            }

            // CUB — Cursor Back: move left n cols, clamp at col 0.
            b'D' => {
                let n = param(params, 0, 1) as usize;
                let col = self.grid.cursor_col();
                self.grid.set_cursor(col.saturating_sub(n), self.grid.cursor_row());
            }

            // CNL — Cursor Next Line: move down n rows, reset col to 0.
            b'E' => {
                let n = param(params, 0, 1) as usize;
                let row = self.grid.cursor_row();
                let new_row = (row + n).min(self.grid.rows() - 1);
                self.grid.set_cursor(0, new_row);
            }

            // CPL — Cursor Previous Line: move up n rows, reset col to 0.
            b'F' => {
                let n = param(params, 0, 1) as usize;
                let row = self.grid.cursor_row();
                self.grid.set_cursor(0, row.saturating_sub(n));
            }

            // CHA — Cursor Horizontal Absolute: move to col n (1-indexed).
            b'G' => {
                let n = param(params, 0, 1) as usize;
                // Convert 1-indexed to 0-indexed; saturating_sub guards n == 0
                // (which cannot happen after param() applies default 1).
                let col = n.saturating_sub(1).min(self.grid.cols() - 1);
                self.grid.set_cursor(col, self.grid.cursor_row());
            }

            // CUP — Cursor Position: move to (row, col) (1-indexed each).
            // HVP (b'f') is identical to CUP.
            b'H' | b'f' => {
                let row_1 = param(params, 0, 1) as usize;
                let col_1 = param(params, 1, 1) as usize;
                // Convert 1-indexed to 0-indexed; set_cursor_cup handles DECOM.
                let row = row_1.saturating_sub(1);
                let col = col_1.saturating_sub(1).min(self.grid.cols() - 1);
                self.grid.set_cursor_cup(col, row);
            }

            // VPA — Vertical Position Absolute: move to row n (1-indexed).
            b'd' => {
                let n = param(params, 0, 1) as usize;
                let row = n.saturating_sub(1).min(self.grid.rows() - 1);
                self.grid.set_cursor(self.grid.cursor_col(), row);
            }

            _ => {}
        }
    }

    /// Apply or clear a single DECSET/DECRST private mode number.
    ///
    /// Unknown mode numbers are silently ignored, as required by the spec.
    fn apply_private_mode(&mut self, mode_num: u16, set: bool) {
        // Mode 1049 — Alternate screen buffer — is handled separately because
        // it involves swapping the full grid state rather than flipping a flag.
        if mode_num == 1049 {
            if set {
                self.grid.enter_alternate_screen();
                // Entering the alternate screen always makes the cursor visible.
                // Mirror this into self.modes so that the flag stays consistent
                // with grid.cursor().is_visible().
                self.modes.insert(ModeFlags::CURSOR_VISIBLE);
                self.modes.insert(ModeFlags::ALTERNATE_SCREEN);
            } else {
                self.grid.exit_alternate_screen();
                self.modes.remove(ModeFlags::ALTERNATE_SCREEN);
                // After restoring the primary screen, re-sync cursor visibility
                // from the restored cursor state rather than forcing a value.
                if self.grid.cursor().is_visible() {
                    self.modes.insert(ModeFlags::CURSOR_VISIBLE);
                } else {
                    self.modes.remove(ModeFlags::CURSOR_VISIBLE);
                }
            }
            self.grid.set_modes(self.modes);
            return;
        }

        let flag = match mode_num {
            1 => ModeFlags::CURSOR_KEYS_APPLICATION,
            6 => ModeFlags::ORIGIN,
            7 => ModeFlags::AUTO_WRAP,
            25 => ModeFlags::CURSOR_VISIBLE,
            1000 => ModeFlags::MOUSE_REPORT_CLICK,
            1002 => ModeFlags::MOUSE_REPORT_BUTTON,
            1003 => ModeFlags::MOUSE_REPORT_ALL,
            1004 => ModeFlags::FOCUS_EVENTS,
            1006 => ModeFlags::MOUSE_SGR_FORMAT,
            2004 => ModeFlags::BRACKETED_PASTE,
            2026 => ModeFlags::SYNCHRONIZED_OUTPUT,
            // Unknown mode: silently ignore.
            _ => return,
        };

        let mut modes = self.modes;
        if set {
            modes.insert(flag);
        } else {
            modes.remove(flag);
        }

        // DECOM (mode 6) homes the cursor on set and reset.
        let is_decom = mode_num == 6;
        // DECAWM (mode 7) — when re-enabling auto-wrap, clear any pending
        // wrap that was accumulated while DECAWM was off.  With DECAWM off
        // the cursor is "stuck" at the last column; enabling DECAWM means
        // the cursor is now at the last column normally, and the next
        // character should write there (not trigger an immediate wrap).
        let is_decawm_enable = mode_num == 7 && set;

        self.modes = modes;
        self.grid.set_modes(modes);

        if is_decom {
            // After changing DECOM, cursor moves to home of the (potentially
            // relative) coordinate space.
            if set {
                // Cursor goes to top of scroll region.
                let region_top = self.grid.scroll_region().0;
                self.grid.set_cursor(0, region_top);
            } else {
                // Cursor goes to absolute (0,0).
                self.grid.set_cursor(0, 0);
            }
        }

        if is_decawm_enable {
            // Clear the wrap_pending flag so the cursor is treated as "at
            // the last column" rather than "about to wrap."
            self.grid.cursor_mut().wrap_pending = false;
        }
    }

    /// Handle scroll-region CSI sequences: DECSTBM (`r`), SU (`S`), SD (`T`).
    fn apply_scroll_region(&mut self, params: &[u16], action: u8) {
        match action {
            // DECSTBM — Set Top and Bottom Margins (CSI top ; bottom r).
            //
            // Sets the scroll region.  Parameters are 1-indexed.  Omitted or
            // zero parameters default to 1 (top) or `rows` (bottom).
            //
            // Rules per spec:
            //   - Both parameters must be within 1..=rows (1-indexed).
            //   - top must be strictly less than bottom.
            //   - Invalid values: the sequence is ignored entirely.
            //   - After a valid set the cursor moves to (0, 0).
            b'r' => {
                let rows = self.grid.rows();
                // A zero top-param defaults to 1; a zero bottom-param
                // defaults to `rows` (full screen).  We handle this manually
                // rather than using the general `param()` helper because the
                // default for bottom is `rows`, not 1.
                let top_1 = {
                    let v = params.first().copied().unwrap_or(0);
                    if v == 0 {
                        1
                    } else {
                        v
                    }
                } as usize;
                let bottom_1 = {
                    let v = params.get(1).copied().unwrap_or(0);
                    // Truncation: rows fits in u16 for any real terminal size.
                    #[allow(clippy::cast_possible_truncation)]
                    if v == 0 {
                        rows as u16
                    } else {
                        v
                    }
                } as usize;

                // Validate: both must be within 1..=rows and top < bottom.
                if top_1 >= 1 && bottom_1 <= rows && top_1 < bottom_1 {
                    self.grid.set_scroll_region(top_1 - 1, bottom_1 - 1);
                    // Cursor moves to (0, 0) after DECSTBM (DECOM is Feature 8).
                    self.grid.set_cursor(0, 0);
                }
                // Invalid values: ignore the sequence entirely.
            }

            // SU — Scroll Up (CSI n S): scroll up n lines within scroll region.
            b'S' => {
                let count = param(params, 0, 1) as usize;
                let (top, bottom) = self.grid.scroll_region();
                self.grid.scroll_up_in_region(count, top, bottom);
            }

            // SD — Scroll Down (CSI n T): scroll down n lines within scroll region.
            b'T' => {
                let count = param(params, 0, 1) as usize;
                let (top, bottom) = self.grid.scroll_region();
                self.grid.scroll_down_in_region(count, top, bottom);
            }

            _ => {}
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

            // LF / VT / FF — move cursor down one row, scrolling within the
            // scroll region if the cursor is at the region's bottom.
            0x0A..=0x0C => {
                let col = self.grid.cursor_col();
                let row = self.grid.cursor_row();
                let (region_top, region_bottom) = self.grid.scroll_region();
                if row == region_bottom {
                    // At the bottom of the scroll region — scroll up within it.
                    let (rt, rb) = (region_top, region_bottom);
                    self.grid.scroll_up_in_region(1, rt, rb);
                    // Cursor stays at region_bottom (now blank).
                    self.grid.set_cursor(col, row);
                } else if row + 1 < self.grid.rows() {
                    // Not at the bottom of the region or screen — just move down.
                    self.grid.set_cursor(col, row + 1);
                }
                // If the cursor is already at the very last row of the screen
                // (and not inside the scroll region), LF is a no-op.
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

    fn csi_dispatch(&mut self, params: &[u16], intermediates: &[u8], action: u8) {
        // DECSET (CSI ? … h) and DECRST (CSI ? … l) — private mode set/reset.
        if intermediates.contains(&b'?') {
            match action {
                b'h' | b'l' => {
                    let set = action == b'h';
                    for &mode_num in params {
                        self.apply_private_mode(mode_num, set);
                    }
                    return;
                }
                _ => return,
            }
        }

        match action {
            // SGR — Select Graphic Rendition
            b'm' => self.apply_sgr(params),

            // Cursor movement sequences (A–H, d, f) delegated to a helper.
            b'A' | b'B' | b'C' | b'D' | b'E' | b'F' | b'G' | b'H' | b'd' | b'f' => {
                self.apply_cursor_movement(params, action);
            }

            // ED — Erase in Display (CSI n J).
            //
            // Erased cells always receive the default style, not the cursor's
            // current SGR style.  The cursor position is not changed.
            b'J' => {
                // `param` substitutes 0 for absent/zero, so default is already 0.
                match params.first().copied().unwrap_or(0) {
                    // 0 or default: erase from cursor to end of screen.
                    0 => self.grid.erase_below(),
                    // 1: erase from start of screen to cursor.
                    1 => self.grid.erase_above(),
                    // 2: erase entire visible screen.
                    2 => self.grid.erase_all(),
                    // 3: erase scrollback buffer — no-op for Phase 1 (scrollback
                    // not yet implemented). Also catches unknown parameters.
                    _ => {}
                }
            }

            // EL — Erase in Line (CSI n K).
            //
            // Erased cells always receive the default style.  The cursor
            // position is not changed.
            b'K' => {
                match params.first().copied().unwrap_or(0) {
                    // 0 or default: erase from cursor to end of line.
                    0 => self.grid.erase_line_right(),
                    // 1: erase from start of line to cursor.
                    1 => self.grid.erase_line_left(),
                    // 2: erase entire current line.
                    2 => self.grid.erase_line_all(),
                    // Unknown parameter — silently ignore.
                    _ => {}
                }
            }

            // ECH — Erase Characters (CSI n X).
            //
            // Erases `n` characters starting at the cursor, clamped at the
            // end of the current line.  The cursor position is not changed.
            b'X' => {
                // Default count is 1 when the parameter is absent or zero.
                let count = param(params, 0, 1) as usize;
                self.grid.erase_chars(count);
            }

            // Scroll region sequences (r, S, T) delegated to a helper.
            b'r' | b'S' | b'T' => self.apply_scroll_region(params, action),

            // All other CSI sequences are silently ignored until later features.
            _ => {}
        }
    }

    fn esc_dispatch(&mut self, _intermediates: &[u8], action: u8) {
        match action {
            // DECSC — Save Cursor (ESC 7).
            b'7' => self.grid.save_cursor(),
            // DECRC — Restore Cursor (ESC 8).
            b'8' => self.grid.restore_cursor(),

            // IND — Index (ESC D): move cursor down one line.
            //
            // If the cursor is at the bottom of the scroll region, scroll up
            // within the region.  Otherwise, just move down.
            b'D' => {
                let col = self.grid.cursor_col();
                let row = self.grid.cursor_row();
                let (region_top, region_bottom) = self.grid.scroll_region();
                if row == region_bottom {
                    self.grid.scroll_up_in_region(1, region_top, region_bottom);
                    self.grid.set_cursor(col, row);
                } else if row + 1 < self.grid.rows() {
                    self.grid.set_cursor(col, row + 1);
                }
            }

            // RI — Reverse Index (ESC M): move cursor up one line.
            //
            // If the cursor is at the top of the scroll region, scroll down
            // within the region.  Otherwise, just move up.
            b'M' => {
                let col = self.grid.cursor_col();
                let row = self.grid.cursor_row();
                let (region_top, region_bottom) = self.grid.scroll_region();
                if row == region_top {
                    self.grid.scroll_down_in_region(1, region_top, region_bottom);
                    self.grid.set_cursor(col, row);
                } else if row > 0 {
                    self.grid.set_cursor(col, row - 1);
                }
            }

            // All other ESC sequences are silently ignored until later features.
            _ => {}
        }
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

    /// Return the current terminal mode flags.
    ///
    /// # Examples
    ///
    /// ```
    /// use teamucks_vte::{modes::ModeFlags, terminal::Terminal};
    ///
    /// let t = Terminal::new(80, 24);
    /// assert!(t.modes().contains(ModeFlags::AUTO_WRAP));
    /// assert!(t.modes().contains(ModeFlags::CURSOR_VISIBLE));
    /// ```
    #[must_use]
    pub fn modes(&self) -> ModeFlags {
        self.state.modes
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
