use super::*;
use crate::test_support::fresh_loaded_test_world;
use crate::test_support::load_additional;

#[test]
fn an_adult_fills_a_bowl_with_mushroom_stew_and_a_calf_does_not() {
    init_globals();
    let world_fixture = fresh_test_world("mooshroom_bowl");
    let player = player_holding(&world_fixture.world, ItemStack::new(&vanilla_items::BOWL));
    let mooshroom = mooshroom(1);

    mooshroom.animal().ageable().set_baby(true);
    assert_eq!(
        mooshroom
            .mob()
            .mob_interact(&player, InteractionHand::MainHand),
        InteractionResult::Pass
    );
    mooshroom.animal().ageable().set_baby(false);
    assert_eq!(
        mooshroom
            .mob()
            .mob_interact(&player, InteractionHand::MainHand),
        InteractionResult::Success
    );
    assert!(held(&player).is(&vanilla_items::MUSHROOM_STEW));
}

#[test]
fn a_brown_mooshroom_remembers_one_flower_for_suspicious_stew() {
    init_globals();
    let world_fixture = fresh_test_world("mooshroom_flower");
    let player = player_holding(
        &world_fixture.world,
        ItemStack::with_count(&vanilla_items::POPPY, 2),
    );
    let mooshroom = mooshroom(1);
    mooshroom.set_variant(MooshroomVariant::Brown);

    assert_eq!(
        mooshroom
            .mob()
            .mob_interact(&player, InteractionHand::MainHand),
        InteractionResult::Success
    );
    assert_eq!(held(&player).count(), 1);
    assert!(mooshroom.stew_effects().is_some());

    // A second flower is refused without being eaten.
    assert_eq!(
        mooshroom
            .mob()
            .mob_interact(&player, InteractionHand::MainHand),
        InteractionResult::Success
    );
    assert_eq!(held(&player).count(), 1);

    player
        .inventory
        .lock()
        .set_selected_item(ItemStack::new(&vanilla_items::BOWL));
    mooshroom
        .mob()
        .mob_interact(&player, InteractionHand::MainHand);
    let stew = held(&player);
    assert!(stew.is(&vanilla_items::SUSPICIOUS_STEW));
    assert!(stew.get(SUSPICIOUS_STEW_EFFECTS).is_some());
    assert!(mooshroom.stew_effects().is_none());
}

#[test]
fn a_red_mooshroom_ignores_flowers() {
    init_globals();
    let world_fixture = fresh_test_world("mooshroom_red_flower");
    let player = player_holding(&world_fixture.world, ItemStack::new(&vanilla_items::POPPY));
    let mooshroom = mooshroom(1);

    assert_eq!(
        mooshroom
            .mob()
            .mob_interact(&player, InteractionHand::MainHand),
        InteractionResult::Pass
    );
    assert!(mooshroom.stew_effects().is_none());
}

#[test]
fn shearing_turns_a_mooshroom_into_a_cow_and_drops_its_mushrooms() {
    init_globals();
    let world_fixture = fresh_loaded_test_world("mooshroom_shear");
    let world = &world_fixture.world;
    let mooshroom = Arc::new(MushroomCowEntity::new(
        &vanilla_entities::MOOSHROOM,
        800_000,
        DVec3::new(0.5, 80.0, 0.5),
        Arc::downgrade(world),
    ));
    world
        .try_add_entity(Arc::<MushroomCowEntity>::clone(&mooshroom))
        .unwrap_or_else(|error| panic!("mooshroom should join the world: {error}"));
    let player = player_holding(world, ItemStack::new(&vanilla_items::SHEARS));
    assert!(world.add_player(Arc::clone(&player), ResetReason::InitialJoin));

    assert_eq!(
        mooshroom
            .mob()
            .mob_interact(&player, InteractionHand::MainHand),
        InteractionResult::Success
    );
    assert!(mooshroom.is_removed());
    let nearby = world.get_entities_in_aabb(&mooshroom.bounding_box().inflate_xyz(2.0, 3.0, 2.0));
    assert!(
        nearby
            .iter()
            .any(|entity| entity.downcast_ref::<CowEntity>().is_some())
    );
    let mushrooms = nearby
        .iter()
        .filter_map(|entity| entity.downcast_ref::<ItemEntity>().map(ItemEntity::item))
        .collect::<Vec<_>>();
    assert_ne!(mushrooms, Vec::<ItemStack>::new());
    assert!(
        mushrooms
            .iter()
            .all(|stack| stack.is(&vanilla_items::RED_MUSHROOM))
    );
    assert_eq!(held(&player).get_damage_value(), 1);
}

#[test]
fn shears_fall_through_on_a_calf() {
    init_globals();
    let world_fixture = fresh_test_world("mooshroom_calf_shear");
    let player = player_holding(&world_fixture.world, ItemStack::new(&vanilla_items::SHEARS));
    let mooshroom = mooshroom(1);
    mooshroom.animal().ageable().set_baby(true);

    assert_eq!(
        mooshroom
            .mob()
            .mob_interact(&player, InteractionHand::MainHand),
        InteractionResult::Pass
    );
    assert!(!mooshroom.is_removed());
}

#[test]
fn mooshroom_saves_and_loads_its_variant_and_stew() {
    init_vanilla_registry();
    let mooshroom = mooshroom(1);
    mooshroom.set_variant(MooshroomVariant::Brown);
    *mooshroom.stew_effects.lock() =
        vanilla_suspicious_stew_effects::from_item(&vanilla_items::CORNFLOWER);

    let mut nbt = NbtCompound::new();
    mooshroom.save_additional(&mut nbt);

    let loaded = self::mooshroom(2);
    load_additional(&loaded, &nbt);
    assert_eq!(loaded.variant(), MooshroomVariant::Brown);
    assert_eq!(loaded.stew_effects(), mooshroom.stew_effects());
}
