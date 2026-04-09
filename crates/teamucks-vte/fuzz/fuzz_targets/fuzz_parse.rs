#![no_main]

use libfuzzer_sys::fuzz_target;
use teamucks_vte::parser::{Parser, Performer};

/// A no-op performer — we just want the parser to not panic.
struct SinkPerformer;

impl Performer for SinkPerformer {
    fn print(&mut self, _c: char) {}
    fn execute(&mut self, _byte: u8) {}
    fn csi_dispatch(&mut self, _params: &[u16], _intermediates: &[u8], _action: u8) {}
    fn esc_dispatch(&mut self, _intermediates: &[u8], _action: u8) {}
    fn osc_dispatch(&mut self, _params: &[&[u8]]) {}
    fn dcs_dispatch(
        &mut self,
        _params: &[u16],
        _intermediates: &[u8],
        _action: u8,
        _data: &[u8],
    ) {
    }
}

fuzz_target!(|data: &[u8]| {
    let mut parser = Parser::new();
    let mut performer = SinkPerformer;
    parser.advance(&mut performer, data);
});
