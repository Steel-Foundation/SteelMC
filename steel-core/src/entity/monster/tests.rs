use std::sync::{Arc, Weak};

use glam::DVec3;
use steel_protocol::packets::game::SoundSource;
use steel_registry::entity_type::EntityTypeRef;
use steel_registry::sound_events;
use steel_registry::{
    init_vanilla_registry, vanilla_blocks, vanilla_damage_types, vanilla_dimension_types,
    vanilla_entities, vanilla_game_rules,
};
use steel_utils::locks::SyncMutex;
use steel_utils::random::legacy_random::LegacyRandom;
use steel_utils::random::{Random, RandomSource};
use steel_utils::types::{Difficulty, GameType, UpdateFlags};
use steel_utils::{BlockPos, BlockStateId, ChunkPos};
use steel_worldgen::density_functions::overworld::OverworldNoiseSettings;

use super::{DEFAULT_XP_REWARD, SKY_LIGHT_SPAWN_ROLL_BOUND};
use crate::behavior::init_behaviors;
use crate::chunk::light::MAX_LIGHT_LEVEL;
use crate::entity::damage::DamageSource;
use crate::entity::entities::PigEntity;
use crate::entity::mob::{Mob, MobBase};
use crate::entity::{
    Entity, EntityBase, EntitySpawnReason, LivingEntity, LivingEntityBase, Monster, PathfinderMob,
    RemovalReason, SharedEntity, entities::mobs::hostile::EndermiteEntity, next_entity_id,
};
use crate::test_support::{
    TestPlayerBuilder, fresh_test_world, fresh_test_world_with_dimension_type,
    insert_ready_full_chunk, test_world,
};
use crate::world::{LevelReader, World};

const INITIAL_TEST_HEALTH: f32 = 20.0;
const LOW_SKY_LIGHT_ROLL_SEED: u64 = 5_120;
const HIGH_SKY_LIGHT_ROLL_SEED: u64 = 14_880;

struct TestMonster {
    base: EntityBase,
    entity_type: EntityTypeRef,
    living_base: LivingEntityBase,
    mob_base: MobBase,
    mob_flags: SyncMutex<i8>,
    health: SyncMutex<f32>,
}

impl TestMonster {
    fn new() -> Self {
        Self::with_world(None)
    }

    fn with_world(world: Option<&Arc<World>>) -> Self {
        init_vanilla_registry();
        Self {
            base: EntityBase::new(
                1,
                DVec3::new(8.0, 65.0, 8.0),
                vanilla_entities::PIG.dimensions,
                world.map_or_else(Weak::new, Arc::downgrade),
            ),
            entity_type: &vanilla_entities::PIG,
            living_base: LivingEntityBase::new(&vanilla_entities::PIG),
            mob_base: MobBase::new(),
            mob_flags: SyncMutex::new(0),
            health: SyncMutex::new(INITIAL_TEST_HEALTH),
        }
    }
}

crate::entity::impl_test_downcast_type!(TestMonster);

impl Entity for TestMonster {
    fn base(&self) -> &EntityBase {
        &self.base
    }

    fn entity_type(&self) -> EntityTypeRef {
        self.entity_type
    }
}

impl LivingEntity for TestMonster {
    fn living_base(&self) -> &LivingEntityBase {
        &self.living_base
    }

    fn get_health(&self) -> f32 {
        *self.health.lock()
    }

    fn set_health(&self, health: f32) {
        *self.health.lock() = health;
    }
}

impl Mob for TestMonster {
    fn mob_base(&self) -> &MobBase {
        &self.mob_base
    }

    fn mob_flags(&self) -> i8 {
        *self.mob_flags.lock()
    }

    fn set_mob_flags(&self, flags: i8) {
        *self.mob_flags.lock() = flags;
    }
}

impl PathfinderMob for TestMonster {}

impl Monster for TestMonster {}

struct SpawnSurfaceLevel {
    default_state: BlockStateId,
    states: Vec<(BlockPos, BlockStateId)>,
}

