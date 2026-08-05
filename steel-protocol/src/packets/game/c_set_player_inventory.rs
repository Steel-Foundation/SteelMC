//! Clientbound player inventory slot update packet.

use steel_macros::{ClientPacket, WriteTo};
use steel_registry::{item_stack::ItemStack, packets::play::C_SET_PLAYER_INVENTORY};

/// Updates one logical slot in the receiving player's inventory.
#[derive(ClientPacket, WriteTo, Clone, Debug)]
#[packet_id(Play = C_SET_PLAYER_INVENTORY)]
pub struct CSetPlayerInventory {
    #[write(as = VarInt)]
    pub slot: i32,
    pub item_stack: ItemStack,
}

#[cfg(test)]
mod tests {
    use steel_utils::serial::WriteTo as _;

    use super::*;

    #[test]
    fn player_inventory_slot_is_encoded_as_varint_before_item_stack() {
        let packet = CSetPlayerInventory {
            slot: 300,
            item_stack: ItemStack::empty(),
        };
        let mut bytes = Vec::new();

        packet.write(&mut bytes).expect("packet should encode");

        assert_eq!(bytes, vec![0xac, 0x02, 0]);
    }
}
