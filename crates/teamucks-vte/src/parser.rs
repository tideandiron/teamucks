//! VTE state-machine parser.
//!
//! The parser is a table-driven implementation of the Paul Flo Williams VTE
//! state diagram (<https://vt100.net/emu/dec_ansi_parser>). It takes raw bytes
//! and emits typed actions to a [`Performer`].
//!
//! # Examples
//!
//! ```
//! use teamucks_vte::parser::{Parser, Performer};
//!
//! struct Record {
//!     printed: Vec<char>,
//! }
//!
//! impl Performer for Record {
//!     fn print(&mut self, c: char) { self.printed.push(c); }
//!     fn execute(&mut self, _byte: u8) {}
//!     fn csi_dispatch(&mut self, _params: &[u16], _intermediates: &[u8], _action: u8) {}
//!     fn esc_dispatch(&mut self, _intermediates: &[u8], _action: u8) {}
//!     fn osc_dispatch(&mut self, _params: &[&[u8]]) {}
//!     fn dcs_dispatch(&mut self, _params: &[u16], _intermediates: &[u8], _action: u8, _data: &[u8]) {}
//! }
//!
//! let mut parser = Parser::new();
//! let mut rec = Record { printed: Vec::new() };
//! parser.advance(&mut rec, b"hello");
//! assert_eq!(rec.printed, ['h', 'e', 'l', 'l', 'o']);
//! ```

pub(crate) mod action;
pub(crate) mod table;

use table::{transition, State};

use crate::params::Params;

/// Maximum number of intermediate bytes buffered per sequence.
const MAX_INTERMEDIATES: usize = 2;

/// Maximum number of bytes accumulated in the OSC buffer.
///
/// Bytes beyond this limit are silently dropped to prevent denial-of-service
/// memory exhaustion. The parser does not transition to an error state.
const MAX_OSC_LEN: usize = 65_536;

/// Maximum number of bytes accumulated in the DCS passthrough buffer.
///
/// Bytes beyond this limit are silently dropped to prevent denial-of-service
/// memory exhaustion. The parser does not transition to an error state.
const MAX_DCS_LEN: usize = 65_536;

/// Trait implemented by the caller to receive parsed VTE events.
///
/// Each method corresponds to one class of terminal action. Methods are called
/// in order as the byte stream is processed.
pub trait Performer {
    /// A printable Unicode character was decoded from the byte stream.
    fn print(&mut self, c: char);

    /// A C0 or C1 control byte was encountered (e.g., `\n`, `\r`, `\x07`).
    fn execute(&mut self, byte: u8);

    /// A complete CSI sequence was parsed.
    ///
    /// - `params`: numeric parameters (empty if none supplied).
    /// - `intermediates`: intermediate bytes (e.g., `b'?'` for `?`-prefixed).
    /// - `action`: the final byte of the CSI sequence.
    fn csi_dispatch(&mut self, params: &[u16], intermediates: &[u8], action: u8);

    /// A complete ESC sequence (not CSI, DCS, OSC) was parsed.
    ///
    /// - `intermediates`: any intermediate bytes collected.
    /// - `action`: the final byte.
    fn esc_dispatch(&mut self, intermediates: &[u8], action: u8);

    /// A complete OSC string was parsed.
    ///
    /// `params` is a slice of byte-string slices. The first element is the
    /// numeric command (e.g., `b"0"`) and subsequent elements are arguments.
    fn osc_dispatch(&mut self, params: &[&[u8]]);

    /// A complete DCS sequence was parsed.
    ///
    /// For Phase 1 this only guarantees no panic. Full DCS support follows.
    fn dcs_dispatch(&mut self, params: &[u16], intermediates: &[u8], action: u8, data: &[u8]);
}

/// Table-driven VTE parser state machine.
///
/// `Parser` is `Send` but not `Sync`. It must not be shared between threads
/// without external synchronisation.
///
/// # Allocation
///
/// `advance` is zero-allocation on the hot path. `osc_buffer` and `dcs_data`
/// grow on demand but allocate only once per sequence, not per character.
///
/// # UTF-8 decoding
///
/// Multi-byte sequences are assembled in an internal four-byte buffer. Invalid
/// sequences are replaced with U+FFFD (replacement character) and the parser
/// recovers without panicking.
pub struct Parser {
    state: State,
    params: Params,
    intermediates: [u8; MAX_INTERMEDIATES],
    intermediates_len: usize,
    /// Raw OSC payload bytes.
    osc_buffer: Vec<u8>,
    /// Byte indices in `osc_buffer` where semicolons were found.
    osc_params_idx: Vec<usize>,
    /// DCS passthrough data.
    dcs_data: Vec<u8>,
    /// UTF-8 assembly buffer.
    utf8_buffer: [u8; 4],
    /// Number of bytes currently in `utf8_buffer`.
    utf8_len: u8,
    /// How many bytes are expected to complete the current UTF-8 sequence.
    utf8_expected: u8,
    /// State we were in when 0x1B arrived, so we can recover DCS/OSC ST
    /// termination.
    prev_state: State,
}

