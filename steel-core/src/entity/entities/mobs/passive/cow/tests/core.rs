use super::*;
use steel_registry::init_vanilla_registry;

#[test]
fn cow_initializes_vanilla_living_attributes_and_health() {
    init_vanilla_registry();

    let cow = CowEntity::new(&vanilla_entities::COW, 1, DVec3::ZERO, Weak::new());

    assert_eq!(cow.get_health().to_bits(), 10.0_f32.to_bits());
    let attributes = cow.attributes().lock();
    assert_eq!(
        attributes
            .required_value(vanilla_attributes::MAX_HEALTH)
            .to_bits(),
        10.0_f64.to_bits()
    );
    assert!(
        (attributes.required_value(vanilla_attributes::MOVEMENT_SPEED) - f64::from(0.2_f32)).abs()
            < 1e-12
    );
}

#[test]
fn cow_uses_vanilla_cow_food_tag() {
    init_vanilla_registry();

    assert!(CowEntity::is_food(&ItemStack::new(&vanilla_items::WHEAT)));
    assert!(!CowEntity::is_food(&ItemStack::new(&vanilla_items::STONE)));
}

#[test]
fn cow_sound_methods_follow_selected_sound_variant() {
    init_vanilla_registry();

    let cow = CowEntity::new(&vanilla_entities::COW, 1, DVec3::ZERO, Weak::new());
    let source = DamageSource::environment(&vanilla_damage_types::GENERIC);

    assert_eq!(
        LivingEntity::sound_volume(&cow).to_bits(),
        0.4_f32.to_bits()
    );
    assert_eq!(
        Mob::ambient_sound(&cow).map(|sound| &sound.key),
        Some(&sound_events::ENTITY_COW_AMBIENT.key)
    );

    cow.set_sound_variant(&vanilla_cow_sound_variants::MOODY);

    assert_eq!(
        Mob::ambient_sound(&cow).map(|sound| &sound.key),
        Some(&cow.sound_variant().ambient_sound.key)
    );
    assert_eq!(
        LivingEntity::hurt_sound(&cow, &source).map(|sound| &sound.key),
        Some(&cow.sound_variant().hurt_sound.key)
    );
    assert_eq!(
        LivingEntity::death_sound(&cow).map(|sound| &sound.key),
        Some(&cow.sound_variant().death_sound.key)
    );
}

#[test]
fn cow_milks_bucket_into_milk_bucket_for_adults() {
    init_vanilla_registry();

    let world = fresh_test_world("cow_milking");
    let player = TestPlayerBuilder::new(world, "Milker", 10).build();
    player
        .inventory
        .lock()
        .set_selected_item(ItemStack::new(&vanilla_items::BUCKET));

    let cow = CowEntity::new(&vanilla_entities::COW, 1, DVec3::ZERO, Weak::new());

    assert_eq!(
        Mob::mob_interact(&cow, player.as_ref(), InteractionHand::MainHand),
        InteractionResult::Success
    );
    assert!(
        player
            .inventory
            .lock()
            .get_selected_item()
            .is(&vanilla_items::MILK_BUCKET)
    );
}

#[test]
fn cow_does_not_milk_when_baby() {
    init_vanilla_registry();

    let world = fresh_test_world("baby_cow_milking");
    let player = TestPlayerBuilder::new(world, "Milker", 11).build();
    player
        .inventory
        .lock()
        .set_selected_item(ItemStack::new(&vanilla_items::BUCKET));

    let cow = CowEntity::new(&vanilla_entities::COW, 1, DVec3::ZERO, Weak::new());
    cow.set_baby(true);

    assert_eq!(
        Mob::mob_interact(&cow, player.as_ref(), InteractionHand::MainHand),
        InteractionResult::Pass
    );
    assert!(
        player
            .inventory
            .lock()
            .get_selected_item()
            .is(&vanilla_items::BUCKET)
    );
}

