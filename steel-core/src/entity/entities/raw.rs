//! NBT-preserving fallback entity.

use std::sync::Weak;

use glam::DVec3;
use simdnbt::borrow::{BaseNbtCompound as BorrowedNbtCompound, NbtCompound as NbtCompoundView};
use simdnbt::owned::NbtCompound;
use steel_registry::entity_type::EntityTypeRef;
use steel_utils::locks::SyncMutex;
use uuid::Uuid;

use crate::entity::{Entity, EntityBase, EntityBaseState};
use crate::world::World;

/// Steel-specific fallback for entity types whose runtime behavior is not implemented yet.
///
/// Vanilla has concrete classes for every entity type. Steel uses this only to preserve
/// worldgen and disk NBT until the corresponding typed implementation is added.
pub struct RawEntity {
    base: EntityBase,
    entity_type: EntityTypeRef,
    data: SyncMutex<NbtCompound>,
}

impl RawEntity {
    /// Creates a fresh raw entity for an entity type Steel cannot behaviorally model yet.
    #[must_use]
    pub fn new(id: i32, position: DVec3, world: Weak<World>, entity_type: EntityTypeRef) -> Self {
        Self {
            base: EntityBase::new(id, position, entity_type.dimensions, world),
            entity_type,
            data: SyncMutex::new(NbtCompound::new()),
        }
    }

    /// Creates a raw entity from base entity data.
    #[must_use]
    #[expect(
        clippy::too_many_arguments,
        reason = "raw fallback must preserve all persisted base entity fields"
    )]
    pub fn from_saved(
        id: i32,
        position: DVec3,
        uuid: Uuid,
        velocity: DVec3,
        rotation: (f32, f32),
        on_ground: bool,
        world: Weak<World>,
        entity_type: EntityTypeRef,
    ) -> Self {
        Self {
            base: EntityBase::with_uuid_and_state(
                id,
                uuid,
                EntityBaseState::new(position, entity_type.dimensions)
                    .with_velocity(velocity)
                    .with_rotation(rotation)
                    .with_on_ground(on_ground),
                world,
            ),
            entity_type,
            data: SyncMutex::new(NbtCompound::new()),
        }
    }

    /// Sets position and rotation, matching vanilla `Entity.snapTo`.
    pub fn snap_to(&self, position: DVec3, yaw: f32, pitch: f32) {
        self.set_position(position);
        self.base.set_rotation((yaw, pitch));
    }

    /// Marks a raw mob as persistent when vanilla structure generation would do so.
    pub fn set_persistence_required(&self) {
        self.data.lock().insert("PersistenceRequired", 1_i8);
    }
}

impl Entity for RawEntity {
    fn base(&self) -> &EntityBase {
        &self.base
    }

    fn entity_type(&self) -> EntityTypeRef {
        self.entity_type
    }

    fn tick(&self) {
        // TODO: Replace raw entity ticking with full vanilla behavior for this entity type.
    }

    fn load_additional(&self, nbt: &BorrowedNbtCompound<'_>) {
        let nbt_view: NbtCompoundView<'_, '_> = nbt.into();
        *self.data.lock() = nbt_view.to_owned();
    }

    fn save_additional(&self, nbt: &mut NbtCompound) {
        *nbt = self.data.lock().clone();
    }
}