impl Parser {
    /// Create a new parser in the Ground state.
    #[must_use]
    pub fn new() -> Self {
        Self {
            state: State::Ground,
            params: Params::new(),
            intermediates: [0u8; MAX_INTERMEDIATES],
            intermediates_len: 0,
            osc_buffer: Vec::new(),
            osc_params_idx: Vec::new(),
            dcs_data: Vec::new(),
            utf8_buffer: [0u8; 4],
            utf8_len: 0,
            utf8_expected: 0,
            prev_state: State::Ground,
        }
    }

    /// Feed `input` bytes into the parser, emitting actions to `performer`.
    ///
    /// This is the hot-path entry point. The call is zero-allocation except
    /// during OSC/DCS data accumulation.
    pub fn advance<P: Performer>(&mut self, performer: &mut P, input: &[u8]) {
        for &byte in input {
            self.process_byte(performer, byte);
        }
    }

    /// Process a single byte.
    #[inline]
    fn process_byte<P: Performer>(&mut self, performer: &mut P, byte: u8) {
        // High bytes (0x80–0xFF) that arrive while in Ground state are
        // UTF-8 leading or continuation bytes.  Route them through the
        // UTF-8 assembler before consulting the state table.
        if self.state == State::Ground && byte >= 0x80 {
            self.process_utf8(performer, byte);
            return;
        }

        // Flush any in-progress UTF-8 sequence if a non-continuation byte
        // arrives in the middle of it.
        if self.utf8_len > 0 && (!(0x80..=0xBF).contains(&byte) || self.state != State::Ground) {
            self.flush_utf8(performer);
        }

        let (action, next_state) = transition(self.state, byte);

        // When ESC arrives (next_state == Escape), record the state we are
        // leaving so the OSC/DCS ST terminator logic can use it.
        if next_state == State::Escape && self.state != State::Escape {
            self.prev_state = self.state;
        }

        // Perform entry actions for the new state when transitioning.
        if next_state != self.state {
            self.perform_entry_action(next_state);
        }

        // Execute the transition action.
        self.perform_action(performer, action, byte);

        self.state = next_state;
    }

    /// UTF-8 multi-byte sequence assembler.
    ///
    /// Handles 2-, 3-, and 4-byte sequences.  Invalid bytes emit U+FFFD.
    #[inline]
    fn process_utf8<P: Performer>(&mut self, performer: &mut P, byte: u8) {
        if self.utf8_len == 0 {
            // Determine expected sequence length from the leading byte.
            let expected = if byte < 0x80 {
                // ASCII — should not arrive here, but handle gracefully.
                performer.print(byte as char);
                return;
            } else if byte & 0xE0 == 0xC0 {
                2
            } else if byte & 0xF0 == 0xE0 {
                3
            } else if byte & 0xF8 == 0xF0 {
                4
            } else {
                // Continuation byte or invalid — emit replacement and reset.
                performer.print('\u{FFFD}');
                return;
            };
            self.utf8_expected = expected;
        }

        if byte & 0xC0 == 0x80 || self.utf8_len == 0 {
            // Valid continuation byte (or first byte when utf8_len == 0).
            self.utf8_buffer[self.utf8_len as usize] = byte;
            self.utf8_len += 1;
        } else if self.utf8_len > 0 {
            // New leading byte arrived before sequence was complete.
            self.flush_utf8(performer);
            self.process_utf8(performer, byte);
            return;
        }

        if self.utf8_len == self.utf8_expected {
            self.flush_utf8(performer);
        }
    }

    /// Decode and emit whatever is in `utf8_buffer`, then reset.
    #[inline]
    fn flush_utf8<P: Performer>(&mut self, performer: &mut P) {
        let slice = &self.utf8_buffer[..self.utf8_len as usize];
        match core::str::from_utf8(slice) {
            Ok(s) => {
                for c in s.chars() {
                    performer.print(c);
                }
            }
            Err(_) => {
                performer.print('\u{FFFD}');
            }
        }
        self.utf8_len = 0;
        self.utf8_expected = 0;
    }

