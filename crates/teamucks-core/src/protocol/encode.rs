/// Encoding logic for the teamucks binary protocol.
///
/// Each message type implements encoding via helper functions that append
/// bytes to a caller-supplied `Vec<u8>`.  The top-level functions
/// [`encode_server_message`] and [`encode_client_message`] produce a
/// complete, length-prefixed wire frame ready for transmission.
use super::{
    CellData, ClientMessage, ColorData, CursorShape, DiffEntry, ProtocolError, ServerMessage,
    MAX_MESSAGE_SIZE,
};

// ---------------------------------------------------------------------------
// Public encode entry points
// ---------------------------------------------------------------------------

/// Encode `msg` into a length-prefixed wire frame and append it to `buf`.
///
/// The frame is: `u32 LE length || payload`.  `length` is the byte count of
/// the payload only (the 4-byte length prefix itself is not counted).
///
/// # Errors
///
/// Returns [`ProtocolError::MessageTooLarge`] if the encoded payload exceeds
/// [`MAX_MESSAGE_SIZE`].
///
/// # Examples
///
/// ```
/// use teamucks_core::protocol::{ServerMessage, PROTOCOL_VERSION};
/// use teamucks_core::protocol::encode::encode_server_message;
///
/// let msg = ServerMessage::HandshakeResponse {
///     protocol_version: PROTOCOL_VERSION,
///     server_name: "teamucks".to_string(),
/// };
/// let mut buf = Vec::new();
/// encode_server_message(&msg, &mut buf).unwrap();
/// assert!(buf.len() > 4); // at least the length prefix
/// ```
pub fn encode_server_message(msg: &ServerMessage, buf: &mut Vec<u8>) -> Result<(), ProtocolError> {
    let mut payload = Vec::new();
    encode_server_payload(msg, &mut payload);
    write_frame(&payload, buf)
}

/// Encode `msg` into a length-prefixed wire frame and append it to `buf`.
///
/// # Errors
///
/// Returns [`ProtocolError::MessageTooLarge`] if the encoded payload exceeds
/// [`MAX_MESSAGE_SIZE`].
///
/// # Examples
///
/// ```
/// use teamucks_core::protocol::{ClientMessage, PROTOCOL_VERSION};
/// use teamucks_core::protocol::encode::encode_client_message;
///
/// let msg = ClientMessage::HandshakeRequest {
///     protocol_version: PROTOCOL_VERSION,
///     cols: 80,
///     rows: 24,
/// };
/// let mut buf = Vec::new();
/// encode_client_message(&msg, &mut buf).unwrap();
/// assert!(buf.len() > 4);
/// ```
pub fn encode_client_message(msg: &ClientMessage, buf: &mut Vec<u8>) -> Result<(), ProtocolError> {
    let mut payload = Vec::new();
    encode_client_payload(msg, &mut payload);
    write_frame(&payload, buf)
}

// ---------------------------------------------------------------------------
// Frame wrapper
// ---------------------------------------------------------------------------

fn write_frame(payload: &[u8], buf: &mut Vec<u8>) -> Result<(), ProtocolError> {
    let len = payload.len();
    // Safe cast: checked below.
    #[allow(clippy::cast_possible_truncation)]
    let len32 = len as u32;
    if len32 > MAX_MESSAGE_SIZE {
        return Err(ProtocolError::MessageTooLarge { size: len32, max: MAX_MESSAGE_SIZE });
    }
    buf.extend_from_slice(&len32.to_le_bytes());
    buf.extend_from_slice(payload);
    Ok(())
}

// ---------------------------------------------------------------------------
// Server message encoding
// ---------------------------------------------------------------------------

fn encode_server_payload(msg: &ServerMessage, buf: &mut Vec<u8>) {
    match msg {
        ServerMessage::HandshakeResponse { protocol_version, server_name } => {
            buf.push(0x01);
            push_u16(*protocol_version, buf);
            push_string(server_name, buf);
        }
        ServerMessage::FullFrame { pane_id, cols, rows, cells } => {
            buf.push(0x02);
            push_u32(*pane_id, buf);
            push_u16(*cols, buf);
            push_u16(*rows, buf);
            push_u16(u16::try_from(cells.len()).unwrap_or(u16::MAX), buf);
            for cell in cells {
                encode_cell(cell, buf);
            }
        }
        ServerMessage::FrameDiff { pane_id, diffs } => {
            buf.push(0x03);
            push_u32(*pane_id, buf);
            push_u16(u16::try_from(diffs.len()).unwrap_or(u16::MAX), buf);
            for diff in diffs {
                encode_diff_entry(diff, buf);
            }
        }
        ServerMessage::CursorUpdate { pane_id, col, row, visible, shape } => {
            buf.push(0x04);
            push_u32(*pane_id, buf);
            push_u16(*col, buf);
            push_u16(*row, buf);
            buf.push(u8::from(*visible));
            encode_cursor_shape(shape, buf);
        }
        ServerMessage::LayoutChange => {
            buf.push(0x05);
        }
        ServerMessage::StatusUpdate { content } => {
            buf.push(0x06);
            push_string(content, buf);
        }
        ServerMessage::Bell { pane_id } => {
            buf.push(0x07);
            push_u32(*pane_id, buf);
        }
        ServerMessage::TitleChange { pane_id, title } => {
            buf.push(0x08);
            push_u32(*pane_id, buf);
            push_string(title, buf);
        }
    }
}

