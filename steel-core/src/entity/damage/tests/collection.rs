use super::*;
use crate::entity::EntityArc;

#[test]
fn removed_self_damaged_entity_keeps_history_while_independently_owned() {
    let history = Arc::new(DamageHistory::default());
    let world = history_world(&history, "removed_history_owner");
    let player = TestPlayerBuilder::new(Arc::clone(&world), "SelfDamage", 1).build();
    player.record_last_damage_source(
        &DamageSource::environment(&vanilla_damage_types::INDIRECT_MAGIC)
            .with_causing_entity(player.clone())
            .with_direct_entity(player.clone()),
    );
    player.set_removed(RemovalReason::Discarded);
    history.collect_unreachable();
    {
        let source = player
            .last_damage_source()
            .expect("removed victim's history");
        let retained = source.causing_entity().expect("self damage");
        assert_eq!(retained.generation(), player.generation());
        assert!(retained.is_removed());
        assert!(source.is_direct());
    }

    let weak_player = EntityArc::downgrade(&player);
    let weak_world = Arc::downgrade(&world);
    drop((player, world));
    history.collect_unreachable();
    assert!(weak_player.upgrade().is_none());
    assert!(weak_world.upgrade().is_none());
}

#[test]
fn live_victim_keeps_removed_sources_and_their_transitive_history() {
    let history = Arc::new(DamageHistory::default());
    let world = history_world(&history, "transitive_damage_history");
    let victim = TestPlayerBuilder::new(Arc::clone(&world), "Victim", 1).build();
    let first = TestPlayerBuilder::new(Arc::clone(&world), "First", 2).build();
    let second = TestPlayerBuilder::new(Arc::clone(&world), "Second", 3).build();
    victim.record_last_damage_source(
        &DamageSource::environment(&vanilla_damage_types::PLAYER_ATTACK)
            .with_causing_entity(first.clone()),
    );
    first.record_last_damage_source(
        &DamageSource::environment(&vanilla_damage_types::PLAYER_ATTACK)
            .with_causing_entity(second.clone()),
    );
    second.record_last_damage_source(
        &DamageSource::environment(&vanilla_damage_types::PLAYER_ATTACK)
            .with_causing_entity(first.clone()),
    );
    first.set_removed(RemovalReason::Discarded);
    second.set_removed(RemovalReason::Discarded);
    let first_generation = first.generation();
    let second_generation = second.generation();
    let first_weak = EntityArc::downgrade(&first);
    let second_weak = EntityArc::downgrade(&second);
    drop((first, second));

    history.collect_unreachable();
    {
        let source = victim.last_damage_source().expect("live victim's history");
        let first = source.causing_entity().expect("first attacker");
        assert_eq!(first.generation(), first_generation);
        let next = first
            .as_living_entity()
            .expect("living attacker")
            .last_damage_source()
            .expect("removed attacker's history");
        assert_eq!(
            next.causing_entity().expect("second attacker").generation(),
            second_generation
        );
        assert!(history.last_damage_source(second_generation).is_some());
    }

    let victim_generation = victim.generation();
    drop(victim);
    history.collect_unreachable();
    assert!(history.last_damage_source(victim_generation).is_none());
    assert!(first_weak.upgrade().is_none());
    assert!(second_weak.upgrade().is_none());
}

#[test]
fn returned_source_roots_removed_entities_and_their_history_until_dropped() {
    let history = Arc::new(DamageHistory::default());
    let world = history_world(&history, "returned_damage_history_root");
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
    let retained = first.last_damage_source().expect("returned source");
    let first_generation = first.generation();
    let second_generation = second.generation();
    let first_weak = EntityArc::downgrade(&first);
    let second_weak = EntityArc::downgrade(&second);
    first.set_removed(RemovalReason::Discarded);
    second.set_removed(RemovalReason::Discarded);
    drop((first, second));

    history.collect_unreachable();
    assert!(history.last_damage_source(first_generation).is_some());
    assert!(history.last_damage_source(second_generation).is_some());
    assert!(
        retained
            .causing_entity()
            .expect("retained attacker")
            .is_removed()
    );

    drop(retained);
    history.collect_unreachable();
    assert!(first_weak.upgrade().is_none());
    assert!(second_weak.upgrade().is_none());
}

#[test]
fn dropped_victims_do_not_root_unrelated_history_through_reused_ids() {
    let history = Arc::new(DamageHistory::default());
    let world = history_world(&history, "reused_id_history_lifetime");
    let victim = TestPlayerBuilder::new(Arc::clone(&world), "Victim", 1).build();
    let attacker = TestPlayerBuilder::new(Arc::clone(&world), "Attacker", 2).build();
    victim.record_last_damage_source(
        &DamageSource::environment(&vanilla_damage_types::PLAYER_ATTACK)
            .with_causing_entity(attacker.clone()),
    );
    let generation = victim.generation();
    let replacement = TestPlayerBuilder::new(Arc::clone(&world), "Replacement", victim.id())
        .uuid(victim.uuid())
        .build();
    let attacker_weak = EntityArc::downgrade(&attacker);
    drop((victim, attacker));

    history.collect_unreachable();
    assert!(history.last_damage_source(generation).is_none());
    assert!(replacement.last_damage_source().is_none());
    assert!(attacker_weak.upgrade().is_none());
}

#[test]
fn world_membership_roots_history_until_the_entity_is_discarded() {
    let history = Arc::new(DamageHistory::default());
    let world = history_world(&history, "world_owned_damage_history");
    insert_ready_full_chunk(&world, steel_utils::ChunkPos::new(0, 0));
    let pig = EntityArc::new(PigEntity::new(
        &vanilla_entities::PIG,
        1,
        DVec3::ZERO,
        Arc::downgrade(&world),
    ));
    world.try_add_entity(pig.clone()).expect("register pig");
    pig.record_last_damage_source(
        &DamageSource::environment(&vanilla_damage_types::GENERIC)
            .with_causing_entity(pig.clone())
            .with_direct_entity(pig.clone()),
    );
    let id = pig.id();
    let generation = pig.generation();
    let weak = EntityArc::downgrade(&pig);
    drop(pig);

    history.collect_unreachable();
    assert!(history.last_damage_source(generation).is_some());
    world
        .get_entity_by_id(id)
        .expect("live pig")
        .set_removed(RemovalReason::Discarded);
    assert!(world.get_entity_by_id(id).is_none());
    history.collect_unreachable();
    assert!(weak.upgrade().is_none());
    assert!(history.last_damage_source(generation).is_none());
}

#[test]
fn live_history_keeps_both_sources_promotable_without_their_own_history() {
    let history = Arc::new(DamageHistory::default());
    let world = history_world(&history, "source_promotion_without_history");
    let victim = TestPlayerBuilder::new(Arc::clone(&world), "Victim", 1).build();
    let cause = TestEntity::shared(2, DVec3::ZERO, Weak::new(), &vanilla_entities::ITEM);
    let direct = TestEntity::shared(3, DVec3::ZERO, Weak::new(), &vanilla_entities::SNOWBALL);
    let cause_weak = EntityArc::downgrade(&cause);
    let direct_weak = EntityArc::downgrade(&direct);
    victim.record_last_damage_source(
        &DamageSource::environment(&vanilla_damage_types::THROWN)
            .with_causing_entity(cause)
            .with_direct_entity(direct),
    );

    history.collect_unreachable();
    assert!(cause_weak.upgrade().is_some());
    assert!(direct_weak.upgrade().is_some());
    drop(victim);
    history.collect_unreachable();
    assert!(cause_weak.upgrade().is_none());
    assert!(direct_weak.upgrade().is_none());
}
