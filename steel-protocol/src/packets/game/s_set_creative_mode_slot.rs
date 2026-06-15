use steel_macros::ServerPacket;
use steel_registry::item_stack::ItemStack;
use steel_utils::serial::ReadFrom;

/// Creative mode slot packet uses the delimited (untrusted) item format
/// where each component value is prefixed with a VarInt byte length.
#[derive(ServerPacket, Clone, Debug)]
pub struct SSetCreativeModeSlot {
    pub slot_num: i16,
    pub item_stack: ItemStack,
}

impl ReadFrom for SSetCreativeModeSlot {
    fn read(data: &mut std::io::Cursor<&[u8]>) -> std::io::Result<Self> {
        let slot_num = i16::read(data)?;
        let item_stack = ItemStack::read_untrusted(data)?;
        Ok(Self {
            slot_num,
            item_stack,
        })
    }
}
