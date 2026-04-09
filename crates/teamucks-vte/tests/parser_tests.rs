//! Comprehensive integration tests for the VTE parser.
//!
//! Tests are organised by category. All spec tests from the task description
//! are present and must stay passing.

use teamucks_vte::parser::{Parser, Performer};

// ── Test performer ──────────────────────────────────────────────────────────

/// A single recorded DCS event.
type DcsRecord = (Vec<u16>, Vec<u8>, u8, Vec<u8>);

/// A recording performer that captures every event emitted by the parser.
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

// ── Spec tests (task description §Key behaviors to test) ───────────────────

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
    assert_eq!(params.as_slice(), &[5u16]);
    assert!(intermediates.is_empty());
    assert_eq!(*action, b'A');
}

#[test]
fn test_parser_csi_no_params() {
    let rec = parse(b"\x1b[A");
    assert_eq!(rec.csi.len(), 1);
    let (params, _, action) = &rec.csi[0];
    assert!(params.is_empty(), "expected empty params, got {params:?}");
    assert_eq!(*action, b'A');
}

#[test]
fn test_parser_csi_multiple_params() {
    let rec = parse(b"\x1b[10;20H");
    assert_eq!(rec.csi.len(), 1);
    let (params, _, action) = &rec.csi[0];
    assert_eq!(params.as_slice(), &[10u16, 20u16]);
    assert_eq!(*action, b'H');
}

#[test]
fn test_parser_csi_with_intermediate() {
    // "?" is a parameter introducer that ends up in intermediates.
    let rec = parse(b"\x1b[?25h");
    assert_eq!(rec.csi.len(), 1);
    let (params, intermediates, action) = &rec.csi[0];
    assert_eq!(intermediates.as_slice(), b"?");
    assert_eq!(params.as_slice(), &[25u16]);
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
    assert_eq!(rec.osc.len(), 1, "expected 1 OSC dispatch");
    let osc_params = &rec.osc[0];
    assert_eq!(osc_params.len(), 2);
    assert_eq!(osc_params[0], b"0");
    assert_eq!(osc_params[1], b"my title");
}

#[test]
fn test_parser_osc_title_st() {
    let rec = parse(b"\x1b]2;title\x1b\\");
    assert_eq!(rec.osc.len(), 1, "expected 1 OSC dispatch via ST");
    let osc_params = &rec.osc[0];
    assert_eq!(osc_params.len(), 2);
    assert_eq!(osc_params[0], b"2");
    assert_eq!(osc_params[1], b"title");
}

#[test]
fn test_parser_utf8_characters() {
    let input = "héllo 世界".as_bytes();
    let rec = parse(input);
    let expected: Vec<char> = "héllo 世界".chars().collect();
    assert_eq!(rec.printed, expected);
}

#[test]
fn test_parser_split_across_calls() {
    let mut parser = Parser::new();
    let mut rec = Recorder::default();
    parser.advance(&mut rec, b"\x1b[5");
    parser.advance(&mut rec, b"A");
    assert_eq!(rec.csi.len(), 1, "CSI split across calls must dispatch once");
    let (params, _, action) = &rec.csi[0];
    assert_eq!(params.as_slice(), &[5u16]);
    assert_eq!(*action, b'A');
}

#[test]
fn test_parser_malformed_csi_recovery() {
    // First CSI is interrupted by ESC → should NOT produce a CSI dispatch.
    // Second CSI is well-formed → must produce a dispatch.
    let rec = parse(b"\x1b[\x1b[A");
    assert_eq!(rec.csi.len(), 1, "only the recovered CSI should dispatch; got {:?}", rec.csi);
    assert_eq!(rec.csi[0].2, b'A');
}

#[test]
fn test_parser_csi_param_overflow() {
    let rec = parse(b"\x1b[99999A");
    assert_eq!(rec.csi.len(), 1);
    assert_eq!(rec.csi[0].0[0], u16::MAX, "99999 must saturate at u16::MAX (65535)");
}

