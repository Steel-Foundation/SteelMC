//! Shared behavior of player and breeze wind charges.

use glam::DVec3;
use steel_registry::{vanilla_damage_types, vanilla_entities};
use steel_utils::WorldAabb;

use crate::enchantment_helper::{self, EnchantmentPostAttackContext};
use crate::entity::damage::DamageSource;
use crate::entity::entity;
use crate::entity::prelude::*;
use crate::entity::projectile::{self, ProjectileHit, ProjectileProps, hurting};
use crate::entity::{EntityMoveError, EntityProps, RemovalReason, SharedEntity};
use crate::world::{ClipHitResult, LevelReader as _};

const ENTITY_HIT_DAMAGE: f32 = 1.0;
const BLOCK_HIT_EXPLOSION_OFFSET: f64 = 0.25;
const MAX_HEIGHT_ABOVE_BUILD_LIMIT: i32 = 30;
const BOUNDING_BOX_Y_OFFSET: f64 = 0.15_f32 as f64;

/// An entity in the shared vanilla wind charge hierarchy.
pub trait AbstractWindCharge: Entity {
    /// Bursts this charge at `position`.
    fn explode(&self, position: DVec3);
}

/// Entity property overrides shared by wind charges.
#[must_use]
pub const fn entity_props<E: AbstractWindCharge + hurting::Parts>() -> EntityProps<E> {
    EntityProps {
        tick: tick::<E>,
        can_collide_with: can_collide_with::<E>,
        push_impulse: |_, _| (),
        make_bounding_box_at: make_bounding_box_at::<E>,
        try_set_position: try_set_position::<E>,
        ..hurting::entity_props()
    }
}

/// Projectile property overrides shared by wind charges.
#[must_use]
pub const fn projectile_props<E: AbstractWindCharge + hurting::Parts>() -> ProjectileProps<E> {
    ProjectileProps {
        can_hit_entity: can_hit_entity::<E>,
        on_hit: on_hit::<E>,
        on_hit_entity: on_hit_entity::<E>,
        on_hit_block: on_hit_block::<E>,
        ..hurting::projectile_props()
    }
}

/// Explodes above the build limit, otherwise ticks as a hurting projectile.
pub fn tick<E: AbstractWindCharge + hurting::Parts>(charge: &E) {
    let Some(world) = charge.level() else {
        return;
    };
    let max_y = world.max_y_exclusive() - 1;
    if charge.block_position().y() > max_y + MAX_HEIGHT_ABOVE_BUILD_LIMIT {
        charge.explode(charge.position());
        charge.set_removed(RemovalReason::Discarded);
    } else {
        hurting::tick(charge);
    }
}

/// Constructs the entity-sized box lowered from the logical position.
pub fn make_bounding_box_at<E: AbstractWindCharge>(charge: &E, position: DVec3) -> WorldAabb {
    let dimensions = charge.entity_type().dimensions;
    let half_width = f64::from(dimensions.half_width());
    let min_y = position.y - BOUNDING_BOX_Y_OFFSET;
    WorldAabb::entity_box(
        position.x,
        min_y,
        position.z,
        half_width,
        f64::from(dimensions.height),
    )
}

fn try_set_position<E: AbstractWindCharge>(
    charge: &E,
    position: DVec3,
) -> Result<(), EntityMoveError> {
    let result = entity::try_set_position(charge, position);
    charge
        .base()
        .set_bounding_box(make_bounding_box_at(charge, charge.position()));
    result
}

fn can_collide_with<E: AbstractWindCharge>(charge: &E, other: &dyn Entity) -> bool {
    !other.is_abstract_wind_charge() && entity::can_collide_with(charge, other)
}

fn can_hit_entity<E: AbstractWindCharge + hurting::Parts>(charge: &E, target: &dyn Entity) -> bool {
    !target.is_abstract_wind_charge()
        && target.entity_type() != &vanilla_entities::END_CRYSTAL
        && hurting::can_hit_entity(charge, target)
}

fn on_hit_entity<E: AbstractWindCharge + hurting::Parts>(
    charge: &E,
    target: &SharedEntity,
    location: DVec3,
) {
    projectile::on_hit_entity(charge, target, location);
    let Some(world) = charge.level() else {
        return;
    };
    let Some(direct) = charge.shared() else {
        return;
    };
    let owner = charge
        .projectile()
        .owner()
        .filter(|owner| owner.is_living_entity());
    if let Some(living_owner) = owner.as_ref().and_then(|owner| owner.as_living_entity()) {
        living_owner.set_last_hurt_mob(Some(target));
    }

    let mut source =
        DamageSource::environment(&vanilla_damage_types::WIND_CHARGE).with_direct_entity(direct);
    if let Some(owner) = owner {
        source = source.with_causing_entity(owner);
    }
    if target.hurt(&world, &source, ENTITY_HIT_DAMAGE) && target.is_living_entity() {
        let context = EnchantmentPostAttackContext::new(target, &source);
        enchantment_helper::do_post_attack_effects(&world, target.as_ref(), &context);
    }
    charge.explode(charge.position());
}

fn on_hit_block<E: AbstractWindCharge + hurting::Parts>(charge: &E, hit: &ClipHitResult) {
    projectile::on_hit_block(charge, hit);
    let normal = hit.direction.offset_vec().as_dvec3() * BLOCK_HIT_EXPLOSION_OFFSET;
    charge.explode(hit.location + normal);
    charge.set_removed(RemovalReason::Discarded);
}

fn on_hit<E: AbstractWindCharge + hurting::Parts>(charge: &E, hit: &ProjectileHit) {
    projectile::on_hit(charge, hit);
    charge.set_removed(RemovalReason::Discarded);
}
