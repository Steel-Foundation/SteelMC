//! Vanilla `Monster`: hostile pathfinder mobs.
//!
//! Monsters have no state of their own; the layer is presets over the mob
//! layer. Monster types also implement [`Enemy`].

use steel_math::fast_floor;
use steel_protocol::packets::game::SoundSource;
use steel_registry::entity_type::EntityTypeRef;
use steel_registry::sound_events;
use steel_registry::vanilla_game_rules::MOB_DROPS;
use steel_utils::BlockPos;

use crate::entity::component::definition::Definition;
use crate::entity::component::pipeline::{Stage, StageId};
use crate::entity::living_entity;
use crate::entity::mob::{self, LIGHT_MAGIC_VALUE_BRIGHTNESS_GATE, Parts};
use crate::entity::prelude::*;
use crate::entity::{Entity, EntityProps, EntitySpawnReason, LivingProps, Mob, MobBase, MobProps};
use crate::world::LevelReader;

/// Marks hostile entities, including those outside the `Monster` hierarchy.
/// Shared rules use this marker to reject leashing.
pub trait Enemy {}

/// Stage setting the experience reward every monster starts with.
pub const XP: StageId = StageId("monster.xp");
/// Stage speeding up despawning in bright light (`Monster.updateNoActionTime`).
pub const NO_ACTION_TIME: StageId = StageId("monster.no_action_time");

/// Vanilla's chains for a monster type: `LivingEntity` → `Mob` → `Monster`.
#[must_use]
pub fn definition<E: Parts>() -> Definition<E> {
    let mut definition = mob::stages::definition::<E>();
    definition.construct = definition
        .construct
        .then([Stage::effect(XP, |mob_base: &MobBase| {
            mob_base.set_xp_reward(5);
        })]);
    // `Monster.aiStep` updates the idle time before `super.aiStep()`.
    definition.ai_step = definition.ai_step.before(
        living_entity::AI_STEP,
        [Stage::effect(NO_ACTION_TIME, |monster: &E| {
            update_no_action_time(monster);
        })],
    );
    definition
}

/// `Monster`'s changes to the living defaults.
#[must_use]
pub const fn living_props<E: Parts>() -> LivingProps<E> {
    LivingProps {
        hurt_sound: |_, _| Some(&sound_events::ENTITY_HOSTILE_HURT),
        death_sound: |_| Some(&sound_events::ENTITY_HOSTILE_DEATH),
        should_drop_experience: |_| true,
        should_drop_loot: |_, world| world.get_game_rule(&MOB_DROPS),
        ..mob::living_props()
    }
}

/// `Monster`'s changes to the mob defaults.
#[must_use]
pub const fn mob_props<E: Parts>() -> MobProps<E> {
    MobProps {
        walk_target_value: |monster, pos| {
            monster
                .level()
                .map_or(0.0, |world| -world.pathfinding_cost_from_light_levels(pos))
        },
        is_preventing_player_rest: |_, _, _| true,
        ..mob::pathfinder_mob_props()
    }
}

/// `Monster`'s changes to the entity hooks.
#[must_use]
pub const fn entity_props<E: Parts>() -> EntityProps<E> {
    EntityProps {
        sound_source: |_| SoundSource::Hostile,
        swim_sound: |_| &sound_events::ENTITY_HOSTILE_SWIM,
        swim_splash_sound: |_| &sound_events::ENTITY_HOSTILE_SPLASH,
        fall_sounds: |_| {
            (
                &sound_events::ENTITY_HOSTILE_SMALL_FALL,
                &sound_events::ENTITY_HOSTILE_BIG_FALL,
            )
        },
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
