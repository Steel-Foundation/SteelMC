//! # Steel Protocol Packet Reader
//!
//! This module contains the implementation of the packet reader.
/*
Credit to https://github.com/Pumpkin-MC/Pumpkin/ for this implementation.
*/

use std::{
    io::{self, Read},
    num::NonZeroU32,
    pin::Pin,
    task::{Context, Poll},
};

use aes::cipher::KeyIvInit;
use flate2::read::ZlibDecoder;
use steel_utils::codec::VarInt;
use steel_utils::serial::ReadFrom;
use tokio::io::{AsyncRead, AsyncReadExt, ReadBuf};

use crate::utils::{
    Aes128Cfb8Dec, MAX_PACKET_DATA_SIZE, MAX_PACKET_SIZE, PacketError, RawPacket, StreamDecryptor,
};

/// A reader that can decrypt data.
pub enum DecryptionReader<R: AsyncRead + Unpin> {
    /// A reader that decrypts data.
    Decrypt(Box<StreamDecryptor<R>>),
    /// A reader that does not decrypt data.
    None(R),
}

impl<R: AsyncRead + Unpin> DecryptionReader<R> {
    /// Upgrades the reader to decrypt data.
    ///
    /// # Panics
    /// - If the reader is already decrypting data.
    #[must_use]
    pub fn upgrade(self, cipher: Aes128Cfb8Dec) -> Self {
        match self {
            Self::None(stream) => Self::Decrypt(Box::new(StreamDecryptor::new(cipher, stream))),
            Self::Decrypt(_) => panic!("Cannot upgrade a stream that already has a cipher!"),
        }
    }
}

impl<R: AsyncRead + Unpin> AsyncRead for DecryptionReader<R> {
    #[inline]
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        match self.get_mut() {
            Self::Decrypt(reader) => {
                let reader = Pin::new(reader);
                reader.poll_read(cx, buf)
            }
            Self::None(reader) => {
                let reader = Pin::new(reader);
                reader.poll_read(cx, buf)
            }
        }
    }
}

/// Decoder: Client -> Server
/// Supports `ZLib` decoding/decompression
/// Supports Aes128 Encryption
pub struct TCPNetworkDecoder<R: AsyncRead + Unpin> {
    reader: DecryptionReader<R>,
    compression: Option<NonZeroU32>,
}

impl<R: AsyncRead + Unpin> TCPNetworkDecoder<R> {
    /// Creates a new `TCPNetworkDecoder`.
    pub const fn new(reader: R) -> Self {
        Self {
            reader: DecryptionReader::None(reader),
            compression: None,
        }
    }

    /// Sets the compression threshold for the decoder.
    pub const fn set_compression(&mut self, threshold: NonZeroU32) {
        self.compression = Some(threshold);
    }

    /// NOTE: Encryption can only be set; a minecraft stream cannot go back to being unencrypted
    ///
    /// # Panics
    /// - If the reader is already decrypting data.
    /// - If the key is invalid.
    pub fn set_encryption(&mut self, key: &[u8; 16]) {
        if matches!(self.reader, DecryptionReader::Decrypt(_)) {
            panic!("Cannot upgrade a stream that already has a cipher!");
        }
        let cipher = Aes128Cfb8Dec::new_from_slices(key, key).expect("invalid key");
        replace_with::replace_with_or_abort(&mut self.reader, |decoder| decoder.upgrade(cipher));
    }

    /// Gets a raw packet from the stream.
    ///
    /// # Errors
    /// - If the packet length is invalid.
    /// - If the packet is too long.
    /// - If the packet is not compressed when it should be.
    /// - If the packet fails to decompress.
    #[expect(clippy::cast_sign_loss)]
    pub async fn get_raw_packet(&mut self) -> Result<RawPacket, PacketError> {
        let packet_len = VarInt::read_async(&mut self.reader).await? as usize;

        if packet_len > MAX_PACKET_SIZE {
            Err(PacketError::OutOfBounds)?;
        }

        // Read the entire packet data into a buffer
        let mut packet_data = vec![0u8; packet_len];
        self.reader
            .read_exact(&mut packet_data)
            .await
            .map_err(|e| PacketError::Other(e.to_string()))?;

        let mut cursor = io::Cursor::new(packet_data.as_slice());

        let (packet_buffer, packet_data_start) = if let Some(threshold) = self.compression {
            let decompressed_len = VarInt::read(&mut cursor)?.0 as usize;
            let raw_packet_len = packet_len - VarInt::written_size(decompressed_len as i32);

            if decompressed_len > MAX_PACKET_DATA_SIZE {
                Err(PacketError::TooLong(decompressed_len))?;
            }

            if decompressed_len > 0 {
                // Decompress the remaining data
                let mut decompressed = Vec::with_capacity(decompressed_len);
                ZlibDecoder::new(&mut cursor)
                    .read_to_end(&mut decompressed)
                    .map_err(|e| PacketError::DecompressionFailed(e.to_string()))?;
                (decompressed, 0)
            } else {
                // Validate that we are not less than the compression threshold
                if raw_packet_len > threshold.get() as _ {
                    Err(PacketError::NotCompressed)?;
                }

                // The rest of the packet data is uncompressed.
                let packet_data_start = cursor.position() as usize;
                (packet_data, packet_data_start)
            }
        } else {
            (packet_data, 0)
        };

        // Parse the packet ID while retaining the buffer that already owns the payload.
        let mut cursor = io::Cursor::new(&packet_buffer[packet_data_start..]);
        let packet_id = VarInt::read(&mut cursor)?.0;
        let payload_start = packet_data_start + cursor.position() as usize;

        Ok(RawPacket::from_buffer(
            packet_id,
            packet_buffer,
            payload_start,
        ))
    }
}

