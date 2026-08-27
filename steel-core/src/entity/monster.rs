use crate::{
    chunk::light::LightLayer,
    entity::{EntitySpawnReason, PathfinderMob, damage::DamageSource},
    player::Player,
    world::{LevelReader, World},
};
use steel_protocol::packets::game::SoundSource;
use steel_registry::{sound_event::SoundEventRef, sound_events, vanilla_game_rules};
use steel_utils::{BlockPos, random::legacy_random::LegacyRandom};

pub trait Monster: PathfinderMob {
    fn monster_ai_step(&self) {
        self.update_swing_time();
        self.update_no_action_time();
    }

    fn update_no_action_time(&self) {
        if self.light_level_dependent_magic_value() > 0.5 {
            self.set_no_action_time(self.no_action_time() + 2);
        }
    }

    fn is_preventing_player_rest(&self, _level: &World, _player: &Player) -> bool {
        true
    }

    // SPAWNING
    fn is_dark_enough_to_spawn(&self, level: &World, pos: BlockPos) -> bool {
        let current_light = level.light_value_at(LightLayer::Block, pos) as i32;
        let block_light_limit = level.dimension_type.monster_spawn_block_light_limit;
        if current_light > rand::random_range(0..=32)
            || (block_light_limit < 15 && current_light > block_light_limit)
        {
            return false;
        }

        let brightness = if level.is_thundering() {
            level.max_local_raw_brightness(pos, 10)
        } else {
            level.max_local_raw_brightness(pos, level.sky_darkening())
        };
        let mut random = LegacyRandom::from_seed(rand::random());
        brightness as i32
            <= level
                .dimension_type
                .monster_spawn_light_level
                .sample(&mut random)
    }

    fn check_monster_spawn_rules(
        &self,
        level: &World,
        spawn_reason: EntitySpawnReason,
        pos: BlockPos,
    ) -> bool {
        (spawn_reason.ignores_light_requirements() || self.is_dark_enough_to_spawn(level, pos))
            && self.check_mob_spawn_rules(level, spawn_reason, pos)
    }
    fn check_any_light_monster_spawn_rules(
        &self,
        level: &World,
        spawn_reason: EntitySpawnReason,
        pos: BlockPos,
    ) -> bool {
        self.check_mob_spawn_rules(level, spawn_reason, pos)
    }
    fn check_surface_monsters_spawn_rules(
        &self,
        level: &World,
        spawn_reason: EntitySpawnReason,
        pos: BlockPos,
    ) -> bool {
        self.check_monster_spawn_rules(level, spawn_reason, pos)
            && (spawn_reason.is_spawner() || level.can_see_sky(pos))
    }

    // SOUNDS
    fn monster_sound_source(&self) -> SoundSource {
        SoundSource::Hostile
    }

    fn monster_swim_sound(&self) -> SoundEventRef {
        &sound_events::ENTITY_HOSTILE_SWIM
    }

    fn monster_swim_splash_sound(&self) -> SoundEventRef {
        return &sound_events::ENTITY_HOSTILE_SPLASH;
    }

    fn monster_hurt_sound(&self, _source: &DamageSource) -> SoundEventRef {
        return &sound_events::ENTITY_HOSTILE_HURT;
    }

    fn monster_death_sound(&self) -> SoundEventRef {
        return &sound_events::ENTITY_HOSTILE_DEATH;
    }

    // TODO: Add fallsounds

    fn monster_walk_target_value(&self, level: &World, pos: BlockPos) -> f32 {
        -level.pathfinding_cost_from_light_levels(pos)
    }

    // LOOT
    fn should_drop_experience(&self) -> bool {
        true
    }

    fn should_drop_loot(&self, level: &World) -> bool {
        level.get_game_rule(&vanilla_game_rules::MOB_DROPS)
    }

    // TODO: Add get_projectile once done
}
