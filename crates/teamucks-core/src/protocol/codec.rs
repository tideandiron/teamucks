/// Async framing codec for the teamucks binary protocol.
///
/// [`ProtocolCodec`] wraps an internal read buffer and provides
/// `async fn` methods for reading and writing complete framed messages on top
/// of any `tokio::io::AsyncRead` / `AsyncWrite` implementation.
///
/// # Frame format
///
/// ```text
/// ┌──────────────┬──────────────────────────────────────┐
/// │  len: u32 LE │  payload (discriminant + fields …)   │
/// └──────────────┴──────────────────────────────────────┘
/// ```
///
/// The codec accumulates bytes into an internal [`bytes::BytesMut`] buffer
/// until a complete frame is available, then hands the payload slice to the
/// decoder.
use bytes::BytesMut;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

use super::{
    decode::{decode_client_message, decode_server_message},
    encode::{encode_client_message, encode_server_message},
    ClientMessage, ProtocolError, ServerMessage, MAX_MESSAGE_SIZE,
};

// ---------------------------------------------------------------------------
// ProtocolCodec
// ---------------------------------------------------------------------------

/// A stateful codec that reads/writes length-prefixed protocol messages.
///
/// Maintain one `ProtocolCodec` per connection direction (or per half-duplex
/// connection).  The codec is not `Clone` or `Send` by itself — wrap it in
/// the appropriate async synchronization primitive if sharing across tasks.
///
/// # Examples
///
/// ```no_run
/// # async fn demo() -> Result<(), teamucks_core::protocol::ProtocolError> {
/// use tokio::net::UnixStream;
/// use teamucks_core::protocol::{ServerMessage, PROTOCOL_VERSION};
/// use teamucks_core::protocol::codec::ProtocolCodec;
///
/// let (mut client_stream, mut server_stream) = UnixStream::pair().unwrap();
/// let mut codec = ProtocolCodec::new();
///
/// let msg = ServerMessage::Bell { pane_id: 1 };
/// ProtocolCodec::write_server_message(&msg, &mut server_stream).await?;
///
/// let received = codec.read_server_message(&mut client_stream).await?;
/// assert_eq!(received, msg);
/// # Ok(())
/// # }
/// ```
pub struct ProtocolCodec {
    read_buf: BytesMut,
}

impl ProtocolCodec {
    /// Create a new codec with an empty read buffer.
    #[must_use]
    pub fn new() -> Self {
        Self { read_buf: BytesMut::with_capacity(4096) }
    }

    // -----------------------------------------------------------------------
    // Reading
    // -----------------------------------------------------------------------

    /// Read exactly one complete server message from `reader`.
    ///
    /// The codec buffers partial reads internally.  Multiple calls on the same
    /// `reader` are safe; leftover bytes from one message are preserved for the
    /// next call.
    ///
    /// # Errors
    ///
    /// - [`ProtocolError::Io`] — underlying read failure or EOF
    /// - [`ProtocolError::MessageTooLarge`] — declared length > [`MAX_MESSAGE_SIZE`]
    /// - Propagates any error returned by the decoder
    pub async fn read_server_message<R: AsyncRead + Unpin>(
        &mut self,
        reader: &mut R,
    ) -> Result<ServerMessage, ProtocolError> {
        let payload = self.read_frame(reader).await?;
        let (msg, _) = decode_server_message(&payload)?;
        Ok(msg)
    }

    /// Read exactly one complete client message from `reader`.
    ///
    /// # Errors
    ///
    /// - [`ProtocolError::Io`] — underlying read failure or EOF
    /// - [`ProtocolError::MessageTooLarge`] — declared length > [`MAX_MESSAGE_SIZE`]
    /// - Propagates any error returned by the decoder
    pub async fn read_client_message<R: AsyncRead + Unpin>(
        &mut self,
        reader: &mut R,
    ) -> Result<ClientMessage, ProtocolError> {
        let payload = self.read_frame(reader).await?;
        let (msg, _) = decode_client_message(&payload)?;
        Ok(msg)
    }

    // -----------------------------------------------------------------------
    // Writing
    // -----------------------------------------------------------------------

    /// Encode `msg` and write the complete length-prefixed frame to `writer`.
    ///
    /// The write is atomic with respect to the writer: either the full frame is
    /// written or an error is returned.
    ///
    /// # Errors
    ///
    /// - [`ProtocolError::MessageTooLarge`] — encoded payload > [`MAX_MESSAGE_SIZE`]
    /// - [`ProtocolError::Io`] — underlying write failure
    pub async fn write_server_message<W: AsyncWrite + Unpin>(
        msg: &ServerMessage,
        writer: &mut W,
    ) -> Result<(), ProtocolError> {
        let mut buf = Vec::new();
        encode_server_message(msg, &mut buf)?;
        writer.write_all(&buf).await?;
        Ok(())
    }

    /// Encode `msg` and write the complete length-prefixed frame to `writer`.
    ///
    /// # Errors
    ///
    /// - [`ProtocolError::MessageTooLarge`] — encoded payload > [`MAX_MESSAGE_SIZE`]
    /// - [`ProtocolError::Io`] — underlying write failure
    pub async fn write_client_message<W: AsyncWrite + Unpin>(
        msg: &ClientMessage,
        writer: &mut W,
    ) -> Result<(), ProtocolError> {
        let mut buf = Vec::new();
        encode_client_message(msg, &mut buf)?;
        writer.write_all(&buf).await?;
        Ok(())
    }

    // -----------------------------------------------------------------------
    // Internal framing
    // -----------------------------------------------------------------------

    /// Fill `self.read_buf` until at least `n` bytes are available.
    async fn fill_at_least<R: AsyncRead + Unpin>(
        &mut self,
        reader: &mut R,
        n: usize,
    ) -> Result<(), ProtocolError> {
        while self.read_buf.len() < n {
            // Reserve space in the BytesMut for an additional read.
            let additional = n - self.read_buf.len();
            self.read_buf.reserve(additional.max(4096));

            // Read into the uninitialized capacity.
            let n_read = reader.read_buf(&mut self.read_buf).await?;
            if n_read == 0 {
                return Err(ProtocolError::Io(std::io::Error::new(
                    std::io::ErrorKind::UnexpectedEof,
                    "connection closed mid-frame",
                )));
            }
        }
        Ok(())
    }

    /// Read and return a complete frame payload (without the 4-byte prefix).
    async fn read_frame<R: AsyncRead + Unpin>(
        &mut self,
        reader: &mut R,
    ) -> Result<Vec<u8>, ProtocolError> {
        // Read the 4-byte length prefix.
        self.fill_at_least(reader, 4).await?;
        let len = u32::from_le_bytes([
            self.read_buf[0],
            self.read_buf[1],
            self.read_buf[2],
            self.read_buf[3],
        ]);

        if len > MAX_MESSAGE_SIZE {
            return Err(ProtocolError::MessageTooLarge { size: len, max: MAX_MESSAGE_SIZE });
        }

        let total = 4 + len as usize;

        // Read the rest of the frame.
        self.fill_at_least(reader, total).await?;

        // Extract payload bytes and advance the buffer.
        let payload = self.read_buf[4..total].to_vec();
        let _ = self.read_buf.split_to(total);
        Ok(payload)
    }
}

impl Default for ProtocolCodec {
    fn default() -> Self {
        Self::new()
    }
}
