//! Clientbound set equipment packet.

use std::io::{Error, ErrorKind, Result, Write};

use steel_macros::ClientPacket;
use steel_registry::{
    equipment::EquipmentSlot, item_stack::ItemStack, packets::play::C_SET_EQUIPMENT,
};
use steel_utils::{codec::VarInt, serial::WriteTo};

const CONTINUE_MASK: u8 = 0x80;

const fn vanilla_equipment_slot_id(slot: EquipmentSlot) -> u8 {
    match slot {
        EquipmentSlot::MainHand => 0,
        EquipmentSlot::OffHand => 1,
        EquipmentSlot::Feet => 2,
        EquipmentSlot::Legs => 3,
        EquipmentSlot::Chest => 4,
        EquipmentSlot::Head => 5,
        EquipmentSlot::Body => 6,
        EquipmentSlot::Saddle => 7,
    }
}

const fn equipment_slot_packet_id(slot: EquipmentSlot, has_next: bool) -> u8 {
    if has_next {
        vanilla_equipment_slot_id(slot) | CONTINUE_MASK
    } else {
        vanilla_equipment_slot_id(slot)
    }
}

/// One equipment slot update.
#[derive(Clone, Debug, PartialEq)]
pub struct EquipmentSlotItem {
    /// Slot being updated.
    pub slot: EquipmentSlot,
    /// New item stack for the slot. Empty stacks clear the slot on the client.
    pub item_stack: ItemStack,
}

/// Updates one or more equipment slots for an entity.
#[derive(ClientPacket, Clone, Debug)]
#[packet_id(Play = C_SET_EQUIPMENT)]
pub struct CSetEquipment {
    /// Entity id whose equipment changed.
    pub entity_id: i32,
    /// Slot updates. Vanilla requires at least one entry when this packet is sent.
    pub slots: Vec<EquipmentSlotItem>,
}

impl CSetEquipment {
    /// Creates a new equipment update packet.
    #[must_use]
    pub const fn new(entity_id: i32, slots: Vec<EquipmentSlotItem>) -> Self {
        Self { entity_id, slots }
    }
}

impl WriteTo for CSetEquipment {
    fn write(&self, writer: &mut impl Write) -> Result<()> {
        if self.slots.is_empty() {
            return Err(Error::new(
                ErrorKind::InvalidInput,
                "CSetEquipment requires at least one slot",
            ));
        }
        VarInt(self.entity_id).write(writer)?;
        let last_index = self.slots.len().saturating_sub(1);
        for (index, slot_item) in self.slots.iter().enumerate() {
            writer.write_all(&[equipment_slot_packet_id(
                slot_item.slot,
                index != last_index,
            )])?;
            slot_item.item_stack.write(writer)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn equipment_packet_uses_vanilla_continue_bit() {
        let packet = CSetEquipment::new(
            42,
            vec![
                EquipmentSlotItem {
                    slot: EquipmentSlot::MainHand,
                    item_stack: ItemStack::empty(),
                },
                EquipmentSlotItem {
                    slot: EquipmentSlot::Head,
                    item_stack: ItemStack::empty(),
                },
            ],
        );
        let mut bytes = Vec::new();

        packet.write(&mut bytes).expect("packet should encode");

        assert_eq!(bytes, vec![42, 0x80, 0, 5, 0]);
    }

    #[test]
    fn equipment_packet_rejects_empty_slot_updates() {
        let packet = CSetEquipment::new(42, Vec::new());

        let error = packet.write(&mut Vec::new()).unwrap_err();

        assert_eq!(error.kind(), std::io::ErrorKind::InvalidInput);
    }
}
