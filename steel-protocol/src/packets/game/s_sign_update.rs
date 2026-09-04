use std::io::Cursor;

use steel_macros::ServerPacket;
use steel_utils::BlockPos;
use steel_utils::serial::{PrefixedRead, ReadFrom};
use steel_utils::types::SignTextSlot;

/// Maximum characters per sign line.
pub const MAX_SIGN_LINE_LENGTH: usize = 384;

/// Serverbound packet sent when a player finishes editing a sign.
#[derive(ServerPacket, Clone, Debug)]
pub struct SSignUpdate {
    /// The position of the sign block.
    pub pos: BlockPos,
    /// The four lines of text. Each line is max 384 characters.
    pub lines: [String; 4],
    /// Which side of the sign was edited.
    pub slot: SignTextSlot,
}

impl ReadFrom for SSignUpdate {
    fn read(data: &mut Cursor<&[u8]>) -> std::io::Result<Self> {
        use steel_utils::codec::VarInt;

        let pos = BlockPos::read(data)?;
        // Vanilla writes the lines as a fixed-size list, so there is no length prefix.
        let lines = [
            String::read_prefixed_bound::<VarInt>(data, MAX_SIGN_LINE_LENGTH)?,
            String::read_prefixed_bound::<VarInt>(data, MAX_SIGN_LINE_LENGTH)?,
            String::read_prefixed_bound::<VarInt>(data, MAX_SIGN_LINE_LENGTH)?,
            String::read_prefixed_bound::<VarInt>(data, MAX_SIGN_LINE_LENGTH)?,
        ];
        let slot = SignTextSlot::read(data)?;

        Ok(Self { pos, lines, slot })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sign_update_reads_lines_before_slot() {
        // BlockPos(0, 64, 0) packed, then four length-prefixed lines, then the slot.
        let mut bytes = 0x0000_0000_0000_0040u64.to_be_bytes().to_vec();
        for line in ["a", "b", "c", "d"] {
            bytes.push(1);
            bytes.extend_from_slice(line.as_bytes());
        }
        bytes.push(1); // SignTextSlot::Front

        let packet = SSignUpdate::read(&mut Cursor::new(&bytes[..])).expect("should decode");

        assert_eq!(packet.pos, BlockPos::new(0, 64, 0));
        assert_eq!(packet.lines, ["a", "b", "c", "d"].map(String::from));
        assert_eq!(packet.slot, SignTextSlot::Front);
    }
}
