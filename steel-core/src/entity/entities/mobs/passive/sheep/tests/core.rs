use super::*;
use crate::entity::living_entity::shearing_loot_items_with_rng;
use crate::test_support::load_additional;
use steel_utils::random::{RandomSource, legacy_random::LegacyRandom};

#[test]
fn sheep_initializes_vanilla_living_attributes_and_health() {
    init_vanilla_registry();
    let sheep = sheep(1);

    assert_eq!(sheep.living().health().to_bits(), 8.0_f32.to_bits());
    assert!(
        (sheep
            .living()
            .attributes()
            .lock()
            .required_value(vanilla_attributes::MOVEMENT_SPEED)
            - f64::from(0.23_f32))
        .abs()
            < 1e-12
    );
    assert!(SheepEntity::is_food(&ItemStack::new(&vanilla_items::WHEAT)));
}

#[test]
fn wool_color_and_sheared_state_share_one_byte_independently() {
    init_vanilla_registry();
    let sheep = sheep(1);

    sheep.set_color(DyeColor::Red);
    sheep.set_sheared(true);
    assert_eq!(sheep.color(), DyeColor::Red);
    assert!(sheep.is_sheared());

    sheep.set_color(DyeColor::Blue);
    assert!(sheep.is_sheared());
    sheep.set_sheared(false);
    assert_eq!(sheep.color(), DyeColor::Blue);
    assert_eq!(
        sheep.living().sheep_loot_state(),
        Some((DyeColor::Blue, false))
    );
}

#[test]
fn eating_grass_regrows_wool_and_speeds_up_a_lamb() {
    init_vanilla_registry();
    let sheep = sheep(1);
    sheep.animal().ageable().set_age(-24_000);
    sheep.set_sheared(true);

    sheep.mob().ate();

    assert!(!sheep.is_sheared());
    assert_eq!(sheep.animal().ageable().age(), -24_000 + 60 * 20);
}

#[test]
fn dye_colors_mix_through_crafting_recipes() {
    init_globals();

    assert_eq!(
        DyeColor::get_mixed_color(DyeColor::Red, DyeColor::White),
        DyeColor::Pink
    );
    assert_eq!(
        DyeColor::get_mixed_color(DyeColor::Red, DyeColor::Yellow),
        DyeColor::Orange
    );
    let unmixable = DyeColor::get_mixed_color(DyeColor::Black, DyeColor::Brown);
    assert!(unmixable == DyeColor::Black || unmixable == DyeColor::Brown);
}

#[test]
fn lambs_get_the_mixed_parent_color() {
    init_globals();
    let world_fixture = fresh_test_world("sheep_breed_offspring");
    let sheep = sheep(1);
    let partner = self::sheep(2);
    sheep.set_color(DyeColor::Red);
    partner.set_color(DyeColor::White);

    let lamb = sheep
        .animal()
        .ageable()
        .breed_offspring(&world_fixture.world, &partner.animal().ageable())
        .expect("sheep breed sheep");
    let lamb = lamb.downcast_ref::<SheepEntity>().expect("lambs are sheep");
    assert_eq!(lamb.color(), DyeColor::Pink);
}

#[test]
fn sheep_saves_and_loads_wool() {
    init_vanilla_registry();
    let sheep = sheep(1);
    sheep.set_color(DyeColor::Lime);
    sheep.set_sheared(true);

    let mut nbt = NbtCompound::new();
    sheep.save_additional(&mut nbt);
    assert_eq!(nbt.byte("Color"), Some(DyeColor::Lime.id() as i8));

    let loaded = self::sheep(2);
    load_additional(&*loaded, &nbt);
    assert_eq!(loaded.color(), DyeColor::Lime);
    assert!(loaded.is_sheared());
}

#[test]
fn dye_recolors_an_unsheared_sheep_and_is_used_up() {
    init_globals();
    let world_fixture = fresh_test_world("sheep_dye");
    let player = TestPlayerBuilder::new(Arc::clone(&world_fixture.world), "Dyer", 2).build();
    let sheep = sheep(1);
    let mut dye = ItemStack::new(&vanilla_items::RED_DYE);

    let result = ITEM_BEHAVIORS
        .get_behavior(&vanilla_items::RED_DYE)
        .interact_living_entity(
            &mut dye,
            &player,
            &sheep.living(),
            InteractionHand::MainHand,
        );
    assert_eq!(result, InteractionResult::Success);
    assert_eq!(sheep.color(), DyeColor::Red);
    assert!(dye.is_empty());

    sheep.set_sheared(true);
    let mut blue = ItemStack::new(&vanilla_items::BLUE_DYE);
    let result = ITEM_BEHAVIORS
        .get_behavior(&vanilla_items::BLUE_DYE)
        .interact_living_entity(
            &mut blue,
            &player,
            &sheep.living(),
            InteractionHand::MainHand,
        );
    assert_eq!(result, InteractionResult::Pass);
    assert_eq!(sheep.color(), DyeColor::Red);
}

#[test]
fn sheep_shear_loot_resolves_the_matching_color_table() {
    init_vanilla_registry();

    let world_fixture = fresh_test_world("sheep_shear_loot");
    let world = &world_fixture.world;
    insert_ready_full_chunk(world, ChunkPos::new(0, 0));
    let sheep = SheepEntity::new(
        &vanilla_entities::SHEEP,
        1,
        DVec3::new(8.0, 65.0, 8.0),
        Arc::downgrade(world),
    );
    sheep.set_color(DyeColor::Red);
    let shared: SharedEntity = sheep;
    world
        .try_add_entity(Arc::clone(&shared))
        .expect("sheep should attach to the loaded test chunk");
    let sheep = shared
        .downcast_ref::<SheepEntity>()
        .expect("shared entity should be a sheep");

    let mut rng = RandomSource::Legacy(LegacyRandom::from_seed(12345));
    let drops = shearing_loot_items_with_rng(
        &sheep.living(),
        &vanilla_loot_tables::SHEARING_SHEEP,
        &ItemStack::new(&vanilla_items::SHEARS),
        &mut rng,
    )
    .expect("shearing loot");

    assert_ne!(drops.len(), 0);
    for drop in &drops {
        assert!(drop.is(&vanilla_items::RED_WOOL));
    }
}
