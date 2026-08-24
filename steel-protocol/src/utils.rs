//! # Steel Protocol Utils
//! Utility functions and types for the protocol.

use std::{
    io,
    pin::Pin,
    task::{Context, Poll, ready},
};

use aes::cipher::{
    Array, BlockCipherEncBackend, BlockCipherEncClosure, BlockCipherEncrypt, BlockModeDecrypt,
    BlockSizeUser, KeyInit, consts::U16,
};
use thiserror::Error;
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};

/// An AES-128 CFB-8 decryptor.
pub type Aes128Cfb8Dec = cfb8::Decryptor<aes::Aes128>;

// This was the smallest release-profiled window that matched larger 1-16 KiB windows.
const ENCRYPTION_BUFFER_SIZE: usize = 256;

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

struct Cfb8Encryptor {
    cipher: aes::Aes128Enc,
    state: u128,
}

impl Cfb8Encryptor {
    fn new(key: &[u8; 16], iv: &[u8; 16]) -> Self {
        Self {
            cipher: aes::Aes128Enc::new(key.into()),
            state: u128::from_be_bytes(*iv),
        }
    }

    fn encrypt(&mut self, bytes: &mut [u8]) {
        struct EncryptClosure<'a> {
            state: &'a mut u128,
            bytes: &'a mut [u8],
        }

        impl BlockSizeUser for EncryptClosure<'_> {
            type BlockSize = U16;
        }

        impl BlockCipherEncClosure for EncryptClosure<'_> {
            #[inline]
            fn call<B: BlockCipherEncBackend<BlockSize = U16>>(self, backend: &B) {
                for byte in self.bytes {
                    let mut encrypted_state = Array::from(self.state.to_be_bytes());
                    backend.encrypt_block_inplace(&mut encrypted_state);
                    let keystream_byte = encrypted_state[0];
                    *byte ^= keystream_byte;
                    // CFB8 drops the most-significant state byte and appends the ciphertext byte.
                    *self.state = self.state.wrapping_shl(u8::BITS) | u128::from(*byte);
                }
            }
        }

        self.cipher.encrypt_with_backend(EncryptClosure {
            state: &mut self.state,
            bytes,
        });
    }
}

/// An encrypted writer with a bounded, reusable ciphertext buffer.
pub(crate) struct StreamEncryptor<W: AsyncWrite + Unpin> {
    cipher: Cfb8Encryptor,
    write: W,
    pending: [u8; ENCRYPTION_BUFFER_SIZE],
    pending_start: usize,
    pending_len: usize,
}

impl<W: AsyncWrite + Unpin> StreamEncryptor<W> {
    pub(crate) fn new(shared_secret: &[u8; 16], stream: W) -> Self {
        Self {
            cipher: Cfb8Encryptor::new(shared_secret, shared_secret),
            write: stream,
            pending: [0; ENCRYPTION_BUFFER_SIZE],
            pending_start: 0,
            pending_len: 0,
        }
    }

    fn poll_drain(&mut self, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        while self.pending_start < self.pending_len {
            let remaining = &self.pending[self.pending_start..self.pending_len];
            let written = match ready!(Pin::new(&mut self.write).poll_write(cx, remaining)) {
                Ok(0) => return Poll::Ready(Err(io::ErrorKind::WriteZero.into())),
                Ok(written) => written,
                Err(error) => return Poll::Ready(Err(error)),
            };
            self.pending_start += written;
        }

        self.pending_start = 0;
        self.pending_len = 0;
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
        ready!(this.poll_drain(cx))?;

        let accepted = buf.len().min(ENCRYPTION_BUFFER_SIZE);
        this.pending[..accepted].copy_from_slice(&buf[..accepted]);
        this.cipher.encrypt(&mut this.pending[..accepted]);
        this.pending_len = accepted;
        Poll::Ready(Ok(accepted))
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        let this = self.get_mut();
        ready!(this.poll_drain(cx))?;
        Pin::new(&mut this.write).poll_flush(cx)
    }

    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        let this = self.get_mut();
        ready!(this.poll_drain(cx))?;
        Pin::new(&mut this.write).poll_shutdown(cx)
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
mod stream_encryptor_tests {
    use std::{
        collections::VecDeque,
        future::Future,
        io,
        pin::Pin,
        task::{Context, Poll, Waker},
    };

