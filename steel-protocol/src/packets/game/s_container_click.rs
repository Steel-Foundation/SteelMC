use std::io::{Cursor, Result};

use rustc_hash::FxHashMap;
use steel_macros::ServerPacket;
use steel_utils::{codec::VarInt, serial::ReadFrom};

/// The type of click action performed on a container slot.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum ClickType {
    Pickup = 0,
    QuickMove = 1,
    Swap = 2,
    Clone = 3,
    Throw = 4,
    QuickCraft = 5,
    PickupAll = 6,
}

impl ReadFrom for ClickType {
    fn read(data: &mut Cursor<&[u8]>) -> Result<Self> {
        let id = VarInt::read(data)?.0;
        Ok(match id {
            0 => ClickType::Pickup,
            1 => ClickType::QuickMove,
            2 => ClickType::Swap,
            3 => ClickType::Clone,
            4 => ClickType::Throw,
            5 => ClickType::QuickCraft,
            6 => ClickType::PickupAll,
            _ => ClickType::Pickup, // Default to Pickup for unknown values
        })
    }
}

/// A hashed representation of component patches for verification.
/// Maps data component type IDs to their hash values.
#[derive(Debug, Clone, Default)]
pub struct HashedPatchMap {
    pub added_components: FxHashMap<i32, i32>,
    pub removed_components: Vec<i32>,
}

impl ReadFrom for HashedPatchMap {
    fn read(data: &mut Cursor<&[u8]>) -> Result<Self> {
        // Read added components map: Map<VarInt, Int>
        let added_count = VarInt::read(data)?.0 as usize;
        let mut added_components = FxHashMap::default();
        for _ in 0..added_count.min(256) {
            let type_id = VarInt::read(data)?.0;
            let hash = i32::read(data)?;
            added_components.insert(type_id, hash);
        }

        // Read removed components set: Collection<VarInt>
        let removed_count = VarInt::read(data)?.0 as usize;
        let mut removed_components = Vec::with_capacity(removed_count.min(256));
        for _ in 0..removed_count.min(256) {
            let type_id = VarInt::read(data)?.0;
            removed_components.push(type_id);
        }

        Ok(Self {
            added_components,
            removed_components,
        })
    }
}

/// A hashed representation of an ItemStack sent from client to server.
/// Used for verification without trusting client data.
#[derive(Debug, Clone)]
pub enum HashedStack {
    Empty,
    Item {
        item_id: i32,
        count: i32,
        components: HashedPatchMap,
    },
}

impl ReadFrom for HashedStack {
    fn read(data: &mut Cursor<&[u8]>) -> Result<Self> {
        // Optional<ActualItem> - bool prefix
        let present = bool::read(data)?;
        if !present {
            return Ok(HashedStack::Empty);
        }

        // ActualItem: Holder<Item> (VarInt), count (VarInt), HashedPatchMap
        let item_id = VarInt::read(data)?.0;
        let count = VarInt::read(data)?.0;
        let components = HashedPatchMap::read(data)?;

        Ok(HashedStack::Item {
            item_id,
            count,
            components,
        })
    }
}

/// Serverbound packet sent when a player clicks in a container.
#[derive(ServerPacket, Debug, Clone)]
pub struct SContainerClick {
    pub container_id: i32,
    pub state_id: i32,
    pub slot_num: i16,
    pub button_num: i8,
    pub click_type: ClickType,
    pub changed_slots: FxHashMap<i16, HashedStack>,
    pub carried_item: HashedStack,
}

impl ReadFrom for SContainerClick {
    fn read(data: &mut Cursor<&[u8]>) -> Result<Self> {
        let container_id = VarInt::read(data)?.0;
        let state_id = VarInt::read(data)?.0;
        let slot_num = i16::read(data)?;
        let button_num = i8::read(data)?;
        let click_type = ClickType::read(data)?;

        // Read changed slots map with max 128 entries
        let slot_count = VarInt::read(data)?.0 as usize;
        let mut changed_slots = FxHashMap::default();
        for _ in 0..slot_count.min(128) {
            let slot = i16::read(data)?;
            let stack = HashedStack::read(data)?;
            changed_slots.insert(slot, stack);
        }

        let carried_item = HashedStack::read(data)?;

        Ok(Self {
            container_id,
            state_id,
            slot_num,
            button_num,
            click_type,
            changed_slots,
            carried_item,
        })
    }
}
