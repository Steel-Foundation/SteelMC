//! Minimal persistent mob entity used before full mob behavior exists.

use std::sync::Weak;

use crossbeam::atomic::AtomicCell;
use glam::DVec3;
use steel_registry::blocks::shapes::AABBd;
use steel_registry::entity_types::EntityTypeRef;
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
}

impl DummyMobEntity {
    /// Creates a fresh dummy mob.
    #[must_use]
    pub fn new(id: i32, position: DVec3, world: Weak<World>, entity_type: EntityTypeRef) -> Self {
        Self {
            base: EntityBase::new(id, position, world),
            entity_type,
            rotation: AtomicCell::new((0.0, 0.0)),
        }
    }

    /// Creates a dummy mob from persistent entity data.
    #[must_use]
    pub fn from_saved(
        id: i32,
        position: DVec3,
        uuid: Uuid,
        rotation: (f32, f32),
        world: Weak<World>,
        entity_type: EntityTypeRef,
    ) -> Self {
        Self {
            base: EntityBase::with_uuid(id, uuid, position, world),
            entity_type,
            rotation: AtomicCell::new(rotation),
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

    fn tick(&self) {
        // TODO: Replace dummy mob ticking with full vanilla mob behavior.
    }
}