impl SpawnSurfaceLevel {
    fn new(default_state: BlockStateId) -> Self {
        Self {
            default_state,
            states: Vec::new(),
        }
    }

    fn with(mut self, pos: BlockPos, state: BlockStateId) -> Self {
        self.states.push((pos, state));
        self
    }
}

impl LevelReader for SpawnSurfaceLevel {
    fn get_block_state(&self, pos: BlockPos) -> BlockStateId {
        self.states
            .iter()
            .find_map(|(state_pos, state)| (*state_pos == pos).then_some(*state))
            .unwrap_or(self.default_state)
    }

    fn raw_brightness(&self, _pos: BlockPos, _sky_darkening: u8) -> u8 {
        0
    }

    fn min_y(&self) -> i32 {
        OverworldNoiseSettings::MIN_Y
    }

    fn height(&self) -> i32 {
        OverworldNoiseSettings::HEIGHT
    }

    fn sea_level(&self) -> i32 {
        OverworldNoiseSettings::SEA_LEVEL
    }
}

#[test]
fn monster_constructor_defaults_match_vanilla() {
    init_vanilla_registry();
    let world = test_world();
    let player = TestPlayerBuilder::new(Arc::clone(world), "RestTest", 1).build();
    let monster = TestMonster::new();
    let source = DamageSource::environment(&vanilla_damage_types::GENERIC);

    assert_eq!(DEFAULT_XP_REWARD, 5);
    assert_eq!(monster.default_xp_reward_monster(), 5);
    assert_eq!(monster.sound_source_monster(), SoundSource::Hostile);
    assert_eq!(
        monster.hurt_sound_monster(&source),
        Some(&sound_events::ENTITY_HOSTILE_HURT)
    );
    assert_eq!(
        monster.death_sound_monster(),
        Some(&sound_events::ENTITY_HOSTILE_DEATH)
    );
    assert_eq!(
        monster.fall_sounds_monster(),
        (
            &sound_events::ENTITY_HOSTILE_SMALL_FALL,
            &sound_events::ENTITY_HOSTILE_BIG_FALL,
        )
    );
    assert_eq!(
        monster.swim_sound_monster(),
        &sound_events::ENTITY_HOSTILE_SWIM
    );
    assert!(
        monster.is_preventing_player_rest(world, &player),
        "a plain monster should keep players from resting"
    );
    assert!(monster.should_drop_experience_monster());
    assert_eq!(
        monster.should_drop_loot_monster(world),
        world.get_game_rule(&vanilla_game_rules::MOB_DROPS)
    );
}

#[test]
fn monster_is_dark_enough_to_spawn_rejects_bright_overworld() {
    let world_fixture = fresh_test_world("monster_spawn_bright_overworld");
    let world = &world_fixture.world;
    let pos = BlockPos::new(8, 65, 8);

    // Full sky light cannot pass the overworld's 0..7 brightness test.
    let mut low_roll = RandomSource::Legacy(LegacyRandom::from_seed(LOW_SKY_LIGHT_ROLL_SEED));
    assert!(
        low_roll.next_i32_bounded(SKY_LIGHT_SPAWN_ROLL_BOUND) < i32::from(MAX_LIGHT_LEVEL),
        "test seed must roll under the sky-light value"
    );
    assert!(!TestMonster::is_dark_enough_to_spawn(
        world,
        pos,
        &mut low_roll
    ));

    let mut high_roll = RandomSource::Legacy(LegacyRandom::from_seed(HIGH_SKY_LIGHT_ROLL_SEED));
    assert!(
        high_roll.next_i32_bounded(SKY_LIGHT_SPAWN_ROLL_BOUND) >= i32::from(MAX_LIGHT_LEVEL),
        "test seed must roll at or above the sky-light value"
    );
    assert!(!TestMonster::is_dark_enough_to_spawn(
        world,
        pos,
        &mut high_roll
    ));
}

