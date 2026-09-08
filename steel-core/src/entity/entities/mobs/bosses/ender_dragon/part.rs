//! The Ender Dragon's sub-entity hitboxes.

use std::sync::Weak;

use glam::DVec3;
use steel_registry::entity_type::{EntityDimensions, EntityTypeRef};
use steel_utils::{Downcast as _, DowncastType, DowncastTypeKey};

use super::EnderDragonEntity;
use crate::entity::damage::DamageSource;
use crate::entity::{Entity, EntityBase, EntityPose, PartEntity, PartEntityBase};
use crate::world::World;

/// One of the Ender Dragon's eight hitboxes.
///
/// Mirrors vanilla `EnderDragonPart`. Splitting the dragon this way is what lets a
/// hit on the head count for full damage while a hit on the tail is scaled down;
/// the scaling itself lives on the dragon, in
/// [`EnderDragonEntity::hurt_part`](super::EnderDragonEntity::hurt_part).
///
/// A part is never sent to a client and never persisted: the client synthesizes all
/// eight from the dragon's network ID, and parts are held only in the world's parts
/// map rather than the entity manager that drives saving. So vanilla's empty
/// `readAdditionalSaveData` / `addAdditionalSaveData` / `defineSynchedData` and its
/// throwing `getAddEntityPacket` all need no counterpart here.
pub struct EnderDragonPart {
    base: EntityBase,
    part_base: PartEntityBase,
    /// Vanilla constructs the part with `super(parentMob.getType(), …)`, so a part
    /// reports the dragon's own entity type. Fire immunity and the damage-cooldown
    /// rules resolve through it.
    entity_type: EntityTypeRef,
}

// SAFETY: The owner-scoped type key uniquely identifies EnderDragonPart.
unsafe impl DowncastType for EnderDragonPart {
    const TYPE_KEY: DowncastTypeKey = DowncastTypeKey::new("steel:entity/ender_dragon_part");
}

impl EnderDragonPart {
    /// Creates a part with its own hitbox.
    ///
    /// Mirrors `new EnderDragonPart(parent, name, width, height)`. `id` comes from
    /// the block the dragon reserved, so that it lands on `parent_id + index + 1`.
    #[must_use]
    pub fn new(
        entity_type: EntityTypeRef,
        id: i32,
        name: &'static str,
        width: f32,
        height: f32,
        position: DVec3,
        world: Weak<World>,
    ) -> Self {
        // Vanilla's constructor calls `refreshDimensions()`; Rust cannot dispatch to
        // an override before `self` exists, so the size is passed straight to the
        // base. `dimensions_for_pose` below keeps the two in agreement afterwards.
        let size = EntityDimensions::with_default_eye_height(width, height);
        Self {
            base: EntityBase::new(id, position, size, world),
            part_base: PartEntityBase::new(name, size),
            entity_type,
        }
    }
}

impl Entity for EnderDragonPart {
    fn base(&self) -> &EntityBase {
        &self.base
    }

    fn entity_type(&self) -> EntityTypeRef {
        self.entity_type
    }

    /// Mirrors vanilla `EnderDragonPart.getDimensions`, which ignores the pose and
    /// returns the part's own size.
    ///
    /// Without this the shared default would report the dragon's 16x8 type
    /// dimensions, and every part's bounding box would snap to it the first time
    /// anything refreshed dimensions.
    fn dimensions_for_pose(&self, _pose: EntityPose) -> EntityDimensions {
        self.part_base.size()
    }

    /// Mirrors vanilla `EnderDragonPart.hurtServer`, which is `final`.
    fn hurt(&self, world: &World, source: &DamageSource, amount: f32) -> bool {
        if self.is_invulnerable_to_base(source) {
            return false;
        }
        // The `Arc` has to outlive the borrow taken from it.
        let Some(parent) = self.parent() else {
            return false;
        };
        let Some(dragon) = parent.downcast_ref::<EnderDragonEntity>() else {
            return false;
        };
        dragon.hurt_part(world, self, source, amount)
    }

    /// Mirrors vanilla `EnderDragonPart.isPickable`.
    ///
    /// The shared default is false for anything that is not a living entity, which
    /// would make `can_be_hit_by_projectile` reject every part and let arrows pass
    /// straight through the dragon.
    fn is_pickable(&self) -> bool {
        true
    }

    /// Mirrors vanilla `EnderDragonPart.is`, which also matches the parent.
    fn is_same_entity(&self, other: &dyn Entity) -> bool {
        self.id() == other.id()
            || self
                .parent()
                .is_some_and(|parent| parent.id() == other.id())
    }
}

impl PartEntity for EnderDragonPart {
    fn part_base(&self) -> &PartEntityBase {
        &self.part_base
    }
}
