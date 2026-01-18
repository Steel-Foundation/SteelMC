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
    /// Could be a `[Box<[u8]>]` but that requires a realloc `if cap != len`
    pub payload: Vec<u8>,
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

///NOTE: This makes lots of small writes; make sure there is a buffer somewhere down the line
pub struct StreamEncryptor<W: AsyncWrite + Unpin> {
    cipher: Aes128Cfb8Enc,
    write: W,
    last_unwritten_encrypted_byte: Option<u8>,
}

impl<W: AsyncWrite + Unpin> StreamEncryptor<W> {
    /// Creates a new `StreamEncryptor`.
    pub fn new(cipher: Aes128Cfb8Enc, stream: W) -> Self {
        debug_assert_eq!(Aes128Cfb8Enc::block_size(), 1);
        Self {
            cipher,
            write: stream,
            last_unwritten_encrypted_byte: None,
        }
    }
}

impl<W: AsyncWrite + Unpin> AsyncWrite for StreamEncryptor<W> {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        let ref_self = self.get_mut();
        let cipher = &mut ref_self.cipher;

        let mut total_written = 0;
        // Decrypt the raw data, note that our block size is 1 byte, so this is always safe
        for block in buf.chunks(Aes128Cfb8Enc::block_size()) {
            let mut out = [0u8];

            if let Some(out_to_use) = ref_self.last_unwritten_encrypted_byte {
                // This assumes that this `poll_write` is called on the same stream of bytes which I
                // think is a fair assumption, since thats an invariant for the TCP stream anyway.

                // This should never panic
                out[0] = out_to_use;
            } else {
                // This is a stream cipher, so this value must be used
                let out_block: &mut Array<u8, _> = (&mut out).into();
                cipher.encrypt_block_b2b(block.try_into().unwrap(), out_block);
            }

            let write = Pin::new(&mut ref_self.write);
            match write.poll_write(cx, &out) {
                Poll::Pending => {
                    ref_self.last_unwritten_encrypted_byte = Some(out[0]);
                    if total_written == 0 {
                        //If we didn't write anything, return pending
                        return Poll::Pending;
                    }
                    // Otherwise, we actually did write something
                    return Poll::Ready(Ok(total_written));
                }
                Poll::Ready(result) => {
                    ref_self.last_unwritten_encrypted_byte = None;
                    match result {
                        Ok(written) => total_written += written,
                        Err(err) => return Poll::Ready(Err(err)),
                    }
                }
            }
        }

        Poll::Ready(Ok(total_written))
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        let ref_self = self.get_mut();
        let write = Pin::new(&mut ref_self.write);
        write.poll_flush(cx)
    }

    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        let ref_self = self.get_mut();
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
    pub fn new(cipher: Aes128Cfb8Dec, stream: R) -> Self {
        Self {
            cipher,
            read: stream,
        }
    }
}

impl<R: AsyncRead + Unpin> AsyncRead for StreamDecryptor<R> {
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
