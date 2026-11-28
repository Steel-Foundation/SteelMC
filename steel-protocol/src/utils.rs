//! # Steel Protocol Utils
//! Utility functions and types for the protocol.

use std::{
    io,
    pin::Pin,
    task::{Context, Poll},
};

use aes::cipher::{Array, BlockModeDecrypt, BlockModeEncrypt, BlockSizeUser};
use thiserror::Error;
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};

/// An AES-128 CFB-8 encryptor.
pub type Aes128Cfb8Enc = cfb8::Encryptor<aes::Aes128>;
/// An AES-128 CFB-8 decryptor.
pub type Aes128Cfb8Dec = cfb8::Decryptor<aes::Aes128>;

/// The maximum size of a packet.
pub const MAX_PACKET_SIZE: usize = 2_097_152;
/// The maximum size of a packet's data.
pub const MAX_PACKET_DATA_SIZE: usize = 8_388_608;

/// Describes the set of packets a connection understands at a given point.
///
/// A connection always starts out in state [`ConnectionProtocol::Handshake`]. In this state,
/// the client sends its desired protocol using [`crate::packets::handshake::SClientIntention`]. The
/// server then either accepts the connection and switches to the desired
/// protocol, or it disconnects the client (for example, in case of an
/// outdated client).
///
/// Each protocol has a `PacketListener` implementation tied to it for
/// server and client respectively.
///
/// Every packet must correspond to exactly one protocol.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum ConnectionProtocol {
    /// The handshake protocol. This is the initial protocol, in which the client tells the server its intention (i.e. which protocol it wants to use).
    Handshake,
    /// The play protocol. This is the main protocol that is used while "in game" and most normal packets reside in here.
    Play,
    /// The status protocol. This protocol is used when a client pings a server while on the multiplayer screen.
    Status,
    /// The login protocol. This is the first protocol the client switches to to join a server. It handles authentication with the mojang servers. After it is complete, the connection is switched to the PLAY protocol.
    Login,
    /// The configuration protocol. Used for syncing registered registries.
    Config,
}

/// A raw packet.
#[derive(Debug)]
pub struct RawPacket {
    /// The ID of the packet.
    pub id: i32,
    buffer: Box<[u8]>,
    payload_start: u32,
}

impl RawPacket {
    /// Creates a raw packet from an already-separated payload.
    #[must_use]
    pub fn new(id: i32, payload: Vec<u8>) -> Self {
        Self {
            id,
            buffer: payload.into_boxed_slice(),
            payload_start: 0,
        }
    }

    #[expect(
        clippy::cast_possible_truncation,
        reason = "packet buffers are limited to MAX_PACKET_DATA_SIZE bytes"
    )]
    pub(crate) fn from_buffer(id: i32, buffer: Vec<u8>, payload_start: usize) -> Self {
        debug_assert!(payload_start <= buffer.len());
        debug_assert!(payload_start <= MAX_PACKET_DATA_SIZE);
        Self {
            id,
            buffer: buffer.into_boxed_slice(),
            payload_start: payload_start as u32,
        }
    }

    /// Returns the packet payload without its packet ID.
    #[must_use]
    pub fn payload(&self) -> &[u8] {
        &self.buffer[self.payload_start as usize..]
    }
}

/// An error that can occur when handling packets.
#[derive(Error, Debug)]
pub enum PacketError {
    #[error("failed to decode packet ID")]
    /// Failed to decode the packet ID.
    DecodeID,
    #[error("packet length {0} exceeds maximum length")]
    /// The packet length exceeds the maximum length.
    TooLong(usize),
    #[error("packet length is out of bounds")]
    /// The packet length is out of bounds.
    OutOfBounds,
    #[error("malformed packet length VarInt: {0}")]
    /// The packet length `VarInt` is malformed.
    MalformedLength(String),
    #[error("malformed packet value: {0}")]
    /// A value in the packet is malformed.
    MalformedValue(String),
    #[error("failed to decompress packet: {0}")]
    /// Failed to decompress the packet.
    DecompressionFailed(String),
    #[error("failed to compress packet: {0}")]
    /// Failed to compress the packet.
    CompressionFailed(String),
    #[error("packet is uncompressed but greater than the threshold")]
    /// The packet is uncompressed but greater than the threshold.
    NotCompressed,
    #[error("failed to decrypt packet: {0}")]
    /// Failed to decrypt the packet.
    DecryptionFailed(String),
    #[error("failed to encrypt packet: {0}")]
    /// Failed to encrypt the packet.
    EncryptionFailed(String),
    #[error("the connection has closed")]
    /// The connection has closed.
    ConnectionClosed,
    #[error("outbound write timed out")]
    /// An outbound write stalled past its deadline.
    WriteTimeout,
    #[error("{0}")]
    /// An error occurred when sending a packet.
    SendError(String),
    #[error("Error: {0}")]
    /// An other error occurred.
    Other(String),
    #[error("Invalid protocol: {0}")]
    /// The protocol is invalid.
    InvalidProtocol(String),
}

