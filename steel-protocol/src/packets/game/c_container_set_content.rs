use steel_macros::{ClientPacket, WriteTo};
use steel_registry::{item_stack::ItemStack, packets::play::C_CONTAINER_SET_CONTENT};

#[derive(ClientPacket, WriteTo, Clone, Debug)]
#[packet_id(Play = C_CONTAINER_SET_CONTENT)]
pub struct CContainerSetContent {
    #[write(as = VarInt)]
    pub container_id: i32,
    #[write(as = VarInt)]
    pub state_id: i32,
    pub items: Vec<ItemStack>,
    pub carried_item: ItemStack,
}
