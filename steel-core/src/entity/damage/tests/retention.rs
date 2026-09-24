use super::*;
use std::ptr;
use steel_registry::vanilla_damage_type_tags::DamageTypeTag;

fn live_pig(world: &Arc<World>, id: i32) -> Arc<PigEntity> {
    let chunk = steel_utils::ChunkPos::new(0, 0);
    if !world.has_full_chunk(chunk) {
        insert_ready_full_chunk(world, chunk);
    }
    let pig = Arc::new(PigEntity::new(
        &vanilla_entities::PIG,
        id,
        DVec3::ZERO,
        Arc::downgrade(world),
    ));
    world.try_add_entity(pig.clone()).expect("register pig");
    pig
}

#[test]
fn removed_history_resolves_shooter_after_projectile_is_released_without_rebinding() {
    let history = Arc::new(DamageHistory::default());
    let world = history_world(&history, "removed_projectile_history");
    let victim = live_pig(&world, 1);
    let shooter = TestPlayerBuilder::new(Arc::clone(&world), "Shooter", 2).build();
    assert!(world.players.insert(shooter.clone()));
    let projectile = TestEntity::shared(3, DVec3::ZERO, Weak::new(), &vanilla_entities::SNOWBALL);
    let projectile_generation = projectile.generation();
    let weak_projectile = Arc::downgrade(&projectile);
    let source = DamageSource::environment(&vanilla_damage_types::THROWN)
        .with_causing_entity(shooter.clone())
        .with_direct_entity(projectile);
    victim.record_last_damage_source(&source);
    victim.set_removed(RemovalReason::Discarded);
    assert!(
        source.direct_entity().is_some(),
        "explicit sources stay strong"
    );
    drop(source);
    assert!(weak_projectile.upgrade().is_none());

    let replacement = TestEntity::shared(3, DVec3::ZERO, Weak::new(), &vanilla_entities::SNOWBALL);
    world
        .try_add_entity(replacement)
        .expect("reuse projectile ID");
    for age in [0, 40] {
        advance_test_game_time_to(&world, age);
        history.expire();
        let recent = victim.last_damage_source().expect("unexpired history");
        assert_eq!(recent.damage_type, &vanilla_damage_types::THROWN);
        assert!(recent.is(&DamageTypeTag::IS_PROJECTILE));
        assert_eq!(
            recent.causing_entity_generation(),
            Some(shooter.generation())
        );
        assert!(ptr::eq(
            recent
                .causing_entity()
                .expect("original shooter")
                .as_player()
                .expect("player"),
            shooter.as_ref(),
        ));
        assert_eq!(
            recent.direct_entity_generation(),
            Some(projectile_generation)
        );
        assert!(recent.direct_entity().is_none());
        assert!(recent.source_position().is_none());
        assert!(!recent.is_direct());
    }
    advance_test_game_time_to(&world, 41);
    assert!(victim.last_damage_source().is_none());
    assert!(world.players.remove_player_sync(&shooter).is_some());
}

#[test]
fn active_history_retains_removed_attacker_but_not_its_removed_attacker() {
    let history = Arc::new(DamageHistory::default());
    let world = history_world(&history, "transitive_damage_history");
    let victim = live_pig(&world, 1);
    let first = live_pig(&world, 2);
    let second = live_pig(&world, 3);
    victim.record_last_damage_source(&DamageSource::direct(
        &vanilla_damage_types::MOB_ATTACK,
        first.clone(),
    ));
    first.record_last_damage_source(&DamageSource::direct(
        &vanilla_damage_types::MOB_ATTACK,
        second.clone(),
    ));
    second.record_last_damage_source(&DamageSource::direct(
        &vanilla_damage_types::MOB_ATTACK,
        first.clone(),
    ));
    let second_generation = second.generation();
    let first_weak = Arc::downgrade(&first);
    let second_weak = Arc::downgrade(&second);
    first.set_removed(RemovalReason::Discarded);
    second.set_removed(RemovalReason::Discarded);
    drop((first, second));
    assert!(second_weak.upgrade().is_none());
    {
        let recent = victim.last_damage_source().expect("active victim history");
        let attacker = recent.causing_entity().expect("retained first attacker");
        assert!(attacker.is_removed());
        let prior = attacker
            .as_living_entity()
            .expect("living attacker")
            .last_damage_source()
            .expect("removed attacker metadata");
        assert_eq!(prior.causing_entity_generation(), Some(second_generation));
        assert!(prior.causing_entity().is_none());
        assert!(prior.direct_entity().is_none());
        assert!(prior.is_direct());
    }
    victim.set_removed(RemovalReason::Discarded);
    assert!(first_weak.upgrade().is_none());
}