// ---------------------------------------------------------------------------
// Client message encoding
// ---------------------------------------------------------------------------

fn encode_client_payload(msg: &ClientMessage, buf: &mut Vec<u8>) {
    match msg {
        ClientMessage::HandshakeRequest { protocol_version, cols, rows } => {
            buf.push(0x10);
            push_u16(*protocol_version, buf);
            push_u16(*cols, buf);
            push_u16(*rows, buf);
        }
        ClientMessage::KeyEvent { key, modifiers } => {
            buf.push(0x11);
            push_u16(u16::try_from(key.len()).unwrap_or(u16::MAX), buf);
            buf.extend_from_slice(key);
            buf.push(*modifiers);
        }
        ClientMessage::MouseEvent { button, col, row, modifiers } => {
            buf.push(0x12);
            buf.push(*button);
            push_u16(*col, buf);
            push_u16(*row, buf);
            buf.push(*modifiers);
        }
        ClientMessage::Resize { cols, rows } => {
            buf.push(0x13);
            push_u16(*cols, buf);
            push_u16(*rows, buf);
        }
        ClientMessage::Command { name } => {
            buf.push(0x14);
            push_string(name, buf);
        }
        ClientMessage::PasteEvent { data } => {
            buf.push(0x15);
            push_string(data, buf);
        }
    }
}

// ---------------------------------------------------------------------------
// Supporting type encoders
// ---------------------------------------------------------------------------

fn encode_cell(cell: &CellData, buf: &mut Vec<u8>) {
    push_string(&cell.grapheme, buf);
    encode_color(&cell.fg, buf);
    encode_color(&cell.bg, buf);
    push_u16(cell.attrs, buf);
    buf.push(cell.flags);
}

fn encode_diff_entry(entry: &DiffEntry, buf: &mut Vec<u8>) {
    match entry {
        DiffEntry::CellChange { col, row, cell } => {
            buf.push(0x01);
            push_u16(*col, buf);
            push_u16(*row, buf);
            encode_cell(cell, buf);
        }
        DiffEntry::LineChange { row, cells } => {
            buf.push(0x02);
            push_u16(*row, buf);
            push_u16(u16::try_from(cells.len()).unwrap_or(u16::MAX), buf);
            for cell in cells {
                encode_cell(cell, buf);
            }
        }
        DiffEntry::RegionScroll { top, bottom, count } => {
            buf.push(0x03);
            push_u16(*top, buf);
            push_u16(*bottom, buf);
            push_i16(*count, buf);
        }
    }
}

fn encode_color(color: &ColorData, buf: &mut Vec<u8>) {
    match color {
        ColorData::Default => buf.push(0x00),
        ColorData::Indexed(idx) => {
            buf.push(0x01);
            buf.push(*idx);
        }
        ColorData::Rgb(r, g, b) => {
            buf.push(0x02);
            buf.push(*r);
            buf.push(*g);
            buf.push(*b);
        }
    }
}

fn encode_cursor_shape(shape: &CursorShape, buf: &mut Vec<u8>) {
    let byte = match shape {
        CursorShape::Block => 0x00,
        CursorShape::Underline => 0x01,
        CursorShape::Bar => 0x02,
    };
    buf.push(byte);
}

// ---------------------------------------------------------------------------
// Primitive writers
// ---------------------------------------------------------------------------

fn push_u16(v: u16, buf: &mut Vec<u8>) {
    buf.extend_from_slice(&v.to_le_bytes());
}

fn push_i16(v: i16, buf: &mut Vec<u8>) {
    buf.extend_from_slice(&v.to_le_bytes());
}

fn push_u32(v: u32, buf: &mut Vec<u8>) {
    buf.extend_from_slice(&v.to_le_bytes());
}

/// Encode a string as `u16 LE length || UTF-8 bytes`.
fn push_string(s: &str, buf: &mut Vec<u8>) {
    let bytes = s.as_bytes();
    // Truncate at u16::MAX bytes; real messages are well under this limit.
    let len = u16::try_from(bytes.len()).unwrap_or(u16::MAX);
    push_u16(len, buf);
    // Write only `len` bytes to stay consistent with the declared length.
    buf.extend_from_slice(&bytes[..usize::from(len)]);
}
