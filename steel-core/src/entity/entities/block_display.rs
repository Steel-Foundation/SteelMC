//! Block display entity implementation.
//!
//! Display entities render a block, item, or text without collision.
//! They're commonly used for visual effects, holograms, and decorations.

use std::sync::Weak;

use glam::DVec3;
use simdnbt::borrow::NbtCompound as BorrowedNbtCompoundView;
use simdnbt::owned::NbtCompound;
use steel_macros::entity_behavior;
use steel_registry::entity_type::EntityTypeRef;
use steel_registry::vanilla_entity_data::BlockDisplayEntityData;
use steel_utils::BlockStateId;
use steel_utils::locks::SyncMutex;
use uuid::Uuid;

use crate::entity::{Entity, EntityBase, EntityBaseLoad, EntitySyncedData};
use crate::world::World;

/// A block display entity that renders a block state at its position.
///
/// Block displays are purely visual entities with no collision.
/// They support transformation (translation, rotation, scale) and
/// interpolation for smooth animations.
#[entity_behavior(class = "BlockDisplay")]
pub struct BlockDisplayEntity {
    /// Common entity fields (id, uuid, position, etc.).
    base: EntityBase,
    /// Vanilla entity type registered for this implementation.
    entity_type: EntityTypeRef,
    /// Synced entity data for network serialization.
    entity_data: SyncMutex<BlockDisplayEntityData>,
}

impl BlockDisplayEntity {
    /// Creates a new block display entity.
    ///
    /// The `id` should be obtained from `next_entity_id()`.
    #[must_use]
    pub fn new(entity_type: EntityTypeRef, id: i32, position: DVec3, world: Weak<World>) -> Self {
        Self {
            base: EntityBase::new(id, position, entity_type.dimensions, world),
            entity_type,
            entity_data: SyncMutex::new(BlockDisplayEntityData::new()),
        }
    }

    /// Creates a new block display entity with a specific UUID.
    ///
    /// The `id` should be obtained from `next_entity_id()`.
    #[must_use]
    pub fn with_uuid(
        entity_type: EntityTypeRef,
        id: i32,
        position: DVec3,
        uuid: Uuid,
        world: Weak<World>,
    ) -> Self {
        Self {
            base: EntityBase::with_uuid(id, uuid, position, entity_type.dimensions, world),
            entity_type,
            entity_data: SyncMutex::new(BlockDisplayEntityData::new()),
        }
    }

    /// Creates a block display entity from saved data.
    ///
    /// Display entities have no physical collision, but vanilla base state is
    /// still persisted and should round-trip through the shared base.
    #[must_use]
    pub fn from_saved(entity_type: EntityTypeRef, load: EntityBaseLoad) -> Self {
        Self {
            base: EntityBase::from_load(load, entity_type.dimensions),
            entity_type,
            entity_data: SyncMutex::new(BlockDisplayEntityData::new()),
        }
    }

    /// Gets a reference to the entity data for reading/modifying synced state.
    pub const fn entity_data(&self) -> &SyncMutex<BlockDisplayEntityData> {
        &self.entity_data
    }

    /// Sets the block state ID of this entity.
    pub fn set_block_state_id(&self, id: BlockStateId) {
        self.entity_data.lock().block_state.set(id);
    }
}

impl Entity for BlockDisplayEntity {
    fn base(&self) -> &EntityBase {
        &self.base
    }

    fn entity_type(&self) -> EntityTypeRef {
        self.entity_type
    }

    fn synced_data(&self) -> Option<&dyn EntitySyncedData> {
        Some(&self.entity_data)
    }

    fn save_additional(&self, nbt: &mut NbtCompound) {
        // Save block state ID directly - these are deterministic in Minecraft
        let block_state_id = *self.entity_data.lock().block_state.get();
        nbt.insert("block_state", i32::from(block_state_id.0));
    }

    fn load_additional(&self, nbt: BorrowedNbtCompoundView<'_, '_>) {
        // Load block state ID
        if let Some(state_id) = nbt.int("block_state") {
            self.entity_data
                .lock()
                .block_state
                .set(BlockStateId(state_id as u16));
        }
    }
}