    /// Actions performed when *entering* a new state.
    ///
    /// The Williams diagram specifies entry actions separate from transition
    /// actions.  We fold them in here.
    #[inline]
    fn perform_entry_action(&mut self, new_state: State) {
        match new_state {
            State::Escape => {
                // Entering Escape clears intermediates and params.
                self.intermediates_len = 0;
                self.params.clear();
            }
            State::CsiEntry => {
                self.intermediates_len = 0;
                self.params.clear();
            }
            State::DcsEntry => {
                self.intermediates_len = 0;
                self.params.clear();
                self.dcs_data.clear();
            }
            State::OscString => {
                self.osc_buffer.clear();
                self.osc_params_idx.clear();
            }
            _ => {
                // No entry action needed for Ground and other states.
            }
        }
    }

    /// Perform a transition action.
    #[inline]
    #[allow(clippy::too_many_lines)] // table-driven match — intentionally large
    fn perform_action<P: Performer>(
        &mut self,
        performer: &mut P,
        action: action::Action,
        byte: u8,
    ) {
        use action::Action;
        match action {
            Action::None | Action::Ignore => {}

            Action::Print => {
                // ASCII printable — single-byte, fast path.
                performer.print(byte as char);
            }

            Action::Execute => {
                performer.execute(byte);
            }

            Action::Clear => {
                self.intermediates_len = 0;
                self.params.clear();
            }

            Action::Collect => {
                // Collect intermediate byte.  Silently drop if buffer full.
                if self.intermediates_len < MAX_INTERMEDIATES {
                    self.intermediates[self.intermediates_len] = byte;
                    self.intermediates_len += 1;
                }
            }

            Action::Param => {
                if byte == b';' {
                    self.params.finish_param();
                } else {
                    self.params.add_digit(byte);
                }
            }

            Action::EscDispatch => {
                // Check if this is an ST terminator (b'\\') for DCS or OSC.
                if byte == b'\\' {
                    match self.prev_state {
                        State::OscString => {
                            self.dispatch_osc(performer);
                            return;
                        }
                        State::DcsPassthrough => {
                            let intermediates = &self.intermediates[..self.intermediates_len];
                            // No final byte for ST-terminated DCS — use 0.
                            let dcs_data = core::mem::take(&mut self.dcs_data);
                            performer.dcs_dispatch(
                                self.params.as_slice(),
                                intermediates,
                                0,
                                &dcs_data,
                            );
                            self.dcs_data = dcs_data; // restore (now empty) allocation
                            self.dcs_data.clear();
                            return;
                        }
                        _ => {}
                    }
                }
                let intermediates = &self.intermediates[..self.intermediates_len];
                performer.esc_dispatch(intermediates, byte);
            }

            Action::CsiDispatch => {
                self.params.finalize();
                let intermediates = &self.intermediates[..self.intermediates_len];
                performer.csi_dispatch(self.params.as_slice(), intermediates, byte);
            }

            Action::Hook => {
                // DCS hook: record final byte in intermediates so we can pass
                // it to dcs_dispatch on Unhook.
                if self.intermediates_len < MAX_INTERMEDIATES {
                    self.intermediates[self.intermediates_len] = byte;
                    self.intermediates_len += 1;
                }
            }

            Action::Put => {
                if self.dcs_data.len() < MAX_DCS_LEN {
                    self.dcs_data.push(byte);
                }
                // Silently drop bytes that exceed MAX_DCS_LEN.
            }

            Action::Unhook => {
                // The hook stored the DCS final byte as the last intermediate.
                let (action_byte, intermediates_len) = if self.intermediates_len > 0 {
                    (self.intermediates[self.intermediates_len - 1], self.intermediates_len - 1)
                } else {
                    (0, 0)
                };
                self.params.finalize();
                let intermediates = &self.intermediates[..intermediates_len];
                let dcs_data = core::mem::take(&mut self.dcs_data);
                performer.dcs_dispatch(
                    self.params.as_slice(),
                    intermediates,
                    action_byte,
                    &dcs_data,
                );
                self.dcs_data = dcs_data;
                self.dcs_data.clear();
            }

            Action::OscStart => {
                self.osc_buffer.clear();
                self.osc_params_idx.clear();
            }

            Action::OscPut => {
                if byte == b';' {
                    // Record split point.
                    self.osc_params_idx.push(self.osc_buffer.len());
                } else if self.osc_buffer.len() < MAX_OSC_LEN {
                    self.osc_buffer.push(byte);
                }
                // Silently drop bytes that exceed MAX_OSC_LEN.
            }

            Action::OscEnd => {
                self.dispatch_osc(performer);
            }
        }
    }