#[test]
fn final_removal_cannot_be_undone_by_a_late_history_write() {
    let history = Arc::new(DamageHistory::default());
    let world = history_world(&history, "late_damage_record");
    let victim = live_pig(&world, 1);
    victim.set_removed(RemovalReason::Discarded);
    let attacker = TestEntity::shared(2, DVec3::ZERO, Weak::new(), &vanilla_entities::ITEM);
    let weak = Arc::downgrade(&attacker);
    victim.record_last_damage_source(&DamageSource::direct(
        &vanilla_damage_types::GENERIC,
        attacker,
    ));
    assert!(weak.upgrade().is_none());
    assert!(
        victim
            .last_damage_source()
            .expect("late record")
            .causing_entity()
            .is_none()
    );

    assert!(victim.base().clear_removed());
    world
        .try_add_entity(victim.clone())
        .expect("reactivate victim");
    let attacker = TestEntity::shared(2, DVec3::ZERO, Weak::new(), &vanilla_entities::ITEM);
    let weak = Arc::downgrade(&attacker);
    victim.record_last_damage_source(&DamageSource::direct(
        &vanilla_damage_types::GENERIC,
        attacker,
    ));
    assert!(weak.upgrade().is_some());
    victim.set_removed(RemovalReason::Discarded);
    assert!(weak.upgrade().is_none());
}

#[test]
fn world_teardown_releases_self_history_on_the_frozen_ownership_sweep() {
    let history = Arc::new(DamageHistory::default());
    let world = history_world(&history, "history_world_teardown");
    let victim = live_pig(&world, 1);
    victim.record_last_damage_source(&DamageSource::direct(
        &vanilla_damage_types::GENERIC,
        victim.clone(),
    ));
    let weak = Arc::downgrade(&victim);
    drop(victim);
    history.expire();
    assert!(
        weak.upgrade().is_some(),
        "world membership retains the victim"
    );
    let weak_world = Arc::downgrade(&world);
    drop(world);
    assert!(weak_world.upgrade().is_none());
    history.expire();
    assert!(weak.upgrade().is_none());
}

#[test]
fn chunk_recovery_preserves_history_until_final_unload() {
    let history = Arc::new(DamageHistory::default());
    let world = history_world(&history, "history_chunk_recovery");
    let victim = live_pig(&world, 1);
    let attacker = TestEntity::shared(2, DVec3::ZERO, Weak::new(), &vanilla_entities::ITEM);
    let weak = Arc::downgrade(&attacker);
    victim.record_last_damage_source(&DamageSource::direct(
        &vanilla_damage_types::GENERIC,
        attacker,
    ));
    let chunk = steel_utils::ChunkPos::new(0, 0);
    let manager = world.entity_manager();
    drop(manager.begin_chunk_unload(chunk));
    history.expire();
    assert!(weak.upgrade().is_some());
    drop(manager.on_chunk_loaded(chunk));
    history.expire();
    assert!(weak.upgrade().is_some());
    drop(manager.begin_chunk_unload(chunk));
    manager.finalize_chunk_unload(chunk);
    assert_eq!(
        victim.removal_reason(),
        Some(RemovalReason::UnloadedToChunk)
    );
    assert!(weak.upgrade().is_none());
    assert!(victim.last_damage_source().is_some());
}

#[test]
fn weak_history_preserves_live_position_and_explicit_position_semantics() {
    let history = Arc::new(DamageHistory::default());
    let world = history_world(&history, "weak_history_position");
    let victim = live_pig(&world, 1);
    let direct = TestEntity::shared(2, DVec3::ZERO, Weak::new(), &vanilla_entities::ITEM);
    let source = DamageSource::environment(&vanilla_damage_types::GENERIC)
        .with_direct_entity(direct.clone());
    victim.record_last_damage_source(&source);
    victim.set_removed(RemovalReason::Discarded);
    let position = DVec3::new(1.0, 2.0, 3.0);
    direct
        .try_set_position(position)
        .expect("move original source");
    {
        let recent = victim.last_damage_source().expect("removed history");
        assert_eq!(recent.source_position(), Some(position));
        assert!(recent.source_position_raw().is_none());
    }
    victim.record_last_damage_source(&source.with_source_position(DVec3::ZERO));
    drop(direct);
    let recent = victim.last_damage_source().expect("positioned history");
    assert!(recent.causing_entity_generation().is_none());
    assert!(recent.direct_entity_generation().is_some());
    assert!(recent.direct_entity().is_none());
    assert_eq!(recent.source_position_raw(), Some(DVec3::ZERO));
    assert_eq!(recent.source_position(), Some(DVec3::ZERO));
    assert!(!recent.is_direct());
}