#[test]
fn monster_is_dark_enough_to_spawn_accepts_dark_nether() {
    let world_fixture = fresh_test_world_with_dimension_type(
        "monster",
        "nether_spawn_dark",
        &vanilla_dimension_types::THE_NETHER,
    );
    let world = &world_fixture.world;
    let pos = BlockPos::new(8, 65, 8);

    // Fresh nether chunks are dark enough for every light-test roll.
    let mut random = RandomSource::Legacy(LegacyRandom::from_seed(LOW_SKY_LIGHT_ROLL_SEED));
    assert!(TestMonster::is_dark_enough_to_spawn(
        world,
        pos,
        &mut random
    ));
}

#[test]
fn monster_spawn_rules_accept_only_sturdy_non_glowing_surfaces() {
    init_vanilla_registry();
    init_behaviors();
    let air = vanilla_blocks::AIR.default_state();
    let stone = vanilla_blocks::STONE.default_state();
    let glowstone = vanilla_blocks::GLOWSTONE.default_state();
    let pos = BlockPos::new(8, 65, 8);
    let surface_pos = pos.below();

    let surface = SpawnSurfaceLevel::new(air);
    assert!(!TestMonster::check_mob_spawn_rules(
        &vanilla_entities::ZOMBIE,
        &surface,
        EntitySpawnReason::Natural,
        pos
    ));

    let surface = SpawnSurfaceLevel::new(air).with(surface_pos, stone);
    assert!(TestMonster::check_mob_spawn_rules(
        &vanilla_entities::ZOMBIE,
        &surface,
        EntitySpawnReason::Natural,
        pos
    ));

    let surface = SpawnSurfaceLevel::new(air).with(surface_pos, glowstone);
    assert!(!TestMonster::check_mob_spawn_rules(
        &vanilla_entities::ZOMBIE,
        &surface,
        EntitySpawnReason::Natural,
        pos
    ));

    let surface = SpawnSurfaceLevel::new(air);
    assert!(TestMonster::check_mob_spawn_rules(
        &vanilla_entities::ZOMBIE,
        &surface,
        EntitySpawnReason::TrialSpawner,
        pos,
    ));
}

#[test]
fn monster_spawn_rules_consult_block_specific_spawn_surfaces() {
    init_vanilla_registry();
    init_behaviors();
    let air = vanilla_blocks::AIR.default_state();
    let soul_sand = vanilla_blocks::SOUL_SAND.default_state();
    let magma = vanilla_blocks::MAGMA_BLOCK.default_state();
    let pos = BlockPos::new(8, 65, 8);
    let surface_pos = pos.below();

    let surface = SpawnSurfaceLevel::new(air).with(surface_pos, soul_sand);
    assert!(TestMonster::check_mob_spawn_rules(
        &vanilla_entities::ZOMBIE,
        &surface,
        EntitySpawnReason::Natural,
        pos
    ));

    let surface = SpawnSurfaceLevel::new(air).with(surface_pos, magma);
    assert!(!TestMonster::check_mob_spawn_rules(
        &vanilla_entities::ZOMBIE,
        &surface,
        EntitySpawnReason::Natural,
        pos
    ));
    assert!(TestMonster::check_mob_spawn_rules(
        &vanilla_entities::MAGMA_CUBE,
        &surface,
        EntitySpawnReason::Natural,
        pos
    ));
}