impl From<io::Error> for PacketError {
    fn from(value: io::Error) -> Self {
        //Todo! Define & Handle all cases
        Self::MalformedValue(value.to_string())
    }
}

/// A stream that encrypts data before writing it to the underlying stream.
///
/// Whole buffers are encrypted at once (mirroring vanilla `CipherBase.encipher`) with
/// `std::io::BufWriter` semantics: every written buffer is reported fully consumed and
/// its ciphertext is retained until the inner writer accepts it, so a sink that accepts
/// partially or stalls can never desync the CFB8 stream. Retained ciphertext is drained
/// by subsequent writes, `poll_flush`, and `poll_shutdown`.
pub struct StreamEncryptor<W: AsyncWrite + Unpin> {
    cipher: Aes128Cfb8Enc,
    write: W,
    /// Encrypted bytes for consumed input that have not been handed to `write` yet.
    output: Vec<u8>,
    /// How many bytes of `output` have already been written to `write`.
    written: usize,
}

impl<W: AsyncWrite + Unpin> StreamEncryptor<W> {
    /// Creates a new `StreamEncryptor`.
    pub fn new(cipher: Aes128Cfb8Enc, stream: W) -> Self {
        debug_assert_eq!(Aes128Cfb8Enc::block_size(), 1);
        Self {
            cipher,
            write: stream,
            output: Vec::new(),
            written: 0,
        }
    }

    /// Writes buffered ciphertext to the inner writer until it is drained, pending,
    /// or fails.
    fn poll_drain(this: &mut Self, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        while this.written < this.output.len() {
            match Pin::new(&mut this.write).poll_write(cx, &this.output[this.written..]) {
                Poll::Ready(Ok(0)) => {
                    return Poll::Ready(Err(io::Error::new(
                        io::ErrorKind::WriteZero,
                        "encrypted write made no progress",
                    )));
                }
                Poll::Ready(Ok(written)) => this.written += written,
                Poll::Ready(Err(error)) => return Poll::Ready(Err(error)),
                Poll::Pending => return Poll::Pending,
            }
        }

        this.output.clear();
        this.written = 0;
        Poll::Ready(Ok(()))
    }
}

impl<W: AsyncWrite + Unpin> AsyncWrite for StreamEncryptor<W> {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        let this = self.get_mut();

        // Ciphertext for already consumed bytes must reach the inner writer before new
        // input is encrypted, or the CFB8 stream would interleave two input sources.
        match Self::poll_drain(this, cx) {
            Poll::Ready(Ok(())) => {}
            Poll::Ready(Err(error)) => return Poll::Ready(Err(error)),
            Poll::Pending => return Poll::Pending,
        }

        if buf.is_empty() {
            return Poll::Ready(Ok(0));
        }

        // Encrypt the whole buffer at once instead of one byte per async write, mirroring
        // vanilla `CipherBase.encipher`. CFB8 is a byte-oriented stream cipher, so every
        // byte is one block.
        this.output.clear();
        this.output.extend_from_slice(buf);
        let cipher = &mut this.cipher;
        for chunk in this.output.as_chunks_mut::<1>().0 {
            let mut out = [0u8];
            let in_block: &Array<u8, _> = (&*chunk).into();
            let out_block: &mut Array<u8, _> = (&mut out).into();
            cipher.encrypt_block_b2b(in_block, out_block);
            chunk[0] = out[0];
        }
        this.written = 0;

