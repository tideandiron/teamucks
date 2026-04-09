# teamucks

A terminal multiplexer for 2026+. Priorities: correctness > performance > AI-native > aesthetics.

## Status

**Early development.** Feature 1 (Workspace Scaffold) and Feature 2 (VTE Parser Core) have shipped. The binary compiles and accepts commands. The VTE parser implements a complete state machine for terminal event parsing, but multiplexing and the server are not yet implemented. This is not yet usable as a terminal application.

## Architecture

The workspace consists of four Rust crates:

- **teamucks-vte** — Virtual terminal emulator library. Implements a Paul Flo Williams state machine for VTE parsing (UTF-8, CSI, ESC, OSC, DCS). Zero unsafe code. Includes property tests and fuzzing. Will publish to crates.io.
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