#[test]
fn monster_spawn_rules_combine_light_and_surface_checks() {
    init_vanilla_registry();
    init_behaviors();
    let overworld_fixture = fresh_test_world("monster_spawn_rules_overworld");
    let overworld = &overworld_fixture.world;
    let nether_fixture = fresh_test_world_with_dimension_type(
        "monster",
        "monster_spawn_rules_nether",
        &vanilla_dimension_types::THE_NETHER,
    );
    let nether = &nether_fixture.world;
    for (name, world) in [("overworld", overworld), ("nether", nether)] {
        insert_ready_full_chunk(world, ChunkPos::new(0, 0));
        let pos = BlockPos::new(8, 65, 8);
        assert!(
            world.set_block(
                pos.below(),
                vanilla_blocks::STONE.default_state(),
                UpdateFlags::UPDATE_ALL,
            ),
            "{name}: stone spawn surface should place"
        );
        let mut random = RandomSource::Legacy(LegacyRandom::from_seed(0));

        assert_eq!(
            TestMonster::check_monster_spawn_rules(
                &vanilla_entities::ZOMBIE,
                world,
                EntitySpawnReason::Natural,
                pos,
                &mut random,
            ),
            name == "nether",
            "{name}: natural spawns must also pass the light check"
        );

        assert!(TestMonster::check_monster_spawn_rules(
            &vanilla_entities::ZOMBIE,
            world,
            EntitySpawnReason::TrialSpawner,
            BlockPos::new(12, 65, 8),
            &mut random,
        ));
        assert!(TestMonster::check_monster_spawn_rules(
            &vanilla_entities::ZOMBIE,
            world,
            EntitySpawnReason::TrialSpawner,
            pos,
            &mut random,
        ));
    }
}

#[test]
fn monster_any_light_spawn_rules_skip_light_but_keep_surface_checks() {
    init_vanilla_registry();
    init_behaviors();
    let world_fixture = fresh_test_world_with_dimension_type(
        "monster",
        "any_light_spawn_rules",
        &vanilla_dimension_types::THE_NETHER,
    );
    let world = &world_fixture.world;
    insert_ready_full_chunk(world, ChunkPos::new(0, 0));
    let pos = BlockPos::new(8, 65, 8);
    assert!(world.set_block(
        pos.below(),
        vanilla_blocks::STONE.default_state(),
        UpdateFlags::UPDATE_ALL,
    ));

    assert!(!TestMonster::check_any_light_monster_spawn_rules(
        &vanilla_entities::ZOMBIE,
        world,
        EntitySpawnReason::Natural,
        BlockPos::new(12, 65, 8),
    ));
    assert!(TestMonster::check_any_light_monster_spawn_rules(
        &vanilla_entities::ZOMBIE,
        world,
        EntitySpawnReason::Natural,
        pos,
    ));
    assert!(TestMonster::check_any_light_monster_spawn_rules(
        &vanilla_entities::ZOMBIE,
        world,
        EntitySpawnReason::Spawner,
        BlockPos::new(12, 65, 8),
    ));
}

#[test]
fn monster_surface_spawn_rules_require_sky_visibility_or_spawner() {
    init_vanilla_registry();
    init_behaviors();
    let overworld_fixture = fresh_test_world("monster_surface_spawn_rules");
    let overworld = &overworld_fixture.world;
    let nether_fixture = fresh_test_world_with_dimension_type(
        "monster",
        "monster_surface_spawn_rules_nether",
        &vanilla_dimension_types::THE_NETHER,
    );
    let nether = &nether_fixture.world;
    let mut random = RandomSource::Legacy(LegacyRandom::from_seed(0));

    // The nether never sees the sky, so even a valid dark surface fails.
    insert_ready_full_chunk(nether, ChunkPos::new(0, 0));
    let nether_pos = BlockPos::new(8, 65, 8);
    assert!(nether.set_block(
        nether_pos.below(),
        vanilla_blocks::STONE.default_state(),
        UpdateFlags::UPDATE_ALL,
    ));
    assert!(!TestMonster::check_surface_monsters_spawn_rules(
        &vanilla_entities::ZOMBIE,
        nether,
        EntitySpawnReason::Natural,
        nether_pos,
        &mut random,
    ));
    assert!(TestMonster::check_surface_monsters_spawn_rules(
        &vanilla_entities::ZOMBIE,
        nether,
        EntitySpawnReason::Spawner,
        nether_pos,
        &mut random,
    ));

    insert_ready_full_chunk(overworld, ChunkPos::new(0, 0));
    let overworld_pos = BlockPos::new(8, 65, 8);
    assert!(overworld.set_block(
        overworld_pos.below(),
        vanilla_blocks::STONE.default_state(),
        UpdateFlags::UPDATE_ALL,
    ));
    assert!(!TestMonster::check_surface_monsters_spawn_rules(
        &vanilla_entities::ZOMBIE,
        overworld,
        EntitySpawnReason::Natural,
        overworld_pos,
        &mut random,
    ));
}

