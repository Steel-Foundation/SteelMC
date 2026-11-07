use super::*;
use crate::entity::entities::ItemEntity;

fn sheep_in_world(world: &Arc<World>) -> Arc<SheepEntity> {
    insert_ready_full_chunk(world, ChunkPos::new(0, 0));
    let sheep = Arc::new(SheepEntity::new(
        &vanilla_entities::SHEEP,
        1,
        DVec3::new(0.5, 80.0, 0.5),
        Arc::downgrade(world),
    ));
    world
        .try_add_entity(Arc::<SheepEntity>::clone(&sheep))
        .unwrap_or_else(|error| panic!("sheep should join the world: {error}"));
    sheep
}

fn player_with_shears(world: &Arc<World>) -> Arc<Player> {
    let player = TestPlayerBuilder::new(Arc::clone(world), "Shearer", 900_000).build();
    assert!(world.add_player(Arc::clone(&player), ResetReason::InitialJoin));
    player
        .inventory
        .lock()
        .set_selected_item(ItemStack::new(&vanilla_items::SHEARS));
    player
}

#[test]
fn shearing_an_adult_drops_its_wool_and_wears_the_shears() {
    init_globals();
    let world_fixture = fresh_test_world("sheep_shearing");
    let world = &world_fixture.world;
    let sheep = sheep_in_world(world);
    sheep.set_color(DyeColor::Lime);
    let player = player_with_shears(world);

    assert_eq!(
        sheep.mob().mob_interact(&player, InteractionHand::MainHand),
        InteractionResult::SuccessServer
    );
    assert!(sheep.is_sheared());
    assert_eq!(
        player
            .inventory
            .lock()
            .get_selected_item()
            .get_damage_value(),
        1
    );
    let wool = world
        .get_entities_in_aabb(&sheep.bounding_box().inflate_xyz(2.0, 2.0, 2.0))
        .into_iter()
        .filter_map(|entity| entity.downcast_ref::<ItemEntity>().map(ItemEntity::item))
        .collect::<Vec<_>>();
    assert_ne!(wool, Vec::<ItemStack>::new());
    assert!(wool.iter().all(|stack| stack.is(&vanilla_items::LIME_WOOL)));
}

#[test]
fn shears_are_swallowed_by_lambs_and_bare_sheep() {
    init_globals();
    let world_fixture = fresh_test_world("sheep_not_ready");
    let world = &world_fixture.world;
    let sheep = sheep_in_world(world);
    let player = player_with_shears(world);

    sheep.animal().ageable().set_baby(true);
    assert_eq!(
        sheep.mob().mob_interact(&player, InteractionHand::MainHand),
        InteractionResult::Consume
    );
    sheep.animal().ageable().set_baby(false);
    sheep.set_sheared(true);
    assert_eq!(
        sheep.mob().mob_interact(&player, InteractionHand::MainHand),
        InteractionResult::Consume
    );
    assert_eq!(
        player
            .inventory
            .lock()
            .get_selected_item()
            .get_damage_value(),
        0
    );
}
