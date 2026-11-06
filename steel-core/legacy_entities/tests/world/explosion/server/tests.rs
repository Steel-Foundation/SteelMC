// Moved from world/explosion/server/tests.rs with the entities it depends on.

fn add_block_attached_targets(
    world: &Arc<World>,
) -> (Arc<ItemFrameEntity>, Arc<LeashFenceKnotEntity>) {
    let item_frame = add_entity(
        world,
        ItemFrameEntity::new_attached(
            &vanilla_entities::ITEM_FRAME,
            next_entity_id(),
            TEST_WALL_POS,
            Direction::West,
            Arc::downgrade(world),
        ),
    );
    let leash_knot = add_entity(
        world,
        LeashFenceKnotEntity::new_attached(
            &vanilla_entities::LEASH_KNOT,
            next_entity_id(),
            BlockPos::new(0, 64, 1),
            Arc::downgrade(world),
        ),
    );
    (item_frame, leash_knot)
}

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

#[test]
fn non_destructive_explosion_ignores_blocklike_entities_without_mob_griefing() {
    let fixture = ready_world("explosion_ignores_blocklike_entities");
    let world = &fixture.world;
    assert!(world.set_game_rule(&MOB_GRIEFING, false));
    let item = item_entity(world, TEST_BLOCK_BOTTOM_CENTER + DVec3::X);
    item.set_item(ItemStack::new(&vanilla_items::STONE));
    let item = add_entity(world, item);
    let (item_frame, leash_knot) = add_block_attached_targets(world);

    world.explode(ExplosionOptions::new(
        TEST_BLOCK_BOTTOM_CENTER,
        ENTITY_EFFECT_TEST_RADIUS,
        ExplosionInteraction::None,
    ));

    assert!(!item.is_removed());
    assert_eq!(item.velocity(), DVec3::ZERO);
    assert_eq!(item_frame.velocity(), DVec3::ZERO);
    assert_eq!(leash_knot.velocity(), DVec3::ZERO);
}

#[test]
fn submerged_source_explosion_does_not_push_block_attached_entities() {
    let fixture = ready_world("submerged_explosion_ignores_block_attached_entities");
    let world = &fixture.world;
    let source = item_entity(world, TEST_BLOCK_BOTTOM_CENTER);
    source
        .base()
        .set_fluid_contact(EntityFluidContact::from_parts(1.0, 0.0, false, false));
    assert!(source.is_in_water());
    let (item_frame, leash_knot) = add_block_attached_targets(world);
    let mut options = ExplosionOptions::new(
        TEST_BLOCK_BOTTOM_CENTER,
        ENTITY_EFFECT_TEST_RADIUS,
        ExplosionInteraction::Block,
    );
    options.source = Some(Arc::new(source) as SharedEntity);

    world.explode(options);

    assert_eq!(item_frame.velocity(), DVec3::ZERO);
    assert_eq!(leash_knot.velocity(), DVec3::ZERO);
}
