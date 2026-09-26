use std::sync::{Arc, Weak};

use glam::DVec3;
use steel_registry::{
    init_vanilla_registry, vanilla_damage_types, vanilla_dimension_types, vanilla_entities,
};
use steel_utils::{Identifier, types::Difficulty};

use super::{DamageHistory, DamageSource};
use crate::entity::entities::{PigEntity, SnowballEntity};
use crate::entity::{Entity, LivingEntity, RemovalReason, SharedEntity};

use crate::level_data::GameTimeSource;
use crate::test_support::{
    TestEntity, TestPlayerBuilder, advance_test_game_time_to,
    create_test_world_with_damage_history, fresh_test_world, insert_ready_full_chunk,
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
fn removed_sources_keep_exact_entities_when_the_world_reuses_their_ids() {
    init_vanilla_registry();
    let world = fresh_test_world("damage_source_identity");
    insert_ready_full_chunk(&world, steel_utils::ChunkPos::new(0, 0));
    let attacker: SharedEntity = Arc::new(PigEntity::new(
        &vanilla_entities::PIG,
        1,
        DVec3::ZERO,
        Arc::downgrade(&world),
    ));
    let projectile: SharedEntity = Arc::new(SnowballEntity::new(
        &vanilla_entities::SNOWBALL,
        2,
        DVec3::ZERO,
        Arc::downgrade(&world),
    ));
    for entity in [&attacker, &projectile] {
        world
            .try_add_entity(Arc::clone(entity))
            .expect("original entity registration");
    }
    let source = DamageSource::environment(&vanilla_damage_types::THROWN)
        .with_causing_entity(attacker.clone())
        .with_direct_entity(projectile.clone());
    for entity in [&attacker, &projectile] {
        entity.set_removed(RemovalReason::Discarded);
        assert!(world.get_entity_by_id(entity.id()).is_none());
        let replacement = TestEntity::shared(
            entity.id(),
            DVec3::ZERO,
            Arc::downgrade(&world),
            &vanilla_entities::ITEM,
        );
        world
            .try_add_entity(replacement)
            .expect("replacement registration");
    }
    let weak_attacker = Arc::downgrade(&attacker);
    let weak_projectile = Arc::downgrade(&projectile);
    let retained = source.clone();
    drop((source, attacker, projectile));
    assert!(Weak::ptr_eq(
        &Arc::downgrade(retained.causing_entity().expect("attacker")),
        &weak_attacker
    ));
    assert!(Weak::ptr_eq(
        &Arc::downgrade(retained.direct_entity().expect("projectile")),
        &weak_projectile
    ));
    assert!(retained.causing_entity().expect("attacker").is_removed());
    assert!(retained.scales_with_difficulty());
    assert!(!retained.is_direct());
    for entity in [
        retained.causing_entity().expect("attacker"),
        retained.direct_entity().expect("projectile"),
    ] {
        world
            .get_entity_by_id(entity.id())
            .expect("replacement")
            .set_removed(RemovalReason::Discarded);
    }
    drop(retained);
    assert!(weak_attacker.upgrade().is_none());
    assert!(weak_projectile.upgrade().is_none());
}

#[test]
fn directness_uses_allocation_identity_and_position_tracks_the_direct_entity() {
    init_vanilla_registry();
    let first = TestEntity::shared(1, DVec3::ZERO, Weak::new(), &vanilla_entities::ITEM);
    let other = TestEntity::shared(1, DVec3::ZERO, Weak::new(), &vanilla_entities::ITEM);
    let environment = DamageSource::environment(&vanilla_damage_types::GENERIC);
    assert!(environment.is_direct());
    let source = DamageSource::direct(&vanilla_damage_types::GENERIC, first.clone());
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
    let entity: SharedEntity = player.clone();
    let source = DamageSource::direct(&vanilla_damage_types::INDIRECT_MAGIC, entity);
    player.record_last_damage_source(&source);
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
    first.record_last_damage_source(
        &DamageSource::environment(&vanilla_damage_types::PLAYER_ATTACK)
            .with_causing_entity(second.clone()),
    );
    second.record_last_damage_source(
        &DamageSource::environment(&vanilla_damage_types::PLAYER_ATTACK)
            .with_causing_entity(first.clone()),
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
    assert!(world.players.insert(victim.clone()));
    let first = TestEntity::shared(2, DVec3::ZERO, Weak::new(), &vanilla_entities::ITEM);
    let weak = Arc::downgrade(&first);
    victim.record_last_damage_source(
        &DamageSource::environment(&vanilla_damage_types::GENERIC).with_direct_entity(first),
    );
    assert!(
        weak.upgrade().is_some(),
        "active history retains its source"
    );
    victim.record_last_damage_source(&DamageSource::environment(&vanilla_damage_types::DROWN));
    assert!(weak.upgrade().is_none());
    assert_eq!(
        victim
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
    player.record_last_damage_source(
        &DamageSource::environment(&vanilla_damage_types::GENERIC)
            .with_direct_entity(player.clone()),
    );
    let retained = player.last_damage_source().expect("recorded source");
    let weak_player = Arc::downgrade(&player);
    let weak_world = Arc::downgrade(&world);
    history.clear();
    assert!(player.last_damage_source().is_none());
    drop((history, player, world));
    assert!(weak_player.upgrade().is_some());
    assert!(weak_world.upgrade().is_some());
    drop(retained);
    assert!(weak_player.upgrade().is_none());
    assert!(weak_world.upgrade().is_none());
}

#[test]
fn projectile_callback_can_build_a_source_after_its_removal() {
    use crate::entity::projectile::Projectile;

    let world = fresh_test_world("discarded_projectile_source");
    let target = Arc::new(PigEntity::new(
        &vanilla_entities::PIG,
        1,
        DVec3::ZERO,
        Arc::downgrade(&world),
    ));
    let projectile = Arc::new(SnowballEntity::new(
        &vanilla_entities::SNOWBALL,
        2,
        DVec3::ZERO,
        Arc::downgrade(&world),
    ));
    projectile.set_removed(RemovalReason::Discarded);
    let target_entity: SharedEntity = target.clone();
    projectile
        .clone()
        .on_hit_entity(&target_entity, DVec3::ZERO);
    let source = target
        .last_damage_source()
        .expect("zero-damage projectile hit is recorded");
    let projectile_entity: SharedEntity = projectile;
    assert!(Arc::ptr_eq(
        source.direct_entity().expect("projectile"),
        &projectile_entity
    ));
    assert!(source.direct_entity().expect("projectile").is_removed());
}

#[test]
fn successful_damage_uses_the_victims_clock_instead_of_the_supplied_world() {
    let history = Arc::new(DamageHistory::default());
    let victim_world = history_world(&history, "damage_victim_clock");
    let supplied_world = history_world(&history, "damage_supplied_clock");
    advance_test_game_time_to(&supplied_world, 100);
    let victim = PigEntity::new(
        &vanilla_entities::PIG,
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
    assert!(victim.last_damage_source().is_some());
    advance_test_game_time_to(&victim_world, 40);
    assert!(victim.last_damage_source().is_some());
    advance_test_game_time_to(&victim_world, 41);
    assert!(victim.last_damage_source().is_none());
}