#[test]
fn cow_breeding_offspring_inherits_parent_variant() {
    init_vanilla_registry();

    let cow = CowEntity::new(&vanilla_entities::COW, 1, DVec3::ZERO, Weak::new());
    let partner = CowEntity::new(&vanilla_entities::COW, 2, DVec3::ZERO, Weak::new());
    let offspring = CowEntity::new(&vanilla_entities::COW, 3, DVec3::ZERO, Weak::new());

    cow.set_variant(&vanilla_cow_variants::WARM);
    partner.set_variant(&vanilla_cow_variants::COLD);
    offspring.set_variant(&vanilla_cow_variants::TEMPERATE);

    cow.initialize_breed_offspring(&partner, &offspring);

    let variant_key = &offspring.variant().key;
    assert!(
        variant_key == &vanilla_cow_variants::WARM.key
            || variant_key == &vanilla_cow_variants::COLD.key
    );
}

#[test]
fn try_as_dyn_exposes_cow_living_entity_behavior() {
    init_vanilla_registry();

    let cow = CowEntity::new(&vanilla_entities::COW, 1, DVec3::ZERO, Weak::new());
    let entity = &cow as &dyn Entity;

    assert!(entity.is_living_entity());
    let Some(living) = entity.as_living_entity() else {
        panic!("cow should expose living behavior");
    };
    assert_eq!(living.get_health().to_bits(), 10.0_f32.to_bits());
}

#[test]
fn try_as_dyn_exposes_cow_pathfinder_mob_behavior() {
    init_vanilla_registry();

    let cow = CowEntity::new(&vanilla_entities::COW, 1, DVec3::ZERO, Weak::new());
    let entity = &cow as &dyn Entity;

    assert!(entity.is_pathfinder_mob());
    let Some(pathfinder) = entity.as_pathfinder_mob() else {
        panic!("cow should expose pathfinder behavior");
    };
    assert!(!pathfinder.is_path_finding());
}

#[test]
fn try_as_dyn_exposes_cow_animal_behavior() {
    init_vanilla_registry();

    let cow = CowEntity::new(&vanilla_entities::COW, 1, DVec3::ZERO, Weak::new());
    let entity = &cow as &dyn Entity;

    assert!(entity.is_animal());
    let Some(animal) = entity.as_animal() else {
        panic!("cow should expose animal behavior");
    };
    animal.set_in_love_time(5);
    assert_eq!(animal.in_love_time(), 5);
    assert!(animal.is_in_love());
}

#[test]
fn cow_ambient_interval_and_source_match_vanilla_animal_defaults() {
    init_vanilla_registry();

    let cow = CowEntity::new(&vanilla_entities::COW, 1, DVec3::ZERO, Weak::new());

    assert_eq!(Mob::ambient_sound_interval(&cow), 120);
    assert_eq!(Entity::sound_source(&cow), SoundSource::Neutral);
}

#[test]
fn cow_finalize_spawn_assigns_registered_variant_and_sound_variant() {
    init_vanilla_registry();

    let world = fresh_test_world("cow_finalize_spawn");
    let cow = CowEntity::new(
        &vanilla_entities::COW,
        1,
        DVec3::new(0.0, 80.0, 0.0),
        Arc::downgrade(&world),
    );

    assert!(
        REGISTRY
            .cow_variants
            .id_from_key(&cow.variant().key)
            .is_some()
    );
    assert!(
        REGISTRY
            .cow_sound_variants
            .id_from_key(&cow.sound_variant().key)
            .is_some()
    );

    let _ = Mob::finalize_spawn(&cow, &world, EntitySpawnReason::Natural, None);

    assert!(
        REGISTRY
            .cow_variants
            .id_from_key(&cow.variant().key)
            .is_some()
    );
    assert!(
        REGISTRY
            .cow_sound_variants
            .id_from_key(&cow.sound_variant().key)
            .is_some()
    );
}