        // Hand what the sink accepts to the inner writer; the rest is a backlog of
        // consumed bytes that later writes, `poll_flush`, and `poll_shutdown` drain. The
        // whole buffer is always reported consumed: the `AsyncWrite` contract lets the
        // caller discard everything past the returned count, and CFB8 ciphertext is bound
        // to the exact plaintext sequence, so a partially consumed write could neither
        // drop nor re-split its unreported tail. A pending sink keeps the backlog and is
        // retried through the flush paths (`BufWriter` semantics).
        match Self::poll_drain(this, cx) {
            Poll::Ready(Err(error)) => return Poll::Ready(Err(error)),
            Poll::Ready(Ok(())) | Poll::Pending => {}
        }

        Poll::Ready(Ok(buf.len()))
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        let ref_self = self.get_mut();
        // Committed ciphertext must reach the inner writer before the flush resolves,
        // or `flush` could report success while bytes are still buffered.
        match Self::poll_drain(ref_self, cx) {
            Poll::Ready(Ok(())) => {}
            Poll::Ready(Err(error)) => return Poll::Ready(Err(error)),
            Poll::Pending => return Poll::Pending,
        }
        let write = Pin::new(&mut ref_self.write);
        write.poll_flush(cx)
    }

    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        let ref_self = self.get_mut();
        // Committed ciphertext must reach the inner writer before shutdown resolves,
        // or shutdown could silently drop buffered bytes.
        match Self::poll_drain(ref_self, cx) {
            Poll::Ready(Ok(())) => {}
            Poll::Ready(Err(error)) => return Poll::Ready(Err(error)),
            Poll::Pending => return Poll::Pending,
        }
        let write = Pin::new(&mut ref_self.write);
        write.poll_shutdown(cx)
    }
}

/// A stream that decrypts data.
pub struct StreamDecryptor<R: AsyncRead + Unpin> {
    cipher: Aes128Cfb8Dec,
    read: R,
}

impl<R: AsyncRead + Unpin> StreamDecryptor<R> {
    /// Creates a new `StreamDecryptor`.
    pub const fn new(cipher: Aes128Cfb8Dec, stream: R) -> Self {
        Self {
            cipher,
            read: stream,
        }
    }
}

impl<R: AsyncRead + Unpin> AsyncRead for StreamDecryptor<R> {
    #[expect(
        clippy::unwrap_used,
        reason = "CFB8 block size is one byte, so each chunk fits the cipher block type"
    )]
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        let ref_self = self.get_mut();
        let read = Pin::new(&mut ref_self.read);
        let cipher = &mut ref_self.cipher;

        // Get the starting position
        let original_fill = buf.filled().len();
        // Read the raw data
        let internal_poll = read.poll_read(cx, buf);

        if matches!(internal_poll, Poll::Ready(Ok(()))) {
            // Decrypt the raw data in-place, note that our block size is 1 byte, so this is always safe
            for block in buf.filled_mut()[original_fill..].chunks_mut(Aes128Cfb8Dec::block_size()) {
                cipher.decrypt_block(block.try_into().unwrap());
            }
        }

        internal_poll
    }
}

#[cfg(test)]
mod tests {
    use aes::cipher::KeyIvInit;
    use tokio::io::AsyncWriteExt;

    use super::*;

    fn test_cipher() -> Aes128Cfb8Enc {
        Aes128Cfb8Enc::new_from_slices(&[0x42; 16], &[0x07; 16]).expect("valid key and iv")
    }

    /// Reference CFB8 keystream application over a whole buffer, matching vanilla
    /// `CipherBase.encipher`.
    fn encrypt_reference(cipher: &mut Aes128Cfb8Enc, data: &[u8]) -> Vec<u8> {
        let mut output = Vec::with_capacity(data.len());
        for byte in data {
            let in_block: &Array<u8, _> = std::slice::from_ref(byte)
                .try_into()
                .expect("one-byte block");
            let mut out = [0u8];
            let out_block: &mut Array<u8, _> = (&mut out).into();
            cipher.encrypt_block_b2b(in_block, out_block);
            output.push(out[0]);
        }
        output
    }

    #[tokio::test]
    async fn encrypts_whole_buffers_like_vanilla_cipher() {
        let plain: Vec<u8> = (0..256u32).map(|i| (i % 251) as u8).collect();
        let expected = encrypt_reference(&mut test_cipher(), &plain);

        let mut sink = Vec::new();
        {
            let mut encryptor = StreamEncryptor::new(test_cipher(), &mut sink);
            encryptor
                .write_all(&plain)
                .await
                .expect("write should succeed");
        }

        assert_eq!(sink, expected);
    }

