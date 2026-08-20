use simdnbt::borrow::BaseNbtCompound as BorrowedNbtCompound;
use simdnbt::owned::NbtCompound;
use std::sync::{Arc, Weak};
use steel_registry::vanilla_block_entity_types;
use steel_utils::{BlockPos, BlockStateId, DowncastType, DowncastTypeKey};

use crate::block_entity::{BlockEntity, BlockEntityBase};
use crate::world::World;

/// Number of slots in a enderchest (3 rows of 9).
pub const ENDERCHEST_SLOTS: usize = 27;

/// Enderchest block entity
pub struct EnderChestBlockEntity {
    base: Arc<BlockEntityBase>,
}

// SAFETY: This key is owned by Steel and uniquely identifies `EnderChestBlockEntity`.
unsafe impl DowncastType for EnderChestBlockEntity {
    const TYPE_KEY: DowncastTypeKey = DowncastTypeKey::new("steel:block_entity/ender_chest");
}

impl EnderChestBlockEntity {
    /// Creates a new enderchest block entity
    #[must_use]
    pub fn new(level: Weak<World>, pos: BlockPos, state: BlockStateId) -> EnderChestBlockEntity {
        let base = Arc::new(BlockEntityBase::new(
            &vanilla_block_entity_types::ENDER_CHEST,
            level,
            pos,
            state,
        ));
        Self { base }
    }

    // TODO: Implement open and close animations
}

impl BlockEntity for EnderChestBlockEntity {
    fn base(&self) -> &BlockEntityBase {
        &self.base
    }

    fn load_additional(&self, _nbt: &BorrowedNbtCompound<'_>) {}

    fn save_additional(&self, _nbt: &mut NbtCompound) {}

    fn get_update_tag(&self) -> Option<NbtCompound> {
        None
    }
}
