//! Block display entity implementation.
//!
//! Display entities render a block, item, or text without collision.
//! They're commonly used for visual effects, holograms, and decorations.

use std::sync::Weak;

use glam::DVec3;
use simdnbt::borrow::{BaseNbtCompound as BorrowedNbtCompound, NbtCompound as NbtCompoundView};
use simdnbt::owned::NbtCompound;
use steel_registry::entity_data::DataValue;
use steel_registry::entity_type::EntityTypeRef;
use steel_registry::vanilla_entities;
use steel_registry::vanilla_entity_data::BlockDisplayEntityData;
use steel_utils::BlockStateId;
use steel_utils::locks::SyncMutex;
use uuid::Uuid;

use crate::entity::{Entity, EntityBase, EntityBaseState};
use crate::world::World;

/// A block display entity that renders a block state at its position.
///
/// Block displays are purely visual entities with no collision.
/// They support transformation (translation, rotation, scale) and
/// interpolation for smooth animations.
pub struct BlockDisplayEntity {
    /// Common entity fields (id, uuid, position, etc.).
    base: EntityBase,
    /// Synced entity data for network serialization.
    entity_data: SyncMutex<BlockDisplayEntityData>,
}

impl BlockDisplayEntity {
    /// Creates a new block display entity.
    ///
    /// The `id` should be obtained from `next_entity_id()`.
    #[must_use]
    pub fn new(id: i32, position: DVec3, world: Weak<World>) -> Self {
        Self {
            base: EntityBase::new(
                id,
                position,
                vanilla_entities::BLOCK_DISPLAY.dimensions,
                world,
            ),
            entity_data: SyncMutex::new(BlockDisplayEntityData::new()),
        }
    }

    /// Creates a new block display entity with a specific UUID.
    ///
    /// The `id` should be obtained from `next_entity_id()`.
    #[must_use]
    pub fn with_uuid(id: i32, position: DVec3, uuid: Uuid, world: Weak<World>) -> Self {
        Self {
            base: EntityBase::with_uuid(
                id,
                uuid,
                position,
                vanilla_entities::BLOCK_DISPLAY.dimensions,
                world,
            ),
            entity_data: SyncMutex::new(BlockDisplayEntityData::new()),
        }
    }

    /// Creates a block display entity from saved data.
    ///
    /// Display entities have no physical collision, but vanilla base state is
    /// still persisted and should round-trip through the shared base.
    #[must_use]
    pub fn from_saved(
        id: i32,
        position: DVec3,
        uuid: Uuid,
        velocity: DVec3,
        rotation: (f32, f32),
        on_ground: bool,
        world: Weak<World>,
    ) -> Self {
        Self {
            base: EntityBase::with_uuid_and_state(
                id,
                uuid,
                EntityBaseState::new(position, vanilla_entities::BLOCK_DISPLAY.dimensions)
                    .with_velocity(velocity)
                    .with_rotation(rotation)
                    .with_on_ground(on_ground),
                world,
            ),
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
        &vanilla_entities::BLOCK_DISPLAY
    }

    fn pack_dirty_entity_data(&self) -> Option<Vec<DataValue>> {
        self.entity_data.lock().pack_dirty()
    }

    fn pack_all_entity_data(&self) -> Vec<DataValue> {
        self.entity_data.lock().pack_all()
    }

    fn save_additional(&self, nbt: &mut NbtCompound) {
        // Save block state ID directly - these are deterministic in Minecraft
        let block_state_id = *self.entity_data.lock().block_state.get();
        nbt.insert("block_state", i32::from(block_state_id.0));
    }

    fn load_additional(&self, nbt: &BorrowedNbtCompound<'_>) {
        // Convert to view type to access accessor methods
        let nbt: NbtCompoundView<'_, '_> = nbt.into();

        // Load block state ID
        if let Some(state_id) = nbt.int("block_state") {
            self.entity_data
                .lock()
                .block_state
                .set(BlockStateId(state_id as u16));
        }
    }
}