    /// A writer that accepts one byte per poll, rejecting the very first poll, to force
    /// `StreamEncryptor` through its partial-write and pending-buffer paths.
    struct OneBytePendingWriter {
        first_poll: bool,
        received: Vec<u8>,
    }

    impl AsyncWrite for OneBytePendingWriter {
        fn poll_write(
            self: Pin<&mut Self>,
            cx: &mut Context<'_>,
            buf: &[u8],
        ) -> Poll<io::Result<usize>> {
            let this = self.get_mut();
            if this.first_poll {
                this.first_poll = false;
                cx.waker().wake_by_ref();
                return Poll::Pending;
            }
            assert!(!buf.is_empty(), "encryptor must never offer an empty slice");
            this.received.push(buf[0]);
            Poll::Ready(Ok(1))
        }

        fn poll_flush(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
            Poll::Ready(Ok(()))
        }

        fn poll_shutdown(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
            Poll::Ready(Ok(()))
        }
    }

    #[tokio::test]
    async fn retains_unwritten_encrypted_bytes_across_partial_writes() {
        let plain: Vec<u8> = (0..64u32).map(|i| (i * 7 % 253) as u8).collect();
        let expected = encrypt_reference(&mut test_cipher(), &plain);

        let mut writer = OneBytePendingWriter {
            first_poll: true,
            received: Vec::new(),
        };
        {
            let mut encryptor = StreamEncryptor::new(test_cipher(), &mut writer);
            encryptor
                .write_all(&plain)
                .await
                .expect("write should succeed");
            encryptor.flush().await.expect("flush should succeed");
        }

        assert_eq!(writer.received, expected);
    }

    /// Accepts one byte per successful poll and pends every other poll (self-waking),
    /// forcing ciphertext retention across separate writes and the flush paths.
    struct TrickleWriter {
        accept_next: bool,
        received: Vec<u8>,
    }

    impl AsyncWrite for TrickleWriter {
        fn poll_write(
            self: Pin<&mut Self>,
            cx: &mut Context<'_>,
            buf: &[u8],
        ) -> Poll<io::Result<usize>> {
            let this = self.get_mut();
            assert!(!buf.is_empty(), "encryptor must never offer an empty slice");
            if !this.accept_next {
                this.accept_next = true;
                cx.waker().wake_by_ref();
                return Poll::Pending;
            }
            this.accept_next = false;
            this.received.push(buf[0]);
            Poll::Ready(Ok(1))
        }

        fn poll_flush(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
            Poll::Ready(Ok(()))
        }

        fn poll_shutdown(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
            Poll::Ready(Ok(()))
        }
    }

    #[tokio::test]
    async fn backpressured_sink_keeps_separate_writes_sequential() {
        let expected = encrypt_reference(&mut test_cipher(), b"abcXY");

        let mut writer = TrickleWriter {
            accept_next: true,
            received: Vec::new(),
        };
        {
            let mut encryptor = StreamEncryptor::new(test_cipher(), &mut writer);
            // Both writes complete while ciphertext is retained; reported consumption is
            // always the full buffer, so the stream must contain both writes in order.
            encryptor
                .write_all(b"abc")
                .await
                .expect("first write should succeed");
            encryptor
                .write_all(b"XY")
                .await
                .expect("second write should succeed");
            encryptor.flush().await.expect("flush should succeed");
        }

        assert_eq!(writer.received, expected);
    }

    #[tokio::test]
    async fn shutdown_drains_retained_ciphertext() {
        let expected = encrypt_reference(&mut test_cipher(), b"abc");

        let mut writer = TrickleWriter {
            accept_next: true,
            received: Vec::new(),
        };
        {
            let mut encryptor = StreamEncryptor::new(test_cipher(), &mut writer);
            encryptor
                .write_all(b"abc")
                .await
                .expect("write should succeed");
            // No flush: shutdown itself must drain the retained ciphertext.
            encryptor
                .shutdown()
                .await
                .expect("shutdown should drain and succeed");
        }

        assert_eq!(writer.received, expected);
    }
}
