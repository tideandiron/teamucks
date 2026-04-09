# Terminal Multiplexer — Product & Feature Plan

## Vision

A modern, Rust-native terminal multiplexer that replaces tmux with a cleaner architecture, modern terminal support, and programmable extensibility. Ships as a single static binary with zero runtime dependencies. Designed from the ground up for contemporary terminal capabilities — inline images, GPU-accelerated renderers, AI-assisted development workflows — while matching tmux's reliability and muscle memory.

**One-liner:** The terminal multiplexer for the next decade of command-line work.

---

## Strategic Positioning

### Who this is for

**Primary audience:** Software engineers who live in the terminal and currently use tmux or screen. They value speed, stability, and keyboard-driven workflows. They're frustrated by tmux's configuration complexity, lack of modern terminal feature support, and the impossibility of extending it without patching C.

**Secondary audience:** Tool authors and AI coding assistants that need programmatic access to terminal content. Today there's no clean way for an external process to read what's on screen in a terminal pane, inspect command output, or react to terminal state. This multiplexer exposes that capability as a first-class API.

**Tertiary audience:** Teams doing pair programming, remote debugging, or live demos who want real-time collaborative terminal sessions without third-party screen-sharing tools.

### Competitive landscape

**tmux** — The incumbent. Rock-solid, universally available, deeply integrated into developer workflows. Weaknesses: Go template-style configuration is arcane, no plugin system, strips modern terminal features (inline images, Kitty keyboard protocol), server architecture is opaque, development pace is slow.

**Zellij** — The Rust challenger. Good UI, WASM plugin system, floating panes. Weaknesses: opinionated layout system that doesn't match tmux muscle memory, WASM plugin overhead, younger and less battle-tested, limited adoption in production infrastructure.

**screen** — Legacy. Still used on servers where tmux isn't installed. Not a real competitor for new adoption.

### Differentiation thesis

We don't win by being "tmux but in Rust." We win by doing three things tmux architecturally cannot:

1. **Programmable terminal** — Expose pane contents and command lifecycle as structured, streaming data via an API. Enable an ecosystem of tools that build on top of the multiplexer.

2. **Modern terminal passthrough** — Don't strip inline images, Kitty keyboard protocol, synchronized output, or any other modern escape sequences. Multiplex them faithfully.

3. **Zero-friction migration** — tmux-compatible keybinding mode as the default. Existing tmux users switch without relearning anything. The new capabilities are additive.

---

## Core Subsystems

### 1. Virtual Terminal Emulator (VTE)

The VTE is the foundation. Every pane runs an independent VTE instance that interprets the byte stream from the child process and maintains a grid of styled cells.

#### Scope

The VTE must correctly handle the escape sequences emitted by all common terminal applications: shells (bash, zsh, fish), editors (vim, neovim, emacs, nano, helix), system monitors (htop, btop, top), file managers (ranger, lf, yazi), TUI frameworks (ratatui, blessed, curses), and pagers (less, bat, delta).

#### Cell model

Each cell in the grid stores a grapheme cluster (supporting multi-codepoint characters like emoji, CJK, combining diacritics) and a packed style descriptor covering foreground color, background color, and attributes. Colors support the full range: default, named 16-color palette, 256-color indexed, and 24-bit RGB. Attributes include bold, dim, italic, underline (including curly, dashed, dotted variants), strikethrough, blink, inverse, hidden, and hyperlink (OSC 8).

Wide characters (CJK, some emoji) occupy two cells. The second cell is a continuation marker. The VTE must handle cursor movement, insertion, and deletion correctly around wide characters.

#### Escape sequence coverage

**Priority 1 — must work at launch:**

- Cursor movement: absolute (CUP), relative (CUU/CUD/CUF/CUB), save/restore (DECSC/DECRC), home
- Erasing: erase in display (ED), erase in line (EL), erase characters (ECH)
- Scrolling: scroll region (DECSTBM), scroll up/down (SU/SD), index/reverse index
- Character attributes: SGR (all standard attributes, 256-color, 24-bit color)
- Mode setting: origin mode (DECOM), auto-wrap (DECAWM), cursor visibility (DECTCEM), application cursor keys (DECCKM), bracketed paste (mode 2004), focus events (mode 1004)
- Alternate screen buffer: DECSET/DECRST 1049
- Tab stops: HTS, TBC, CHT
- Character sets: G0/G1 designate and invoke (for line-drawing characters)
- Window title: OSC 0, OSC 2
- Mouse reporting: modes 1000, 1002, 1003, 1006 (SGR format)