#[cfg(test)]
mod payload_tests {
    use std::{io::Write as _, num::NonZeroU32};

    use flate2::{Compression, write::ZlibEncoder};

    use super::TCPNetworkDecoder;

    const VARINT_DATA_MASK: u8 = 0x7f;
    const VARINT_CONTINUE_BIT: u8 = 0x80;

    fn write_varint(mut value: u32, output: &mut Vec<u8>) {
        loop {
            if value <= u32::from(VARINT_DATA_MASK) {
                output.push(value as u8);
                return;
            }
            output.push((value as u8 & VARINT_DATA_MASK) | VARINT_CONTINUE_BIT);
            value >>= 7;
        }
    }

    fn packet_data(packet_id: u32, payload: &[u8]) -> Vec<u8> {
        let mut data = Vec::with_capacity(payload.len() + 5);
        write_varint(packet_id, &mut data);
        data.extend_from_slice(payload);
        data
    }

    fn frame(data: &[u8]) -> Vec<u8> {
        let mut framed = Vec::with_capacity(data.len() + 5);
        write_varint(data.len() as u32, &mut framed);
        framed.extend_from_slice(data);
        framed
    }

    #[tokio::test]
    async fn payload_slice_starts_after_framing_and_packet_id() {
        let packet_id = 300;
        let payload = b"payload bytes";
        let frame = frame(&packet_data(packet_id, payload));
        let mut decoder = TCPNetworkDecoder::new(frame.as_slice());

        let packet = decoder.get_raw_packet().await.expect("decode plain packet");

        assert_eq!(packet.id, packet_id as i32);
        assert_eq!(packet.payload(), payload);
    }

    #[tokio::test]
    async fn uncompressed_payload_slice_skips_compression_length() {
        let packet_id = 300;
        let payload = b"payload bytes";
        let packet_data = packet_data(packet_id, payload);
        let mut compression_data = Vec::with_capacity(packet_data.len() + 1);
        write_varint(0, &mut compression_data);
        compression_data.extend_from_slice(&packet_data);
        let frame = frame(&compression_data);
        let mut decoder = TCPNetworkDecoder::new(frame.as_slice());
        decoder.set_compression(NonZeroU32::new(256).expect("nonzero threshold"));

        let packet = decoder
            .get_raw_packet()
            .await
            .expect("decode uncompressed packet");

        assert_eq!(packet.id, packet_id as i32);
        assert_eq!(packet.payload(), payload);
    }

    #[tokio::test]
    async fn decompressed_payload_slice_starts_after_packet_id() {
        let packet_id = 300;
        let payload = vec![0x5a; 1_024];
        let packet_data = packet_data(packet_id, &payload);
        let mut encoder = ZlibEncoder::new(Vec::new(), Compression::fast());
        encoder
            .write_all(&packet_data)
            .expect("compress packet data");
        let compressed = encoder.finish().expect("finish packet compression");
        let mut compression_data = Vec::with_capacity(compressed.len() + 5);
        write_varint(packet_data.len() as u32, &mut compression_data);
        compression_data.extend_from_slice(&compressed);
        let frame = frame(&compression_data);
        let mut decoder = TCPNetworkDecoder::new(frame.as_slice());
        decoder.set_compression(NonZeroU32::new(256).expect("nonzero threshold"));

        let packet = decoder
            .get_raw_packet()
            .await
            .expect("decode compressed packet");

        assert_eq!(packet.id, packet_id as i32);
        assert_eq!(packet.payload(), payload);
    }
}

