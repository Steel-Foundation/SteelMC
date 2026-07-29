use super::*;

#[test]
fn pig_breeding_offspring_inherits_parent_variant() {
    init_test_registry();

    let pig = PigEntity::new(&vanilla_entities::PIG, 1, DVec3::ZERO, Weak::new());
    let partner = PigEntity::new(
        &vanilla_entities::PIG,
        2,
        DVec3::new(1.0, 0.0, 0.0),
        Weak::new(),
    );
    let offspring = PigEntity::new(
        &vanilla_entities::PIG,
        3,
        DVec3::new(2.0, 0.0, 0.0),
        Weak::new(),
    );
    pig.set_variant(&vanilla_pig_variants::WARM);
    partner.set_variant(&vanilla_pig_variants::COLD);
    offspring.set_variant(&vanilla_pig_variants::TEMPERATE);

    pig.initialize_breed_offspring(&partner, &offspring);

    let variant_key = &offspring.variant().key;
    assert!(
        variant_key == &vanilla_pig_variants::WARM.key
            || variant_key == &vanilla_pig_variants::COLD.key
    );
}

#[test]
fn pig_mob_ai_increments_no_action_time() {
    init_test_registry();

    let pig = PigEntity::new(&vanilla_entities::PIG, 1, DVec3::ZERO, Weak::new());

    pig.set_no_action_time(12);
    Mob::mob_server_ai_step(&pig);

    assert_eq!(pig.no_action_time(), 13);
}

#[test]
fn pig_damage_resets_no_action_time() {
    init_test_registry();

    let pig = PigEntity::new(&vanilla_entities::PIG, 1, DVec3::ZERO, Weak::new());
    let source = DamageSource::environment(&vanilla_damage_types::GENERIC);

    pig.set_no_action_time(42);
    assert!(pig.hurt_server(test_world(), &source, 1.0));

    assert_eq!(pig.no_action_time(), 0);
}

#[test]
fn pig_keeps_vanilla_animal_far_away_persistence() {
    init_test_registry();

    let pig = PigEntity::new(&vanilla_entities::PIG, 1, DVec3::ZERO, Weak::new());

    assert!(!pig.remove_when_far_away(f64::MAX));
}

#[test]
fn pig_registers_vanilla_passive_goal_foundations() {
    init_test_registry();

    let pig = PigEntity::new(&vanilla_entities::PIG, 1, DVec3::ZERO, Weak::new());

    let selector = pig.mob_base().goal_selector().lock();
    assert_eq!(selector.available_goal_count(), 9);
    assert_eq!(
        selector.available_goal_priorities(),
        vec![0, 1, 3, 4, 4, 5, 6, 7, 8]
    );
    drop(selector);
    assert!(pig.mob_base().navigation().lock().can_float());
}

#[test]
fn pig_path_target_feeds_move_control_forward_input() {
    init_test_registry();

    let pig = PigEntity::new(&vanilla_entities::PIG, 1, DVec3::ZERO, Weak::new());
    let path = Path::new(vec![Node::new(1, 0, 0)], BlockPos::new(1, 0, 0), true);

    let level = EmptyNavigationLevel::new();
    assert!(
        pig.mob_base()
            .navigation()
            .lock()
            .move_to(&level, path, 1.0, pig.position())
    );
    let target = {
        let mut navigation = pig.mob_base().navigation().lock();
        navigation.next_move_target(NavigationTickContext {
            mob_position: pig.position(),
            mob_bounding_box_width: pig.bounding_box().width(),
            mob_speed: pig.get_speed(),
            game_time: 0,
        })
    };
    let Some((target, speed_modifier)) = target else {
        panic!("navigation should provide a move target");
    };

    pig.set_wanted_position(target, speed_modifier);
    Mob::tick_move_control(&pig);

    assert_eq!(pig.get_speed().to_bits(), 0.25_f32.to_bits());
    assert_eq!(pig.travel_input().forward().to_bits(), 0.25_f32.to_bits());
}

#[test]
fn pig_age_updates_synchronized_baby_flag_on_boundary() {
    init_test_registry();

    let pig = PigEntity::new(&vanilla_entities::PIG, 1, DVec3::ZERO, Weak::new());

    pig.set_age(-1);
    assert!(pig.is_baby());
    assert!(*pig.entity_data.lock().ageable_mob().baby.get());

    pig.set_age(0);
    assert!(!pig.is_baby());
    assert!(!*pig.entity_data.lock().ageable_mob().baby.get());
}

#[test]
fn pig_age_boundary_refreshes_dimensions() {
    init_test_registry();

    let pig = PigEntity::new(&vanilla_entities::PIG, 1, DVec3::ZERO, Weak::new());
    let adult_dimensions = vanilla_entities::PIG.dimensions;

    assert_eq!(pig.base().dimensions(), adult_dimensions);

    pig.set_age(-1);
    let baby_dimensions = PIG_BABY_DIMENSIONS;
    assert_eq!(pig.base().dimensions(), baby_dimensions);
    assert_eq!(baby_dimensions.eye_height.to_bits(), 0.40625_f32.to_bits());
    assert_eq!(
        baby_dimensions
            .attachments
            .get_clamped(EntityAttachment::Passenger, 0, 0.0, baby_dimensions)
            .y
            .to_bits(),
        0.5_f64.to_bits()
    );
    assert_eq!(
        pig.bounding_box().width().to_bits(),
        f64::from(baby_dimensions.width).to_bits()
    );
    assert_eq!(
        pig.bounding_box().height().to_bits(),
        f64::from(baby_dimensions.height).to_bits()
    );

    pig.set_age(0);
    assert_eq!(pig.base().dimensions(), adult_dimensions);
    assert_eq!(
        pig.bounding_box().width().to_bits(),
        f64::from(adult_dimensions.width).to_bits()
    );
    assert_eq!(
        pig.bounding_box().height().to_bits(),
        f64::from(adult_dimensions.height).to_bits()
    );
}

#[test]
fn pig_scale_attribute_refreshes_dimensions() {
    init_test_registry();

    let pig = PigEntity::new(&vanilla_entities::PIG, 1, DVec3::ZERO, Weak::new());
    let adult_dimensions = vanilla_entities::PIG.dimensions;

    pig.attributes()
        .lock()
        .set_base_value(vanilla_attributes::SCALE, 2.0);
    LivingEntity::refresh_dirty_attributes(&pig);

    let scaled_dimensions = adult_dimensions.scale(2.0);
    assert_eq!(pig.base().dimensions(), scaled_dimensions);
    assert_eq!(
        pig.bounding_box().width().to_bits(),
        f64::from(scaled_dimensions.width).to_bits()
    );
    assert_eq!(
        pig.bounding_box().height().to_bits(),
        f64::from(scaled_dimensions.height).to_bits()
    );
}
