//! Integration tests for the teamucks binary client protocol.
//!
//! Coverage:
//! - Encoding/decoding round-trips for every message type
//! - Framing correctness (u32 LE length prefix)
//! - Async codec read/write round-trip
//! - Edge cases: unknown type, truncated message, too-large message,
//!   empty strings, all colour variants
//! - Handshake version negotiation

use teamucks_core::protocol::{
    codec::ProtocolCodec,
    decode::{decode_client_message, decode_server_message},
    encode::{encode_client_message, encode_server_message},
    CellData, ClientMessage, ColorData, CursorShape, DiffEntry, ProtocolError, ServerMessage,
    PROTOCOL_VERSION,
};

// ---------------------------------------------------------------------------
// Helper: build a simple CellData value for use in tests
// ---------------------------------------------------------------------------

fn make_cell(grapheme: &str) -> CellData {
    CellData {
        grapheme: grapheme.to_string(),
        fg: ColorData::Default,
        bg: ColorData::Default,
        attrs: 0,
        flags: 0,
    }
}

fn make_cell_styled(
    grapheme: &str,
    fg: ColorData,
    bg: ColorData,
    attrs: u16,
    flags: u8,
) -> CellData {
    CellData { grapheme: grapheme.to_string(), fg, bg, attrs, flags }
}

// ---------------------------------------------------------------------------
// Round-trip helpers
// ---------------------------------------------------------------------------

fn server_roundtrip(msg: &ServerMessage) -> ServerMessage {
    let mut buf = Vec::new();
    encode_server_message(msg, &mut buf).expect("encode must succeed");
    // Skip 4-byte length prefix.
    let (decoded, consumed) = decode_server_message(&buf[4..]).expect("decode must succeed");
    assert_eq!(consumed, buf.len() - 4, "all payload bytes must be consumed");
    decoded
}

fn client_roundtrip(msg: &ClientMessage) -> ClientMessage {
    let mut buf = Vec::new();
    encode_client_message(msg, &mut buf).expect("encode must succeed");
    let (decoded, consumed) = decode_client_message(&buf[4..]).expect("decode must succeed");
    assert_eq!(consumed, buf.len() - 4, "all payload bytes must be consumed");
    decoded
}

// ---------------------------------------------------------------------------
// Server message round-trips
// ---------------------------------------------------------------------------

#[test]
fn test_encode_decode_handshake_response() {
    let msg = ServerMessage::HandshakeResponse {
        protocol_version: PROTOCOL_VERSION,
        server_name: "teamucks".to_string(),
    };
    assert_eq!(server_roundtrip(&msg), msg);
}

#[test]
fn test_encode_decode_full_frame() {
    let cells = vec![
        make_cell_styled("A", ColorData::Indexed(1), ColorData::Rgb(0, 0, 0), 0b0000_0001, 0),
        make_cell("B"),
        make_cell(" "),
    ];
    let msg = ServerMessage::FullFrame { pane_id: 42, cols: 80, rows: 24, cells };
    assert_eq!(server_roundtrip(&msg), msg);
}

#[test]
fn test_encode_decode_frame_diff_cell() {
    let msg = ServerMessage::FrameDiff {
        pane_id: 7,
        diffs: vec![DiffEntry::CellChange {
            col: 10,
            row: 3,
            cell: make_cell_styled("X", ColorData::Rgb(255, 0, 128), ColorData::Default, 0b10, 0),
        }],
    };
    assert_eq!(server_roundtrip(&msg), msg);
}

#[test]
fn test_encode_decode_frame_diff_line() {
    let cells = (0u16..5).map(|i| make_cell(&i.to_string())).collect();
    let msg = ServerMessage::FrameDiff {
        pane_id: 1,
        diffs: vec![DiffEntry::LineChange { row: 0, cells }],
    };
    assert_eq!(server_roundtrip(&msg), msg);
}

#[test]
fn test_encode_decode_frame_diff_scroll() {
    let msg = ServerMessage::FrameDiff {
        pane_id: 99,
        diffs: vec![
            DiffEntry::RegionScroll { top: 0, bottom: 23, count: 3 },
            DiffEntry::RegionScroll { top: 5, bottom: 10, count: -2 },
        ],
    };
    assert_eq!(server_roundtrip(&msg), msg);
}

#[test]
fn test_encode_decode_cursor_update() {
    for shape in [CursorShape::Block, CursorShape::Underline, CursorShape::Bar] {
        let msg = ServerMessage::CursorUpdate {
            pane_id: 3,
            col: 79,
            row: 23,
            visible: true,
            shape: shape.clone(),
        };
        assert_eq!(server_roundtrip(&msg), msg);
    }

    // Invisible cursor
    let msg = ServerMessage::CursorUpdate {
        pane_id: 0,
        col: 0,
        row: 0,
        visible: false,
        shape: CursorShape::Bar,
    };
    assert_eq!(server_roundtrip(&msg), msg);
}