/* TODO: Tests.
#[cfg(test)]
mod tests {

    use std::io::Write;

    use super::*;
    use aes::Aes128;
    use cfb8::Encryptor as Cfb8Encryptor;
    use cfb8::cipher::AsyncStreamCipher;
    use flate2::Compression;
    use flate2::write::ZlibEncoder;

    /// Helper function to compress data using libdeflater's Zlib compressor
    fn compress_zlib(data: &[u8]) -> Vec<u8> {
        let mut compressed = Vec::new();
        ZlibEncoder::new(&mut compressed, Compression::default())
            .write_all(data)
            .unwrap();
        compressed
    }

    /// Helper function to encrypt data using AES-128 CFB-8 mode
    fn encrypt_aes128(data: &mut [u8], key: &[u8; 16], iv: &[u8; 16]) {
        let encryptor = Cfb8Encryptor::<Aes128>::new_from_slices(key, iv).expect("Invalid key/iv");
        encryptor.encrypt(data);
    }

    /// Helper function to build a packet with optional compression and encryption
    fn build_packet(
        packet_id: i32,
        payload: &[u8],
        compress: bool,
        key: Option<&[u8; 16]>,
        iv: Option<&[u8; 16]>,
    ) -> Vec<u8> {
        let mut buffer = Vec::new();

        if compress {
            // Create a buffer that includes `packet_id_varint` and payload
            let mut data_to_compress = Vec::new();
            let packet_id_varint = VarInt(packet_id);
            data_to_compress.write_var_int(&packet_id_varint).unwrap();
            data_to_compress.write_slice(payload).unwrap();

            // Compress the combined data
            let compressed_payload = compress_zlib(&data_to_compress);
            let data_len = data_to_compress.len() as i32; // 1 + payload.len()
            let data_len_varint = VarInt(data_len);
            buffer.write_var_int(&data_len_varint).unwrap();
            buffer.write_slice(&compressed_payload).unwrap();
        } else {
            // No compression; `data_len` is payload length
            let packet_id_varint = VarInt(packet_id);
            buffer.write_var_int(&packet_id_varint).unwrap();
            buffer.write_slice(payload).unwrap();
        }

        // Calculate packet length: length of buffer
        let packet_len = buffer.len() as i32;
        let packet_len_varint = VarInt(packet_len);
        let mut packet_length_encoded = Vec::new();
        {
            packet_len_varint
                .encode(&mut packet_length_encoded)
                .unwrap();
        }

        // Create a new buffer for the entire packet
        let mut packet = Vec::new();
        packet.extend_from_slice(&packet_length_encoded);
        packet.extend_from_slice(&buffer);

        // Encrypt if key and IV are provided.
        if let (Some(k), Some(v)) = (key, iv) {
            encrypt_aes128(&mut packet, k, v);
            packet
        } else {
            packet
        }
    }

    /// Test decoding without compression and encryption
    #[tokio::test]
    async fn test_decode_without_compression_and_encryption() {
        // Sample packet data: packet_id = 1, payload = "Hello"
        let packet_id = 1;
        let payload = b"Hello";

        // Build the packet without compression and encryption
        let packet = build_packet(packet_id, payload, false, None, None);

        // Initialize the decoder without compression and encryption
        let mut decoder = TCPNetworkDecoder::new(packet.as_slice());

        // Attempt to decode
        let raw_packet = decoder.get_raw_packet().await.expect("Decoding failed");

        assert_eq!(raw_packet.id, packet_id);
        assert_eq!(raw_packet.payload.as_ref(), payload);
    }

    /// Test decoding with compression
    #[tokio::test]
    async fn test_decode_with_compression() {
        // Sample packet data: packet_id = 2, payload = "Hello, compressed world!"
        let packet_id = 2;
        let payload = b"Hello, compressed world!";

        // Build the packet with compression enabled
        let packet = build_packet(packet_id, payload, true, None, None);

        // Initialize the decoder with compression enabled
        let mut decoder = TCPNetworkDecoder::new(packet.as_slice());
        // Larger than payload
        decoder.set_compression(1000);

        // Attempt to decode
        let raw_packet = decoder.get_raw_packet().await.expect("Decoding failed");

        assert_eq!(raw_packet.id, packet_id);
        assert_eq!(raw_packet.payload.as_ref(), payload);
    }

    /// Test decoding with encryption
    #[tokio::test]
    async fn test_decode_with_encryption() {
        // Sample packet data: packet_id = 3, payload = "Hello, encrypted world!"
        let packet_id = 3;
        let payload = b"Hello, encrypted world!";

        // Define encryption key and IV
        let key = [0x00u8; 16]; // Example key

        // Build the packet with encryption enabled (no compression)
        let packet = build_packet(packet_id, payload, false, Some(&key), Some(&key));

        // Initialize the decoder with encryption enabled
        let mut decoder = TCPNetworkDecoder::new(packet.as_slice());
        decoder.set_encryption(&key);

        // Attempt to decode
        let raw_packet = decoder.get_raw_packet().await.expect("Decoding failed");

        assert_eq!(raw_packet.id, packet_id);
        assert_eq!(raw_packet.payload.as_ref(), payload);
    }

    /// Test decoding with both compression and encryption
    #[tokio::test]
    async fn test_decode_with_compression_and_encryption() {
        // Sample packet data: packet_id = 4, payload = "Hello, compressed and encrypted world!"
        let packet_id = 4;
        let payload = b"Hello, compressed and encrypted world!";

        // Define encryption key and IV
        let key = [0x01u8; 16]; // Example key
        let iv = [0x01u8; 16]; // Example IV

        // Build the packet with both compression and encryption enabled
        let packet = build_packet(packet_id, payload, true, Some(&key), Some(&iv));

        // Initialize the decoder with both compression and encryption enabled
        let mut decoder = TCPNetworkDecoder::new(packet.as_slice());
        decoder.set_compression(1000);
        decoder.set_encryption(&key);

        // Attempt to decode
        let raw_packet = decoder.get_raw_packet().await.expect("Decoding failed");

        assert_eq!(raw_packet.id, packet_id);
        assert_eq!(raw_packet.payload.as_ref(), payload);
    }

    /// Test decoding with invalid compressed data
    #[tokio::test]
    async fn test_decode_with_invalid_compressed_data() {
        // Sample packet data: packet_id = 5, payload_len = 10, but compressed data is invalid
        let data_len = 10; // Expected decompressed size
        let invalid_compressed_data = vec![0xFF, 0xFF, 0xFF]; // Invalid Zlib data

        // Build the packet with compression enabled but invalid compressed data
        let mut buffer = Vec::new();
        let data_len_varint = VarInt(data_len);
        buffer.write_var_int(&data_len_varint).unwrap();
        buffer.write_slice(&invalid_compressed_data).unwrap();

        // Calculate packet length: VarInt(data_len) + invalid compressed data
        let packet_len = buffer.len() as i32;
        let packet_len_varint = VarInt(packet_len);

        // Create a new buffer for the entire packet
        let mut packet_buffer = Vec::new();
        packet_buffer.write_var_int(&packet_len_varint).unwrap();
        packet_buffer.write_slice(&buffer).unwrap();

        let packet_bytes = packet_buffer;

        // Initialize the decoder with compression enabled
        let mut decoder = TCPNetworkDecoder::new(&packet_bytes[..]);
        decoder.set_compression(1000);

        // Attempt to decode and expect a decompression error
        let result = decoder.get_raw_packet().await;

        if result.is_ok() {
            panic!("This should have errored!");
        }
    }

    /// Test decoding with a zero-length packet
    #[tokio::test]
    async fn test_decode_with_zero_length_packet() {
        // Sample packet data: packet_id = 7, payload = "" (empty)
        let packet_id = 7;
        let payload = b"";

        // Build the packet without compression and encryption
        let packet = build_packet(packet_id, payload, false, None, None);

        // Initialize the decoder without compression and encryption
        let mut decoder = TCPNetworkDecoder::new(packet.as_slice());

        // Attempt to decode and expect a read error
        let raw_packet = decoder.get_raw_packet().await.unwrap();
        assert_eq!(raw_packet.id, packet_id);
        assert_eq!(raw_packet.payload.as_ref(), payload);
    }

    /// Test decoding with maximum length packet
    #[tokio::test]
    async fn test_decode_with_maximum_length_packet() {
        // Sample packet data: packet_id = 8, payload = "A" repeated MAX_PACKET_SIZE times
        // Sample packet data: packet_id = 8, payload = "A" repeated (MAX_PACKET_SIZE - 1) times
        let packet_id = 8;
        let payload = vec![0x41u8; MAX_PACKET_SIZE as usize - 1]; // "A" repeated

        // Build the packet with compression enabled
        let packet = build_packet(packet_id, &payload, true, None, None);
        println!("Built packet (with compression, maximum length): {packet:?}");

        // Initialize the decoder with compression enabled
        let mut decoder = TCPNetworkDecoder::new(packet.as_slice());
        decoder.set_compression(MAX_PACKET_SIZE as usize + 1);

        // Attempt to decode
        let result = decoder.get_raw_packet().await;

        let raw_packet = result.unwrap();
        assert_eq!(raw_packet.id, packet_id);
        assert_eq!(raw_packet.payload.as_ref(), payload);
    }
}
 */
