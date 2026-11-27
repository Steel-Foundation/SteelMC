//! NBT-preserving fallback block entity.

use std::sync::Weak;

use simdnbt::borrow::{BaseNbtCompound as BorrowedNbtCompound, NbtCompound as NbtCompoundView};
use simdnbt::owned::NbtCompound;
use steel_registry::block_entity_type::BlockEntityTypeRef;
use steel_utils::{BlockPos, BlockStateId, DowncastType, DowncastTypeKey, locks::SyncMutex};

use crate::block_entity::{BlockEntity, BlockEntityBase};
use crate::world::World;

struct UnimplementedBlockEntityState {
    data: NbtCompound,
}

/// Steel-specific fallback for block entity types whose runtime behavior is not implemented yet.
///
/// Vanilla has concrete classes for every block entity type. Steel uses this only to preserve
/// worldgen and disk NBT until the corresponding typed implementation is added.
pub struct UnimplementedBlockEntity {
    base: BlockEntityBase,
    state: SyncMutex<UnimplementedBlockEntityState>,
}

// SAFETY: This key identifies the Steel fallback implementation, independently
// of the Minecraft block-entity registry entry stored inside it.
unsafe impl DowncastType for UnimplementedBlockEntity {
    const TYPE_KEY: DowncastTypeKey = DowncastTypeKey::new("steel:block_entity/unimplemented");
}

impl UnimplementedBlockEntity {
    /// Creates an unimplemented block entity without additional NBT.
    #[must_use]
    pub fn new(
        block_entity_type: BlockEntityTypeRef,
        level: Weak<World>,
        pos: BlockPos,
        state: BlockStateId,
    ) -> Self {
        Self {
            base: BlockEntityBase::new(block_entity_type, level, pos, state),
            state: SyncMutex::new(UnimplementedBlockEntityState {
                data: NbtCompound::new(),
            }),
        }
    }

    /// Loads an unimplemented block entity from owned NBT, including stored components.
    pub fn with_data(
        block_entity_type: BlockEntityTypeRef,
        level: Weak<World>,
        pos: BlockPos,
        state: BlockStateId,
        data: NbtCompound,
    ) -> Result<Self, simdnbt::Error> {
        let entity = Self::new(block_entity_type, level, pos, state);
        entity.load_with_owned_components(&data)?;
        Ok(entity)
    }
}

impl BlockEntity for UnimplementedBlockEntity {
    fn base(&self) -> &BlockEntityBase {
        &self.base
    }

    fn load_additional(&self, nbt: &BorrowedNbtCompound<'_>) {
        let nbt_view: NbtCompoundView<'_, '_> = nbt.into();
        let mut data = nbt_view.to_owned();
        while data.remove("components").is_some() {}
        self.state.lock().data = data;
    }

    fn save_additional(&self, nbt: &mut NbtCompound) {
        *nbt = self.state.lock().data.clone();
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Weak;

    use steel_registry::data_components::{DataComponentMap, vanilla_components::REPAIR_COST};
    use steel_registry::{init_vanilla_registry, vanilla_block_entity_types, vanilla_blocks};

    use super::*;

    #[test]
    fn full_metadata_replaces_stale_unimplemented_metadata() {
        init_vanilla_registry();
        let mut data = NbtCompound::new();
        data.insert("id", "minecraft:chest");
        data.insert("x", 100_i32);
        data.insert("custom", 7_i32);
        let mut components = DataComponentMap::new();
        components.set(REPAIR_COST, Some(3));
        data.insert("components", components.to_nbt_tag_ref());
        let entity = UnimplementedBlockEntity::with_data(
            &vanilla_block_entity_types::BARREL,
            Weak::new(),
            BlockPos::new(2, 70, -4),
            vanilla_blocks::BARREL.default_state(),
            data,
        )
        .expect("fallback block entity should load owned NBT");

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
        assert_eq!(entity.collect_components().get(REPAIR_COST), Some(3));
        assert!(!custom.contains("id"));
        assert!(!custom.contains("x"));
        assert!(!custom.contains("components"));
        assert_eq!(custom.int("custom"), Some(7));
    }

    #[test]
    #[should_panic(expected = "invalid block entity minecraft:barrel state minecraft:stone")]
    fn constructor_rejects_a_type_state_mismatch() {
        init_vanilla_registry();
        let _ = UnimplementedBlockEntity::new(
            &vanilla_block_entity_types::BARREL,
            Weak::new(),
            BlockPos::new(2, 70, -4),
            vanilla_blocks::STONE.default_state(),
        );
    }
}
