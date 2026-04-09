use criterion::{criterion_group, criterion_main, Criterion, Throughput};
use teamucks_vte::parser::{Parser, Performer};

struct NullPerformer;
impl Performer for NullPerformer {
    fn print(&mut self, _: char) {}
    fn execute(&mut self, _: u8) {}
    fn csi_dispatch(&mut self, _: &[u16], _: &[u8], _: u8) {}
    fn esc_dispatch(&mut self, _: &[u8], _: u8) {}
    fn osc_dispatch(&mut self, _: &[&[u8]]) {}
    fn dcs_dispatch(&mut self, _: &[u16], _: &[u8], _: u8, _: &[u8]) {}
}

fn generate_mixed_input(size: usize) -> Vec<u8> {
    let pattern = b"hello world\x1b[31;1mred bold\x1b[0m normal \x1b[38;2;255;128;0mrgb\x1b[0m\r\n";
    pattern.iter().copied().cycle().take(size).collect()
}

fn bench_parse_throughput(c: &mut Criterion) {
    let mut group = c.benchmark_group("parse_throughput");
    for size in [1_000_000, 10_000_000, 100_000_000] {
        let input = generate_mixed_input(size);
        group.throughput(Throughput::Bytes(size as u64));
        group.bench_with_input(criterion::BenchmarkId::new("mixed", size), &input, |b, input| {
            b.iter(|| {
                let mut parser = Parser::new();
                let mut performer = NullPerformer;
                parser.advance(&mut performer, input);
            });
        });
    }
    group.finish();
}

criterion_group!(benches, bench_parse_throughput);
criterion_main!(benches);
