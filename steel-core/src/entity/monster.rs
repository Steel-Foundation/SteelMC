//! Shared hostile-mob markers, behavior, and spawn rules.

use steel_math::fast_floor;
use steel_protocol::packets::game::SoundSource;
use steel_registry::dimension_type::MonsterSpawnLightLevel;
use steel_registry::entity_type::EntityTypeRef;
use steel_registry::sound_event::SoundEventRef;
use steel_registry::sound_events;
use steel_registry::vanilla_game_rules::MOB_DROPS;
use steel_utils::BlockPos;
use steel_utils::random::Random;
use steel_utils::types::Difficulty;

use crate::chunk::light::{LightLayer, MAX_LIGHT_LEVEL};
use crate::entity::damage::DamageSource;
use crate::entity::mob::LIGHT_MAGIC_VALUE_BRIGHTNESS_GATE;
use crate::entity::{EntitySpawnReason, Mob, PathfinderMob, SharedEntity};
use crate::physics::MoveResult;
use crate::player::Player;
use crate::world::{LevelReader, World};

pub const DEFAULT_XP_REWARD: i32 = 5;

const THUNDERING_SKY_DARKENING: u8 = 10;
const BRIGHT_LIGHT_NO_ACTION_TIME_INCREMENT: i32 = 2;
const SKY_LIGHT_SPAWN_ROLL_BOUND: i32 = 32;

/// Marks hostile entities, including those outside the `Monster` hierarchy.
/// Shared rules use this marker to reject leashing.
pub trait Enemy {}

impl<T: Monster + ?Sized> Enemy for T {}

/// Shared hostile-mob behavior. Concrete mobs delegate to `_monster` hooks
/// to avoid name collisions with base-trait methods.
pub trait Monster: PathfinderMob + Enemy {
    fn default_xp_reward_monster(&self) -> i32 {
        DEFAULT_XP_REWARD
    }

    fn sound_source_monster(&self) -> SoundSource {
        SoundSource::Hostile
    }

    fn hurt_sound_monster(&self, _source: &DamageSource) -> Option<SoundEventRef> {
        Some(&sound_events::ENTITY_HOSTILE_HURT)
    }

    fn death_sound_monster(&self) -> Option<SoundEventRef> {
        Some(&sound_events::ENTITY_HOSTILE_DEATH)
    }

    fn fall_sounds_monster(&self) -> (SoundEventRef, SoundEventRef) {
        (
            &sound_events::ENTITY_HOSTILE_SMALL_FALL,
            &sound_events::ENTITY_HOSTILE_BIG_FALL,
        )
    }

    /// Returns the hostile water-entry splash sound.
    fn swim_splash_sound_monster(&self) -> SoundEventRef {
        &sound_events::ENTITY_HOSTILE_SPLASH
    }

    /// Returns the hostile swim sound.
    fn swim_sound_monster(&self) -> SoundEventRef {
        &sound_events::ENTITY_HOSTILE_SWIM
    }

    /// Adds two idle ticks in bright light to hasten despawning.
    fn update_no_action_time(&self) {
        let Some(world) = self.level() else {
            return;
        };
        let eye_pos = BlockPos::new(
            fast_floor(self.position().x),
            fast_floor(self.get_eye_y()),
            fast_floor(self.position().z),
        );
        if world.light_level_dependent_magic_value(eye_pos) > LIGHT_MAGIC_VALUE_BRIGHTNESS_GATE {
            self.set_no_action_time(self.no_action_time() + BRIGHT_LIGHT_NO_ACTION_TIME_INCREMENT);
        }
    }

    fn monster_ai_step(&self, entity: &SharedEntity) -> Option<MoveResult> {
        self.update_no_action_time();
        self.mob_ai_step(entity)
    }

    /// Favors darker positions for pathfinding.
    fn get_walk_target_value_monster(&self, pos: BlockPos) -> f32 {
        let Some(world) = self.level() else {
            return 0.0;
        };
        -world.pathfinding_cost_from_light_levels(pos)
    }

    fn is_preventing_player_rest(&self, _level: &World, _player: &Player) -> bool {
        true
    }

