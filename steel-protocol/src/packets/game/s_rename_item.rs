//! Serverbound packet for renaming the item inside of an anvil's first slot

use std::io::Cursor;

use steel_macros::ServerPacket;
use steel_utils::serial::{ReadFrom, prefixed_read::read_utf};

const MAX_PACKET_NAME_LENGTH: usize = 32_767;

/// Sent by the client when the player changes an anvil's item name.
#[derive(ServerPacket, Clone, Debug)]
pub struct SRenameItem {
    /// The new name
    pub name: String,
}

impl ReadFrom for SRenameItem {
    fn read(data: &mut Cursor<&[u8]>) -> std::io::Result<Self> {
        Ok(Self {
            name: read_utf(data, MAX_PACKET_NAME_LENGTH)?,
        })
    }
}

#[cfg(test)]
mod tests {
    use steel_utils::{
        codec::VarInt,
        serial::{ReadFrom as _, WriteTo as _},
    };

    use super::{MAX_PACKET_NAME_LENGTH, SRenameItem};

    fn encoded_string(bytes: &[u8]) -> Vec<u8> {
        let mut encoded = Vec::with_capacity(VarInt::MAX_SIZE + bytes.len());
        VarInt(bytes.len() as i32)
            .write(&mut encoded)
            .expect("test string length should encode");
        encoded.extend_from_slice(bytes);
        encoded
    }

    fn decode(bytes: &[u8]) -> std::io::Result<SRenameItem> {
        SRenameItem::read(&mut std::io::Cursor::new(bytes))
    }

    #[test]
    fn accepts_vanilla_utf16_boundary() {
        let name = "a".repeat(MAX_PACKET_NAME_LENGTH);
        let decoded = decode(&encoded_string(name.as_bytes()))
            .expect("the Vanilla UTF-16 boundary should decode");
        assert_eq!(decoded.name, name);
    }

    #[test]
    fn rejects_more_than_vanilla_utf16_boundary() {
        let name = "a".repeat(MAX_PACKET_NAME_LENGTH + 1);
        assert!(decode(&encoded_string(name.as_bytes())).is_err());
    }

    #[test]
    fn accepts_multibyte_name_above_the_old_byte_bound() {
        let name = format!("{}X", "§".repeat(16_251));
        assert_eq!(name.encode_utf16().count(), 16_252);
        assert_eq!(name.len(), 32_503);

        let decoded =
            decode(&encoded_string(name.as_bytes())).expect("Vanilla accepts this packet name");
        assert_eq!(decoded.name, name);
    }

    #[test]
    fn rejects_encoded_length_above_vanilla_maximum() {
        let bytes = vec![b'a'; MAX_PACKET_NAME_LENGTH * 3 + 1];
        assert!(decode(&encoded_string(&bytes)).is_err());
    }

    #[test]
    fn malformed_utf8_uses_replacement_characters() {
        let decoded = decode(&encoded_string(&[0xFF]))
            .expect("Vanilla decodes malformed UTF-8 with replacement");
        assert_eq!(decoded.name, "\u{FFFD}");
    }

    #[test]
    fn malformed_surrogate_sequence_uses_one_replacement_character() {
        let decoded = decode(&encoded_string(&[0xED, 0xA0, 0x80]))
            .expect("Vanilla replaces the complete malformed surrogate sequence");
        assert_eq!(decoded.name, "\u{FFFD}");
    }
}
