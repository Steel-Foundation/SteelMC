//! Minimal persistent mob entity used before full mob behavior exists.

use std::sync::Weak;
use std::sync::atomic::{AtomicBool, Ordering};

use crossbeam::atomic::AtomicCell;
use glam::DVec3;
use simdnbt::borrow::{BaseNbtCompound as BorrowedNbtCompound, NbtCompound as NbtCompoundView};
use simdnbt::owned::NbtCompound;
use steel_registry::blocks::shapes::AABBd;
use steel_registry::entity_types::EntityTypeRef;
use steel_utils::locks::SyncMutex;
use uuid::Uuid;

use crate::entity::{Entity, EntityBase};
use crate::world::World;

/// Placeholder for mobs that must exist for vanilla worldgen side effects.
///
/// This intentionally differs from vanilla entity classes: it preserves identity,
/// type, position, rotation, dimensions, and persistence, but does not implement
/// AI, attributes, equipment, sounds, or combat yet.
pub struct DummyMobEntity {
    base: EntityBase,
    entity_type: EntityTypeRef,
    rotation: AtomicCell<(f32, f32)>,
    velocity: SyncMutex<DVec3>,
    on_ground: AtomicBool,
    data: SyncMutex<NbtCompound>,
}

impl DummyMobEntity {
    /// Creates a fresh dummy mob.
    #[must_use]
    pub fn new(id: i32, position: DVec3, world: Weak<World>, entity_type: EntityTypeRef) -> Self {
        Self {
            base: EntityBase::new(id, position, world),
            entity_type,
            rotation: AtomicCell::new((0.0, 0.0)),
            velocity: SyncMutex::new(DVec3::ZERO),
            on_ground: AtomicBool::new(false),
            data: SyncMutex::new(NbtCompound::new()),
        }
    }

    /// Creates a dummy mob from persistent entity data.
    #[must_use]
    #[expect(
        clippy::too_many_arguments,
        reason = "dummy mobs preserve persisted base fields plus their registry type"
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
            base: EntityBase::with_uuid(id, uuid, position, world),
            entity_type,
            rotation: AtomicCell::new(rotation),
            velocity: SyncMutex::new(velocity),
            on_ground: AtomicBool::new(on_ground),
            data: SyncMutex::new(NbtCompound::new()),
        }
    }

    /// Sets position and rotation, matching vanilla `Entity.snapTo`.
    pub fn snap_to(&self, position: DVec3, yaw: f32, pitch: f32) {
        self.set_position(position);
        self.rotation.store((yaw, pitch));
    }
}

impl Entity for DummyMobEntity {
    fn base(&self) -> Option<&EntityBase> {
        Some(&self.base)
    }

    fn entity_type(&self) -> EntityTypeRef {
        self.entity_type
    }

    fn bounding_box(&self) -> AABBd {
        let pos = self.position();
        let dims = self.entity_type().dimensions;
        let half_width = f64::from(dims.width) / 2.0;
        let height = f64::from(dims.height);
        AABBd {
            min_x: pos.x - half_width,
            min_y: pos.y,
            min_z: pos.z - half_width,
            max_x: pos.x + half_width,
            max_y: pos.y + height,
            max_z: pos.z + half_width,
        }
    }

    fn rotation(&self) -> (f32, f32) {
        self.rotation.load()
    }

    fn velocity(&self) -> DVec3 {
        *self.velocity.lock()
    }

    fn set_velocity(&self, velocity: DVec3) {
        *self.velocity.lock() = velocity;
    }

    fn on_ground(&self) -> bool {
        self.on_ground.load(Ordering::Relaxed)
    }

    fn set_on_ground(&self, on_ground: bool) {
        self.on_ground.store(on_ground, Ordering::Relaxed);
    }

    fn tick(&self) {
        // TODO: Replace dummy mob ticking with full vanilla mob behavior.
    }

    fn load_additional(&self, nbt: &BorrowedNbtCompound<'_>) {
        let nbt_view: NbtCompoundView<'_, '_> = nbt.into();
        *self.data.lock() = nbt_view.to_owned();
    }

    fn save_additional(&self, nbt: &mut NbtCompound) {
        *nbt = self.data.lock().clone();
    }
}
