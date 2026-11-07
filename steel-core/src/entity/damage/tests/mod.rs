use crate::entity::LivingParts as _;
use crate::entity::prelude::*;
use std::sync::{Arc, Weak};

use glam::DVec3;
use steel_registry::{
    init_vanilla_registry, vanilla_damage_types, vanilla_dimension_types, vanilla_entities,
};
use steel_utils::{Identifier, types::Difficulty};

use super::{DamageHistory, DamageSource};
use crate::entity::entities::ChickenEntity;
use crate::entity::{Entity, RemovalReason};

use crate::level_data::GameTimeSource;
use crate::player::Player;
use crate::test_support::{
    TestEntity, TestPlayerBuilder, advance_test_game_time_to,
    create_test_world_with_damage_history, insert_ready_full_chunk,
};
use crate::world::World;

mod retention;

fn history_world(history: &Arc<DamageHistory>, name: &'static str) -> Arc<World> {
    create_test_world_with_damage_history(
        Identifier::vanilla_static(name),
        Difficulty::Normal,
        &vanilla_dimension_types::OVERWORLD,
        GameTimeSource::Primary,
        history,
    )
}

#[test]
fn directness_uses_allocation_identity_and_position_tracks_the_direct_entity() {
    init_vanilla_registry();
    let first = TestEntity::shared(1, DVec3::ZERO, Weak::new(), &vanilla_entities::ITEM);
    let other = TestEntity::shared(1, DVec3::ZERO, Weak::new(), &vanilla_entities::ITEM);
    let environment = DamageSource::environment(&vanilla_damage_types::GENERIC);
    assert!(environment.is_direct());
    let source = DamageSource::direct(&vanilla_damage_types::GENERIC, Arc::clone(&first));
    assert!(source.is_direct());
    assert!(!source.clone().with_causing_entity(other).is_direct());
    first
        .try_set_position(DVec3::new(1.0, 2.0, 3.0))
        .expect("move retained source");
    assert_eq!(source.source_position(), Some(first.position()));
    assert!(source.source_position_raw().is_none());
    let positioned = source.with_source_position(DVec3::new(4.0, 5.0, 6.0));
    first.try_set_position(DVec3::ZERO).expect("move again");
    assert_eq!(
        positioned.source_position(),
        Some(DVec3::new(4.0, 5.0, 6.0))
    );
}

#[test]
fn returned_self_damage_source_outlives_history_expiry_without_a_cycle() {
    let history = Arc::new(DamageHistory::default());
    let world = history_world(&history, "self_damage_history");
    let player = TestPlayerBuilder::new(Arc::clone(&world), "SelfDamage", 1).build();
    let weak = Arc::downgrade(&player);
    let entity = Arc::clone(&player);
    let source = DamageSource::direct(&vanilla_damage_types::INDIRECT_MAGIC, entity);
    player.living().record_last_damage_source(&source);
    let generation = player.generation();
    drop(source);
    advance_test_game_time_to(&world, 40);
    history.expire();
    let retained = history
        .last_damage_source(generation)
        .expect("age forty is retained");
    advance_test_game_time_to(&world, 41);
    history.expire();
    assert!(history.last_damage_source(generation).is_none());
    drop(player);
    assert!(
        weak.upgrade().is_some(),
        "returned source independently owns the player"
    );
    drop(retained);
    assert!(weak.upgrade().is_none());
}

#[test]
fn unmanaged_mutual_damage_does_not_retain_players_or_worlds() {
    let history = Arc::new(DamageHistory::default());
    let world = history_world(&history, "mutual_damage_history");
    let first = TestPlayerBuilder::new(Arc::clone(&world), "First", 1).build();
    let second = TestPlayerBuilder::new(Arc::clone(&world), "Second", 2).build();
    first.living().record_last_damage_source(
        &DamageSource::environment(&vanilla_damage_types::PLAYER_ATTACK)
            .with_causing_entity(Arc::<Player>::clone(&second)),
    );
    second.living().record_last_damage_source(
        &DamageSource::environment(&vanilla_damage_types::PLAYER_ATTACK)
            .with_causing_entity(Arc::<Player>::clone(&first)),
    );
    let first_weak = Arc::downgrade(&first);
    let second_weak = Arc::downgrade(&second);
    let world_weak = Arc::downgrade(&world);
    drop((first, second, world));
    history.expire();
    assert!(first_weak.upgrade().is_none());
    assert!(second_weak.upgrade().is_none());
    assert!(world_weak.upgrade().is_none());
}

#[test]
fn replacing_damage_history_releases_the_previous_source() {
    let history = Arc::new(DamageHistory::default());
    let world = history_world(&history, "damage_history_replacement");
    let victim = TestPlayerBuilder::new(Arc::clone(&world), "Victim", 1).build();
    assert!(world.players.insert(Arc::clone(&victim)));
    let first = TestEntity::shared(2, DVec3::ZERO, Weak::new(), &vanilla_entities::ITEM);
    let weak = Arc::downgrade(&first);
    victim.living().record_last_damage_source(
        &DamageSource::environment(&vanilla_damage_types::GENERIC).with_direct_entity(first),
    );
    assert!(
        weak.upgrade().is_some(),
        "active history retains its source"
    );
    victim
        .living()
        .record_last_damage_source(&DamageSource::environment(&vanilla_damage_types::DROWN));
    assert!(weak.upgrade().is_none());
    assert_eq!(
        victim
            .living()
            .last_damage_source()
            .expect("replacement")
            .damage_type,
        &vanilla_damage_types::DROWN
    );
    assert!(world.players.remove_player_sync(&victim).is_some());
}

#[test]
fn returned_source_survives_history_clear_and_owner_teardown() {
    let history = Arc::new(DamageHistory::default());
    let world = history_world(&history, "damage_history_teardown");
    let player = TestPlayerBuilder::new(Arc::clone(&world), "Retained", 1).build();
    player.living().record_last_damage_source(
        &DamageSource::environment(&vanilla_damage_types::GENERIC)
            .with_direct_entity(Arc::<Player>::clone(&player)),
    );
    let retained = player
        .living()
        .last_damage_source()
        .expect("recorded source");
    let weak_player = Arc::downgrade(&player);
    let weak_world = Arc::downgrade(&world);
    history.clear();
    assert!(player.living().last_damage_source().is_none());
    drop((history, player, world));
    assert!(weak_player.upgrade().is_some());
    assert!(weak_world.upgrade().is_some());
    drop(retained);
    assert!(weak_player.upgrade().is_none());
    assert!(weak_world.upgrade().is_none());
}

#[test]
fn successful_damage_uses_the_victims_clock_instead_of_the_supplied_world() {
    let history = Arc::new(DamageHistory::default());
    let victim_world = history_world(&history, "damage_victim_clock");
    let supplied_world = history_world(&history, "damage_supplied_clock");
    advance_test_game_time_to(&supplied_world, 100);
    let victim = ChickenEntity::new(
        &vanilla_entities::CHICKEN,
        1,
        DVec3::ZERO,
        Arc::downgrade(&victim_world),
    );
    assert!(victim.hurt(
        &supplied_world,
        &DamageSource::environment(&vanilla_damage_types::GENERIC),
        1.0
    ));
    advance_test_game_time_to(&supplied_world, 200);
    history.expire();
    assert!(victim.living().last_damage_source().is_some());
    // Keep vanilla's boundary explicit so changing the implementation constant cannot mask drift.
    advance_test_game_time_to(&victim_world, 40);
    assert!(victim.living().last_damage_source().is_some());
    advance_test_game_time_to(&victim_world, 41);
    assert!(victim.living().last_damage_source().is_none());
}
