//! Shared zstd framing policy for persisted data.
//!
//! Region files and player data files both persist wincode payloads inside zstd
//! frames. Framing them through this module keeps the compression level and the
//! checksum policy identical across both subsystems.

use std::io::{self, Write};

/// Compression level used for every persisted zstd frame.
pub(crate) const PERSIST_COMPRESSION_LEVEL: i32 = 3;

/// Compresses `data` into a zstd frame carrying a content checksum.
///
/// The checksum lets the decoder reject corrupted payloads at the frame
/// boundary instead of surfacing them as plausible-looking decoded data.
/// Decoding needs no matching change: zstd validates the checksum whenever a
/// frame carries one, so frames written before this policy existed stay
/// readable and existing files load unchanged.
pub(crate) fn encode_checked(data: &[u8]) -> io::Result<Vec<u8>> {
    let mut encoder = zstd::Encoder::new(Vec::new(), PERSIST_COMPRESSION_LEVEL)?;
    encoder.include_checksum(true)?;
    encoder.write_all(data)?;
    encoder.finish()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Bit 2 of a zstd frame header descriptor is the content checksum flag.
    const CONTENT_CHECKSUM_FLAG: u8 = 0b0000_0100;

    fn sample_payload() -> Vec<u8> {
        (0..4096u32).flat_map(u32::to_le_bytes).collect()
    }

    /// Deterministic incompressible bytes.
    ///
    /// zstd stores incompressible input as a raw block, so a flipped bit inside
    /// one leaves a structurally valid frame that decodes to different bytes.
    /// That is the corruption class only a checksum can catch.
    fn incompressible_payload() -> Vec<u8> {
        let mut state: u64 = 0x2545_F491_4F6C_DD1D;
        (0..4096)
            .map(|_| {
                state ^= state << 13;
                state ^= state >> 7;
                state ^= state << 17;
                (state >> 24) as u8
            })
            .collect()
    }

    #[test]
    fn encoded_frames_advertise_a_content_checksum() {
        let frame = encode_checked(&sample_payload()).expect("payload should compress");

        // Frame layout: [0..4] magic, [4] frame header descriptor.
        assert_eq!(
            frame[0..4],
            [0x28, 0xB5, 0x2F, 0xFD],
            "expected a zstd frame magic"
        );
        assert_ne!(
            frame[4] & CONTENT_CHECKSUM_FLAG,
            0,
            "frame header should advertise a content checksum"
        );
    }

    #[test]
    fn encoded_frames_round_trip() {
        let payload = sample_payload();
        let frame = encode_checked(&payload).expect("payload should compress");

        let decoded = zstd::decode_all(&frame[..]).expect("frame should decode");
        assert_eq!(decoded, payload);
    }

    /// Pins the behaviour the checksum exists for: a single flipped bit that a
    /// checksumless frame decodes as if nothing were wrong.
    #[test]
    fn silent_payload_corruption_is_rejected() {
        // Well inside the raw block for both frames, clear of the checksum
        // trailer, so each sees the same single-bit payload corruption.
        const CORRUPT_AT: usize = 100;

        let payload = incompressible_payload();
        let legacy = zstd::encode_all(&payload[..], PERSIST_COMPRESSION_LEVEL)
            .expect("payload should compress");
        let checked = encode_checked(&payload).expect("payload should compress");

        let mut corrupted_legacy = legacy.clone();
        corrupted_legacy[CORRUPT_AT] ^= 0x01;
        let decoded_legacy =
            zstd::decode_all(&corrupted_legacy[..]).expect("legacy frame decodes despite damage");
        assert_ne!(
            decoded_legacy, payload,
            "the corruption must actually change the decoded bytes"
        );

        let mut corrupted_checked = checked.clone();
        corrupted_checked[CORRUPT_AT] ^= 0x01;
        assert!(
            zstd::decode_all(&corrupted_checked[..]).is_err(),
            "the same corruption must be caught once the frame carries a checksum"
        );
    }

    #[test]
    fn checksumless_frames_still_decode() {
        // Files written before this policy carry no checksum. They must keep
        // loading, so enabling checksums stays backwards compatible.
        let payload = sample_payload();
        let legacy = zstd::encode_all(&payload[..], PERSIST_COMPRESSION_LEVEL)
            .expect("payload should compress");

        assert_eq!(
            legacy[4] & CONTENT_CHECKSUM_FLAG,
            0,
            "legacy frames should carry no checksum"
        );
        assert_eq!(
            zstd::decode_all(&legacy[..]).expect("legacy frame should decode"),
            payload
        );
    }
}
