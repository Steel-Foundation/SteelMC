use std::sync::Arc;

use glam::DVec3;
use steel_registry::{
    init_vanilla_registry, vanilla_blocks, vanilla_dimension_types, vanilla_entities,
};
use steel_utils::ChunkPos;
use steel_utils::types::{GameType, UpdateFlags};

use crate::behavior::init_behaviors;
use crate::entity::entities::{ChickenEntity, EndermiteEntity};
use crate::entity::prelude::*;
use crate::entity::{EntityBase, RemovalReason, SharedEntity, next_entity_id};
use crate::test_support::{
    TestPlayerBuilder, fresh_loaded_test_world, fresh_test_world,
    fresh_test_world_with_dimension_type, insert_ready_full_chunk,
};

#[test]
fn monster_ai_step_grows_no_action_time_by_vanilla_light_rate() {
    init_vanilla_registry();
    init_behaviors();
    let bright_world_fixture = fresh_loaded_test_world("monster_ai_step_bright");
    let bright_world = &bright_world_fixture.world;
    let bright_mob = EndermiteEntity::new(
        &vanilla_entities::ENDERMITE,
        next_entity_id(),
        DVec3::new(8.0, 65.0, 8.0),
        Arc::downgrade(bright_world),
    );
    bright_world
        .try_add_entity(Arc::clone(&bright_mob) as SharedEntity)
        .expect("test endermite should attach to the loaded chunk");

    let _ = bright_mob.living().ai_step();

    // Bright light adds two idle ticks alongside the normal AI tick.
    assert_eq!(bright_mob.living().no_action_time(), 3);

    let dark_world_fixture = fresh_test_world_with_dimension_type(
        "monster",
        "monster_ai_step_dark",
        &vanilla_dimension_types::THE_NETHER,
    );
    let dark_world = &dark_world_fixture.world;
    insert_ready_full_chunk(dark_world, ChunkPos::new(0, 0));
    let dark_mob = EndermiteEntity::new(
        &vanilla_entities::ENDERMITE,
        next_entity_id(),
        DVec3::new(8.0, 65.0, 8.0),
        Arc::downgrade(dark_world),
    );
    dark_world
        .try_add_entity(Arc::clone(&dark_mob) as SharedEntity)
        .expect("test endermite should attach to the loaded chunk");

    let _ = dark_mob.living().ai_step();

    assert_eq!(dark_mob.living().no_action_time(), 1);
}

#[test]
fn monster_check_spawn_obstruction_rejects_liquids_without_checking_blocks() {
    init_vanilla_registry();
    init_behaviors();
    let world_fixture = fresh_loaded_test_world("monster_spawn_obstruction");
    let world = &world_fixture.world;
    let mob = EndermiteEntity::new(
        &vanilla_entities::ENDERMITE,
        next_entity_id(),
        DVec3::new(8.0, 65.0, 8.0),
        Arc::downgrade(world),
    );
    let mob_pos = mob.block_position();

    assert!(
        mob.mob().check_spawn_obstruction(world),
        "an empty spawn box should be unobstructed"
    );

    assert!(world.set_block(
        mob_pos,
        vanilla_blocks::WATER.default_state(),
        UpdateFlags::UPDATE_ALL,
    ));
    assert!(
        !mob.mob().check_spawn_obstruction(world),
        "liquid inside the spawn box must reject the spawn"
    );

    assert!(world.set_block(
        mob_pos,
        vanilla_blocks::STONE.default_state(),
        UpdateFlags::UPDATE_ALL,
    ));
    assert!(
        mob.mob().check_spawn_obstruction(world),
        "block collisions are checked separately from entity spawn obstruction"
    );

    assert!(world.set_block(
        mob_pos,
        vanilla_blocks::AIR.default_state(),
        UpdateFlags::UPDATE_ALL,
    ));
    assert!(mob.mob().check_spawn_obstruction(world));
}

#[test]
fn monster_spawn_obstruction_checks_entities_and_excludes_spectators_and_shared_vehicles() {
    init_vanilla_registry();
    init_behaviors();
    let world_fixture = fresh_loaded_test_world("monster_spawn_entity_obstruction");
    let world = &world_fixture.world;
    let position = DVec3::new(8.0, 65.0, 8.0);
    let mob = EndermiteEntity::new(
        &vanilla_entities::ENDERMITE,
        next_entity_id(),
        position,
        Arc::downgrade(world),
    );
    let mob_entity: SharedEntity = Arc::<EndermiteEntity>::clone(&mob);
    world
        .try_add_entity(Arc::clone(&mob_entity))
        .expect("mob should join world");
    assert!(mob.mob().check_spawn_obstruction(world));

    let player =
        TestPlayerBuilder::new(Arc::clone(world), "SpawnBlocker", next_entity_id()).build();
    player.base().set_position_local(position);
    let player_entity: SharedEntity = Arc::clone(&player) as SharedEntity;
    world
        .try_add_entity(Arc::clone(&player_entity))
        .expect("player should join world");
    assert!(!mob.mob().check_spawn_obstruction(world));

    player.restore_game_modes(GameType::Spectator, None);
    assert!(mob.mob().check_spawn_obstruction(world));
    player.restore_game_modes(GameType::Survival, None);
    assert!(!mob.mob().check_spawn_obstruction(world));

    EntityBase::restore_passenger_relationship(&player_entity, &mob_entity);
    assert!(mob.mob().check_spawn_obstruction(world));
    mob.stop_riding();
    // Dismounting puts the mob on top of the player; move it back inside.
    mob.try_set_position(position)
        .expect("the mob should move back inside the player");
    assert!(!mob.mob().check_spawn_obstruction(world));

    player.set_removed(RemovalReason::Discarded);
    assert!(mob.mob().check_spawn_obstruction(world));
}

#[test]
fn enemy_monsters_cannot_be_leashed() {
    init_vanilla_registry();
    let world_fixture = fresh_test_world("enemy_leash");
    let world = &world_fixture.world;
    let hostile: SharedEntity = EndermiteEntity::new(
        &vanilla_entities::ENDERMITE,
        next_entity_id(),
        DVec3::ZERO,
        Arc::downgrade(world),
    );
    let pig: SharedEntity = ChickenEntity::new(
        &vanilla_entities::CHICKEN,
        next_entity_id(),
        DVec3::new(2.0, 0.0, 0.0),
        Arc::downgrade(world),
    );

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