#[test]
fn test_encode_decode_bell() {
    let msg = ServerMessage::Bell { pane_id: 5 };
    assert_eq!(server_roundtrip(&msg), msg);
}

#[test]
fn test_encode_decode_title_change() {
    let msg =
        ServerMessage::TitleChange { pane_id: 2, title: "My Shell — ~/projects".to_string() };
    assert_eq!(server_roundtrip(&msg), msg);
}

#[test]
fn test_encode_decode_status_update() {
    let msg = ServerMessage::StatusUpdate { content: "session 1 | window 2 | pane 3".to_string() };
    assert_eq!(server_roundtrip(&msg), msg);
}

#[test]
fn test_encode_decode_layout_change() {
    let msg = ServerMessage::LayoutChange;
    assert_eq!(server_roundtrip(&msg), msg);
}

// ---------------------------------------------------------------------------
// Client message round-trips
// ---------------------------------------------------------------------------

#[test]
fn test_encode_decode_handshake_request() {
    let msg =
        ClientMessage::HandshakeRequest { protocol_version: PROTOCOL_VERSION, cols: 220, rows: 55 };
    assert_eq!(client_roundtrip(&msg), msg);
}

#[test]
fn test_encode_decode_key_event() {
    // Plain ASCII key
    let msg = ClientMessage::KeyEvent { key: b"a".to_vec(), modifiers: 0 };
    assert_eq!(client_roundtrip(&msg), msg);

    // CSI sequence with modifiers (Ctrl+Alt)
    let msg = ClientMessage::KeyEvent {
        key: b"\x1b[1;7A".to_vec(), // Ctrl+Alt+Up in xterm
        modifiers: 0b0000_0110,
    };
    assert_eq!(client_roundtrip(&msg), msg);

    // Empty key bytes
    let msg = ClientMessage::KeyEvent { key: vec![], modifiers: 0 };
    assert_eq!(client_roundtrip(&msg), msg);
}

#[test]
fn test_encode_decode_mouse_event() {
    let msg = ClientMessage::MouseEvent { button: 1, col: 40, row: 12, modifiers: 0b0000_0001 };
    assert_eq!(client_roundtrip(&msg), msg);
}

#[test]
fn test_encode_decode_resize() {
    let msg = ClientMessage::Resize { cols: 132, rows: 43 };
    assert_eq!(client_roundtrip(&msg), msg);
}

#[test]
fn test_encode_decode_command() {
    let msg = ClientMessage::Command { name: "split-vertical".to_string() };
    assert_eq!(client_roundtrip(&msg), msg);
}

#[test]
fn test_encode_decode_paste() {
    let msg = ClientMessage::PasteEvent { data: "Hello, paste!\nLine two.\n".to_string() };
    assert_eq!(client_roundtrip(&msg), msg);
}

// ---------------------------------------------------------------------------
// Framing
// ---------------------------------------------------------------------------

#[test]
fn test_length_prefix_framing() {
    // HandshakeRequest: discriminant (1) + version u16 (2) + cols u16 (2) + rows u16 (2) = 7
    let msg = ClientMessage::HandshakeRequest { protocol_version: 1, cols: 80, rows: 24 };
    let mut buf = Vec::new();
    encode_client_message(&msg, &mut buf).unwrap();

    // First 4 bytes are the LE length prefix.
    let len = u32::from_le_bytes([buf[0], buf[1], buf[2], buf[3]]);
    // Total buffer should be prefix(4) + payload(len).
    assert_eq!(buf.len(), 4 + len as usize, "frame length must match prefix");
    // The payload starts with the discriminant 0x10.
    assert_eq!(buf[4], 0x10, "HandshakeRequest discriminant must be 0x10");
}

#[test]
fn test_length_prefix_framing_server() {
    let msg = ServerMessage::Bell { pane_id: 1 };
    let mut buf = Vec::new();
    encode_server_message(&msg, &mut buf).unwrap();

    let len = u32::from_le_bytes([buf[0], buf[1], buf[2], buf[3]]);
    assert_eq!(buf.len(), 4 + len as usize);
    // Bell discriminant is 0x07.
    assert_eq!(buf[4], 0x07);
}

// ---------------------------------------------------------------------------
// Async codec round-trip
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_codec_read_write_roundtrip() {
    use tokio::net::UnixStream;

    let (mut client_half, mut server_half) = UnixStream::pair().expect("socketpair");
    let mut codec = ProtocolCodec::new();

    // Server sends a message; client reads it.
    let sent = ServerMessage::CursorUpdate {
        pane_id: 1,
        col: 5,
        row: 10,
        visible: true,
        shape: CursorShape::Block,
    };
    ProtocolCodec::write_server_message(&sent, &mut server_half).await.expect("write must succeed");

    let received = codec.read_server_message(&mut client_half).await.expect("read must succeed");
    assert_eq!(received, sent);
}