    /// Build OSC parameter slices and call `performer.osc_dispatch`.
    fn dispatch_osc<P: Performer>(&mut self, performer: &mut P) {
        // Build slices: params are the sub-strings separated by semicolons.
        // osc_params_idx holds the byte offsets in osc_buffer where each
        // semicolon appeared (semicolons themselves are NOT in osc_buffer).
        //
        // Layout example for "0;my title":
        //   osc_buffer     = b"0my title"
        //   osc_params_idx = [1]   (position of ';' → first param ends at 1)
        //
        // We build slices: [0..1, 1..end]
        //
        // Avoid heap allocation on the hot path: use a fixed-size array for
        // param slices. OSC sequences rarely have more than a handful of params.
        const MAX_OSC_PARAMS: usize = 16;

        let buf = &self.osc_buffer;
        let splits = &self.osc_params_idx;

        let mut param_slices: [&[u8]; MAX_OSC_PARAMS] = [b""; MAX_OSC_PARAMS];
        let mut param_count = 0usize;

        let mut start = 0usize;
        for &split in splits {
            if param_count < MAX_OSC_PARAMS {
                param_slices[param_count] = &buf[start..split];
                param_count += 1;
            }
            start = split;
        }
        if param_count < MAX_OSC_PARAMS {
            param_slices[param_count] = &buf[start..];
            param_count += 1;
        }

        performer.osc_dispatch(&param_slices[..param_count]);
    }
}

impl Default for Parser {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod unit_tests {
    use super::*;

    /// A single recorded DCS event.
    type DcsRecord = (Vec<u16>, Vec<u8>, u8, Vec<u8>);

    /// A test performer that records all events.
    #[derive(Default)]
    struct Recorder {
        printed: Vec<char>,
        executed: Vec<u8>,
        csi: Vec<(Vec<u16>, Vec<u8>, u8)>,
        esc: Vec<(Vec<u8>, u8)>,
        osc: Vec<Vec<Vec<u8>>>,
        dcs: Vec<DcsRecord>,
    }

    impl Performer for Recorder {
        fn print(&mut self, c: char) {
            self.printed.push(c);
        }
        fn execute(&mut self, byte: u8) {
            self.executed.push(byte);
        }
        fn csi_dispatch(&mut self, params: &[u16], intermediates: &[u8], action: u8) {
            self.csi.push((params.to_vec(), intermediates.to_vec(), action));
        }
        fn esc_dispatch(&mut self, intermediates: &[u8], action: u8) {
            self.esc.push((intermediates.to_vec(), action));
        }
        fn osc_dispatch(&mut self, params: &[&[u8]]) {
            self.osc.push(params.iter().map(|p| p.to_vec()).collect());
        }
        fn dcs_dispatch(&mut self, params: &[u16], intermediates: &[u8], action: u8, data: &[u8]) {
            self.dcs.push((params.to_vec(), intermediates.to_vec(), action, data.to_vec()));
        }
    }

    fn parse(input: &[u8]) -> Recorder {
        let mut parser = Parser::new();
        let mut rec = Recorder::default();
        parser.advance(&mut rec, input);
        rec
    }

    // ── TDD spec tests ──────────────────────────────────────────────────────

    #[test]
    fn test_parser_prints_ascii_characters() {
        let rec = parse(b"hello");
        assert_eq!(rec.printed, ['h', 'e', 'l', 'l', 'o']);
    }

    #[test]
    fn test_parser_executes_control_characters() {
        let rec = parse(b"\n\r");
        assert_eq!(rec.executed, [0x0A, 0x0D]);
    }

    #[test]
    fn test_parser_csi_cursor_up() {
        let rec = parse(b"\x1b[5A");
        assert_eq!(rec.csi.len(), 1);
        let (params, intermediates, action) = &rec.csi[0];
        assert_eq!(params.as_slice(), &[5]);
        assert!(intermediates.is_empty());
        assert_eq!(*action, b'A');
    }

    #[test]
    fn test_parser_csi_no_params() {
        let rec = parse(b"\x1b[A");
        assert_eq!(rec.csi.len(), 1);
        let (params, _, action) = &rec.csi[0];
        assert!(params.is_empty());
        assert_eq!(*action, b'A');
    }

    #[test]
    fn test_parser_csi_multiple_params() {
        let rec = parse(b"\x1b[10;20H");
        assert_eq!(rec.csi.len(), 1);
        let (params, _, action) = &rec.csi[0];
        assert_eq!(params.as_slice(), &[10, 20]);
        assert_eq!(*action, b'H');
    }