**Priority 2 — needed for full compatibility:**

- OSC 52 (clipboard access)
- OSC 7 (current working directory reporting)
- OSC 133 (shell integration / command markers)
- DECSET/DECRST 2026 (synchronized output)
- Kitty keyboard protocol (progressive enhancement modes)
- Sixel graphics and Kitty graphics protocol (inline images)
- OSC 8 (hyperlinks)

**Priority 3 — nice to have:**

- DECRQM (request mode) and DA (device attributes) responses
- DECSCA (character protection)
- DECFI/DECBI (forward/backward index)

#### Scrollback management

Each pane maintains a scrollback buffer of configurable depth (default: 10,000 lines). Lines that scroll off the top of the visible area are appended to the scrollback buffer.

Scrollback is stored in compressed chunks to control memory usage. When a chunk reaches a threshold size (e.g., 4KB of raw cell data), it is compressed using a simple run-length encoding scheme over the style data (adjacent cells with identical styles are common). Chunks are decompressed on demand when the user scrolls or searches.

Soft-wrap tracking: each row stores a flag indicating whether it was soft-wrapped (line exceeded pane width) or hard-wrapped (explicit newline). This enables correct reflow on pane resize — soft-wrapped lines are joined and re-wrapped to the new width, while hard-wrapped lines remain separate.

#### Reflow on resize

When a pane's dimensions change, the VTE must reflow content. The algorithm:

1. Walk the visible grid and scrollback, joining soft-wrapped consecutive rows into logical lines.
2. Re-wrap each logical line to the new width, setting soft-wrap flags as needed.
3. Adjust the cursor position to its correct location in the reflowed grid.
4. Send SIGWINCH to the child process with the new dimensions.

Edge cases to handle: reflow with wide characters that don't fit at the end of a narrower line (they must wrap entirely), reflow of content containing tab characters (tab stops must be recalculated), and reflow while the alternate screen buffer is active (the alternate screen should not be reflowed — it's typically a full-screen application that will redraw).

#### Performance targets

- Parse and apply escape sequences at a sustained rate of at least 500 MB/s on a single core.
- Grid updates for a full 200×50 screen redraw must complete in under 1ms.
- Memory usage per pane: under 1MB for 10,000 lines of scrollback.

---

### 2. Layout Engine

The layout engine manages the spatial arrangement of panes within a window. It supports two classes of panes: tiled panes organized in a tree structure, and floating panes with absolute positioning and z-ordering.

#### Tiled layout model

The tiling layout is a binary tree. Each internal node represents a split (horizontal or vertical) with a ratio that determines how space is divided between its two children. Each leaf node is a pane.

Operations on the tree:

- **Split:** Replace a leaf with an internal node whose children are the original pane and a new pane. The user chooses horizontal or vertical split and the initial ratio (default 0.5).
- **Close:** Remove a leaf. Its sibling is promoted to replace the parent internal node, reclaiming the space.
- **Resize:** Adjust the ratio of an internal node. Propagate the new dimensions to all descendant panes.
- **Swap:** Exchange two leaf nodes in the tree. Their positions in the layout switch.
- **Rotate:** Transform a horizontal split into a vertical split or vice versa, recursively through a subtree.
- **Zoom:** Temporarily expand a single pane to fill the entire window. The tree structure is preserved; other panes are simply not rendered. Unzoom restores the previous layout.

Minimum pane dimensions are enforced (e.g., 5 columns × 2 rows). Splits that would create panes below the minimum are rejected.

#### Floating panes

Floating panes exist outside the tiling tree. Each has an absolute position (x, y), dimensions (width, height), and a z-index. They are rendered on top of the tiled layout.

Operations: create, move (arrow keys or mouse drag), resize (from edges/corners), minimize (collapse to a title bar), pin (prevent accidental movement), cycle z-order.

Use cases: a persistent log viewer, a quick scratchpad terminal, a command palette overlay, a picture-in-picture pane showing a running process while working in another.