#[test]
fn monster_spawn_rules_refuse_hostile_types_on_peaceful_difficulty() {
    init_vanilla_registry();
    init_behaviors();
    let world_fixture = fresh_test_world("monster_peaceful_spawn_rules");
    let world = &world_fixture.world;
    insert_ready_full_chunk(world, ChunkPos::new(0, 0));
    let pos = BlockPos::new(8, 65, 8);
    assert!(world.set_block(
        pos.below(),
        vanilla_blocks::STONE.default_state(),
        UpdateFlags::UPDATE_ALL,
    ));
    let mut random = RandomSource::Legacy(LegacyRandom::from_seed(0));

    // Spawners bypass light and surface checks, isolating the difficulty gate.
    assert!(TestMonster::check_monster_spawn_rules(
        &vanilla_entities::ZOMBIE,
        world,
        EntitySpawnReason::TrialSpawner,
        pos,
        &mut random,
    ));

    world.set_difficulty(Difficulty::Peaceful);
    assert!(
        !TestMonster::check_monster_spawn_rules(
            &vanilla_entities::ZOMBIE,
            world,
            EntitySpawnReason::TrialSpawner,
            pos,
            &mut random,
        ),
        "spawn rules must refuse types that cannot exist on Peaceful"
    );
    assert!(
        !TestMonster::check_any_light_monster_spawn_rules(
            &vanilla_entities::ZOMBIE,
            world,
            EntitySpawnReason::TrialSpawner,
            pos,
        ),
        "any-light spawn rules must also refuse hostile types on Peaceful"
    );
    assert!(
        !TestMonster::check_surface_monsters_spawn_rules(
            &vanilla_entities::ZOMBIE,
            world,
            EntitySpawnReason::Spawner,
            pos,
            &mut random,
        ),
        "surface spawn rules delegate to the same difficulty clause"
    );
    assert!(
        TestMonster::check_any_light_monster_spawn_rules(
            &vanilla_entities::COW,
            world,
            EntitySpawnReason::TrialSpawner,
            pos,
        ),
        "the clause mirrors vanilla `allowed_in_peaceful`, not a blanket refusal"
    );

    world.set_difficulty(Difficulty::Normal);
    assert!(
        TestMonster::check_monster_spawn_rules(
            &vanilla_entities::ZOMBIE,
            world,
            EntitySpawnReason::TrialSpawner,
            pos,
            &mut random,
        ),
        "the same spawn passes once difficulty leaves Peaceful"
    );
}

#[test]
fn monster_walk_target_value_prefers_darkness() {
    let overworld_fixture = fresh_test_world("monster_walk_target_overworld");
    let overworld = &overworld_fixture.world;
    let nether_fixture = fresh_test_world_with_dimension_type(
        "monster",
        "monster_walk_target_nether",
        &vanilla_dimension_types::THE_NETHER,
    );
    let nether = &nether_fixture.world;
    let pos = BlockPos::new(8, 65, 8);

    let bright_monster = TestMonster::with_world(Some(overworld));
    assert!(
        bright_monster.get_walk_target_value_monster(pos) < 0.0,
        "bright positions should be penalized"
    );
    assert!(
        bright_monster.get_walk_target_value(pos) < 0.0,
        "the PathfinderMob default must dispatch monsters to the dark preference"
    );
    let dark_monster = TestMonster::with_world(Some(nether));
    assert!(
        dark_monster.get_walk_target_value_monster(pos) > 0.0,
        "dark positions should be preferred"
    );
}

