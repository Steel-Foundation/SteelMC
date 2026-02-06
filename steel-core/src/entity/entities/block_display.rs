//! Block display entity implementation.
//!
//! Display entities render a block, item, or text without collision.
//! They're commonly used for visual effects, holograms, and decorations.

use std::sync::Weak;

use simdnbt::borrow::{BaseNbtCompound as BorrowedNbtCompound, NbtCompound as NbtCompoundView};
use simdnbt::owned::NbtCompound;
use steel_registry::blocks::shapes::AABBd;
use steel_registry::entity_data::DataValue;
use steel_registry::entity_types::EntityTypeRef;
use steel_registry::vanilla_entities;
use steel_registry::vanilla_entity_data::BlockDisplayEntityData;
use steel_utils::BlockStateId;
use steel_utils::locks::SyncMutex;
use steel_utils::math::Vector3;
use uuid::Uuid;

use crate::entity::{Entity, EntityBase};
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
    pub fn new(id: i32, position: Vector3<f64>, world: Weak<World>) -> Self {
        Self {
            base: EntityBase::new(id, position, world),
            entity_data: SyncMutex::new(BlockDisplayEntityData::new()),
        }
    }

    /// Creates a new block display entity with a specific UUID.
    ///
    /// The `id` should be obtained from `next_entity_id()`.
    #[must_use]
    pub fn with_uuid(id: i32, position: Vector3<f64>, uuid: Uuid, world: Weak<World>) -> Self {
        Self {
            base: EntityBase::with_uuid(id, uuid, position, world),
            entity_data: SyncMutex::new(BlockDisplayEntityData::new()),
        }
    }

    /// Creates a block display entity from saved data.
    ///
    /// Display entities don't use velocity, rotation, or `on_ground`, so this is
    /// essentially an alias for `with_uuid`. Type-specific data is restored
    /// via `load_additional()` after construction.
    #[must_use]
    pub fn from_saved(id: i32, position: Vector3<f64>, uuid: Uuid, world: Weak<World>) -> Self {
        Self::with_uuid(id, position, uuid, world)
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
    fn base(&self) -> Option<&EntityBase> {
        Some(&self.base)
    }

    fn entity_type(&self) -> EntityTypeRef {
        vanilla_entities::BLOCK_DISPLAY
    }

    fn bounding_box(&self) -> AABBd {
        // Display entities have zero-size bounding boxes (no collision)
        let pos = self.position();
        AABBd {
            min_x: pos.x,
            min_y: pos.y,
            min_z: pos.z,
            max_x: pos.x,
            max_y: pos.y,
            max_z: pos.z,
        }
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
