//! Vanilla `AbstractWindCharge` — the shared player/breeze wind charge behavior.
//!
//! Wind charges fly in a straight line (no acceleration, inertia 1), never burn,
//! ignore other wind charges and end crystals, and burst into a block-triggering
//! wind explosion on any impact.

use std::sync::Arc;

use glam::DVec3;
use steel_registry::{vanilla_damage_types, vanilla_entities};
use steel_utils::WorldAabb;

use crate::enchantment_helper::{self, EnchantmentPostAttackContext};
use crate::entity::damage::DamageSource;
use crate::entity::projectile::{AbstractHurtingProjectile, ProjectileHit};
use crate::entity::{Entity, RemovalReason, SharedEntity};
use crate::world::{ClipHitResult, LevelReader as _};

/// Damage dealt to a directly hit entity.
const ENTITY_HIT_DAMAGE: f32 = 1.0;
/// How far a block-hit burst is pushed out along the hit face normal.
const BLOCK_HIT_EXPLOSION_OFFSET: f64 = 0.25;
/// Wind charges burst on their own this many blocks above the build limit.
const MAX_HEIGHT_ABOVE_BUILD_LIMIT: i32 = 30;
/// Vanilla lowers the wind charge bounding box below its position by this much.
const BOUNDING_BOX_Y_OFFSET: f64 = 0.15_f32 as f64;

/// Vanilla-shaped behavior shared by entities that extend `AbstractWindCharge`.
///
/// Concrete wind charges route their `Entity`/`Projectile` overrides to the
/// `wind_charge_*` methods, since Rust trait defaults cannot override supertraits.
pub trait AbstractWindCharge: AbstractHurtingProjectile {
    /// Bursts this wind charge at `position`.
    fn explode(self: Arc<Self>, position: DVec3);

    /// Vanilla `AbstractWindCharge.tick`.
    fn wind_charge_tick(self: Arc<Self>) {
        let Some(world) = self.level() else {
            return;
        };
        let max_y = world.max_y_exclusive() - 1;
        if self.block_position().y() > max_y + MAX_HEIGHT_ABOVE_BUILD_LIMIT {
            let position = self.position();
            Arc::clone(&self).explode(position);
            self.set_removed(RemovalReason::Discarded);
            return;
        }
        self.hurting_projectile_tick();
    }

    /// Vanilla `AbstractWindCharge.makeBoundingBox`.
    fn wind_charge_bounding_box(&self, position: DVec3) -> WorldAabb {
        let dimensions = self.entity_type().dimensions;
        let half_width = f64::from(dimensions.width / 2.0);
        let min_y = position.y - BOUNDING_BOX_Y_OFFSET;
        WorldAabb::new(
            position.x - half_width,
            min_y,
            position.z - half_width,
            position.x + half_width,
            min_y + f64::from(dimensions.height),
            position.z + half_width,
        )
    }

    /// Vanilla `AbstractWindCharge.canCollideWith`.
    fn wind_charge_can_collide_with(&self, other: &dyn Entity) -> bool {
        !other.is_abstract_wind_charge()
            && other.can_be_collided_with(Some(self.as_entity_event_source()))
            && !self.is_passenger_of_same_vehicle(other)
    }

    /// Vanilla `AbstractWindCharge.canHitEntity`.
    fn wind_charge_can_hit_entity(&self, entity: &dyn Entity) -> bool {
        !entity.is_abstract_wind_charge()
            && entity.entity_type() != &vanilla_entities::END_CRYSTAL
            && self.hurting_can_hit_entity(entity)
    }

    /// Vanilla `AbstractWindCharge.onHitEntity`: one point of wind charge
    /// damage credited to a living owner, then a burst at this charge's position.
    fn wind_charge_on_hit_entity(self: Arc<Self>, entity: &SharedEntity) {
        let owner = self
            .get_owner()
            .filter(|owner| owner.as_living_entity().is_some());
        if let Some(living_owner) = owner.as_ref().and_then(|owner| owner.as_living_entity()) {
            living_owner.set_last_hurt_mob(Some(entity));
        }

        let direct = Arc::clone(&self).into_shared_entity();
        let mut source = DamageSource::environment(&vanilla_damage_types::WIND_CHARGE)
            .with_direct_entity(direct);
        if let Some(owner) = owner {
            source = source.with_causing_entity(owner);
        }
        if let Some(world) = entity.level()
            && entity.hurt(&world, &source, ENTITY_HIT_DAMAGE)
            && entity.as_living_entity().is_some()
        {
            let context = EnchantmentPostAttackContext::new(entity, &source);
            enchantment_helper::do_post_attack_effects(&world, entity.as_ref(), &context);
        }

        let position = self.position();
        self.explode(position);
    }

    /// Vanilla `AbstractWindCharge.onHitBlock`: runs the block's projectile
    /// callback, then bursts just outside the hit face.
    fn wind_charge_on_hit_block(self: Arc<Self>, hit: &ClipHitResult) {
        self.projectile_on_hit_block(hit);
        let normal = hit.direction.offset_vec().as_dvec3() * BLOCK_HIT_EXPLOSION_OFFSET;
        Arc::clone(&self).explode(hit.location + normal);
        self.set_removed(RemovalReason::Discarded);
    }

    /// Vanilla `AbstractWindCharge.onHit`: dispatches the hit, then discards.
    fn wind_charge_on_hit(self: Arc<Self>, hit: &ProjectileHit) {
        Arc::clone(&self).projectile_on_hit(hit);
        self.set_removed(RemovalReason::Discarded);
    }
}