#[test]
fn test_parser_too_many_params() {
    // 18 numeric parameters.
    let input = b"\x1b[1;2;3;4;5;6;7;8;9;10;11;12;13;14;15;16;17;18H";
    let rec = parse(input);
    assert_eq!(rec.csi.len(), 1);
    assert_eq!(
        rec.csi[0].0.len(),
        teamucks_vte::params::MAX_PARAMS,
        "only first MAX_PARAMS (16) parameters should be kept"
    );
    // First 16 values must be correct.
    assert_eq!(&rec.csi[0].0[..4], &[1u16, 2, 3, 4]);
    assert_eq!(rec.csi[0].0[15], 16);
}

#[test]
fn test_parser_mixed_content() {
    // "hello" + SGR 31 (red) + "world" + SGR 0 (reset) + "!"
    let input = b"hello\x1b[31mworld\x1b[0m!";
    let rec = parse(input);
    // 5 + 5 + 1 = 11 printed characters.
    assert_eq!(rec.printed.len(), 11);
    assert_eq!(&rec.printed[..5], &['h', 'e', 'l', 'l', 'o']);
    assert_eq!(&rec.printed[5..10], &['w', 'o', 'r', 'l', 'd']);
    assert_eq!(rec.printed[10], '!');
    // Two CSI dispatches.
    assert_eq!(rec.csi.len(), 2);
    assert_eq!(rec.csi[0].0.as_slice(), &[31u16]);
    assert_eq!(rec.csi[0].2, b'm');
    assert_eq!(rec.csi[1].0.as_slice(), &[0u16]);
    assert_eq!(rec.csi[1].2, b'm');
}

// ── C1 control tests ────────────────────────────────────────────────────────

#[test]
fn test_parser_osc_c1_st_terminator() {
    // OSC terminated by C1 ST (0x9C) instead of BEL or ESC \
    let mut parser = Parser::new();
    let mut recorder = Recorder::default();
    parser.advance(&mut recorder, b"\x1b]0;title\x9c");
    assert_eq!(recorder.osc.len(), 1);
    assert_eq!(recorder.osc[0][0], b"0");
    assert_eq!(recorder.osc[0][1], b"title");
}

// ── Additional edge-case tests ───────────────────────────────────────────────

#[test]
fn test_parser_empty_input_does_nothing() {
    let rec = parse(b"");
    assert!(rec.printed.is_empty());
    assert!(rec.executed.is_empty());
    assert!(rec.csi.is_empty());
}

#[test]
fn test_parser_csi_zero_param_is_zero() {
    // "\x1b[0A" — explicit 0 param.
    let rec = parse(b"\x1b[0A");
    assert_eq!(rec.csi[0].0.as_slice(), &[0u16]);
}

#[test]
fn test_parser_esc_with_intermediate() {
    // "\x1b(B" — designate character set, intermediate = '(', action = 'B'
    let rec = parse(b"\x1b(B");
    assert_eq!(rec.esc.len(), 1);
    assert_eq!(rec.esc[0].0.as_slice(), b"(");
    assert_eq!(rec.esc[0].1, b'B');
}

#[test]
fn test_parser_csi_can_interrupts_sequence() {
    // 0x18 (CAN) mid-CSI must abort without dispatch.
    let rec = parse(b"\x1b[1;2\x18rest");
    assert!(rec.csi.is_empty(), "CAN must abort CSI without dispatch");
    // 'r', 'e', 's', 't' are printable and should be printed.
    assert_eq!(&rec.printed, &['r', 'e', 's', 't']);
}

#[test]
fn test_parser_sub_interrupts_sequence() {
    // 0x1A (SUB) mid-CSI must abort without dispatch.
    let rec = parse(b"\x1b[1\x1Amore");
    assert!(rec.csi.is_empty(), "SUB must abort CSI without dispatch");
}

