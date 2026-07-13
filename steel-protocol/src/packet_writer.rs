//! # Steel Protocol Packet Writer
//!
//! This module contains the implementation of the packet writer.
/*
Credit to https://github.com/Pumpkin-MC/Pumpkin/ for this implementation.
*/

use std::{
    io,
    pin::Pin,
    task::{Context, Poll},
};

use aes::cipher::KeyIvInit;
use thiserror::Error;
use tokio::io::{AsyncWrite, AsyncWriteExt};

use crate::{
    packet_traits::EncodedPacket,
    utils::{Aes128Cfb8Enc, PacketError, StreamEncryptor},
};

// raw -> compress -> encrypt
/// A writer that can encrypt data.
pub enum EncryptionWriter<W: AsyncWrite + Unpin> {
    /// A writer that encrypts data.
    Encrypt(Box<StreamEncryptor<W>>),
    /// A writer that does not encrypt data.
    None(W),
}

impl<W: AsyncWrite + Unpin> EncryptionWriter<W> {
    /// Upgrades the writer to encrypt data.
    ///
    /// # Panics
    /// - If the writer is already encrypting data.
    #[must_use]
    pub fn upgrade(self, cipher: Aes128Cfb8Enc) -> Self {
        match self {
            Self::None(stream) => Self::Encrypt(Box::new(StreamEncryptor::new(cipher, stream))),
            Self::Encrypt(_) => panic!("Cannot upgrade a stream that already has a cipher!"),
        }
    }
}

impl<W: AsyncWrite + Unpin> AsyncWrite for EncryptionWriter<W> {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        match self.get_mut() {
            Self::Encrypt(writer) => {
                let writer = Pin::new(writer);
                writer.poll_write(cx, buf)
            }
            Self::None(writer) => {
                let writer = Pin::new(writer);
                writer.poll_write(cx, buf)
            }
        }
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        match self.get_mut() {
            Self::Encrypt(writer) => {
                let writer = Pin::new(writer);
                writer.poll_flush(cx)
            }
            Self::None(writer) => {
                let writer = Pin::new(writer);
                writer.poll_flush(cx)
            }
        }
    }

    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        match self.get_mut() {
            Self::Encrypt(writer) => {
                let writer = Pin::new(writer);
                writer.poll_shutdown(cx)
            }
            Self::None(writer) => {
                let writer = Pin::new(writer);
                writer.poll_shutdown(cx)
            }
        }
    }
}

/// Encoder: Server -> Client
/// Supports `ZLib` endecoding/compression
/// Supports Aes128 Encryption
pub struct TCPNetworkEncoder<W: AsyncWrite + Unpin> {
    writer: EncryptionWriter<W>,
}

impl<W: AsyncWrite + Unpin> TCPNetworkEncoder<W> {
    /// Creates a new `TCPNetworkEncoder`.
    pub const fn new(writer: W) -> Self {
        Self {
            writer: EncryptionWriter::None(writer),
        }
    }

    /// NOTE: Encryption can only be set; a minecraft stream cannot go back to being unencrypted
    ///
    /// # Panics
    /// - If the stream is already encrypted.
    /// - If the key is invalid.
    pub fn set_encryption(&mut self, key: &[u8; 16]) {
        if matches!(self.writer, EncryptionWriter::Encrypt(_)) {
            panic!("Cannot upgrade a stream that already has a cipher!");
        }
        let cipher = Aes128Cfb8Enc::new_from_slices(key, key).expect("invalid key");
        replace_with::replace_with_or_abort(&mut self.writer, |encoder| encoder.upgrade(cipher));
    }

    /// Writes a packet to the stream.
    ///
    /// # Errors
    /// - If the packet fails to write.
    /// - If the stream fails to flush.
    pub async fn write_packet(&mut self, packet: &EncodedPacket) -> Result<(), PacketError> {
        self.writer
            .write_all(&packet.encoded_data)
            .await
            .map_err(|e| PacketError::EncryptionFailed(e.to_string()))?;

        self.writer
            .flush()
            .await
            .map_err(|e| PacketError::EncryptionFailed(e.to_string()))
    }
}

/// An error that occurs when the compression level is invalid.
#[derive(Error, Debug)]
#[error("Invalid compression Level")]
pub struct CompressionLevelError;

