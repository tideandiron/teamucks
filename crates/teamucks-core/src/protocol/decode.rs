/// Decoding logic for the teamucks binary protocol.
///
/// [`decode_server_message`] and [`decode_client_message`] parse a raw payload
/// slice (discriminant + fields) into the corresponding message enum variant.
/// They return the parsed value and the number of bytes consumed so callers can
/// detect trailing data or advance a shared cursor.
use super::{
    CellData, ClientMessage, ColorData, CursorShape, DiffEntry, ProtocolError, ServerMessage,
};

// ---------------------------------------------------------------------------
// Public decode entry points
// ---------------------------------------------------------------------------

/// Decode a server message from `payload` (discriminant + fields, no length
/// prefix).
///
/// Returns `(message, bytes_consumed)`.
///
/// # Errors
///
/// - [`ProtocolError::UnknownMessageType`] — discriminant not recognised
/// - [`ProtocolError::Truncated`] — payload ended prematurely
/// - [`ProtocolError::InvalidUtf8`] — a string field contained invalid UTF-8
///
/// # Examples
///
/// ```
/// use teamucks_core::protocol::{ServerMessage, PROTOCOL_VERSION};
/// use teamucks_core::protocol::encode::encode_server_message;
/// use teamucks_core::protocol::decode::decode_server_message;
///
/// let msg = ServerMessage::HandshakeResponse {
///     protocol_version: PROTOCOL_VERSION,
///     server_name: "teamucks".to_string(),
/// };
/// let mut frame = Vec::new();
/// encode_server_message(&msg, &mut frame).unwrap();
/// // Skip the 4-byte length prefix to get the payload.
/// let (decoded, _) = decode_server_message(&frame[4..]).unwrap();
/// assert_eq!(decoded, msg);
/// ```
pub fn decode_server_message(payload: &[u8]) -> Result<(ServerMessage, usize), ProtocolError> {
    let mut cur = Cursor::new(payload);
    let discriminant = cur.read_u8()?;
    let msg = match discriminant {
        0x01 => {
            let protocol_version = cur.read_u16()?;
            let server_name = cur.read_string()?;
            ServerMessage::HandshakeResponse { protocol_version, server_name }
        }
        0x02 => {
            let pane_id = cur.read_u32()?;
            let cols = cur.read_u16()?;
            let rows = cur.read_u16()?;
            let count = cur.read_u16()? as usize;
            let mut cells = Vec::with_capacity(count);
            for _ in 0..count {
                cells.push(cur.read_cell()?);
            }
            ServerMessage::FullFrame { pane_id, cols, rows, cells }
        }
        0x03 => {
            let pane_id = cur.read_u32()?;
            let count = cur.read_u16()? as usize;
            let mut diffs = Vec::with_capacity(count);
            for _ in 0..count {
                diffs.push(cur.read_diff_entry()?);
            }
            ServerMessage::FrameDiff { pane_id, diffs }
        }
        0x04 => {
            let pane_id = cur.read_u32()?;
            let col = cur.read_u16()?;
            let row = cur.read_u16()?;
            let visible_byte = cur.read_u8()?;
            let shape = cur.read_cursor_shape()?;
            ServerMessage::CursorUpdate { pane_id, col, row, visible: visible_byte != 0, shape }
        }
        0x05 => ServerMessage::LayoutChange,
        0x06 => {
            let content = cur.read_string()?;
            ServerMessage::StatusUpdate { content }
        }
        0x07 => {
            let pane_id = cur.read_u32()?;
            ServerMessage::Bell { pane_id }
        }
        0x08 => {
            let pane_id = cur.read_u32()?;
            let title = cur.read_string()?;
            ServerMessage::TitleChange { pane_id, title }
        }
        tag => return Err(ProtocolError::UnknownMessageType(tag)),
    };
    Ok((msg, cur.pos))
}

/// Decode a client message from `payload` (discriminant + fields, no length
/// prefix).
///
/// Returns `(message, bytes_consumed)`.
///
/// # Errors
///
/// - [`ProtocolError::UnknownMessageType`] — discriminant not recognised
/// - [`ProtocolError::Truncated`] — payload ended prematurely
/// - [`ProtocolError::InvalidUtf8`] — a string field contained invalid UTF-8
///
/// # Examples
///
/// ```
/// use teamucks_core::protocol::{ClientMessage, PROTOCOL_VERSION};
/// use teamucks_core::protocol::encode::encode_client_message;
/// use teamucks_core::protocol::decode::decode_client_message;
///
/// let msg = ClientMessage::HandshakeRequest {
///     protocol_version: PROTOCOL_VERSION,
///     cols: 80,
///     rows: 24,
/// };
/// let mut frame = Vec::new();
/// encode_client_message(&msg, &mut frame).unwrap();
/// let (decoded, _) = decode_client_message(&frame[4..]).unwrap();
/// assert_eq!(decoded, msg);
/// ```
pub fn decode_client_message(payload: &[u8]) -> Result<(ClientMessage, usize), ProtocolError> {
    let mut cur = Cursor::new(payload);
    let discriminant = cur.read_u8()?;
    let msg = match discriminant {
        0x10 => {
            let protocol_version = cur.read_u16()?;
            let cols = cur.read_u16()?;
            let rows = cur.read_u16()?;
            ClientMessage::HandshakeRequest { protocol_version, cols, rows }
        }
        0x11 => {
            let len = cur.read_u16()? as usize;
            let key = cur.read_bytes(len)?;
            let modifiers = cur.read_u8()?;
            ClientMessage::KeyEvent { key, modifiers }
        }
        0x12 => {
            let button = cur.read_u8()?;
            let col = cur.read_u16()?;
            let row = cur.read_u16()?;
            let modifiers = cur.read_u8()?;
            ClientMessage::MouseEvent { button, col, row, modifiers }
        }
        0x13 => {
            let cols = cur.read_u16()?;
            let rows = cur.read_u16()?;
            ClientMessage::Resize { cols, rows }
        }
        0x14 => {
            let name = cur.read_string()?;
            ClientMessage::Command { name }
        }
        0x15 => {
            let data = cur.read_string()?;
            ClientMessage::PasteEvent { data }
        }
        tag => return Err(ProtocolError::UnknownMessageType(tag)),
    };
    Ok((msg, cur.pos))
}

