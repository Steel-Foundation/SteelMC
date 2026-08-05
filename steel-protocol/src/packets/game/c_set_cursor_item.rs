use steel_macros::{ClientPacket, WriteTo};
use steel_registry::{item_stack::ItemStack, packets::play::C_SET_CURSOR_ITEM};

#[derive(ClientPacket, WriteTo, Clone, Debug)]
#[packet_id(Play = C_SET_CURSOR_ITEM)]
pub struct CSetCursorItem {
    pub item_stack: ItemStack,
}
