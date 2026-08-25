use super::*;

/// Trials for the random breeding-color fallback assertion.
const COLOR_FALLBACK_TRIALS: u32 = 32;
/// Trials for the weighted spawn-color sampling assertions.
const SPAWN_COLOR_TRIALS: u32 = 64;
/// Vanilla tick rate, converting `AgeableMob.ageUp` seconds into ticks.
const TICKS_PER_SECOND: i32 = 20;

#[test]
fn sheep_initializes_vanilla_living_attributes_and_health() {
    init_vanilla_registry();

    let sheep = SheepEntity::new(&vanilla_entities::SHEEP, 1, DVec3::ZERO, Weak::new());

    assert_eq!(
        sheep.get_health().to_bits(),
        sheep.get_max_health().to_bits()
    );
    let attributes = sheep.attributes().lock();
    assert_eq!(
        attributes
            .required_value(vanilla_attributes::MAX_HEALTH)
            .to_bits(),
        8.0_f64.to_bits()
    );
    assert!(
        (attributes.required_value(vanilla_attributes::MOVEMENT_SPEED) - f64::from(0.23_f32)).abs()
            < 1e-12
    );
}

#[test]
fn sheep_uses_vanilla_sheep_food_tag() {
    init_vanilla_registry();

    assert!(SheepEntity::is_food(&ItemStack::new(&vanilla_items::WHEAT)));
    assert!(!SheepEntity::is_food(&ItemStack::new(
        &vanilla_items::STONE
    )));
}

#[test]
fn sheep_wool_color_starts_white_and_unsheared() {
    init_vanilla_registry();

    let sheep = SheepEntity::new(&vanilla_entities::SHEEP, 1, DVec3::ZERO, Weak::new());

    assert_eq!(sheep.color(), DyeColor::White);
    assert!(!sheep.is_sheared());
    assert!(sheep.ready_for_shearing());
}

#[test]
fn sheep_color_and_sheared_state_share_the_vanilla_wool_byte() {
    init_vanilla_registry();

    let sheep = SheepEntity::new(&vanilla_entities::SHEEP, 1, DVec3::ZERO, Weak::new());

    sheep.set_color(DyeColor::Pink);
    assert_eq!(sheep.color(), DyeColor::Pink);
    assert!(!sheep.is_sheared());

    sheep.set_sheared(true);
    assert!(sheep.is_sheared());
    assert_eq!(sheep.color(), DyeColor::Pink);

    sheep.set_color(DyeColor::Blue);
    assert_eq!(sheep.color(), DyeColor::Blue);
    assert!(sheep.is_sheared());

    sheep.set_sheared(false);
    assert!(!sheep.is_sheared());
    assert_eq!(sheep.color(), DyeColor::Blue);
}

#[test]
fn sheep_sound_methods_follow_vanilla_sheep_sounds() {
    init_vanilla_registry();

    let sheep = SheepEntity::new(&vanilla_entities::SHEEP, 1, DVec3::ZERO, Weak::new());
    let source = DamageSource::environment(&vanilla_damage_types::GENERIC);

    assert_eq!(
        LivingEntity::sound_volume(&sheep).to_bits(),
        0.4_f32.to_bits()
    );
    assert_eq!(
        Mob::ambient_sound(&sheep).map(|sound| &sound.key),
        Some(&sound_events::ENTITY_SHEEP_AMBIENT.key)
    );
    assert_eq!(
        LivingEntity::hurt_sound(&sheep, &source).map(|sound| &sound.key),
        Some(&sound_events::ENTITY_SHEEP_HURT.key)
    );
    assert_eq!(
        LivingEntity::death_sound(&sheep).map(|sound| &sound.key),
        Some(&sound_events::ENTITY_SHEEP_DEATH.key)
    );
}

#[test]
fn sheep_shear_drops_wool_and_damages_shears() {
    init_vanilla_registry();

    let world = fresh_test_world("sheep_shearing");
    insert_ready_full_chunk(&world, ChunkPos::new(0, 0));
    let sheep = SheepEntity::new(
        &vanilla_entities::SHEEP,
        1,
        DVec3::new(8.0, 65.0, 8.0),
        Arc::downgrade(&world),
    );
    let shared: SharedEntity = Arc::new(sheep);
    world
        .try_add_entity(Arc::clone(&shared))
        .expect("sheep should attach to the loaded test chunk");
    let sheep = shared
        .downcast_ref::<SheepEntity>()
        .expect("shared entity should be a sheep");

    let player = TestPlayerBuilder::new(world, "Shearer", next_entity_id()).build();
    player
        .inventory
        .lock()
        .set_selected_item(ItemStack::new(&vanilla_items::SHEARS));

    assert_eq!(
        Mob::mob_interact(sheep, player.as_ref(), InteractionHand::MainHand),
        InteractionResult::SuccessServer
    );
    assert!(sheep.is_sheared());
    assert_eq!(
        player
            .inventory
            .lock()
            .get_selected_item_mut()
            .get_damage_value(),
        1
    );
}

