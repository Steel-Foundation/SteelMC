//! Block display entity implementation.
//!
//! Display entities render a block, item, or text without collision.
//! They're commonly used for visual effects, holograms, and decorations.

use std::sync::Weak;

use simdnbt::borrow::NbtCompound as BorrowedNbtCompoundView;
use simdnbt::owned::NbtCompound;
use steel_macros::entity_behavior;
use steel_registry::entity_type::EntityTypeRef;
use steel_registry::vanilla_entities;
use steel_registry::vanilla_entity_data::BlockDisplayEntityData;
use steel_utils::BlockStateId;
use steel_utils::{DowncastType, DowncastTypeKey};

use glam::DVec3;

use crate::entity::{Entity, EntityBase, EntityBaseLoad, EntitySyncedData, SharedEntity};
use crate::world::World;

/// A block display entity that renders a block state at its position.
///
/// Block displays are purely visual entities with no collision.
/// They support transformation (translation, rotation, scale) and
/// interpolation for smooth animations.
#[entity_behavior(class = "block_display")]
pub struct BlockDisplayEntity {
    /// Weak back-reference to the containing `EntityBase`.
    base: Weak<EntityBase>,
    /// Synced entity data for network serialization.
    entity_data: BlockDisplayEntityData,
}

// SAFETY: This key is owned by Steel and uniquely identifies `BlockDisplayEntity`.
unsafe impl DowncastType for BlockDisplayEntity {
    const TYPE_KEY: DowncastTypeKey = DowncastTypeKey::new("steel:entity/block_display");
}

impl BlockDisplayEntity {
    fn build(base: Weak<EntityBase>) -> Self {
        Self {
            base,

            entity_data: BlockDisplayEntityData::new(),
        }
    }

    /// Creates a new block display entity.
    #[must_use]
    pub fn new(id: i32, position: DVec3, world: Weak<World>) -> SharedEntity {
        EntityBase::pack_with(
            id,
            position,
            vanilla_entities::BLOCK_DISPLAY.dimensions,
            world,
            |base| Self::build(base),
        )
    }

    /// Creates a block display entity from saved data.
    ///
    /// Display entities have no physical collision, but vanilla base state is
    /// still persisted and should round-trip through the shared base.
    #[must_use]
    pub fn from_saved(load: EntityBaseLoad) -> SharedEntity {
        EntityBase::pack_loaded_with(load, vanilla_entities::BLOCK_DISPLAY.dimensions, |base| {
            Self::build(base)
        })
    }

    /// Gets a reference to the entity data for reading/modifying synced state.
    pub const fn entity_data(&self) -> &BlockDisplayEntityData {
        &self.entity_data
    }

    /// Sets the block state ID of this entity.
    pub fn set_block_state_id(&mut self, id: BlockStateId) {
        self.entity_data.set_block_state(id);
    }
}

impl Entity for BlockDisplayEntity {
    fn base_weak(&self) -> &Weak<EntityBase> {
        &self.base
    }

    fn entity_type(&self) -> EntityTypeRef {
        &vanilla_entities::BLOCK_DISPLAY
    }

    fn synced_data(&self) -> Option<&dyn EntitySyncedData> {
        Some(&self.entity_data)
    }

    fn synced_data_mut(&mut self) -> Option<&mut dyn EntitySyncedData> {
        Some(&mut self.entity_data)
    }

    fn save_additional(&self, nbt: &mut NbtCompound) {
        // Save block state ID directly - these are deterministic in Minecraft
        let block_state_id = *self.entity_data.block_state.get();
        nbt.insert("block_state", i32::from(block_state_id.0));
    }

    fn load_additional(&mut self, nbt: BorrowedNbtCompoundView<'_, '_>) {
        // Load block state ID
        if let Some(state_id) = nbt.int("block_state") {
            self.entity_data
                .block_state
                .set(BlockStateId(state_id as u16));
        }
    }
}