    #[test]
    fn test_parser_csi_with_intermediate() {
        let rec = parse(b"\x1b[?25h");
        assert_eq!(rec.csi.len(), 1);
        let (params, intermediates, action) = &rec.csi[0];
        assert_eq!(intermediates.as_slice(), b"?");
        assert_eq!(params.as_slice(), &[25]);
        assert_eq!(*action, b'h');
    }

    #[test]
    fn test_parser_esc_dispatch() {
        let rec = parse(b"\x1b7");
        assert_eq!(rec.esc.len(), 1);
        let (intermediates, action) = &rec.esc[0];
        assert!(intermediates.is_empty());
        assert_eq!(*action, b'7');
    }

    #[test]
    fn test_parser_osc_title_bel() {
        let rec = parse(b"\x1b]0;my title\x07");
        assert_eq!(rec.osc.len(), 1);
        let params = &rec.osc[0];
        assert_eq!(params.len(), 2);
        assert_eq!(params[0], b"0");
        assert_eq!(params[1], b"my title");
    }

    #[test]
    fn test_parser_osc_title_st() {
        let rec = parse(b"\x1b]2;title\x1b\\");
        assert_eq!(rec.osc.len(), 1);
        let params = &rec.osc[0];
        assert_eq!(params.len(), 2);
        assert_eq!(params[0], b"2");
        assert_eq!(params[1], b"title");
    }

    #[test]
    fn test_parser_utf8_characters() {
        let rec = parse("héllo 世界".as_bytes());
        let expected: Vec<char> = "héllo 世界".chars().collect();
        assert_eq!(rec.printed, expected);
    }

    #[test]
    fn test_parser_split_across_calls() {
        let mut parser = Parser::new();
        let mut rec = Recorder::default();
        parser.advance(&mut rec, b"\x1b[5");
        parser.advance(&mut rec, b"A");
        assert_eq!(rec.csi.len(), 1);
        let (params, _, action) = &rec.csi[0];
        assert_eq!(params.as_slice(), &[5]);
        assert_eq!(*action, b'A');
    }

    #[test]
    fn test_parser_malformed_csi_recovery() {
        // "\x1b[\x1b[A" — first CSI interrupted by ESC, second CSI completes.
        let rec = parse(b"\x1b[\x1b[A");
        // We should get exactly one CsiDispatch from the recovered "\x1b[A".
        assert_eq!(rec.csi.len(), 1);
        assert_eq!(rec.csi[0].2, b'A');
    }

    #[test]
    fn test_parser_csi_param_overflow() {
        let rec = parse(b"\x1b[99999A");
        assert_eq!(rec.csi.len(), 1);
        assert_eq!(rec.csi[0].0[0], u16::MAX);
    }

    #[test]
    fn test_parser_too_many_params() {
        // 18 parameters separated by semicolons.
        let input = b"\x1b[1;2;3;4;5;6;7;8;9;10;11;12;13;14;15;16;17;18H";
        let rec = parse(input);
        assert_eq!(rec.csi.len(), 1);
        // Only first MAX_PARAMS (16) are kept.
        assert_eq!(rec.csi[0].0.len(), crate::params::MAX_PARAMS);
    }

    #[test]
    fn test_parser_osc_buffer_cap() {
        // Feed an OSC string that is much longer than MAX_OSC_LEN.
        // The parser must not grow the buffer beyond MAX_OSC_LEN and must not
        // panic.
        let mut input = Vec::with_capacity(MAX_OSC_LEN * 2 + 8);
        input.extend_from_slice(b"\x1b]0;");
        // Fill with 'x' bytes well beyond the cap.
        input.resize(MAX_OSC_LEN * 2 + 4, b'x');
        input.push(0x07); // BEL terminator
        let rec = parse(&input);
        // The OSC must still be dispatched (capped, not dropped entirely).
        assert_eq!(rec.osc.len(), 1);
        // The second param (the title) must be capped at MAX_OSC_LEN.
        assert!(rec.osc[0][1].len() <= MAX_OSC_LEN);
    }

    #[test]
    fn test_parser_mixed_content() {
        let input = b"hello\x1b[31mworld\x1b[0m!";
        let rec = parse(input);
        // 5 "hello" + 5 "world" + 1 "!" = 11 printed chars
        assert_eq!(rec.printed.len(), 11);
        assert_eq!(rec.printed[0], 'h');
        assert_eq!(rec.printed[5], 'w');
        assert_eq!(rec.printed[10], '!');
        // Two CSI dispatches
        assert_eq!(rec.csi.len(), 2);
        assert_eq!(rec.csi[0].0, &[31]);
        assert_eq!(rec.csi[0].2, b'm');
        assert_eq!(rec.csi[1].0, &[0]);
        assert_eq!(rec.csi[1].2, b'm');
    }
}