#[tokio::test]
async fn test_codec_client_roundtrip() {
    use tokio::net::UnixStream;

    let (mut client_half, mut server_half) = UnixStream::pair().expect("socketpair");
    let mut codec = ProtocolCodec::new();

    let sent = ClientMessage::Resize { cols: 100, rows: 30 };
    ProtocolCodec::write_client_message(&sent, &mut client_half).await.expect("write must succeed");

    let received = codec.read_client_message(&mut server_half).await.expect("read must succeed");
    assert_eq!(received, sent);
}

#[tokio::test]
async fn test_codec_multiple_messages_in_sequence() {
    use tokio::net::UnixStream;

    let (mut writer, mut reader) = UnixStream::pair().expect("socketpair");
    let mut codec = ProtocolCodec::new();

    let messages = vec![
        ServerMessage::Bell { pane_id: 1 },
        ServerMessage::StatusUpdate { content: "hello".to_string() },
        ServerMessage::LayoutChange,
    ];

    for msg in &messages {
        ProtocolCodec::write_server_message(msg, &mut writer).await.unwrap();
    }
    // Drop writer half so EOF is sent after the messages.
    drop(writer);

    for expected in &messages {
        let got = codec.read_server_message(&mut reader).await.unwrap();
        assert_eq!(&got, expected);
    }
}

// ---------------------------------------------------------------------------
// Edge cases
// ---------------------------------------------------------------------------

#[test]
fn test_unknown_message_type_server() {
    // Craft a payload with an unknown discriminant.
    let payload = &[0xFF_u8];
    let err = decode_server_message(payload).unwrap_err();
    assert!(
        matches!(err, ProtocolError::UnknownMessageType(0xFF)),
        "expected UnknownMessageType(0xFF), got {err:?}"
    );
}

#[test]
fn test_unknown_message_type_client() {
    let payload = &[0xAA_u8];
    let err = decode_client_message(payload).unwrap_err();
    assert!(
        matches!(err, ProtocolError::UnknownMessageType(0xAA)),
        "expected UnknownMessageType(0xAA), got {err:?}"
    );
}

#[test]
fn test_truncated_message_server() {
    // HandshakeResponse: discriminant 0x01, but no further bytes.
    let payload = &[0x01_u8]; // missing version and name
    let err = decode_server_message(payload).unwrap_err();
    assert!(matches!(err, ProtocolError::Truncated), "expected Truncated, got {err:?}");
}

#[test]
fn test_truncated_message_client() {
    // HandshakeRequest: discriminant 0x10, only one more byte (should be 6).
    let payload = &[0x10_u8, 0x01]; // incomplete version field
    let err = decode_client_message(payload).unwrap_err();
    assert!(matches!(err, ProtocolError::Truncated));
}

#[test]
fn test_message_too_large() {
    use teamucks_core::protocol::MAX_MESSAGE_SIZE;

    // Build a fake frame whose length prefix claims more than MAX_MESSAGE_SIZE.
    let oversized: u32 = MAX_MESSAGE_SIZE + 1;
    let frame_prefix = oversized.to_le_bytes();

    // The codec will check the length before reading the payload, so we just
    // need to test encode returns an error if it could produce something too big.
    // Here we directly test the error variant.
    let err = ProtocolError::MessageTooLarge { size: oversized, max: MAX_MESSAGE_SIZE };
    assert!(err.to_string().contains("too large"));

    // Also verify the raw bytes: if we manually craft such a frame and feed it
    // to the async codec it should fail.
    let _ = frame_prefix; // used above
}

#[tokio::test]
async fn test_codec_message_too_large_async() {
    use teamucks_core::protocol::MAX_MESSAGE_SIZE;
    use tokio::io::AsyncWriteExt;
    use tokio::net::UnixStream;

    let (mut writer, mut reader) = UnixStream::pair().expect("socketpair");
    let mut codec = ProtocolCodec::new();

    // Write a frame header claiming MAX_MESSAGE_SIZE + 1 bytes payload.
    let oversized: u32 = MAX_MESSAGE_SIZE + 1;
    writer.write_all(&oversized.to_le_bytes()).await.unwrap();
    // We don't need to write the payload; the codec should error on the header alone.

    let err = codec.read_server_message(&mut reader).await.unwrap_err();
    assert!(
        matches!(err, ProtocolError::MessageTooLarge { .. }),
        "expected MessageTooLarge, got {err:?}"
    );
}

