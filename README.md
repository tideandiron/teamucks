# teamucks

A terminal multiplexer for 2026+. Priorities: correctness > performance > AI-native > aesthetics.

## Status

**Phase 1 complete: VTE crate feature-complete with 457 tests.** The terminal emulator library now fully parses and renders terminal output with all Priority 1 escape sequences, scrollback, content reflow, and property tests. Next: multiplexer infrastructure (Phase 2).

## Architecture

The workspace consists of four Rust crates:

- **teamucks-vte** — Complete terminal emulator library. Parses terminal byte streams using a Paul Flo Williams state machine (UTF-8, CSI, ESC, OSC, DCS), renders to a grid with full SGR support (all attributes, 16/256/24-bit colors), handles all Priority 1 escape sequences (cursor, erase, scroll regions, modes, alternate screen, tabs, charsets), manages scrollback with configurable capacity, and reflows content on resize. 457 tests, property tests, fuzz targets. Zero unsafe code. Publishing to crates.io.
- **teamucks-proto** — Protobuf API definitions for the content inspection interface. Stubbed.
- **teamucks-core** — Domain model, server, layout engine, and rendering logic.
- **teamucks** — Single binary that embeds server and client.

## Vision

teamucks is opinionated. No tmux compatibility. The content inspection API (protobuf over unix socket) is a first-class interface, making it native to AI agents. The audience is software engineers who live in the terminal.

## Building

Requires Rust 1.75 or later.

```bash
cargo build --release
```

Run tests:

```bash
cargo test --workspace
```

Run benchmarks (VTE parser throughput):

```bash
cargo bench -p teamucks-vte
```

## License

Licensed under either of Apache License 2.0 or MIT at your option. See [LICENSE-APACHE](LICENSE-APACHE) or [LICENSE-MIT](LICENSE-MIT).