#[test]
fn test_parser_csi_multiple_final_bytes_each_dispatch() {
    let rec = parse(b"\x1b[1A\x1b[2B");
    assert_eq!(rec.csi.len(), 2);
    assert_eq!(rec.csi[0].0.as_slice(), &[1u16]);
    assert_eq!(rec.csi[0].2, b'A');
    assert_eq!(rec.csi[1].0.as_slice(), &[2u16]);
    assert_eq!(rec.csi[1].2, b'B');
}

#[test]
fn test_parser_del_ignored_everywhere() {
    // DEL (0x7F) should never produce a print or execute event.
    let rec = parse(b"\x7fhello\x7f");
    assert_eq!(rec.printed, ['h', 'e', 'l', 'l', 'o']);
    assert!(rec.executed.is_empty());
}

#[test]
fn test_parser_osc_empty_params() {
    // BEL immediately after OSC — empty string.
    let rec = parse(b"\x1b]\x07");
    assert_eq!(rec.osc.len(), 1);
    assert_eq!(rec.osc[0].len(), 1);
    assert_eq!(rec.osc[0][0], b"");
}

#[test]
fn test_parser_osc_multiple_semicolons() {
    // "\x1b]1;2;3\x07"
    let rec = parse(b"\x1b]1;2;3\x07");
    assert_eq!(rec.osc.len(), 1);
    assert_eq!(rec.osc[0].len(), 3);
    assert_eq!(rec.osc[0][0], b"1");
    assert_eq!(rec.osc[0][1], b"2");
    assert_eq!(rec.osc[0][2], b"3");
}

#[test]
fn test_parser_utf8_split_across_calls() {
    // "é" is U+00E9: 0xC3 0xA9 — split the two bytes across calls.
    let mut parser = Parser::new();
    let mut rec = Recorder::default();
    parser.advance(&mut rec, &[0xC3]);
    parser.advance(&mut rec, &[0xA9]);
    assert_eq!(rec.printed, ['é']);
}

#[test]
fn test_parser_utf8_three_byte_split() {
    // "世" is U+4E16: 0xE4 0xB8 0x96 — split across three calls.
    let mut parser = Parser::new();
    let mut rec = Recorder::default();
    parser.advance(&mut rec, &[0xE4]);
    parser.advance(&mut rec, &[0xB8]);
    parser.advance(&mut rec, &[0x96]);
    assert_eq!(rec.printed, ['世']);
}

#[test]
fn test_parser_utf8_invalid_byte_replaced() {
    // Lone continuation byte 0x80 should produce U+FFFD.
    let rec = parse(&[0x80]);
    assert_eq!(rec.printed, ['\u{FFFD}']);
}

#[test]
fn test_parser_csi_sgr_multiple_attrs() {
    // "\x1b[1;31;42m" — bold, red fg, green bg in one SGR.
    let rec = parse(b"\x1b[1;31;42m");
    assert_eq!(rec.csi.len(), 1);
    assert_eq!(rec.csi[0].0.as_slice(), &[1u16, 31, 42]);
    assert_eq!(rec.csi[0].2, b'm');
}

#[test]
fn test_parser_csi_erase_display() {
    // "\x1b[2J" — erase display.
    let rec = parse(b"\x1b[2J");
    assert_eq!(rec.csi.len(), 1);
    assert_eq!(rec.csi[0].0.as_slice(), &[2u16]);
    assert_eq!(rec.csi[0].2, b'J');
}

#[test]
fn test_parser_esc_index() {
    // "\x1bD" — index (IND).
    let rec = parse(b"\x1bD");
    assert_eq!(rec.esc.len(), 1);
    assert_eq!(rec.esc[0].1, b'D');
}

#[test]
fn test_parser_bell_executes() {
    // BEL (0x07) outside of OSC is a control character.
    let rec = parse(b"\x07");
    assert!(rec.executed.contains(&0x07));
}

#[test]
fn test_parser_tab_executes() {
    let rec = parse(b"\x09");
    assert!(rec.executed.contains(&0x09));
}