/* TODO: Tests.
#[cfg(test)]
mod tests {
    use std::io::Read;

    use super::*;
    use crate::java::client::status::CStatusResponse;
    use crate::packet::Packet;
    use crate::{ClientPacket, ReadingError};
    use aes::Aes128;
    use cfb8::Decryptor as Cfb8Decryptor;
    use cfb8::cipher::AsyncStreamCipher;
    use flate2::read::ZlibDecoder;
    use pumpkin_data::packet::client:STATUS_STATUS_RESPONSE;
    use pumpkin_macros::packet;
    use serde::Serialize;

    /// Define a custom packet for testing maximum packet size
    #[derive(Serialize)]
    #[packet(STATUS_STATUS_RESPONSE)]
    pub struct MaxSizePacket {
        data: Vec<u8>,
    }

    impl MaxSizePacket {
        pub fn new(size: usize) -> Self {
            Self {
                data: vec![0xAB; size], // Fill with arbitrary data
            }
        }
    }

    /// Helper function to decode a `VarInt` from bytes
    fn decode_varint(buffer: &mut &[u8]) -> Result<i32, ReadingError> {
        Ok(buffer.get_var_int()?.0)
    }

    /// Helper function to decompress data using libdeflater's Zlib decompressor
    fn decompress_zlib(data: &[u8], expected_size: usize) -> Result<Vec<u8>, std::io::Error> {
        assert!(!data.is_empty());
        let mut decompressed = vec![0u8; expected_size];
        ZlibDecoder::new(data).read_exact(&mut decompressed)?;
        Ok(decompressed)
    }

    /// Helper function to decrypt data using AES-128 CFB-8 mode
    fn decrypt_aes128(encrypted_data: &mut [u8], key: &[u8; 16], iv: &[u8; 16]) {
        let decryptor = Cfb8Decryptor::<Aes128>::new_from_slices(key, iv).expect("Invalid key/iv");
        decryptor.decrypt(encrypted_data);
    }

    /// Helper function to build a packet with optional compression and encryption
    async fn build_packet_with_encoder<T: ClientPacket>(
        packet: &T,
        compression_info: Option<(CompressionThreshold, CompressionLevel)>,
        key: Option<&[u8; 16]>,
    ) -> Box<[u8]> {
        let mut buf = Vec::new();
        let mut encoder = TCPNetworkEncoder::new(&mut buf);
        if let Some(compression_info) = compression_info {
            encoder.set_compression(compression_info);
        }

        if let Some(key) = key {
            encoder.set_encryption(key);
        }

        let mut packet_buf = Vec::new();
        let writer = &mut packet_buf;
        writer.write_var_int(&VarInt(T::PACKET_ID)).unwrap();
        packet.write_packet_data(writer).unwrap();

        encoder.write_packet(packet_buf.into()).await.unwrap();

        buf.into_boxed_slice()
    }

    /// Test encoding without compression and encryption
    #[tokio::test]
    async fn test_encode_without_compression_and_encryption() {
        // Create a CStatusResponse packet
        let packet =
            CStatusResponse::new(String::from("{\"description\": \"A Minecraft Server\"}"));

        // Build the packet without compression and encryption
        let packet_bytes = build_packet_with_encoder(&packet, None, None).await;

        // Decode the packet manually to verify correctness
        let mut buffer = &packet_bytes[..];

        // Read packet length VarInt
        let packet_length = decode_varint(&mut buffer).expect("Failed to decode packet length");
        assert_eq!(
            packet_length as usize,
            buffer.len(),
            "Packet length mismatch"
        );

        // Read packet ID VarInt
        let decoded_packet_id = decode_varint(&mut buffer).expect("Failed to decode packet ID");
        assert_eq!(decoded_packet_id, CStatusResponse::PACKET_ID);

        // Remaining buffer is the payload
        // We need to obtain the expected payload
        let mut expected_payload = Vec::new();
        packet.write_packet_data(&mut expected_payload).unwrap();

        assert_eq!(buffer, expected_payload);
    }

    /// Test encoding with compression
    #[tokio::test]
    async fn test_encode_with_compression() {
        // Create a CStatusResponse packet
        let packet =
            CStatusResponse::new("{\"description\": \"A Minecraft Server\"}".parse().unwrap());

        // Build the packet with compression enabled
        let packet_bytes = build_packet_with_encoder(&packet, Some((0, 6)), None).await;

        // Decode the packet manually to verify correctness
        let mut buffer = &packet_bytes[..];

        // Read packet length VarInt
        let packet_length = decode_varint(&mut buffer).expect("Failed to decode packet length");
        assert_eq!(
            packet_length as usize,
            buffer.len(),
            "Packet length mismatch"
        );

        // Read data length VarInt (uncompressed data length)
        let data_length = decode_varint(&mut buffer).expect("Failed to decode data length");
        let mut expected_payload = Vec::new();
        packet.write_packet_data(&mut expected_payload).unwrap();
        let uncompressed_data_length =
            VarInt(CStatusResponse::PACKET_ID).written_size() + expected_payload.len();
        assert_eq!(data_length as usize, uncompressed_data_length);

        // Remaining buffer is the compressed data
        let compressed_data = buffer;

        // Decompress the data
        let decompressed_data = decompress_zlib(compressed_data, data_length as usize)
            .expect("Failed to decompress data");

        // Verify packet ID and payload
        let mut decompressed_buffer = &decompressed_data[..];

        // Read packet ID VarInt
        let decoded_packet_id =
            decode_varint(&mut decompressed_buffer).expect("Failed to decode packet ID");
        assert_eq!(decoded_packet_id, CStatusResponse::PACKET_ID);

        // Remaining buffer is the payload
        assert_eq!(decompressed_buffer, expected_payload);
    }

    /// Test encoding with encryption
    #[tokio::test]
    async fn test_encode_with_encryption() {
        // Create a CStatusResponse packet
        let packet =
            CStatusResponse::new("{\"description\": \"A Minecraft Server\"}".parse().unwrap());

        // Encryption key and IV (IV is the same as key in this case)
        let key = [0x00u8; 16]; // Example key

        // Build the packet with encryption enabled (no compression)
        let mut packet_bytes = build_packet_with_encoder(&packet, None, Some(&key)).await;

        // Decrypt the packet
        decrypt_aes128(&mut packet_bytes, &key, &key);

        // Decode the packet manually to verify correctness
        let mut buffer = &packet_bytes[..];

        // Read packet length VarInt
        let packet_length = decode_varint(&mut buffer).expect("Failed to decode packet length");
        assert_eq!(
            packet_length as usize,
            buffer.len(),
            "Packet length mismatch"
        );

        // Read packet ID VarInt
        let decoded_packet_id = decode_varint(&mut buffer).expect("Failed to decode packet ID");
        assert_eq!(decoded_packet_id, CStatusResponse::PACKET_ID);

        // Remaining buffer is the payload
        let mut expected_payload = Vec::new();
        packet.write_packet_data(&mut expected_payload).unwrap();
        assert_eq!(buffer, expected_payload);
    }

    /// Test encoding with both compression and encryption
    #[tokio::test]
    async fn test_encode_with_compression_and_encryption() {
        // Create a CStatusResponse packet
        let packet =
            CStatusResponse::new("{\"description\": \"A Minecraft Server\"}".parse().unwrap());

        // Encryption key and IV (IV is the same as key in this case)
        let key = [0x01u8; 16]; // Example key

        // Build the packet with both compression and encryption enabled
        // Compression threshold is set to 0 to force compression
        let mut packet_bytes = build_packet_with_encoder(&packet, Some((0, 6)), Some(&key)).await;

        // Decrypt the packet
        decrypt_aes128(&mut packet_bytes, &key, &key);

        // Decode the packet manually to verify correctness
        let mut buffer = &packet_bytes[..];

        // Read packet length VarInt
        let packet_length = decode_varint(&mut buffer).expect("Failed to decode packet length");
        assert_eq!(
            packet_length as usize,
            buffer.len(),
            "Packet length mismatch"
        );

        // Read data length VarInt (uncompressed data length)
        let data_length = decode_varint(&mut buffer).expect("Failed to decode data length");
        let mut expected_payload = Vec::new();
        packet.write_packet_data(&mut expected_payload).unwrap();
        let uncompressed_data_length =
            VarInt(CStatusResponse::PACKET_ID).written_size() + expected_payload.len();
        assert_eq!(data_length as usize, uncompressed_data_length);

        // Remaining buffer is the compressed data
        let compressed_data = buffer;

        // Decompress the data
        let decompressed_data = decompress_zlib(compressed_data, data_length as usize)
            .expect("Failed to decompress data");

        // Verify packet ID and payload
        let mut decompressed_buffer = &decompressed_data[..];

        // Read packet ID VarInt
        let decoded_packet_id =
            decode_varint(&mut decompressed_buffer).expect("Failed to decode packet ID");
        assert_eq!(decoded_packet_id, CStatusResponse::PACKET_ID);

        // Remaining buffer is the payload
        assert_eq!(decompressed_buffer, expected_payload);
    }

    /// Test encoding with zero-length payload
    #[tokio::test]
    async fn test_encode_with_zero_length_payload() {
        // Create a CStatusResponse packet with empty payload
        let packet = CStatusResponse::new(String::from(""));

        // Build the packet without compression and encryption
        let packet_bytes = build_packet_with_encoder(&packet, None, None).await;

        // Decode the packet manually to verify correctness
        let mut buffer = &packet_bytes[..];

        // Read packet length VarInt
        let packet_length = decode_varint(&mut buffer).expect("Failed to decode packet length");
        assert_eq!(
            packet_length as usize,
            buffer.len(),
            "Packet length mismatch"
        );

        // Read packet ID VarInt
        let decoded_packet_id = decode_varint(&mut buffer).expect("Failed to decode packet ID");
        assert_eq!(decoded_packet_id, CStatusResponse::PACKET_ID);

        // Remaining buffer is the payload (empty)
        let mut expected_payload = Vec::new();
        packet.write_packet_data(&mut expected_payload).unwrap();

        assert_eq!(
            buffer.len(),
            expected_payload.len(),
            "Payload length mismatch"
        );
        assert_eq!(buffer, expected_payload);
    }

    /// Test encoding with maximum length payload
    #[tokio::test]
    async fn test_encode_with_maximum_string_length() {
        // Maximum allowed string length is 32767 bytes
        let max_string_length = 32767;
        let payload_str = "A".repeat(max_string_length);
        let packet = CStatusResponse::new(payload_str);

        // Build the packet without compression and encryption
        let packet_bytes = build_packet_with_encoder(&packet, None, None).await;

        // Verify that the packet size does not exceed MAX_PACKET_SIZE as usize
        assert!(
            packet_bytes.len() <= MAX_PACKET_SIZE as usize,
            "Packet size exceeds maximum allowed size"
        );

        // Decode the packet manually to verify correctness
        let mut buffer = &packet_bytes[..];

        // Read packet length VarInt
        let packet_length = decode_varint(&mut buffer).expect("Failed to decode packet length");
        assert_eq!(
            packet_length as usize,
            buffer.len(),
            "Packet length mismatch"
        );

        // Read packet ID VarInt
        let decoded_packet_id = decode_varint(&mut buffer).expect("Failed to decode packet ID");
        // Assume packet ID is 0 for CStatusResponse
        assert_eq!(decoded_packet_id, CStatusResponse::PACKET_ID);

        // Remaining buffer is the payload
        let mut expected_payload = Vec::new();
        packet.write_packet_data(&mut expected_payload).unwrap();

        assert_eq!(buffer, expected_payload);
    }

    /// Test encoding a packet that exceeds MAX_PACKET_SIZE as usize
    #[tokio::test]
    #[should_panic(expected = "TooLong")]
    async fn test_encode_packet_exceeding_maximum_size() {
        // Create a custom packet with data exceeding MAX_PACKET_SIZE as usize
        let data_size = MAX_PACKET_SIZE as usize + 1; // Exceed by 1 byte
        let packet = MaxSizePacket::new(data_size);

        // Build the packet without compression and encryption
        // This should panic with PacketEncodeError::TooLong
        build_packet_with_encoder(&packet, None, None).await;
    }

    /// Test encoding with a small payload that should not be compressed
    #[tokio::test]
    async fn test_encode_small_payload_no_compression() {
        // Create a CStatusResponse packet with small payload
        let packet = CStatusResponse::new(String::from("Hi"));

        // Build the packet with compression enabled
        // Compression threshold is set to a value higher than payload length
        let packet_bytes = build_packet_with_encoder(&packet, Some((10, 6)), None).await;

        // Decode the packet manually to verify that it was not compressed
        let mut buffer = &packet_bytes[..];

        // Read packet length VarInt
        let packet_length = decode_varint(&mut buffer).expect("Failed to decode packet length");
        assert_eq!(
            packet_length as usize,
            buffer.len(),
            "Packet length mismatch"
        );

        // Read data length VarInt (should be 0 indicating no compression)
        let data_length = decode_varint(&mut buffer).expect("Failed to decode data length");
        assert_eq!(
            data_length, 0,
            "Data length should be 0 indicating no compression"
        );

        // Read packet ID VarInt
        let decoded_packet_id = decode_varint(&mut buffer).expect("Failed to decode packet ID");
        assert_eq!(decoded_packet_id, CStatusResponse::PACKET_ID);

        // Remaining buffer is the payload
        let mut expected_payload = Vec::new();
        packet.write_packet_data(&mut expected_payload).unwrap();

        assert_eq!(buffer, expected_payload);
    }
}
*/
