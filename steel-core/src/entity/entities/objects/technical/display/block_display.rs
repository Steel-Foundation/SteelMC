//! Vanilla `Display.BlockDisplay`: renders a block state without collision.

use simdnbt::borrow::NbtCompound as BorrowedNbtCompoundView;
use simdnbt::owned::NbtCompound;
use steel_macros::entity_behavior;
use steel_registry::entity_type::EntityTypeRef;
use steel_registry::vanilla_entity_data::BlockDisplayEntityData;
use steel_registry::{REGISTRY, vanilla_blocks};
use steel_utils::BlockStateId;
use steel_utils::locks::SyncMutex;

use super::DisplayBase;
use crate::block_entity::block_state_nbt;
use crate::entity::{Entity, EntityBase, EntityProps, EntityTable};

/// A display entity that renders the block model associated with a [`BlockStateId`].
///
/// The state shared by all displays is reached through [`Parts::display`](super::Parts::display).
#[entity_behavior(class = "BlockDisplay")]
#[derive(Entity)]
#[entity(save = Self::save_block_display, load = Self::load_block_display)]
#[entity(key = "steel:entity/block_display", constructors)]
pub struct BlockDisplayEntity {
    #[entity(base)]
    base: EntityBase,
    #[entity(entity_type)]
    entity_type: EntityTypeRef,
    #[entity(component)]
    display: DisplayBase,
    #[entity(synced)]
    entity_data: SyncMutex<BlockDisplayEntityData>,
}

impl EntityTable for BlockDisplayEntity {
    const ENTITY: EntityProps<Self> = super::entity_props();
}

impl BlockDisplayEntity {
    fn from_base(base: EntityBase, entity_type: EntityTypeRef) -> Self {
        base.set_no_physics(true);
        Self {
            base,
            entity_type,
            display: DisplayBase,
            entity_data: SyncMutex::new(BlockDisplayEntityData::new()),
        }
    }

    /// Gets the block state (by ID) of this block display.
    #[must_use]
    pub fn block_state(&self) -> BlockStateId {
        *self.entity_data.lock().block_state.get()
    }

    /// Sets the block state (by ID) of this block display.
    pub fn set_block_state(&self, id: BlockStateId) {
        self.entity_data.lock().block_state.set(id);
    }

    fn save_block_display(&self, nbt: &mut NbtCompound) {
        nbt.insert("block_state", block_state_nbt::save(self.block_state()));
    }

    fn load_block_display(&self, nbt: BorrowedNbtCompoundView<'_, '_>) {
        // TODO: Support compact default states and the property fallback from BlockState.CODEC
        self.set_block_state(
            nbt.compound("block_state")
                .and_then(block_state_nbt::load)
                .or_else(|| {
                    let id = BlockStateId(u16::try_from(nbt.int("block_state")?).ok()?);

                    REGISTRY.blocks.by_state_id(id).map(|_| id)
                })
                .unwrap_or(vanilla_blocks::AIR.default_state()),
        );
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Weak;

    use glam::DVec3;
    use steel_registry::{init_vanilla_registry, vanilla_entities};

    use super::*;
    use crate::test_support::load_additional;

    #[test]
    fn block_display_saves_its_block_state_by_name() {
        init_vanilla_registry();
        let display = BlockDisplayEntity::new(
            &vanilla_entities::BLOCK_DISPLAY,
            1,
            DVec3::ZERO,
            Weak::new(),
        );
        let state = vanilla_blocks::OAK_STAIRS.default_state();
        display.set_block_state(state);

        let mut nbt = NbtCompound::new();
        display.save_additional(&mut nbt);
        assert_eq!(
            nbt.compound("block_state")
                .and_then(|state| state.string("id"))
                .map(ToString::to_string),
            Some("minecraft:oak_stairs".to_owned())
        );

        let loaded = BlockDisplayEntity::new(
            &vanilla_entities::BLOCK_DISPLAY,
            2,
            DVec3::ZERO,
            Weak::new(),
        );
        load_additional(&*loaded, &nbt);
        assert_eq!(loaded.block_state(), state);
    }
}
