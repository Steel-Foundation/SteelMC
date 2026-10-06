//! End portal block entity.

use std::sync::Weak;

use simdnbt::borrow::BaseNbtCompound as BorrowedNbtCompound;
use simdnbt::owned::NbtCompound;
use steel_registry::vanilla_block_entity_types;
use steel_utils::{BlockPos, BlockStateId, DowncastType, DowncastTypeKey};

use crate::block_entity::{BlockEntity, BlockEntityBase};
use crate::world::World;

/// Vanilla `TheEndPortalBlockEntity`.
pub struct EndPortalBlockEntity {
    base: BlockEntityBase,
}

// SAFETY: This key is owned by Steel and uniquely identifies `EndPortalBlockEntity`.
unsafe impl DowncastType for EndPortalBlockEntity {
    const TYPE_KEY: DowncastTypeKey = DowncastTypeKey::new("steel:block_entity/end_portal");
}

impl EndPortalBlockEntity {
    /// Creates an End portal block entity.
    #[must_use]
    pub fn new(level: Weak<World>, pos: BlockPos, state: BlockStateId) -> Self {
        Self {
            base: BlockEntityBase::new(&vanilla_block_entity_types::END_PORTAL, level, pos, state),
        }
    }
}

impl BlockEntity for EndPortalBlockEntity {
    fn base(&self) -> &BlockEntityBase {
        &self.base
    }

    fn load_additional(&self, _nbt: &BorrowedNbtCompound<'_>) {}

    fn save_additional(&self, _nbt: &mut NbtCompound) {}

    fn get_update_tag(&self) -> Option<NbtCompound> {
        Some(NbtCompound::new())
    }
}