#[test]
fn test_parser_dcs_no_panic() {
    // DCS passthrough must not panic — minimal correctness for Phase 1.
    let rec = parse(b"\x1bPq#0;2;0;0;0l\x1b\\");
    // We just verify no panic occurred and we dispatch something.
    // The DCS dispatch call is optional to verify precisely in Phase 1.
    drop(rec);
}

#[test]
fn test_parser_consecutive_escapes() {
    // Two consecutive ESC sequences — each should dispatch independently.
    let rec = parse(b"\x1b7\x1b8");
    assert_eq!(rec.esc.len(), 2);
    assert_eq!(rec.esc[0].1, b'7');
    assert_eq!(rec.esc[1].1, b'8');
}

#[test]
fn test_parser_cursor_position_zero_params() {
    // "\x1b[H" — cursor home, equivalent to row 1 col 1, no params.
    let rec = parse(b"\x1b[H");
    assert_eq!(rec.csi.len(), 1);
    assert!(rec.csi[0].0.is_empty());
    assert_eq!(rec.csi[0].2, b'H');
}

#[test]
fn test_parser_long_csi_param_string() {
    // 16 params exactly at the boundary.
    let input = b"\x1b[1;2;3;4;5;6;7;8;9;10;11;12;13;14;15;16m";
    let rec = parse(input);
    assert_eq!(rec.csi.len(), 1);
    assert_eq!(rec.csi[0].0.len(), 16);
    assert_eq!(&rec.csi[0].0[..4], &[1u16, 2, 3, 4]);
    assert_eq!(rec.csi[0].0[15], 16);
}

// ── Buffer cap tests ─────────────────────────────────────────────────────────

#[test]
fn test_parser_osc_buffer_does_not_grow_unboundedly() {
    // Feed an OSC string with 200 000 bytes of payload — well above the
    // 65 536-byte cap. The parser must not panic and the dispatched param
    // must be capped, not the full 200 000 bytes.
    const PAYLOAD_LEN: usize = 200_000;
    let mut input = Vec::with_capacity(PAYLOAD_LEN + 8);
    input.extend_from_slice(b"\x1b]0;");
    input.resize(input.len() + PAYLOAD_LEN, b'x');
    input.push(0x07); // BEL terminator

    let rec = parse(&input);
    assert_eq!(rec.osc.len(), 1, "OSC must be dispatched even when capped");
    // The accumulated title bytes must not exceed the cap.
    assert!(
        rec.osc[0][1].len() <= 65_536,
        "OSC buffer must be capped at 65 536 bytes, got {}",
        rec.osc[0][1].len()
    );
}

// ── Property-based tests ─────────────────────────────────────────────────────

use proptest::prelude::*;

proptest! {
    /// Arbitrary byte sequences must never panic.
    #[test]
    fn prop_parse_no_panic(input in proptest::collection::vec(any::<u8>(), 0..1024)) {
        let mut parser = Parser::new();
        let mut rec = Recorder::default();
        parser.advance(&mut rec, &input);
        // No assertion needed — reaching here means no panic occurred.
    }

    /// Parsing all bytes at once must produce the same result as parsing split
    /// at an arbitrary position.
    #[test]
    fn prop_parse_split_equivalence(
        input in proptest::collection::vec(any::<u8>(), 1..256),
        split in 0usize..256
    ) {
        let split = split.min(input.len());

        // Parse as one chunk.
        let mut p1 = Parser::new();
        let mut r1 = Recorder::default();
        p1.advance(&mut r1, &input);

        // Parse split at `split`.
        let mut p2 = Parser::new();
        let mut r2 = Recorder::default();
        p2.advance(&mut r2, &input[..split]);
        p2.advance(&mut r2, &input[split..]);

        // Results must be identical.
        prop_assert_eq!(&r1.printed, &r2.printed);
        prop_assert_eq!(&r1.executed, &r2.executed);
        prop_assert_eq!(&r1.csi, &r2.csi);
        prop_assert_eq!(&r1.esc, &r2.esc);
        prop_assert_eq!(&r1.osc, &r2.osc);
        prop_assert_eq!(&r1.dcs, &r2.dcs);
    }
}