    fn should_drop_experience_monster(&self) -> bool {
        true
    }

    /// Allows loot when `mobDrops` is enabled, including for babies.
    fn should_drop_loot_monster(&self, world: &World) -> bool {
        world.get_game_rule(&MOB_DROPS)
    }

    /// Checks dimension light limits using the caller's seeded RNG.
    fn is_dark_enough_to_spawn(level: &World, pos: BlockPos, random: &mut impl Random) -> bool
    where
        Self: Sized,
    {
        if i32::from(level.light_value_at(LightLayer::Sky, pos))
            > random.next_i32_bounded(SKY_LIGHT_SPAWN_ROLL_BOUND)
        {
            return false;
        }

        let dimension_type = &level.dimension_type;
        let block_light_limit = dimension_type.monster_spawn_block_light_limit;
        if block_light_limit < i32::from(MAX_LIGHT_LEVEL)
            && i32::from(level.light_value_at(LightLayer::Block, pos)) > block_light_limit
        {
            return false;
        }

        let sky_darkening = if level.is_thundering() {
            THUNDERING_SKY_DARKENING
        } else {
            level.sky_darkening()
        };
        let brightness = level.max_local_raw_brightness(pos, sky_darkening);
        i32::from(brightness)
            <= sample_monster_spawn_light_test(&dimension_type.monster_spawn_light_level, random)
    }

    /// Checks difficulty, light, and the spawn surface.
    fn check_monster_spawn_rules(
        entity_type: EntityTypeRef,
        level: &World,
        spawn_reason: EntitySpawnReason,
        pos: BlockPos,
        random: &mut impl Random,
    ) -> bool
    where
        Self: Sized,
    {
        is_spawn_allowed_on_difficulty(entity_type, level)
            && (spawn_reason.ignores_light_requirements()
                || Self::is_dark_enough_to_spawn(level, pos, random))
            && <Self as Mob>::check_mob_spawn_rules(entity_type, level, spawn_reason, pos)
    }

    /// Checks difficulty and the spawn surface without a light requirement.
    fn check_any_light_monster_spawn_rules(
        entity_type: EntityTypeRef,
        level: &World,
        spawn_reason: EntitySpawnReason,
        pos: BlockPos,
    ) -> bool
    where
        Self: Sized,
    {
        is_spawn_allowed_on_difficulty(entity_type, level)
            && <Self as Mob>::check_mob_spawn_rules(entity_type, level, spawn_reason, pos)
    }

    /// Adds sky visibility to monster spawn rules, except for spawners.
    fn check_surface_monsters_spawn_rules(
        entity_type: EntityTypeRef,
        level: &World,
        spawn_reason: EntitySpawnReason,
        pos: BlockPos,
        random: &mut impl Random,
    ) -> bool
    where
        Self: Sized,
    {
        Self::check_monster_spawn_rules(entity_type, level, spawn_reason, pos, random)
            && (spawn_reason.is_spawner() || level.can_see_sky(pos))
    }
}

/// Applies vanilla's `EntityType.canSpawn` difficulty gate here until Steel
/// has a creation/spawner gate that rejects hostile types on Peaceful.
fn is_spawn_allowed_on_difficulty(entity_type: EntityTypeRef, level: &World) -> bool {
    entity_type.allowed_in_peaceful || level.difficulty() != Difficulty::Peaceful
}

/// Samples the dimension's spawn-light provider. Vanilla uses uniform
/// providers; unknown distributions currently return their minimum.
fn sample_monster_spawn_light_test(
    level: &MonsterSpawnLightLevel,
    random: &mut impl Random,
) -> i32 {
    match level {
        MonsterSpawnLightLevel::Simple(level) => *level,
        MonsterSpawnLightLevel::Complex {
            distribution_type,
            min_inclusive,
            max_inclusive,
        } => {
            if *distribution_type == "minecraft:uniform" {
                random.next_i32_between(*min_inclusive, *max_inclusive)
            } else {
                *min_inclusive
            }
        }
    }
}

#[cfg(test)]
mod tests;
