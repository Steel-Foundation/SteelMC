//! The Ender Dragon's sub-entity hitboxes.

use std::sync::Weak;

use glam::DVec3;
use steel_registry::entity_type::{EntityDimensions, EntityTypeRef};
use steel_utils::{Downcast as _, DowncastType, DowncastTypeKey};

use super::EnderDragonEntity;
use crate::entity::damage::DamageSource;
use crate::entity::{Entity, EntityBase, EntityPose, PartEntity, PartEntityBase};
use crate::world::World;

/// One of the Ender Dragon's eight hitboxes (vanilla `EnderDragonPart`). Per-part damage
/// scaling lives in [`EnderDragonEntity::hurt_part`](super::EnderDragonEntity::hurt_part).
///
/// Never sent to clients (they synthesize parts from the dragon's network ID) nor saved
/// (parts live only in the world's parts map), so vanilla's empty save/synched-data
/// overrides and throwing `getAddEntityPacket` have no counterpart.
pub struct EnderDragonPart {
    base: EntityBase,
    part_base: PartEntityBase,
    /// The dragon's own type, as vanilla passes `parentMob.getType()`; fire immunity and
    /// damage cooldowns resolve through it.
    entity_type: EntityTypeRef,
}

// SAFETY: The owner-scoped type key uniquely identifies EnderDragonPart.
unsafe impl DowncastType for EnderDragonPart {
    const TYPE_KEY: DowncastTypeKey = DowncastTypeKey::new("steel:entity/ender_dragon_part");
}

impl EnderDragonPart {
    /// Creates a part with its own hitbox. `id` comes from the block the dragon reserved,
    /// landing on `parent_id + index + 1`.
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
        // Vanilla calls `refreshDimensions()` here; Rust cannot dispatch to an override
        // before `self` exists, so the size goes straight to the base.
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

    /// Ignores the pose, like vanilla. The default would report the dragon's 16x8 type
    /// size and snap every part's box to it on the next dimension refresh.
    fn dimensions_for_pose(&self, _pose: EntityPose) -> EntityDimensions {
        self.part_base.size()
    }

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

    /// The non-living default is false, which would let projectiles pass straight
    /// through the dragon.
    fn is_pickable(&self) -> bool {
        true
    }

    /// Also matches the parent, as vanilla `EnderDragonPart.is` does.
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
