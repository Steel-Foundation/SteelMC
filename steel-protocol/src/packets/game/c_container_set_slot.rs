use steel_macros::{ClientPacket, WriteTo};
use steel_registry::{item_stack::ItemStack, packets::play::C_CONTAINER_SET_SLOT};

#[derive(ClientPacket, WriteTo, Clone, Debug)]
#[packet_id(Play = C_CONTAINER_SET_SLOT)]
pub struct CContainerSetSlot {
    #[write(as = VarInt)]
    pub container_id: i32,
    #[write(as = VarInt)]
    pub state_id: i32,
    pub slot: i16,
    pub item_stack: ItemStack,
}
