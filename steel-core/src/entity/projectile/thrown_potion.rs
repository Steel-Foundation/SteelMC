//! Vanilla `AbstractThrownPotion`: what splash and lingering potions share.

use crate::entity::Living;
use crate::entity::prelude::*;
use crate::entity::projectile;
use std::sync::Arc;

use steel_registry::blocks::block_state_ext::BlockStateExt as _;
use steel_registry::blocks::properties::BlockStateProperties;
use steel_registry::data_components::{PotionContents, vanilla_components};
use steel_registry::item_stack::ItemStack;
use steel_registry::potion::Potion;
use steel_registry::vanilla_block_tags::BlockTag;
use steel_registry::vanilla_potion_tags::PotionTag;
use steel_registry::{level_events, vanilla_damage_types};
use steel_utils::{BlockPos, BlockStateId, Direction, types::UpdateFlags};

use crate::behavior::MOB_EFFECT_BEHAVIORS;
use crate::behavior::blocks::{CampfireBlock, CandleBlock};
use crate::entity::damage::DamageSource;
use crate::entity::projectile::throwable_item;
use crate::entity::projectile::{Projectile, ProjectileProps, throwable};
use crate::entity::{EntityProps, ProjectileHit, RemovalReason, SharedEntity};
use crate::world::ClipHitResult;
use crate::world::World;

/// Squared distance within which a splash reaches an entity.
pub const SPLASH_RANGE_SQ: f64 = 16.0;
/// How far the splash search box extends past the potion's box on the X and Z axes.
pub const SPLASH_HORIZONTAL_REACH: f64 = 4.0;
/// How far the splash search box extends past the potion's box on the Y axis.
pub const SPLASH_VERTICAL_REACH: f64 = 2.0;

/// A concrete vanilla `AbstractThrownPotion` type.
pub trait Parts: throwable_item::Parts {
    /// Applies the potion's effects where it landed: a splash or a cloud.
    fn on_hit_as_potion(
        &self,
        entity: &SharedEntity,
        world: &Arc<World>,
        potion_item: &ItemStack,
        hit: &ProjectileHit,
    );
}

/// `AbstractThrownPotion`'s [`EntityProps::default_gravity`].
#[must_use]
pub const fn default_gravity<E: Parts>(_: &E) -> f64 {
    0.05
}

/// `AbstractThrownPotion`'s overrides of the entity properties.
#[must_use]
pub const fn entity_props<E: Parts>() -> EntityProps<E> {
    EntityProps {
        default_gravity: default_gravity::<E>,
        ..throwable::entity_props()
    }
}

/// `AbstractThrownPotion`'s [`ProjectileProps::horizontal_hurt_knockback_direction`].
pub fn horizontal_hurt_knockback_direction<E: Parts>(
    potion: &E,
    hurt_entity: &Living<'_>,
    _: &DamageSource,
) -> (f64, f64) {
    let delta = hurt_entity.position() - potion.position();
    (delta.x, delta.z)
}

/// `AbstractThrownPotion`'s overrides of the projectile properties.
#[must_use]
pub const fn projectile_props<E: Parts>() -> ProjectileProps<E> {
    ProjectileProps {
        horizontal_hurt_knockback_direction: horizontal_hurt_knockback_direction::<E>,
        on_hit: on_hit::<E>,
        on_hit_block: on_hit_block::<E>,
        ..ProjectileProps::new()
    }
}

/// `AbstractThrownPotion`'s [`ProjectileProps::on_hit`]: after `super`, affects
/// water-sensitive and burning entities, applies the potion, then breaks with
/// its particles and sound.
pub fn on_hit<E: Parts>(potion: &E, hit: &ProjectileHit) {
    projectile::on_hit(potion, hit);
    let Some(world) = potion.level() else {
        return;
    };
    let Some(entity) = potion.shared() else {
        return;
    };

    let potion_item = potion.throwable_item().item();
    let contents =
        potion_item.get_or_default(vanilla_components::POTION_CONTENTS, PotionContents::empty());
    affect_entities_around(&potion.projectile(), &entity, &world, &contents);
    if contents.has_effects() {
        potion.on_hit_as_potion(&entity, &world, &potion_item, hit);
    }

    let has_instant_effects = contents
        .potion()
        .is_some_and(|potion| potion_has_instant_effects(potion.value()));
    let (event_type, sound_event) = if has_instant_effects {
        (
            level_events::PARTICLES_INSTANT_POTION_SPLASH,
            level_events::SOUND_INSTANT_POTION_SPLASH,
        )
    } else {
        (
            level_events::PARTICLES_SPELL_POTION_SPLASH,
            level_events::SOUND_SPELL_POTION_SPLASH,
        )
    };
    world.level_event(
        event_type,
        potion.block_position(),
        contents.get_color(),
        None,
    );
    if !potion.is_silent() {
        world.level_event(sound_event, potion.block_position(), 0, None);
    }
    potion.set_removed(RemovalReason::Discarded);
}