#[test]
fn monster_ai_step_grows_no_action_time_by_vanilla_light_rate() {
    init_vanilla_registry();
    init_behaviors();
    let bright_world_fixture = fresh_test_world("monster_ai_step_bright");
    let bright_world = &bright_world_fixture.world;
    insert_ready_full_chunk(bright_world, ChunkPos::new(0, 0));
    let bright_mob = Arc::new(EndermiteEntity::new(
        &vanilla_entities::ENDERMITE,
        next_entity_id(),
        DVec3::new(8.0, 65.0, 8.0),
        Arc::downgrade(bright_world),
    ));
    bright_world
        .try_add_entity(Arc::clone(&bright_mob) as SharedEntity)
        .expect("test endermite should attach to the loaded chunk");

    let bright_mob_entity: SharedEntity = Arc::<EndermiteEntity>::clone(&bright_mob);
    bright_mob.ai_step(&bright_mob_entity);

    // Bright light adds two idle ticks alongside the normal AI tick.
    assert_eq!(bright_mob.no_action_time(), 3);

    let dark_world_fixture = fresh_test_world_with_dimension_type(
        "monster",
        "monster_ai_step_dark",
        &vanilla_dimension_types::THE_NETHER,
    );
    let dark_world = &dark_world_fixture.world;
    insert_ready_full_chunk(dark_world, ChunkPos::new(0, 0));
    let dark_mob = Arc::new(EndermiteEntity::new(
        &vanilla_entities::ENDERMITE,
        next_entity_id(),
        DVec3::new(8.0, 65.0, 8.0),
        Arc::downgrade(dark_world),
    ));
    dark_world
        .try_add_entity(Arc::clone(&dark_mob) as SharedEntity)
        .expect("test endermite should attach to the loaded chunk");

    let dark_mob_entity: SharedEntity = Arc::<EndermiteEntity>::clone(&dark_mob);
    dark_mob.ai_step(&dark_mob_entity);

    assert_eq!(dark_mob.no_action_time(), 1);
}

#[test]
fn monster_check_spawn_obstruction_rejects_liquids_without_checking_blocks() {
    init_vanilla_registry();
    init_behaviors();
    let world_fixture = fresh_test_world("monster_spawn_obstruction");
    let world = &world_fixture.world;
    insert_ready_full_chunk(world, ChunkPos::new(0, 0));
    let mob = EndermiteEntity::new(
        &vanilla_entities::ENDERMITE,
        next_entity_id(),
        DVec3::new(8.0, 65.0, 8.0),
        Arc::downgrade(world),
    );
    let mob_pos = mob.block_position();

    assert!(
        mob.check_spawn_obstruction(world),
        "an empty spawn box should be unobstructed"
    );

    assert!(world.set_block(
        mob_pos,
        vanilla_blocks::WATER.default_state(),
        UpdateFlags::UPDATE_ALL,
    ));
    assert!(
        !mob.check_spawn_obstruction(world),
        "liquid inside the spawn box must reject the spawn"
    );

    assert!(world.set_block(
        mob_pos,
        vanilla_blocks::STONE.default_state(),
        UpdateFlags::UPDATE_ALL,
    ));
    assert!(
        mob.check_spawn_obstruction(world),
        "block collisions are checked separately from entity spawn obstruction"
    );

    assert!(world.set_block(
        mob_pos,
        vanilla_blocks::AIR.default_state(),
        UpdateFlags::UPDATE_ALL,
    ));
    assert!(mob.check_spawn_obstruction(world));
}