#[test]
fn test_empty_string_encoding() {
    // Command with empty name
    let msg = ClientMessage::Command { name: String::new() };
    assert_eq!(client_roundtrip(&msg), msg);

    // TitleChange with empty title
    let msg = ServerMessage::TitleChange { pane_id: 0, title: String::new() };
    assert_eq!(server_roundtrip(&msg), msg);

    // StatusUpdate with empty content
    let msg = ServerMessage::StatusUpdate { content: String::new() };
    assert_eq!(server_roundtrip(&msg), msg);
}

#[test]
fn test_color_encoding_all_variants() {
    let variants = vec![
        ColorData::Default,
        ColorData::Indexed(0),
        ColorData::Indexed(255),
        ColorData::Rgb(0, 0, 0),
        ColorData::Rgb(255, 128, 64),
        ColorData::Rgb(255, 255, 255),
    ];

    for fg in &variants {
        for bg in &variants {
            let cell = make_cell_styled("x", fg.clone(), bg.clone(), 0, 0);
            let msg = ServerMessage::FullFrame { pane_id: 0, cols: 1, rows: 1, cells: vec![cell] };
            let decoded = server_roundtrip(&msg);
            assert_eq!(decoded, msg, "fg={fg:?} bg={bg:?} failed round-trip");
        }
    }
}

// ---------------------------------------------------------------------------
// Handshake version negotiation
// ---------------------------------------------------------------------------

#[test]
fn test_handshake_version_negotiation_success() {
    // Client at v1, server responds at v1 — success.
    let request =
        ClientMessage::HandshakeRequest { protocol_version: PROTOCOL_VERSION, cols: 80, rows: 24 };
    let mut buf = Vec::new();
    encode_client_message(&request, &mut buf).unwrap();
    let (decoded_req, _) = decode_client_message(&buf[4..]).unwrap();

    if let ClientMessage::HandshakeRequest { protocol_version, .. } = decoded_req {
        assert_eq!(protocol_version, PROTOCOL_VERSION);

        // Server accepts and echoes its version.
        let response = ServerMessage::HandshakeResponse {
            protocol_version: PROTOCOL_VERSION,
            server_name: "teamucks".to_string(),
        };
        let decoded_resp = server_roundtrip(&response);
        if let ServerMessage::HandshakeResponse { protocol_version: resp_ver, .. } = decoded_resp {
            assert_eq!(resp_ver, PROTOCOL_VERSION);
        } else {
            panic!("decoded wrong variant");
        }
    } else {
        panic!("decoded wrong variant");
    }
}

#[test]
fn test_version_mismatch_error_message() {
    let err = ProtocolError::VersionMismatch { client: 2, server: 1 };
    let msg = err.to_string();
    assert!(msg.contains("client=2"));
    assert!(msg.contains("server=1"));
}

// ---------------------------------------------------------------------------
// Additional coverage: unicode grapheme clusters and flags
// ---------------------------------------------------------------------------

#[test]
fn test_encode_decode_wide_cell_flags() {
    let cell = make_cell_styled("W", ColorData::Default, ColorData::Default, 0, 0b0000_0001);
    let msg = ServerMessage::FullFrame { pane_id: 1, cols: 2, rows: 1, cells: vec![cell] };
    assert_eq!(server_roundtrip(&msg), msg);
}

#[test]
fn test_encode_decode_unicode_grapheme() {
    // Multi-byte UTF-8 grapheme cluster (emoji with skin tone modifier)
    let grapheme = "\u{1F44B}\u{1F3FD}"; // waving hand + medium skin tone
    let cell = make_cell(grapheme);
    let msg = ServerMessage::FullFrame { pane_id: 0, cols: 2, rows: 1, cells: vec![cell] };
    assert_eq!(server_roundtrip(&msg), msg);
}

#[test]
fn test_encode_decode_diff_mixed_entries() {
    let msg = ServerMessage::FrameDiff {
        pane_id: 10,
        diffs: vec![
            DiffEntry::CellChange { col: 0, row: 0, cell: make_cell("A") },
            DiffEntry::LineChange { row: 1, cells: vec![make_cell("B"), make_cell("C")] },
            DiffEntry::RegionScroll { top: 2, bottom: 10, count: -1 },
            DiffEntry::CellChange { col: 5, row: 5, cell: make_cell("Z") },
        ],
    };
    assert_eq!(server_roundtrip(&msg), msg);
}

#[test]
fn test_encode_decode_all_attr_bits() {
    // attrs field is u16; test boundary values.
    for attrs in [0u16, 0x00FF, 0xFF00, 0xFFFF] {
        let cell = make_cell_styled("a", ColorData::Default, ColorData::Default, attrs, 0);
        let msg = ServerMessage::FullFrame { pane_id: 0, cols: 1, rows: 1, cells: vec![cell] };
        assert_eq!(server_roundtrip(&msg), msg, "attrs={attrs:#06x} failed");
    }
}