// ---------------------------------------------------------------------------
// Cursor — byte-slice reader
// ---------------------------------------------------------------------------

struct Cursor<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> Cursor<'a> {
    fn new(data: &'a [u8]) -> Self {
        Self { data, pos: 0 }
    }

    fn remaining(&self) -> usize {
        self.data.len() - self.pos
    }

    fn read_u8(&mut self) -> Result<u8, ProtocolError> {
        if self.remaining() < 1 {
            return Err(ProtocolError::Truncated);
        }
        let v = self.data[self.pos];
        self.pos += 1;
        Ok(v)
    }

    fn read_u16(&mut self) -> Result<u16, ProtocolError> {
        if self.remaining() < 2 {
            return Err(ProtocolError::Truncated);
        }
        let v = u16::from_le_bytes([self.data[self.pos], self.data[self.pos + 1]]);
        self.pos += 2;
        Ok(v)
    }

    fn read_i16(&mut self) -> Result<i16, ProtocolError> {
        if self.remaining() < 2 {
            return Err(ProtocolError::Truncated);
        }
        let v = i16::from_le_bytes([self.data[self.pos], self.data[self.pos + 1]]);
        self.pos += 2;
        Ok(v)
    }

    fn read_u32(&mut self) -> Result<u32, ProtocolError> {
        if self.remaining() < 4 {
            return Err(ProtocolError::Truncated);
        }
        let v = u32::from_le_bytes([
            self.data[self.pos],
            self.data[self.pos + 1],
            self.data[self.pos + 2],
            self.data[self.pos + 3],
        ]);
        self.pos += 4;
        Ok(v)
    }

    fn read_bytes(&mut self, n: usize) -> Result<Vec<u8>, ProtocolError> {
        if self.remaining() < n {
            return Err(ProtocolError::Truncated);
        }
        let slice = &self.data[self.pos..self.pos + n];
        self.pos += n;
        Ok(slice.to_vec())
    }

    /// Read a `u16 LE`-length-prefixed UTF-8 string.
    fn read_string(&mut self) -> Result<String, ProtocolError> {
        let len = self.read_u16()? as usize;
        let bytes = self.read_bytes(len)?;
        String::from_utf8(bytes).map_err(|_| ProtocolError::InvalidUtf8)
    }

    fn read_color(&mut self) -> Result<ColorData, ProtocolError> {
        let tag = self.read_u8()?;
        match tag {
            0x00 => Ok(ColorData::Default),
            0x01 => {
                let idx = self.read_u8()?;
                Ok(ColorData::Indexed(idx))
            }
            0x02 => {
                let r = self.read_u8()?;
                let g = self.read_u8()?;
                let b = self.read_u8()?;
                Ok(ColorData::Rgb(r, g, b))
            }
            _ => Err(ProtocolError::UnknownMessageType(tag)),
        }
    }

    fn read_cursor_shape(&mut self) -> Result<CursorShape, ProtocolError> {
        let tag = self.read_u8()?;
        match tag {
            0x00 => Ok(CursorShape::Block),
            0x01 => Ok(CursorShape::Underline),
            0x02 => Ok(CursorShape::Bar),
            _ => Err(ProtocolError::UnknownMessageType(tag)),
        }
    }

    fn read_cell(&mut self) -> Result<CellData, ProtocolError> {
        let grapheme = self.read_string()?;
        let fg = self.read_color()?;
        let bg = self.read_color()?;
        let attrs = self.read_u16()?;
        let flags = self.read_u8()?;
        Ok(CellData { grapheme, fg, bg, attrs, flags })
    }

    fn read_diff_entry(&mut self) -> Result<DiffEntry, ProtocolError> {
        let tag = self.read_u8()?;
        match tag {
            0x01 => {
                let col = self.read_u16()?;
                let row = self.read_u16()?;
                let cell = self.read_cell()?;
                Ok(DiffEntry::CellChange { col, row, cell })
            }
            0x02 => {
                let row = self.read_u16()?;
                let count = self.read_u16()? as usize;
                let mut cells = Vec::with_capacity(count);
                for _ in 0..count {
                    cells.push(self.read_cell()?);
                }
                Ok(DiffEntry::LineChange { row, cells })
            }
            0x03 => {
                let top = self.read_u16()?;
                let bottom = self.read_u16()?;
                let count = self.read_i16()?;
                Ok(DiffEntry::RegionScroll { top, bottom, count })
            }
            tag => Err(ProtocolError::UnknownMessageType(tag)),
        }
    }
}
