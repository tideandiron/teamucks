/// Binary client protocol for teamucks.
///
/// All messages are length-prefixed: a `u32` LE byte count followed by the
/// payload. The first byte of every payload is a `u8` message-type
/// discriminant.
///
/// # Wire format
///
/// ```text
/// ┌──────────────┬──────────────────────────────────────┐
/// │  len: u32 LE │  payload (discriminant + fields …)   │
/// └──────────────┴──────────────────────────────────────┘
/// ```
///
/// Strings are encoded as a `u16 LE` byte count followed by UTF-8 bytes.
/// `Vec` fields are encoded as a `u16 LE` element count followed by elements.
/// Enum variants are encoded as a `u8` discriminant.
pub mod codec;
pub mod decode;
pub mod encode;

// ---------------------------------------------------------------------------
// Protocol constants
// ---------------------------------------------------------------------------

/// Negotiated protocol version sent in the handshake.
pub const PROTOCOL_VERSION: u16 = 1;

/// Maximum permitted message size (16 MiB).
pub const MAX_MESSAGE_SIZE: u32 = 16 * 1024 * 1024;

// ---------------------------------------------------------------------------
// Error type
// ---------------------------------------------------------------------------

/// Errors produced by the binary protocol encoder/decoder.
#[derive(Debug, thiserror::Error)]
pub enum ProtocolError {
    /// Underlying I/O failure.
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    /// The discriminant byte does not correspond to any known message type.
    #[error("unknown message type: {0}")]
    UnknownMessageType(u8),

    /// A string field contained invalid UTF-8.
    #[error("invalid UTF-8 in message")]
    InvalidUtf8,

    /// The length prefix exceeds [`MAX_MESSAGE_SIZE`].
    #[error("message too large: {size} bytes (max {max})")]
    MessageTooLarge {
        /// Observed message size in bytes.
        size: u32,
        /// Maximum permitted message size in bytes.
        max: u32,
    },

    /// The payload ended before all fields were decoded.
    #[error("unexpected end of message")]
    Truncated,

    /// The client and server speak incompatible protocol versions.
    #[error("protocol version mismatch: client={client}, server={server}")]
    VersionMismatch {
        /// Version advertised by the client.
        client: u16,
        /// Version supported by the server.
        server: u16,
    },
}

// ---------------------------------------------------------------------------
// Message types
// ---------------------------------------------------------------------------

/// Messages sent from the server to a visual client.
#[derive(Debug, Clone, PartialEq)]
pub enum ServerMessage {
    /// Handshake reply carrying the negotiated protocol version.
    HandshakeResponse {
        /// Negotiated protocol version (always [`PROTOCOL_VERSION`] for now).
        protocol_version: u16,
        /// Human-readable server identifier.
        server_name: String,
    },

    /// A complete frame for a pane, replacing all previous cell data.
    FullFrame {
        /// Pane this frame belongs to.
        pane_id: u32,
        /// Number of columns.
        cols: u16,
        /// Number of rows.
        rows: u16,
        /// All cells, in row-major order (`row * cols + col`).
        cells: Vec<CellData>,
    },

    /// A partial frame update containing only changed cells/regions.
    FrameDiff {
        /// Pane this diff belongs to.
        pane_id: u32,
        /// List of individual diff entries.
        diffs: Vec<DiffEntry>,
    },

    /// Cursor position and shape update.
    CursorUpdate {
        /// Pane that owns this cursor.
        pane_id: u32,
        /// Zero-based column position.
        col: u16,
        /// Zero-based row position.
        row: u16,
        /// Whether the cursor is visible.
        visible: bool,
        /// Cursor shape.
        shape: CursorShape,
    },

    /// Placeholder for Feature 18 layout notifications.
    LayoutChange,

    /// Status bar content update.
    StatusUpdate {
        /// New status bar content.
        content: String,
    },

    /// Terminal bell in the specified pane.
    Bell {
        /// Pane that rang the bell.
        pane_id: u32,
    },

    /// Window title changed for a pane.
    TitleChange {
        /// Pane whose title changed.
        pane_id: u32,
        /// New title.
        title: String,
    },
}

/// Messages sent from a visual client to the server.
#[derive(Debug, Clone, PartialEq)]
pub enum ClientMessage {
    /// Initial handshake carrying the client's protocol version and dimensions.
    HandshakeRequest {
        /// Protocol version the client speaks.
        protocol_version: u16,
        /// Client terminal width in columns.
        cols: u16,
        /// Client terminal height in rows.
        rows: u16,
    },

    /// A raw key press event.
    KeyEvent {
        /// Raw key bytes (UTF-8 sequences, CSI sequences, etc.).
        key: Vec<u8>,
        /// Modifier bitmask (shift=1, alt=2, ctrl=4, meta=8).
        modifiers: u8,
    },

    /// A mouse button event.
    MouseEvent {
        /// Button identifier.
        button: u8,
        /// Zero-based column.
        col: u16,
        /// Zero-based row.
        row: u16,
        /// Modifier bitmask.
        modifiers: u8,
    },

    /// Terminal resize notification.
    Resize {
        /// New width in columns.
        cols: u16,
        /// New height in rows.
        rows: u16,
    },

    /// Named command (e.g., `"split-vertical"`, `"close-pane"`).
    Command {
        /// Command name.
        name: String,
    },

    /// Paste event carrying arbitrary text.
    PasteEvent {
        /// Pasted text.
        data: String,
    },
}

// ---------------------------------------------------------------------------
// Supporting types
// ---------------------------------------------------------------------------

/// A single terminal cell's content and style.
#[derive(Debug, Clone, PartialEq)]
pub struct CellData {
    /// Rendered grapheme cluster (UTF-8).
    pub grapheme: String,
    /// Foreground colour.
    pub fg: ColorData,
    /// Background colour.
    pub bg: ColorData,
    /// Packed attribute bits (bold, italic, underline, etc.).
    pub attrs: u16,
    /// Cell flags: bit 0 = wide character, bit 1 = wide continuation.
    pub flags: u8,
}

/// A single entry in a [`ServerMessage::FrameDiff`].
#[derive(Debug, Clone, PartialEq)]
pub enum DiffEntry {
    /// A single changed cell.
    CellChange {
        /// Zero-based column.
        col: u16,
        /// Zero-based row.
        row: u16,
        /// New cell data.
        cell: CellData,
    },

    /// A complete row replacement.
    LineChange {
        /// Zero-based row.
        row: u16,
        /// New cell data for the entire row (length == `cols`).
        cells: Vec<CellData>,
    },

    /// A scroll operation within a region.
    ///
    /// Positive `count` scrolls the region upward (content moves up).
    /// Negative `count` scrolls downward (content moves down).
    RegionScroll {
        /// First row of the scroll region (inclusive).
        top: u16,
        /// Last row of the scroll region (inclusive).
        bottom: u16,
        /// Scroll amount; negative means scroll down.
        count: i16,
    },
}

/// Terminal colour representation.
#[derive(Debug, Clone, PartialEq)]
pub enum ColorData {
    /// Use the terminal's default foreground/background colour.
    Default,
    /// An entry in the 256-colour palette.
    Indexed(u8),
    /// A 24-bit RGB colour.
    Rgb(u8, u8, u8),
}

/// Cursor shape variants.
#[derive(Debug, Clone, PartialEq)]
pub enum CursorShape {
    /// Filled block covering the whole cell.
    Block,
    /// Underline below the cell.
    Underline,
    /// Thin vertical bar at the left of the cell.
    Bar,
}
