//! Ender chest block entity implementation.

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
}

impl BlockEntity for EnderChestBlockEntity {
    fn base(&self) -> &BlockEntityBase {
        &self.base
    }

    fn load_additional(&self, _nbt: &BaseNbtCompound<'_>) {}

    fn save_additional(&self, _nbt: &mut NbtCompound) {}
}
