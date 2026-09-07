//! Vanilla-shaped hostile mob foundations (`Enemy`, `Monster`).

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

use crate::chunk::light::LightLayer;
use crate::entity::damage::DamageSource;
use crate::entity::mob::LIGHT_MAGIC_VALUE_BRIGHTNESS_GATE;
use crate::entity::{EntitySpawnReason, Mob, PathfinderMob};
use crate::physics::MoveResult;
use crate::player::Player;
use crate::world::{LevelReader, World};

/// Vanilla `Monster` constructor: `this.xpReward = 5`.
///
/// Hostile mob constructors assign the shared reward through
/// `MobBase::set_xp_reward` and only override it when their mob type differs
/// (an endermite uses 3).
pub const DEFAULT_XP_REWARD: i32 = 5;

/// Vanilla `Monster.isDarkEnoughToSpawn` passes this fixed sky darkening while
/// thundering instead of the dimension's current `getSkyDarken`.
const THUNDERING_SKY_DARKENING: u8 = 10;

/// Vanilla `Enemy` marker: mobs that count as hostile toward the player.
///
/// Vanilla classes that are hostile without necessarily extending `Monster`
/// (ghasts, phantoms, slimes, hoglins, the ender dragon, ...) implement this
/// marker directly. Steel mirrors that by making `Monster` extend it; non-`Monster`
/// enemies implement it on their own future traits.
///
/// The marker carries no behavior of its own; shared hostile-mob rules consult
/// it, e.g. vanilla `Mob.canBeLeashed` returns `false` for every `Enemy`.
pub trait Enemy {}

/// Shared hostile-mob base, mirroring vanilla `Monster extends PathfinderMob
/// implements Enemy`.
///
/// Vanilla `Monster` carries the constructor-level defaults every hostile mob
/// shares (`xpReward = 5`, hostile sound source and sound variants, light-based
/// no-action-time growth in `aiStep`, the dark-spawn rules, dropping experience,
/// and preventing players from resting). Concrete hostile mobs implement this
/// trait and only override what their mob type changes.
///
/// Hooks whose names collide with the base entity traits (`sound_source`,
/// `hurt_sound`, `get_walk_target_value`, ...) follow Steel's `Animal`
/// convention: the shared default lives here under a `_monster` suffix, and the
/// concrete mob's base-trait override delegates to it.
/// Every `Monster` is an `Enemy`, mirroring vanilla's `Monster implements Enemy`.
impl<T: Monster + ?Sized> Enemy for T {}

pub trait Monster: PathfinderMob + Enemy {
    /// Returns the vanilla `Monster` constructor experience reward.
    ///
    /// Hostile mob constructors assign it through
    /// [`Mob::set_xp_reward`](Mob::set_xp_reward) and only override it when
    /// their mob type differs (an endermite uses 3).
    fn default_xp_reward_monster(&self) -> i32 {
        DEFAULT_XP_REWARD
    }

    /// Vanilla `Monster.getSoundSource`.
    fn sound_source_monster(&self) -> SoundSource {
        SoundSource::Hostile
    }

    /// Vanilla `Monster.getHurtSound`.
    fn hurt_sound_monster(&self, _source: &DamageSource) -> Option<SoundEventRef> {
        Some(&sound_events::ENTITY_HOSTILE_HURT)
    }

    /// Vanilla `Monster.getDeathSound`.
    fn death_sound_monster(&self) -> Option<SoundEventRef> {
        Some(&sound_events::ENTITY_HOSTILE_DEATH)
    }

    /// Vanilla `Monster.getFallSounds`.
    fn fall_sounds_monster(&self) -> (SoundEventRef, SoundEventRef) {
        (
            &sound_events::ENTITY_HOSTILE_SMALL_FALL,
            &sound_events::ENTITY_HOSTILE_BIG_FALL,
        )
    }

    /// Vanilla `Monster.getSwimSound`.
    ///
    /// Vanilla also overrides `getSwimSplashSound` to `entity.hostile.splash`;
    /// Steel plays one swim sound per entity, so the splash variant has no hook
    /// to override yet.
    fn swim_sound_monster(&self) -> SoundEventRef {
        &sound_events::ENTITY_HOSTILE_SWIM
    }

