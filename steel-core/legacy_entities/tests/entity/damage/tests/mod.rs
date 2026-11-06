// Moved from entity/damage/tests/mod.rs with the entities it depends on.

#[test]
fn removed_sources_keep_exact_entities_when_the_world_reuses_their_ids() {
    init_vanilla_registry();
    let world_fixture = fresh_test_world("damage_source_identity");
    let world = &world_fixture.world;
    insert_ready_full_chunk(world, steel_utils::ChunkPos::new(0, 0));
    let attacker: SharedEntity = Arc::new(ChickenEntity::new(
        &vanilla_entities::CHICKEN,
        1,
        DVec3::ZERO,
        Arc::downgrade(world),
    ));
    let projectile: SharedEntity = Arc::new(SnowballEntity::new(
        &vanilla_entities::SNOWBALL,
        2,
        DVec3::ZERO,
        Arc::downgrade(world),
    ));
    for entity in [&attacker, &projectile] {
        world
            .try_add_entity(Arc::clone(entity))
            .expect("original entity registration");
    }
    let source = DamageSource::environment(&vanilla_damage_types::THROWN)
        .with_causing_entity(Arc::clone(&attacker))
        .with_direct_entity(Arc::clone(&projectile));
    for entity in [&attacker, &projectile] {
        entity.set_removed(RemovalReason::Discarded);
        assert!(world.get_entity_by_id(entity.id()).is_none());
        let replacement = TestEntity::shared(
            entity.id(),
            DVec3::ZERO,
            Arc::downgrade(world),
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
fn projectile_callback_can_build_a_source_after_its_removal() {
    use crate::entity::projectile::Projectile;

    let world_fixture = fresh_test_world("discarded_projectile_source");
    let world = &world_fixture.world;
    let target = Arc::new(ChickenEntity::new(
        &vanilla_entities::CHICKEN,
        1,
        DVec3::ZERO,
        Arc::downgrade(world),
    ));
    let projectile = Arc::new(SnowballEntity::new(
        &vanilla_entities::SNOWBALL,
        2,
        DVec3::ZERO,
        Arc::downgrade(world),
    ));
    projectile.set_removed(RemovalReason::Discarded);
    let target_entity: SharedEntity = Arc::<ChickenEntity>::clone(&target);
    Arc::clone(&projectile).on_hit_entity(&target_entity, DVec3::ZERO);
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