#[test]
fn sheep_shear_interaction_is_consumed_when_not_ready() {
    init_vanilla_registry();

    let world = fresh_test_world("sheep_shear_consumed");
    insert_ready_full_chunk(&world, ChunkPos::new(0, 0));
    let sheep = SheepEntity::new(
        &vanilla_entities::SHEEP,
        1,
        DVec3::new(8.0, 65.0, 8.0),
        Arc::downgrade(&world),
    );
    let shared: SharedEntity = Arc::new(sheep);
    world
        .try_add_entity(Arc::clone(&shared))
        .expect("sheep should attach to the loaded test chunk");
    let sheep = shared
        .downcast_ref::<SheepEntity>()
        .expect("shared entity should be a sheep");

    let player = TestPlayerBuilder::new(world, "Shearer", 11).build();
    player
        .inventory
        .lock()
        .set_selected_item(ItemStack::new(&vanilla_items::SHEARS));

    sheep.set_sheared(true);
    assert_eq!(
        Mob::mob_interact(sheep, player.as_ref(), InteractionHand::MainHand),
        InteractionResult::Consume
    );

    sheep.set_sheared(false);
    sheep.set_baby(true);
    assert_eq!(
        Mob::mob_interact(sheep, player.as_ref(), InteractionHand::MainHand),
        InteractionResult::Consume
    );
}

#[test]
fn sheep_breeding_mixes_parent_colors_through_dye_recipes() {
    init_vanilla_registry();
    init_entities();

    let world = fresh_test_world("sheep_breeding");

    for (parent_color, partner_color, expected) in [
        (DyeColor::White, DyeColor::Red, DyeColor::Pink),
        (DyeColor::Yellow, DyeColor::Red, DyeColor::Orange),
        (DyeColor::Purple, DyeColor::Pink, DyeColor::Magenta),
        (DyeColor::Blue, DyeColor::White, DyeColor::LightBlue),
        (DyeColor::Black, DyeColor::White, DyeColor::Gray),
        (DyeColor::Gray, DyeColor::White, DyeColor::LightGray),
        (DyeColor::Green, DyeColor::White, DyeColor::Lime),
    ] {
        let partner = SheepEntity::new(&vanilla_entities::SHEEP, 2, DVec3::ZERO, Weak::new());
        partner.set_color(partner_color);
        let partner_shared: SharedEntity = Arc::new(partner);
        let partner = partner_shared
            .as_animal()
            .expect("sheep should be an animal");

        let sheep = SheepEntity::new(&vanilla_entities::SHEEP, 1, DVec3::ZERO, Weak::new());
        let sheep_shared: SharedEntity = Arc::new(sheep);
        let sheep = sheep_shared
            .downcast_ref::<SheepEntity>()
            .expect("shared entity should be a sheep");
        sheep.set_color(parent_color);

        let offspring = Animal::get_breed_offspring(sheep, &world, partner)
            .expect("sheep breeding should create an offspring");
        let offspring = offspring
            .downcast_ref::<SheepEntity>()
            .expect("offspring should be a sheep");
        assert_eq!(offspring.color(), expected);
    }
}

#[test]
fn sheep_breeding_falls_back_to_a_parent_color_without_a_mix_recipe() {
    init_vanilla_registry();
    init_entities();

    let world = fresh_test_world("sheep_breeding_fallback");
    let partner = SheepEntity::new(&vanilla_entities::SHEEP, 2, DVec3::ZERO, Weak::new());
    partner.set_color(DyeColor::Black);
    let partner_shared: SharedEntity = Arc::new(partner);
    let partner = partner_shared
        .as_animal()
        .expect("sheep should be an animal");

    let sheep = SheepEntity::new(&vanilla_entities::SHEEP, 1, DVec3::ZERO, Weak::new());
    let sheep_shared: SharedEntity = Arc::new(sheep);
    let sheep = sheep_shared
        .downcast_ref::<SheepEntity>()
        .expect("shared entity should be a sheep");
    sheep.set_color(DyeColor::Green);

    for _ in 0..COLOR_FALLBACK_TRIALS {
        let offspring = Animal::get_breed_offspring(sheep, &world, partner)
            .expect("sheep breeding should create an offspring");
        let offspring = offspring
            .downcast_ref::<SheepEntity>()
            .expect("offspring should be a sheep");
        assert!(
            matches!(offspring.color(), DyeColor::Green | DyeColor::Black),
            "offspring color must fall back to a parent color"
        );
    }
}

