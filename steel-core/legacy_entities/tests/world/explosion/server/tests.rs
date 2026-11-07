// Moved from world/explosion/server/tests.rs with the entities it depends on.

#[test]
fn mob_explosion_does_not_push_vehicles_when_mob_griefing_is_disabled() {
    let fixture = ready_world("explosion_ignores_vehicles_without_mob_griefing");
    let world = &fixture.world;
    assert!(world.set_game_rule(&MOB_GRIEFING, false));
    let minecart = add_entity(
        world,
        ChestMinecartEntity::new(
            &vanilla_entities::CHEST_MINECART,
            next_entity_id(),
            TEST_BLOCK_BOTTOM_CENTER + DVec3::X,
            Arc::downgrade(world),
        ),
    );
    let pig: SharedEntity = Arc::new(ChickenEntity::new(
        &vanilla_entities::CHICKEN,
        next_entity_id(),
        TEST_BLOCK_BOTTOM_CENTER,
        Arc::downgrade(world),
    ));
    let mut options = ExplosionOptions::new(
        TEST_BLOCK_BOTTOM_CENTER,
        ENTITY_EFFECT_TEST_RADIUS,
        ExplosionInteraction::Mob,
    );
    options.source = Some(pig);

    world.explode(options);

    assert_eq!(minecart.velocity(), DVec3::ZERO);
}