#[test]
fn monster_spawn_obstruction_checks_entities_and_excludes_spectators_and_shared_vehicles() {
    init_vanilla_registry();
    init_behaviors();
    let world_fixture = fresh_test_world("monster_spawn_entity_obstruction");
    let world = &world_fixture.world;
    insert_ready_full_chunk(world, ChunkPos::new(0, 0));
    let position = DVec3::new(8.0, 65.0, 8.0);
    let mob = Arc::new(EndermiteEntity::new(
        &vanilla_entities::ENDERMITE,
        next_entity_id(),
        position,
        Arc::downgrade(world),
    ));
    let mob_entity: SharedEntity = Arc::<EndermiteEntity>::clone(&mob);
    world
        .try_add_entity(Arc::clone(&mob_entity))
        .expect("mob should join world");
    assert!(mob.check_spawn_obstruction(world));

    let player =
        TestPlayerBuilder::new(Arc::clone(world), "SpawnBlocker", next_entity_id()).build();
    player.base().set_position_local(position);
    let player_entity: SharedEntity = Arc::clone(&player) as SharedEntity;
    world
        .try_add_entity(Arc::clone(&player_entity))
        .expect("player should join world");
    assert!(!mob.check_spawn_obstruction(world));

    player.restore_game_modes(GameType::Spectator, None);
    assert!(mob.check_spawn_obstruction(world));
    player.restore_game_modes(GameType::Survival, None);
    assert!(!mob.check_spawn_obstruction(world));

    EntityBase::restore_passenger_relationship(&player_entity, &mob_entity);
    assert!(mob.check_spawn_obstruction(world));
    mob.stop_riding();
    assert!(!mob.check_spawn_obstruction(world));

    player.set_removed(RemovalReason::Discarded);
    assert!(mob.check_spawn_obstruction(world));
}

#[test]
fn enemy_monsters_cannot_be_leashed() {
    init_vanilla_registry();
    let world_fixture = fresh_test_world("enemy_leash");
    let world = &world_fixture.world;
    let hostile: SharedEntity = Arc::new(EndermiteEntity::new(
        &vanilla_entities::ENDERMITE,
        next_entity_id(),
        DVec3::ZERO,
        Arc::downgrade(world),
    ));
    let pig: SharedEntity = Arc::new(PigEntity::new(
        &vanilla_entities::PIG,
        next_entity_id(),
        DVec3::new(2.0, 0.0, 0.0),
        Arc::downgrade(world),
    ));

    assert!(hostile.is_enemy(), "monsters are vanilla `Enemy`s");
    assert!(!pig.is_enemy(), "a passive animal is not a vanilla `Enemy`");
    let hostile_leashable = hostile
        .as_leashable()
        .expect("a hostile mob implements Leashable");
    assert!(
        !hostile_leashable.can_be_leashed(),
        "enemy mobs must not be leashable"
    );
    let pig_leashable = pig.as_leashable().expect("a pig implements Leashable");
    assert!(
        pig_leashable.can_be_leashed(),
        "non-enemy mobs keep their existing leashability"
    );
    assert!(
        !hostile_leashable.can_have_a_leash_attached_to(pig.as_ref()),
        "a lead must not attach to a hostile mob"
    );
    assert!(
        pig_leashable.can_have_a_leash_attached_to(hostile.as_ref()),
        "a lead should still attach to a non-enemy mob in reach"
    );
    assert!(
        !pig_leashable.can_have_a_leash_attached_to(pig.as_ref()),
        "an entity can never leash to itself"
    );
}

#[test]
fn monster_downcasts_from_shared_entity() {
    init_vanilla_registry();
    let world_fixture = fresh_test_world("monster_downcast");
    let world = &world_fixture.world;
    let endermite: SharedEntity = Arc::new(EndermiteEntity::new(
        &vanilla_entities::ENDERMITE,
        next_entity_id(),
        DVec3::ZERO,
        Arc::downgrade(world),
    ));

    assert!(endermite.is_monster());
    let monster = endermite
        .as_monster()
        .expect("endermite should be a monster");
    assert_eq!(monster.sound_source_monster(), SoundSource::Hostile);
    let mob = endermite.as_mob().expect("endermite should be a mob");
    assert_eq!(
        mob.xp_reward(),
        3,
        "the endermite constructor assigns its own reward"
    );
}