#[test]
fn sheep_shearing_drop_spawns_one_item_entity_per_count_unit() {
    use crate::entity::entities::ItemEntity;
    use steel_utils::WorldAabb;

    init_vanilla_registry();

    let world = fresh_test_world("sheep_drop_count");
    insert_ready_full_chunk(&world, ChunkPos::new(0, 0));
    let sheep = SheepEntity::new(
        &vanilla_entities::SHEEP,
        next_entity_id(),
        DVec3::new(8.0, 65.0, 8.0),
        Arc::downgrade(&world),
    );
    let shared: SharedEntity = Arc::new(sheep);
    world
        .try_add_entity(Arc::clone(&shared))
        .expect("sheep should attach to the loaded test chunk");
    let sheep = shared
        .downcast_ref::<SheepEntity>()
        .expect("shared entity should be a sheep");

    sheep.spawn_shearing_drop(&ItemStack::with_count(&vanilla_items::RED_WOOL, 3));

    let aabb = WorldAabb::new(7.0, 64.0, 7.0, 9.0, 68.0, 9.0);
    let item_count = world
        .get_entities_in_aabb(&aabb)
        .into_iter()
        .filter(|entity| entity.downcast_ref::<ItemEntity>().is_some())
        .count();
    assert_eq!(
        item_count, 3,
        "vanilla spawns one count-1 entity per drop count unit"
    );
}

#[test]
fn dye_item_dyes_an_unsheared_sheep_and_consumes_the_dye() {
    use crate::behavior::{ITEM_BEHAVIORS, init_behaviors};

    init_vanilla_registry();
    init_behaviors();

    let world = fresh_test_world("sheep_dye");
    insert_ready_full_chunk(&world, ChunkPos::new(0, 0));
    let sheep = SheepEntity::new(
        &vanilla_entities::SHEEP,
        1,
        DVec3::new(8.0, 65.0, 8.0),
        Arc::downgrade(&world),
    );
    let shared: SharedEntity = Arc::new(sheep);
    world
        .try_add_entity(Arc::clone(&shared))
        .expect("sheep should attach to the loaded test chunk");
    let sheep = shared
        .downcast_ref::<SheepEntity>()
        .expect("shared entity should be a sheep");

    let player = TestPlayerBuilder::new(world, "Dyer", next_entity_id()).build();
    let mut dye = ItemStack::with_count(&vanilla_items::RED_DYE, 2);
    let behavior = ITEM_BEHAVIORS.get_behavior(dye.item());

    let result = behavior.interact_living_entity(
        &mut dye,
        player.as_ref(),
        sheep,
        InteractionHand::MainHand,
    );

    assert_eq!(result, InteractionResult::Success);
    assert_eq!(sheep.color(), DyeColor::Red);
    assert_eq!(dye.count(), 1);
}

#[test]
fn dye_item_passes_for_sheared_or_matching_color_sheep() {
    use crate::behavior::{ITEM_BEHAVIORS, init_behaviors};

    init_vanilla_registry();
    init_behaviors();

    let world = fresh_test_world("sheep_dye_pass");
    insert_ready_full_chunk(&world, ChunkPos::new(0, 0));
    let sheep = SheepEntity::new(
        &vanilla_entities::SHEEP,
        1,
        DVec3::new(8.0, 65.0, 8.0),
        Arc::downgrade(&world),
    );
    let shared: SharedEntity = Arc::new(sheep);
    world
        .try_add_entity(Arc::clone(&shared))
        .expect("sheep should attach to the loaded test chunk");
    let sheep = shared
        .downcast_ref::<SheepEntity>()
        .expect("shared entity should be a sheep");
    let player = TestPlayerBuilder::new(world, "Dyer", next_entity_id()).build();
    let mut dye = ItemStack::new(&vanilla_items::RED_DYE);
    let behavior = ITEM_BEHAVIORS.get_behavior(dye.item());

    sheep.set_sheared(true);
    assert_eq!(
        behavior.interact_living_entity(
            &mut dye,
            player.as_ref(),
            sheep,
            InteractionHand::MainHand
        ),
        InteractionResult::Pass
    );

    sheep.set_sheared(false);
    sheep.set_color(DyeColor::Red);
    assert_eq!(
        behavior.interact_living_entity(
            &mut dye,
            player.as_ref(),
            sheep,
            InteractionHand::MainHand
        ),
        InteractionResult::Pass
    );
    assert_eq!(dye.count(), 1);
}

