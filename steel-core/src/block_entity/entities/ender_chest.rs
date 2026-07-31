//! Ender chest block entity implementation.
//!
//! Handles lid animations and sound effects for ender chests.

use std::sync::{Arc, Weak};

use simdnbt::borrow::BaseNbtCompound;
use simdnbt::owned::NbtCompound;
use steel_registry::vanilla_block_entity_types;
use steel_utils::{BlockPos, BlockStateId, DowncastType, DowncastTypeKey};

use crate::block_entity::{BlockEntity, BlockEntityBase};
use crate::world::World;

/// Ender chest block entity.
pub struct EnderChestBlockEntity {
    base: Arc<BlockEntityBase>,
}

// SAFETY: This key is owned by Steel and uniquely identifies `EnderChestBlockEntity`.
unsafe impl DowncastType for EnderChestBlockEntity {
    const TYPE_KEY: DowncastTypeKey = DowncastTypeKey::new("steel:block_entity/ender_chest");
}

impl EnderChestBlockEntity {
    /// Creates a new ender chest block entity.
    #[must_use]
    pub fn new(level: Weak<World>, pos: BlockPos, state: BlockStateId) -> Self {
        Self {
            base: Arc::new(BlockEntityBase::new(
                &vanilla_block_entity_types::ENDER_CHEST,
                level,
                pos,
                state,
            )),
        }
    }

    /// Called when a player starts looking into the ender chest.
    #[expect(
        clippy::unused_self,
        reason = "Will use self once ContainerOpenersCounter is implemented"
    )]
    pub const fn start_open(&mut self) {
        // TODO: Implement ContainerOpenersCounter to track open state and play sounds.
        // Needs a lid controller and block events.
    }

    /// Called when a player stops looking into the ender chest.
    #[expect(
        clippy::unused_self,
        reason = "Will use self once ContainerOpenersCounter is implemented"
    )]
    pub const fn stop_open(&mut self) {
        // TODO: Implement ContainerOpenersCounter to track open state and play sounds.
        // Needs a lid controller and block events.
    }
}

impl BlockEntity for EnderChestBlockEntity {
    fn base(&self) -> &BlockEntityBase {
        &self.base
    }

    fn load_additional(&self, _nbt: &BaseNbtCompound<'_>) {}

    fn save_additional(&self, _nbt: &mut NbtCompound) {}
}