    use aes::cipher::KeyIvInit;
    use tokio::io::{AsyncWrite, AsyncWriteExt};

    use super::{Cfb8Encryptor, ENCRYPTION_BUFFER_SIZE, StreamEncryptor};

    const KEY: [u8; 16] = [
        0x2b, 0x7e, 0x15, 0x16, 0x28, 0xae, 0xd2, 0xa6, 0xab, 0xf7, 0x15, 0x88, 0x09, 0xcf, 0x4f,
        0x3c,
    ];
    const IV: [u8; 16] = [
        0x00, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0a, 0x0b, 0x0c, 0x0d, 0x0e,
        0x0f,
    ];

    enum WriteStep {
        Pending,
        Limit(usize),
        Zero,
        Error,
    }

    #[derive(Default)]
    struct ScriptedWriter {
        steps: VecDeque<WriteStep>,
        bytes: Vec<u8>,
        flushes: usize,
        shutdowns: usize,
    }

    impl ScriptedWriter {
        fn with_steps(steps: impl IntoIterator<Item = WriteStep>) -> Self {
            Self {
                steps: steps.into_iter().collect(),
                ..Self::default()
            }
        }
    }

    impl AsyncWrite for ScriptedWriter {
        fn poll_write(
            self: Pin<&mut Self>,
            cx: &mut Context<'_>,
            bytes: &[u8],
        ) -> Poll<io::Result<usize>> {
            let this = self.get_mut();
            match this.steps.pop_front() {
                Some(WriteStep::Pending) => {
                    cx.waker().wake_by_ref();
                    Poll::Pending
                }
                Some(WriteStep::Limit(limit)) => {
                    let written = bytes.len().min(limit);
                    this.bytes.extend_from_slice(&bytes[..written]);
                    Poll::Ready(Ok(written))
                }
                Some(WriteStep::Zero) => Poll::Ready(Ok(0)),
                Some(WriteStep::Error) => {
                    Poll::Ready(Err(io::Error::other("scripted write failure")))
                }
                None => {
                    this.bytes.extend_from_slice(bytes);
                    Poll::Ready(Ok(bytes.len()))
                }
            }
        }

        fn poll_flush(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
            self.get_mut().flushes += 1;
            Poll::Ready(Ok(()))
        }

        fn poll_shutdown(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
            self.get_mut().shutdowns += 1;
            Poll::Ready(Ok(()))
        }
    }

    fn deterministic_bytes(len: usize, seed: u32) -> Vec<u8> {
        let mut state = seed.max(1);
        (0..len)
            .map(|_| {
                state ^= state << 13;
                state ^= state >> 17;
                state ^= state << 5;
                state as u8
            })
            .collect()
    }

    fn reference_ciphertext(plaintext: &[u8], iv: &[u8; 16]) -> Vec<u8> {
        let mut ciphertext = plaintext.to_vec();
        let mut cipher = cfb8::Encryptor::<aes::Aes128>::new(&KEY.into(), iv.into());
        cipher.encrypt(&mut ciphertext);
        ciphertext
    }

    #[test]
    fn matches_nist_cfb8_vector() {
        let mut plaintext = [
            0x6b, 0xc1, 0xbe, 0xe2, 0x2e, 0x40, 0x9f, 0x96, 0xe9, 0x3d, 0x7e, 0x11, 0x73, 0x93,
            0x17, 0x2a, 0xae, 0x2d,
        ];
        let expected = [
            0x3b, 0x79, 0x42, 0x4c, 0x9c, 0x0d, 0xd4, 0x36, 0xba, 0xce, 0x9e, 0x0e, 0xd4, 0x58,
            0x6a, 0x4f, 0x32, 0xb9,
        ];

        Cfb8Encryptor::new(&KEY, &IV).encrypt(&mut plaintext);

        assert_eq!(plaintext, expected);
    }