#### Borders and status

Pane borders are drawn using Unicode box-drawing characters. The active pane's border is highlighted with a distinct color. Borders between adjacent tiled panes are shared (single line, not double) to avoid wasting space.

Each pane optionally displays a title bar showing: pane index, running command name (derived from the child process), current working directory (from OSC 7 if available), and user-defined title.

A global status bar (top or bottom, configurable) shows: session name, window list with indicators for the active window and windows with unseen activity, system clock, and user-defined status segments (similar to tmux's status-left and status-right).

#### Layout presets and persistence

Built-in layout presets for common workflows: IDE layout (large editor pane + terminal below + file tree left), monitoring layout (grid of equal panes), presentation layout (one large pane + small panes on the side for notes and timer).

Layouts are serializable. The user can save the current layout to a named preset and restore it later. Serialization format is a simple human-readable text format (not TOML/YAML — the layout tree serializes naturally as nested s-expressions or a compact custom syntax).

---

### 3. Server Protocol & Client Architecture

The multiplexer uses a server/client architecture. The server owns all sessions, windows, panes, and child processes. Clients connect to render the display and send input.

#### Server lifecycle

The server starts on first use (first session creation) and runs as a background daemon. It persists after all clients disconnect. It exits when the last session is closed, or on explicit shutdown command.

The server listens on a Unix domain socket at a well-known path (e.g., `$XDG_RUNTIME_DIR/tmx/default.sock` or `$TMPDIR/tmx-$UID/default.sock`). Multiple named servers are supported (analogous to tmux's `-L` flag).

The server manages: session lifecycle (create, rename, kill), window lifecycle within sessions, pane lifecycle within windows, the PTY file descriptors for all child processes, all VTE parser instances, the layout tree, and the scrollback buffers.

#### Client connection flow

1. Client opens a Unix domain socket connection to the server.
2. Client sends a handshake message: protocol version, requested session (attach to existing or create new), client terminal dimensions, and terminal capabilities (color depth, Kitty keyboard support, image protocol support).
3. Server responds with session state: window list, active window, layout tree, and an initial full frame for the visible area.
4. Server streams frame updates to the client. Client streams input events to the server.
5. On disconnect, the server marks the client as detached. Sessions continue running.

#### Frame update protocol

The server maintains a "last sent frame" per client. On each render tick (triggered by PTY output or user action), the server computes a diff between the current frame and the last sent frame.

Update types:

- **CellDiff:** A specific cell changed. Includes position, new grapheme, and new style.
- **LineDiff:** An entire line changed. More efficient than individual cell diffs when many cells on a line update.
- **RegionScroll:** A rectangular region scrolled by N lines. The client can use terminal scroll commands rather than redrawing.
- **CursorUpdate:** Cursor position, shape, or visibility changed.
- **LayoutChange:** The pane layout changed (split, close, resize). Client must redraw borders and recompute pane positions.
- **StatusUpdate:** Status bar content changed.
- **Bell:** A pane rang the bell.
- **TitleChange:** A pane's title changed (OSC 0/2 from child process).

The protocol is binary, compact, and designed for low overhead. Messages are length-prefixed and use fixed-width integer encodings. No serialization framework — the message format is simple enough to hand-code.

Batching: the server coalesces updates within a render tick into a single batch message. If a pane produces output faster than the client can render (e.g., `cat /dev/urandom`), the server drops intermediate frames and sends only the latest state. This prevents client buffer bloat.

#### Input protocol

The client sends input events to the server:

- **KeyEvent:** Key press with modifiers. Uses the Kitty keyboard protocol representation internally (unambiguous key identification).
- **MouseEvent:** Button press/release/move/scroll with position and modifiers.
- **Resize:** Client terminal dimensions changed.
- **Command:** A multiplexer command (e.g., split pane, switch window, enter copy mode). These bypass the active pane and are handled by the server directly.
- **PasteEvent:** Bracketed paste content.

The server routes key and mouse events to the active pane's PTY (translating back to the byte sequences the child process expects). Commands are handled by the server's command dispatcher.

#### Multi-client support

Multiple clients can attach to the same session simultaneously. Each client has independent terminal dimensions. The server handles this by using the smallest client dimensions for the session's panes (matching tmux's behavior), or optionally by allowing each client to have an independent viewport (crop/scroll over a larger virtual canvas).

Client-specific state: cursor position display (each client sees the active pane's cursor, but collaborative mode shows all clients' cursors), notification preferences, and keybinding overrides.

---

### 4. Content Inspection API

A Unix domain socket API that allows external processes to programmatically read pane contents, subscribe to changes, and query command history.

#### API access

The API is served on a secondary socket (e.g., `$XDG_RUNTIME_DIR/tmx/default-api.sock`) or as a subprotocol on the main socket. Communication uses newline-delimited JSON for simplicity and broad language compatibility.

#### Endpoints

**Pane content:**

- `get_pane_content(pane_id, options)` — Returns the current visible grid as structured data. Options: include_scrollback (bool, default false), scrollback_lines (number of history lines to include), strip_formatting (bool, return plain text vs styled text).

- `get_pane_text(pane_id, region)` — Returns plain text from a rectangular region of a pane. Useful for OCR-like extraction of specific screen areas.

**Subscriptions:**

- `subscribe_pane_output(pane_id)` — Stream of raw bytes written by the child process to the PTY. Allows an external tool to see exactly what the shell is producing.

- `subscribe_pane_content(pane_id, options)` — Stream of structured frame diffs. External tool sees what changed on screen without polling.

- `subscribe_commands(pane_id)` — Stream of command lifecycle events (command started, command finished with exit code and duration). Requires OSC 133 shell integration or prompt detection heuristics.

**Command history:**

- `get_command_history(pane_id, limit)` — Returns recent commands with their start time, end time, exit code, duration, and optionally their output. This is built from OSC 133 markers or prompt detection.

**Session/layout queries:**

- `list_sessions()` — All sessions with their window and pane structure.
- `get_layout(session_id, window_id)` — The layout tree for a window as structured data.

**Actions:**

- `send_keys(pane_id, keys)` — Inject input into a pane as if the user typed it.
- `run_command(pane_id, command)` — Send a command string followed by Enter. Optionally wait for completion and return the output (requires shell integration).
- `create_pane(window_id, options)` — Programmatically create a pane with specified command, dimensions, and position.
- `capture_pane(pane_id, format)` — Capture pane content as plain text, ANSI-formatted text, HTML, or SVG.

#### Security model

The API socket has the same filesystem permissions as the main socket (user-only by default). An optional token-based auth scheme allows finer-grained access control: generate a read-only token for monitoring tools, a full-access token for automation tools.

Rate limiting prevents runaway subscribers from degrading multiplexer performance.

---

## Feature Roadmap

### Phase 1 — Functional Multiplexer (Months 1–3)

Goal: A working terminal multiplexer that can replace tmux for basic daily use.

**Milestone 1.1 — Single pane (Month 1)**

- VTE parser handling Priority 1 escape sequences
- PTY management (fork, exec, SIGWINCH, SIGCHLD)
- Grid rendering to the host terminal
- Input handling (legacy and Kitty keyboard protocol detection)
- Scrollback buffer with basic scroll-up/scroll-down
- Correct reflow on terminal resize

**Milestone 1.2 — Splits and navigation (Month 2)**

- Tiled layout engine with horizontal and vertical splits
- Pane navigation (directional movement between panes)
- Pane resize (keyboard-driven)
- Pane close with space reclamation
- Pane zoom (toggle fullscreen for active pane)
- Border rendering with active pane highlighting
- Basic status bar (session name, pane index)

**Milestone 1.3 — Server/client and sessions (Month 3)**

- Server daemon with Unix domain socket
- Client attach/detach
- Multiple sessions and windows
- Session persistence across client disconnects
- Window creation, navigation, and closing
- tmux-compatible default keybindings (Ctrl-B prefix)
- Basic configuration file (keybindings, colors, status bar format)

**Exit criteria:** Daily-drivable for the development team. All standard CLI applications (vim, htop, less, bash, zsh, fish) work correctly. Attach/detach is reliable.

### Phase 2 — Competitive Parity (Months 4–6)

Goal: Feature-complete enough that switching from tmux involves no regression for common workflows.

**Milestone 2.1 — Copy mode and search (Month 4)**

- Copy mode with vi-style navigation through scrollback
- Visual selection (character, line, block modes)
- Regex search through scrollback with match highlighting
- Search-as-you-type with incremental results
- Copy to system clipboard (OSC 52)
- Multi-pane search (search across all panes simultaneously)

**Milestone 2.2 — Configuration and customization (Month 5)**

- Configuration file format (TOML) with live reload
- Keybinding customization with full key sequence support
- Color theme system with built-in themes (matching popular terminal themes)
- Status bar customization (segments, format strings, colors)
- Per-pane and per-window options (scrollback depth, title format)
- Mouse support (click to focus pane, drag to resize splits, scroll)

**Milestone 2.3 — Robustness and polish (Month 6)**

- Comprehensive VTE test suite (vttest, esctest, application-specific tests)
- Priority 2 escape sequence support (OSC 52, OSC 7, OSC 133, synchronized output)
- Performance optimization (target: no perceptible lag even under heavy output like `find /` or compilation scrollback)
- Graceful degradation on minimal terminals (detect capabilities, fall back)
- Man page and documentation
- Packaging (Homebrew, Cargo install, static Linux binary, AUR)

**Exit criteria:** A tmux user can switch with minimal keybinding adjustment. No correctness regressions across the standard application test matrix.

### Phase 3 — Differentiation (Months 7–10)

Goal: Features that tmux cannot do and that justify switching.

**Milestone 3.1 — Content Inspection API (Month 7)**

- API socket with JSON protocol
- Pane content read (visible grid and scrollback)
- Pane output subscription (streaming raw bytes)
- Command lifecycle events (via OSC 133 shell integration)
- send_keys and run_command actions
- Python and TypeScript client libraries (thin wrappers over the JSON protocol)

**Milestone 3.2 — Smart pane features (Month 8)**

- Command detection (OSC 133 and heuristic prompt detection)
- Command timestamping (start, end, duration, exit code)
- Command output folding (collapse finished command output to summary line, expand on demand)
- Command-level scrollback navigation (jump between commands, not just lines)
- Command output capture (select and copy the output of a specific command)

**Milestone 3.3 — Floating panes and modern terminal support (Month 9)**

- Floating pane creation, movement, resize, z-ordering
- Floating pane minimize/restore
- Inline image passthrough (Kitty graphics protocol, Sixel)
- Kitty keyboard protocol passthrough
- Hyperlink passthrough and click handling (OSC 8)

**Milestone 3.4 — Session persistence (Month 10)**

- Serialize session state to disk (layout, scrollback, command list per pane)
- Restore sessions on server restart (recreate layout, relaunch commands, restore scrollback)
- Named layout presets (save, load, list)
- Startup configuration (auto-create sessions with predefined layouts and commands)

**Exit criteria:** The content inspection API is stable and documented. At least one external tool (an AI coding assistant integration or a monitoring dashboard) demonstrates the API's value. Floating panes and inline images work reliably.

### Phase 4 — Ecosystem (Months 11+)

Goal: Build the ecosystem that creates lock-in and community.

**Collaborative sessions** — Multiple users on the same session with distinct cursors, independent or shared viewports, and access control (read-only spectators vs full participants).

**Web client** — A browser-based client that connects to the server via WebSocket. Renders panes to a canvas or DOM. Enables remote access without SSH, demo sharing, and documentation with embedded live terminals.

**Plugin system** — Define hooks for events (pane created, command completed, output matched regex). Plugins are separate processes that communicate over the content inspection API. No in-process plugin execution — plugins crash independently. Provide a plugin registry and package manager.

**IDE integration** — VS Code and JetBrains extensions that embed multiplexer panes in the IDE's terminal panel, with full API access for the IDE to inspect pane contents.

**Recording and playback** — Record a terminal session as a structured event log. Play it back at any speed. Export to animated SVG, GIF, or asciicast format. Useful for documentation, bug reports, and tutorials.

---

## Configuration Philosophy

Configuration should be minimal for most users and powerful for those who need it. The default keybindings match tmux (Ctrl-B prefix). The default theme adapts to the terminal's color scheme. The default status bar shows useful information without clutter.

Configuration is a single TOML file. No scripting language in the config — conditional logic and dynamic behavior belong in plugins or the API, not in configuration. Every configuration option has a sensible default documented inline.

Example configuration surface:

- Keybinding declarations (prefix key, key sequences mapped to actions)
- Color theme (named colors for borders, status bar, active/inactive pane indicators)
- Status bar layout (left, center, right segments with format strings)
- Default pane options (scrollback depth, shell command, working directory)
- Session startup recipes (named layouts with predefined pane commands)
- Mouse behavior toggles
- API access settings (socket path, auth tokens)

---

## Success Metrics

### Adoption milestones

- **Month 3:** 10 daily active dogfood users (the development team and close collaborators)
- **Month 6:** First public release. 500 GitHub stars. 100 unique installs via Homebrew/Cargo.
- **Month 10:** 2,000 GitHub stars. First third-party tool built on the content inspection API. First conference talk or blog post by an external user.
- **Month 12:** 5,000 GitHub stars. Included in at least one popular dotfiles repository or "awesome" list.

### Quality metrics

- Zero crash bugs in the VTE parser (fuzz-tested against random input)
- 100% pass rate on the standard VTE test suites (vttest, esctest)
- Application compatibility matrix: vim, neovim, emacs, htop, btop, less, man, bat, delta, lazygit, fish, zsh, bash, tmux-inside-multiplexer (nested), SSH sessions all work correctly
- Attach latency under 50ms on a session with 20 panes
- Memory usage under 50MB for a session with 10 panes and 10,000 lines of scrollback each
- No frame drops or input lag under sustained output of 100MB/s from a single pane

### Ecosystem metrics

- At least 3 community-built tools using the content inspection API by month 12
- At least 1 AI coding assistant integration (Copilot, Claude, Cursor, or similar) using the API by month 12
- Plugin registry with at least 10 published plugins by month 18

---

## Risks and Mitigations

**Risk: tmux muscle memory is too strong.** Users won't switch if any common workflow breaks. Mitigation: tmux-compatible keybindings as default, and an explicit compatibility testing matrix that covers the most common tmux operations. Publish a migration guide.

**Risk: VTE correctness is a bottomless pit.** The xterm spec is vast and full of edge cases. Applications rely on undocumented behaviors. Mitigation: Prioritize ruthlessly. Test against real applications, not the spec. The Phase 1 escape sequence list covers 99% of real-world usage. Add long-tail sequences based on user bug reports, not preemptive implementation.

**Risk: Server/client architecture adds complexity and latency.** Mitigation: Benchmark frame diff encoding and transmission. The protocol must add less than 1ms of latency over direct terminal rendering. If the protocol is the bottleneck, fall back to shared-memory IPC between co-located server and client.

**Risk: Content inspection API is a solution in search of a problem.** Mitigation: Build at least one compelling integration ourselves (AI assistant terminal awareness) before marketing the API as a feature. If the integration isn't useful, the API isn't the right abstraction.

**Risk: Scope creep.** A terminal multiplexer touches everything. Mitigation: The phased roadmap is designed so each phase produces a shippable, useful product. Phase 1 is a usable multiplexer. Phase 2 is a tmux replacement. Phase 3 is the differentiated product. Phase 4 is ecosystem investment. Each phase can be the stopping point if resources or interest shift.

---

## Open Questions

1. **Naming.** The name matters more than it should. It needs to be short, memorable, typeable as a command, and not already taken on crates.io.

2. **Scripting vs API.** tmux's `send-keys`, `split-window`, etc. are powerful for shell scripting. Our API is JSON over Unix sockets — more structured but less convenient from a shell script. Do we also expose a CLI subcommand interface (`tmx send-keys -t pane1 "ls"`) that wraps the API?

3. **Nested multiplexer behavior.** Running this multiplexer inside itself, or inside tmux, or tmux inside it. How do we handle prefix key passthrough? tmux uses `send-prefix` — do we adopt the same convention?

4. **Sixel vs Kitty graphics.** Both are in use. Supporting both is significant work. Do we pick one to support first, and if so, which? Kitty protocol is more capable but Sixel has broader existing support.

5. **License.** MIT maximizes adoption. GPLv3 prevents proprietary forks but limits embedding. Apache-2.0 with patent grant is the conservative choice for a Rust project. Decision impacts ecosystem willingness to build on the API.
