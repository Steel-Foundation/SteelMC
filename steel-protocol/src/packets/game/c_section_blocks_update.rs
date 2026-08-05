use std::io::{Result, Write};

use steel_macros::ClientPacket;
use steel_registry::packets::play::C_SECTION_BLOCKS_UPDATE;
use steel_utils::{
    BlockStateId, PackedSectionBlockPos, SectionPos,
    codec::{VarInt, VarLong},
    serial::WriteTo,
};

#[derive(ClientPacket, Clone, Debug)]
#[packet_id(Play = C_SECTION_BLOCKS_UPDATE)]
pub struct CSectionBlocksUpdate {
    pub section_pos: SectionPos,
    pub changes: Vec<BlockChange>,
}

#[derive(Clone, Debug)]
pub struct BlockChange {
    pub pos: PackedSectionBlockPos,
    pub block_state: BlockStateId,
}

impl WriteTo for CSectionBlocksUpdate {
    fn write(&self, writer: &mut impl Write) -> Result<()> {
        // Write section position
        self.section_pos.write(writer)?;

        // Write number of changes
        VarInt(self.changes.len() as i32).write(writer)?;

        // Write each change as a packed VarLong
        for change in &self.changes {
            // Pack as: (block_state_id << 12) | packed_pos
            let block_id = i32::from(change.block_state.0);
            let packed = i64::from(block_id) << 12 | i64::from(change.pos.as_u16());

            VarLong(packed).write(writer)?;
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use steel_utils::{
        BlockStateId, PackedSectionBlockPos, SectionPos,
        codec::{VarInt, VarLong},
        serial::ReadFrom,
    };

    use super::{BlockChange, CSectionBlocksUpdate};
    use steel_utils::serial::WriteTo;

    #[test]
    fn writes_changes_with_section_relative_positions() {
        let packet = CSectionBlocksUpdate {
            section_pos: SectionPos::new(1, -2, 3),
            changes: vec![BlockChange {
                pos: PackedSectionBlockPos::from_local_xyz(1, 15, 2).unwrap(),
                block_state: BlockStateId(42),
            }],
        };

        let mut data = Vec::new();
        packet.write(&mut data).unwrap();

        let mut cursor = Cursor::new(data.as_slice());
        assert_eq!(SectionPos::read(&mut cursor).unwrap(), packet.section_pos);
        assert_eq!(VarInt::read(&mut cursor).unwrap().0, 1);
        assert_eq!(
            VarLong::read(&mut cursor).unwrap().0,
            (42_i64 << 12) | 0x12f
        );
    }
}