    #[test]
    fn matches_rustcrypto_across_segmentations() {
        const LENGTHS: [usize; 11] = [0, 1, 2, 15, 16, 17, 127, 4_095, 4_096, 4_097, 65_537];
        const SEGMENT_PATTERNS: [&[usize]; 5] = [
            &[1],
            &[2, 3, 5, 7],
            &[15, 16, 17],
            &[255, 4_096],
            &[8_191, 31, 1_024],
        ];

        for len in LENGTHS {
            let plaintext = deterministic_bytes(len, len as u32 + 1);
            let expected = reference_ciphertext(&plaintext, &IV);

            for pattern in SEGMENT_PATTERNS {
                let mut actual = plaintext.clone();
                let mut cipher = Cfb8Encryptor::new(&KEY, &IV);
                let mut offset = 0;
                for segment_len in pattern.iter().copied().cycle() {
                    if offset == actual.len() {
                        break;
                    }
                    let end = (offset + segment_len).min(actual.len());
                    cipher.encrypt(&mut actual[offset..end]);
                    offset = end;
                }

                assert_eq!(actual, expected, "length {len}, pattern {pattern:?}");
            }
        }
    }

    #[tokio::test]
    async fn handles_pending_partial_writes_and_packet_boundaries() {
        let writer = ScriptedWriter::with_steps([
            WriteStep::Pending,
            WriteStep::Limit(3),
            WriteStep::Pending,
            WriteStep::Limit(17),
            WriteStep::Limit(1_023),
        ]);
        let mut encryptor = StreamEncryptor::new(&KEY, writer);
        let packets = [
            deterministic_bytes(31, 1),
            deterministic_bytes(ENCRYPTION_BUFFER_SIZE + 37, 2),
            deterministic_bytes(65_537, 3),
        ];
        let plaintext: Vec<_> = packets.iter().flatten().copied().collect();

        for packet in &packets {
            encryptor
                .write_all(packet)
                .await
                .expect("packet should be accepted");
            encryptor.flush().await.expect("packet should flush");
        }

        assert_eq!(
            encryptor.write.bytes,
            reference_ciphertext(&plaintext, &KEY)
        );
        assert_eq!(encryptor.write.flushes, packets.len());
    }

    #[tokio::test]
    async fn canceled_write_keeps_only_the_accepted_prefix() {
        let writer = ScriptedWriter::with_steps([WriteStep::Pending]);
        let mut encryptor = StreamEncryptor::new(&KEY, writer);
        let interrupted = deterministic_bytes(ENCRYPTION_BUFFER_SIZE + 37, 4);
        let following = deterministic_bytes(113, 5);

        let mut write = Box::pin(encryptor.write_all(&interrupted));
        let mut context = Context::from_waker(Waker::noop());
        assert!(write.as_mut().poll(&mut context).is_pending());
        drop(write);

        encryptor
            .write_all(&following)
            .await
            .expect("following write should succeed");
        encryptor.flush().await.expect("ciphertext should flush");

        let accepted_plaintext: Vec<_> = interrupted[..ENCRYPTION_BUFFER_SIZE]
            .iter()
            .chain(&following)
            .copied()
            .collect();
        assert_eq!(
            encryptor.write.bytes,
            reference_ciphertext(&accepted_plaintext, &KEY)
        );
    }

    #[tokio::test]
    async fn write_zero_and_error_leave_ciphertext_pending_for_retry() {
        for failure in [WriteStep::Zero, WriteStep::Error] {
            let writer = ScriptedWriter::with_steps([failure]);
            let mut encryptor = StreamEncryptor::new(&KEY, writer);
            let plaintext = deterministic_bytes(ENCRYPTION_BUFFER_SIZE, 6);
            encryptor
                .write_all(&plaintext)
                .await
                .expect("plaintext should be accepted");

            assert!(encryptor.flush().await.is_err());
            encryptor
                .flush()
                .await
                .expect("retry should drain ciphertext");

            assert_eq!(
                encryptor.write.bytes,
                reference_ciphertext(&plaintext, &KEY)
            );
            assert_eq!(encryptor.write.flushes, 1);
        }
    }

    #[tokio::test]
    async fn shutdown_drains_pending_ciphertext_first() {
        let mut encryptor = StreamEncryptor::new(&KEY, ScriptedWriter::default());
        let plaintext = deterministic_bytes(1_025, 7);
        encryptor
            .write_all(&plaintext)
            .await
            .expect("plaintext should be accepted");

        encryptor.shutdown().await.expect("shutdown should succeed");

        assert_eq!(
            encryptor.write.bytes,
            reference_ciphertext(&plaintext, &KEY)
        );
        assert_eq!(encryptor.write.shutdowns, 1);
    }
}