/// Hurts water-sensitive entities in range and extinguishes burning ones, as
/// the potion's tags allow.
fn affect_entities_around(
    potion: &Projectile<'_>,
    entity: &SharedEntity,
    world: &Arc<World>,
    contents: &PotionContents,
) {
    let hurts_water_sensitive = contents.is_in_tag(&PotionTag::HURTS_WATER_SENSITIVE_ENTITIES);
    let extinguishes = contents.is_in_tag(&PotionTag::EXTINGUISHES_ENTITIES);
    // TODO: rehydrate nearby axolotls for `REHYDRATES_AXOLOTLS` once axolotls exist.
    if !hurts_water_sensitive && !extinguishes {
        return;
    }

    let aabb = potion.bounding_box().inflate_xyz(
        SPLASH_HORIZONTAL_REACH,
        SPLASH_VERTICAL_REACH,
        SPLASH_HORIZONTAL_REACH,
    );
    let potion_pos = potion.position();

    for target in world.get_entities_in_aabb(&aabb) {
        if target.as_living_entity().is_none() {
            continue;
        }
        let sensitive_to_water = target.entity_type().flags.is_sensitive_to_water;
        if !sensitive_to_water && !target.is_on_fire() {
            continue;
        }
        if potion_pos.distance_squared(target.position()) >= SPLASH_RANGE_SQ {
            continue;
        }

        if hurts_water_sensitive && sensitive_to_water {
            let mut damage = DamageSource::environment(&vanilla_damage_types::INDIRECT_MAGIC)
                .with_direct_entity(Arc::clone(entity));
            if let Some(owner) = potion.owner() {
                damage = damage.with_causing_entity(owner);
            }
            target.hurt(world, &damage, 1.0);
        }

        if extinguishes && target.is_on_fire() && target.is_alive() {
            target.extinguish_fire();
        }
    }
}

/// `AbstractThrownPotion`'s [`ProjectileProps::on_hit_block`]: after `super`,
/// `DOUSES_FIRE` potions put out fire around the hit face.
pub fn on_hit_block<E: Parts>(potion: &E, hit: &ClipHitResult) {
    projectile::on_hit_block(potion, hit);
    let projectile = potion.projectile();
    let Some(world) = potion.level() else {
        return;
    };

    let contents = potion
        .throwable_item()
        .item()
        .get_or_default(vanilla_components::POTION_CONTENTS, PotionContents::empty());
    if !contents.is_in_tag(&PotionTag::DOUSES_FIRE) {
        return;
    }

    let block_effect_pos = hit.direction.relative(hit.block_pos);
    douse_fire(&projectile, &world, block_effect_pos);
    douse_fire(
        &projectile,
        &world,
        hit.direction.opposite().relative(block_effect_pos),
    );
    for direction in Direction::HORIZONTAL {
        douse_fire(&projectile, &world, direction.relative(block_effect_pos));
    }
}

/// Puts out fire, lit campfires and lit candles at `pos`.
fn douse_fire(potion: &Projectile<'_>, world: &Arc<World>, pos: BlockPos) {
    let state = world.get_block_state(pos);
    if state.get_block().has_tag(&BlockTag::FIRE) {
        world.destroy_block_by_entity(pos, false, potion.entity());
    } else if CandleBlock::is_lit(state) {
        CandleBlock::extinguish(None, state, world, pos);
    } else if CampfireBlock::is_lit_campfire(state) {
        douse_lit_campfire(world, pos, state, potion.owner());
    }
}

/// Checks for instant effects in the base potion excluding custom effects
fn potion_has_instant_effects(potion: &Potion) -> bool {
    potion.effects.iter().any(|effect| {
        MOB_EFFECT_BEHAVIORS
            .get_behavior(effect.effect)
            .as_instantaneous()
            .is_some()
    })
}

/// Plays the extinguish event, douses the campfire and clears `LIT`.
fn douse_lit_campfire(
    world: &Arc<World>,
    pos: BlockPos,
    state: BlockStateId,
    owner: Option<SharedEntity>,
) {
    world.level_event(level_events::SOUND_EXTINGUISH_FIRE, pos, 0, None);
    CampfireBlock::douse(owner.as_deref(), world, pos);
    world.set_block(
        pos,
        state.set_value(&BlockStateProperties::LIT, false),
        UpdateFlags::UPDATE_ALL,
    );
}
