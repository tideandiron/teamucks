# teamucks

A terminal multiplexer for 2026+. Priorities: correctness > performance > AI-native > aesthetics.

## Status

**Phase 1 complete: All 23 features merged. 905 tests.** The foundation is built: terminal emulator (teamucks-vte) with full VTE parsing and rendering, multiplexer infrastructure (teamucks-core) with PTY management, server daemon, layout engine, and client protocol. Components are feature-complete and well-tested. Integration into a running multiplexer binary is Phase 2.

## Architecture

The workspace consists of four Rust crates:

- **teamucks-vte** — Complete terminal emulator library. Paul Flo Williams state machine (UTF-8, CSI, ESC, OSC, DCS). Full SGR (all attributes, 16/256/24-bit colors). All Priority 1 escape sequences (cursor, erase, scroll regions, modes, alternate screen, tabs, charsets). Scrollback with reflow on resize. 457 tests, property tests, fuzz targets. Zero unsafe code.
- **teamucks-proto** — Protobuf API for content inspection over unix socket. Foundation for AI-native interfaces.
- **teamucks-core** — Multiplexer infrastructure: PTY management, server daemon, binary client protocol, async codec, pane entity with frame diff, input handling (prefix key state machine), tiled layout engine, border rendering, window/session model, status bar, mouse support, TOML configuration. 448 tests.
- **teamucks** — Single binary embedding server and client. Currently a placeholder; actual startup and session management is Phase 2.

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