    /// Vanilla `Monster.updateNoActionTime`: while standing in bright light, the
    /// monster's no-action timer grows twice as fast so it despawns sooner.
    fn update_no_action_time(&self) {
        let Some(world) = self.level() else {
            return;
        };
        // Vanilla `Monster.updateNoActionTime` calls the eye-position
        // `LivingEntity.getLightLevelDependentMagicValue()` overload.
        let eye_pos = BlockPos::new(
            fast_floor(self.position().x),
            fast_floor(self.get_eye_y()),
            fast_floor(self.position().z),
        );
        if world.light_level_dependent_magic_value(eye_pos) > LIGHT_MAGIC_VALUE_BRIGHTNESS_GATE {
            self.set_no_action_time(self.no_action_time() + 2);
        }
    }

    /// Vanilla `Monster.aiStep`: advance the swing timer, apply light-based
    /// no-action-time growth, then the standard mob `aiStep`.
    ///
    /// Vanilla advances `LivingEntity.updateSwingTime` from `Monster.aiStep`
    /// (and from `Player` and `Mannequin`); passive mobs never swing their
    /// animation, so hostile mobs are the only mobs that do.
    fn monster_ai_step(&self) -> Option<MoveResult> {
        self.update_swing_time();
        self.update_no_action_time();
        self.mob_ai_step()
    }

    /// Vanilla `Monster.getWalkTargetValue`: monsters prefer darker positions.
    fn get_walk_target_value_monster(&self, pos: BlockPos) -> f32 {
        let Some(world) = self.level() else {
            return 0.0;
        };
        -world.pathfinding_cost_from_light_levels(pos)
    }

    /// Vanilla `Monster.isPreventingPlayerRest`: monsters keep nearby players
    /// from sleeping in a bed.
    fn is_preventing_player_rest(&self, _level: &World, _player: &Player) -> bool {
        true
    }

    /// Vanilla `Monster.shouldDropExperience`.
    fn should_drop_experience_monster(&self) -> bool {
        true
    }

    /// Vanilla `Monster.shouldDropLoot`: monsters drop loot whenever the
    /// `mobDrops` rule allows it, even as babies.
    fn should_drop_loot_monster(&self, world: &World) -> bool {
        world.get_game_rule(&MOB_DROPS)
    }

    /// Vanilla `Monster.isDarkEnoughToSpawn`.
    ///
    /// `random` matches vanilla's `RandomSource` parameter so the check stays
    /// deterministic for callers that seed their randomness.
    fn is_dark_enough_to_spawn(level: &World, pos: BlockPos, random: &mut impl Random) -> bool
    where
        Self: Sized,
    {
        if i32::from(level.light_value_at(LightLayer::Sky, pos)) > random.next_i32_bounded(32) {
            return false;
        }

        let dimension_type = &level.dimension_type;
        let block_light_limit = dimension_type.monster_spawn_block_light_limit;
        if block_light_limit < 15
            && i32::from(level.light_value_at(LightLayer::Block, pos)) > block_light_limit
        {
            return false;
        }

        // Vanilla 26.2 `Monster.isDarkEnoughToSpawn`: the thundering branch
        // passes a fixed darkening of 10, the clear branch the dimension's
        // current `getSkyDarken` (via the no-arg `getMaxLocalRawBrightness`).
        let sky_darkening = if level.is_thundering() {
            THUNDERING_SKY_DARKENING
        } else {
            level.sky_darkening()
        };
        let brightness = level.max_local_raw_brightness(pos, sky_darkening);
        i32::from(brightness)
            <= sample_monster_spawn_light_test(&dimension_type.monster_spawn_light_level, random)
    }

    /// Vanilla `Monster.checkMonsterSpawnRules`.
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

    /// Vanilla `Monster.checkAnyLightMonsterSpawnRules`.
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

    /// Vanilla `Monster.checkSurfaceMonstersSpawnRules`.
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

/// Steel-level peaceful clause mirroring vanilla `EntityType.canSpawn`.
///
/// Vanilla never asks `Monster.checkMonsterSpawnRules` for hostile types on
/// Peaceful: `EntityType.canSpawn`/`create` refuses those spawns and the
/// natural spawner skips hostile categories. Steel has no `canSpawn` gate or
/// natural spawner yet, so the spawn-rule helpers enforce the clause
/// themselves; drop it once a spawn-rule invocation mirrors vanilla's
/// `EntityType.create` (documented divergence).
fn is_spawn_allowed_on_difficulty(entity_type: EntityTypeRef, level: &World) -> bool {
    entity_type.allowed_in_peaceful || level.difficulty() != Difficulty::Peaceful
}

/// Samples the dimension's vanilla `monster_spawn_light_test` int provider.
///
/// Vanilla only ships `uniform` providers in dimension type data; unknown
/// distribution types fall back to the minimum for robustness.
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
