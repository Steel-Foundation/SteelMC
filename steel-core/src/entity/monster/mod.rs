//! Vanilla `Monster`: hostile pathfinder mobs.
//!
//! Monsters have no state of their own; the layer is presets over the mob
//! layer. Monster types also implement [`Enemy`].

use crate::entity::DamageSource;
use crate::physics::MoveResult;
use crate::player::Player;
use crate::world::World;
use steel_math::fast_floor;
use steel_protocol::packets::game::SoundSource;
use steel_registry::entity_type::EntityTypeRef;
use steel_registry::sound_event::SoundEventRef;
use steel_registry::sound_events;
use steel_registry::vanilla_game_rules::MOB_DROPS;
use steel_utils::BlockPos;

use crate::entity::mob::{self, LIGHT_MAGIC_VALUE_BRIGHTNESS_GATE, Parts};
use crate::entity::prelude::*;
use crate::entity::{Entity, EntityProps, EntitySpawnReason, LivingProps, Mob, MobBase, MobProps};
use crate::world::LevelReader;

/// Marks hostile entities, including those outside the `Monster` hierarchy.
/// Shared rules use this marker to reject leashing.
pub trait Enemy {}

/// `Monster`'s constructor: the experience reward every monster starts with.
pub fn construct(mob_base: &MobBase) {
    mob_base.set_xp_reward(5);
}

/// `Monster`'s [`LivingProps::hurt_sound`].
#[expect(
    clippy::unnecessary_wraps,
    reason = "the table entry fixes the signature"
)]
pub fn hurt_sound<E: Parts>(_: &E, _: &DamageSource) -> Option<SoundEventRef> {
    Some(&sound_events::ENTITY_HOSTILE_HURT)
}

/// `Monster`'s [`LivingProps::death_sound`].
#[expect(
    clippy::unnecessary_wraps,
    reason = "the table entry fixes the signature"
)]
pub fn death_sound<E: Parts>(_: &E) -> Option<SoundEventRef> {
    Some(&sound_events::ENTITY_HOSTILE_DEATH)
}

/// `Monster`'s [`LivingProps::should_drop_experience`].
pub const fn should_drop_experience<E: Parts>(_: &E) -> bool {
    true
}

/// `Monster`'s [`LivingProps::should_drop_loot`].
pub fn should_drop_loot<E: Parts>(_: &E, world: &World) -> bool {
    world.get_game_rule(&MOB_DROPS)
}

/// `Monster`'s [`LivingProps::ai_step`].
///
/// `Monster.aiStep` updates the idle time before `super.aiStep()`.
pub fn ai_step<E: Parts>(monster: &E) -> Option<MoveResult> {
    update_no_action_time(monster);
    mob::ai_step(monster)
}

/// `Monster`'s changes to the living defaults.
#[must_use]
pub const fn living_props<E: Parts>() -> LivingProps<E> {
    LivingProps {
        hurt_sound: hurt_sound::<E>,
        death_sound: death_sound::<E>,
        should_drop_experience: should_drop_experience::<E>,
        should_drop_loot: should_drop_loot::<E>,
        ai_step: ai_step::<E>,
        ..mob::living_props()
    }
}

/// `Monster`'s [`MobProps::walk_target_value`].
pub fn walk_target_value<E: Parts>(monster: &E, pos: BlockPos) -> f32 {
    monster
        .level()
        .map_or(0.0, |world| -world.pathfinding_cost_from_light_levels(pos))
}

/// `Monster`'s [`MobProps::is_preventing_player_rest`].
pub const fn is_preventing_player_rest<E: Parts>(_: &E, _: &World, _: &Player) -> bool {
    true
}

/// `Monster`'s changes to the mob defaults.
#[must_use]
pub const fn mob_props<E: Parts>() -> MobProps<E> {
    MobProps {
        walk_target_value: walk_target_value::<E>,
        is_preventing_player_rest: is_preventing_player_rest::<E>,
        ..mob::pathfinder::mob_props()
    }
}

/// `Monster`'s [`EntityProps::sound_source`].
pub const fn sound_source<E: Parts>(_: &E) -> SoundSource {
    SoundSource::Hostile
}

/// `Monster`'s [`EntityProps::swim_sound`].
pub fn swim_sound<E: Parts>(_: &E) -> SoundEventRef {
    &sound_events::ENTITY_HOSTILE_SWIM
}

/// `Monster`'s [`EntityProps::swim_splash_sound`].
pub fn swim_splash_sound<E: Parts>(_: &E) -> SoundEventRef {
    &sound_events::ENTITY_HOSTILE_SPLASH
}

/// `Monster`'s [`EntityProps::fall_sounds`].
pub fn fall_sounds<E: Parts>(_: &E) -> (SoundEventRef, SoundEventRef) {
    (
        &sound_events::ENTITY_HOSTILE_SMALL_FALL,
        &sound_events::ENTITY_HOSTILE_BIG_FALL,
    )
}

/// `Monster`'s changes to the entity properties.
#[must_use]
pub const fn entity_props<E: Parts>() -> EntityProps<E> {
    EntityProps {
        sound_source: sound_source::<E>,
        swim_sound: swim_sound::<E>,
        swim_splash_sound: swim_splash_sound::<E>,
        fall_sounds: fall_sounds::<E>,
        ..mob::entity_props()
    }
}

/// `Monster.updateNoActionTime`: bright light makes monsters idle faster,
/// so they despawn sooner.
fn update_no_action_time(monster: &dyn Entity) {
    let Some(world) = monster.level() else {
        return;
    };
    let Some(living) = monster.as_living_entity() else {
        return;
    };
    let position = monster.position();
    let eye_pos = BlockPos::new(
        fast_floor(position.x),
        fast_floor(monster.eye_y()),
        fast_floor(position.z),
    );
    if world.light_level_dependent_magic_value(eye_pos) > LIGHT_MAGIC_VALUE_BRIGHTNESS_GATE {
        living.set_no_action_time(living.no_action_time() + 2);
    }
}

/// `Monster.checkAnyLightMonsterSpawnRules`: the mob spawn rules, at any light.
pub fn check_any_light_monster_spawn_rules(
    entity_type: EntityTypeRef,
    level: &dyn LevelReader,
    spawn_reason: EntitySpawnReason,
    pos: BlockPos,
) -> bool {
    Mob::check_mob_spawn_rules(entity_type, level, spawn_reason, pos)
}

#[cfg(test)]
mod tests;
