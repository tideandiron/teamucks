# teamucks

A terminal multiplexer for 2026+. Priorities: correctness > performance > AI-native > aesthetics.

## Status

**Early development.** Features 1–3 have shipped. Feature 1 (Workspace Scaffold), Feature 2 (VTE Parser Core), and Feature 3 (Cell Model & Grid) are complete. The VTE crate now has a full data model for terminal rendering (packed styles, grapheme storage, cells, rows, and grid with cursor management). The binary compiles and accepts commands. Multiplexing and the server are not yet implemented. This is not yet usable as a terminal application.

## Architecture

The workspace consists of four Rust crates:

- **teamucks-vte** — Virtual terminal emulator library. Implements a Paul Flo Williams state machine for VTE parsing (UTF-8, CSI, ESC, OSC, DCS) and a complete data model for terminal rendering (grid, cells, rows, styles, grapheme storage). Includes 142 tests across the crate. Zero unsafe code. Includes property tests and fuzzing. Will publish to crates.io.
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