#[test]
fn sheep_ate_regrows_wool_and_speeds_up_baby_ageing() {
    init_vanilla_registry();

    let sheep = SheepEntity::new(&vanilla_entities::SHEEP, 1, DVec3::ZERO, Weak::new());
    sheep.set_sheared(true);
    sheep.set_age(-24_000);
    let age_before = sheep.get_age();

    Mob::ate(&sheep);

    assert!(!sheep.is_sheared());
    assert_eq!(
        sheep.get_age(),
        age_before + ATE_AGE_UP_SECONDS * TICKS_PER_SECOND
    );
}

#[test]
fn sheep_ate_keeps_adult_age_unchanged() {
    init_vanilla_registry();

    let sheep = SheepEntity::new(&vanilla_entities::SHEEP, 1, DVec3::ZERO, Weak::new());
    sheep.set_sheared(true);

    Mob::ate(&sheep);

    assert!(!sheep.is_sheared());
    assert_eq!(sheep.get_age(), 0);
}

#[test]
fn sheep_spawn_color_uses_vanilla_biome_configurations() {
    init_vanilla_registry();

    let desert = REGISTRY
        .biomes
        .by_key(&vanilla_biomes::DESERT.key)
        .expect("desert biome should be registered");
    let snowy_plains = REGISTRY
        .biomes
        .by_key(&vanilla_biomes::SNOWY_PLAINS.key)
        .expect("snowy plains biome should be registered");
    let plains = REGISTRY
        .biomes
        .by_key(&vanilla_biomes::PLAINS.key)
        .expect("plains biome should be registered");

    let warm_colors: Vec<DyeColor> = WARM_SPAWN_COLORS.iter().map(|(c, _)| *c).collect();
    let cold_colors: Vec<DyeColor> = COLD_SPAWN_COLORS.iter().map(|(c, _)| *c).collect();
    let temperate_colors: Vec<DyeColor> = TEMPERATE_SPAWN_COLORS.iter().map(|(c, _)| *c).collect();

    for _ in 0..SPAWN_COLOR_TRIALS {
        let mut random = LegacyRandom::from_seed(rand::random());
        let warm = SheepEntity::random_sheep_color(desert, &mut random);
        let cold = SheepEntity::random_sheep_color(snowy_plains, &mut random);
        let temperate = SheepEntity::random_sheep_color(plains, &mut random);

        assert!(warm_colors.contains(&warm), "desert spawn color {warm:?}");
        assert!(cold_colors.contains(&cold), "snowy spawn color {cold:?}");
        assert!(
            temperate_colors.contains(&temperate),
            "plains spawn color {temperate:?}"
        );
    }
}

#[test]
fn sheep_shear_loot_resolves_the_matching_color_table() {
    init_vanilla_registry();

    let world = fresh_test_world("sheep_shear_loot");
    insert_ready_full_chunk(&world, ChunkPos::new(0, 0));
    let sheep = SheepEntity::new(
        &vanilla_entities::SHEEP,
        1,
        DVec3::new(8.0, 65.0, 8.0),
        Arc::downgrade(&world),
    );
    sheep.set_color(DyeColor::Red);
    let shared: SharedEntity = Arc::new(sheep);
    world
        .try_add_entity(Arc::clone(&shared))
        .expect("sheep should attach to the loaded test chunk");
    let sheep = shared
        .downcast_ref::<SheepEntity>()
        .expect("shared entity should be a sheep");

    let mut rng = rand::rng();
    let drops = shearing_loot_items_with_rng(
        sheep,
        &vanilla_loot_tables::SHEARING_SHEEP,
        &ItemStack::new(&vanilla_items::SHEARS),
        &mut rng,
    );

    assert_ne!(drops.len(), 0);
    for drop in &drops {
        assert!(drop.is(&vanilla_items::RED_WOOL));
    }
}
