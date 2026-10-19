use steel_macros::{ClientPacket, WriteTo};
use steel_registry::packets::play::C_OPEN_SIGN_EDITOR;
use steel_utils::BlockPos;
use steel_utils::types::SignTextSlot;

/// Clientbound packet sent to open the sign editor GUI.
#[derive(ClientPacket, WriteTo, Clone, Debug)]
#[packet_id(Play = C_OPEN_SIGN_EDITOR)]
pub struct COpenSignEditor {
    /// The position of the sign block.
    pub pos: BlockPos,
    /// Which side of the sign to edit.
    #[write(as = VarInt)]
    pub slot: SignTextSlot,
}

#[cfg(test)]
mod tests {
    use steel_utils::serial::WriteTo as _;

    use super::*;

    #[test]
    fn open_sign_editor_writes_slot_as_varint() {
        let mut bytes = Vec::new();
        COpenSignEditor {
            pos: BlockPos::new(0, 64, 0),
            slot: SignTextSlot::Front,
        }
        .write(&mut bytes)
        .expect("packet should encode");

        // Packed BlockPos, then the slot id as a VarInt.
        assert_eq!(
            bytes,
            [&0x0000_0000_0000_0040u64.to_be_bytes()[..], &[1]].concat()
        );
    }
}
