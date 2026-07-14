//! NBT-preserving fallback block entity.

use std::sync::{Arc, Weak};

use simdnbt::borrow::{BaseNbtCompound as BorrowedNbtCompound, NbtCompound as NbtCompoundView};
use simdnbt::owned::NbtCompound;
use steel_registry::block_entity_type::BlockEntityTypeRef;
use steel_utils::{BlockPos, BlockStateId, DowncastType, DowncastTypeKey};

use crate::block_entity::BlockEntity;
use crate::world::World;

/// Steel-specific fallback for block entity types whose runtime behavior is not implemented yet.
///
/// Vanilla has concrete classes for every block entity type. Steel uses this only to preserve
/// worldgen and disk NBT until the corresponding typed implementation is added.
pub struct RawBlockEntity {
    block_entity_type: BlockEntityTypeRef,
    level: Weak<World>,
    pos: BlockPos,
    state: BlockStateId,
    removed: bool,
    data: NbtCompound,
}

// SAFETY: This key identifies the Steel fallback implementation, independently
// of the Minecraft block-entity registry entry stored inside it.
unsafe impl DowncastType for RawBlockEntity {
    const TYPE_KEY: DowncastTypeKey = DowncastTypeKey::new("steel:block_entity/raw");
}

impl RawBlockEntity {
    /// Creates a raw block entity without additional NBT.
    #[must_use]
    pub fn new(
        block_entity_type: BlockEntityTypeRef,
        level: Weak<World>,
        pos: BlockPos,
        state: BlockStateId,
    ) -> Self {
        Self::with_data(block_entity_type, level, pos, state, NbtCompound::new())
    }

    /// Creates a raw block entity with already-owned additional NBT.
    #[must_use]
    pub const fn with_data(
        block_entity_type: BlockEntityTypeRef,
        level: Weak<World>,
        pos: BlockPos,
        state: BlockStateId,
        data: NbtCompound,
    ) -> Self {
        Self {
            block_entity_type,
            level,
            pos,
            state,
            removed: false,
            data,
        }
    }
}

impl BlockEntity for RawBlockEntity {
    fn get_type(&self) -> BlockEntityTypeRef {
        self.block_entity_type
    }

    fn get_block_pos(&self) -> BlockPos {
        self.pos
    }

    fn get_block_state(&self) -> BlockStateId {
        self.state
    }

    fn set_block_state(&mut self, state: BlockStateId) {
        self.state = state;
    }

    fn is_removed(&self) -> bool {
        self.removed
    }

    fn set_removed(&mut self) {
        self.removed = true;
    }

    fn clear_removed(&mut self) {
        self.removed = false;
    }

    fn get_level(&self) -> Option<Arc<World>> {
        self.level.upgrade()
    }

    fn load_additional(&mut self, nbt: &BorrowedNbtCompound<'_>) {
        let nbt_view: NbtCompoundView<'_, '_> = nbt.into();
        self.data = nbt_view.to_owned();
    }

    fn save_additional(&self, nbt: &mut NbtCompound) {
        *nbt = self.data.clone();
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Weak;

    use steel_registry::{
        test_support::init_test_registry, vanilla_block_entity_types, vanilla_blocks,
    };

    use super::*;

    #[test]
    fn full_metadata_replaces_stale_raw_metadata() {
        init_test_registry();
        let mut data = NbtCompound::new();
        data.insert("id", "minecraft:chest");
        data.insert("x", 100_i32);
        data.insert("custom", 7_i32);
        let entity = RawBlockEntity::with_data(
            &vanilla_block_entity_types::BARREL,
            Weak::new(),
            BlockPos::new(2, 70, -4),
            vanilla_blocks::BARREL.default_state(),
            data,
        );

        let saved = entity.save_with_full_metadata();
        let custom = entity.save_custom_only();

        assert_eq!(
            saved.string("id").map(ToString::to_string),
            Some("minecraft:barrel".to_owned())
        );
        assert_eq!(saved.int("x"), Some(2));
        assert_eq!(saved.int("y"), Some(70));
        assert_eq!(saved.int("z"), Some(-4));
        assert_eq!(saved.int("custom"), Some(7));
        assert!(!custom.contains("id"));
        assert!(!custom.contains("x"));
        assert_eq!(custom.int("custom"), Some(7));
    }
}
