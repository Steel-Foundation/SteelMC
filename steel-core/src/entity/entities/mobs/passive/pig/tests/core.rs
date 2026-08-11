use super::*;

#[test]
fn pig_initializes_vanilla_living_attributes_and_health() {
    init_vanilla_registry();

    let pig = PigEntity::new(&vanilla_entities::PIG, 1, DVec3::ZERO, Weak::new());

    assert_eq!(pig.get_health().to_bits(), 10.0_f32.to_bits());
    let attributes = pig.attributes().lock();
    assert_eq!(
        attributes
            .required_value(vanilla_attributes::MAX_HEALTH)
            .to_bits(),
        10.0_f64.to_bits()
    );
    assert_eq!(
        attributes
            .required_value(vanilla_attributes::MOVEMENT_SPEED)
            .to_bits(),
        0.25_f64.to_bits()
    );
    assert_eq!(
        attributes
            .required_value(vanilla_attributes::FOLLOW_RANGE)
            .to_bits(),
        16.0_f64.to_bits()
    );
    assert_eq!(
        attributes
            .required_value(vanilla_attributes::TEMPT_RANGE)
            .to_bits(),
        10.0_f64.to_bits()
    );
}

#[test]
fn try_as_dyn_exposes_pig_living_entity_behavior() {
    init_vanilla_registry();

    let pig = PigEntity::new(&vanilla_entities::PIG, 1, DVec3::ZERO, Weak::new());
    let entity = &pig as &dyn Entity;

    assert!(entity.is_living_entity());
    let Some(living) = entity.as_living_entity() else {
        panic!("pig should expose living behavior");
    };
    assert_eq!(living.get_health().to_bits(), 10.0_f32.to_bits());
}

#[test]
fn try_as_dyn_exposes_pig_pathfinder_mob_behavior() {
    init_vanilla_registry();

    let pig = PigEntity::new(&vanilla_entities::PIG, 1, DVec3::ZERO, Weak::new());
    let entity = &pig as &dyn Entity;

    assert!(entity.is_pathfinder_mob());
    let Some(pathfinder) = entity.as_pathfinder_mob() else {
        panic!("pig should expose pathfinder behavior");
    };
    assert!(!pathfinder.is_path_finding());
}

#[test]
fn try_as_dyn_exposes_pig_mob_behavior() {
    init_vanilla_registry();

    let pig = PigEntity::new(&vanilla_entities::PIG, 1, DVec3::ZERO, Weak::new());
    let entity = &pig as &dyn Entity;

    assert!(entity.is_mob());
    let Some(mob) = entity.as_mob() else {
        panic!("pig should expose mob behavior");
    };
    assert_eq!(
        mob.equipment_drop_chance(EquipmentSlot::Saddle).to_bits(),
        0.085_f32.to_bits()
    );
}

#[test]
fn try_as_dyn_exposes_pig_animal_behavior() {
    init_vanilla_registry();

    let pig = PigEntity::new(&vanilla_entities::PIG, 1, DVec3::ZERO, Weak::new());
    let entity = &pig as &dyn Entity;

    assert!(entity.is_animal());
    let Some(animal) = entity.as_animal() else {
        panic!("pig should expose animal behavior");
    };
    animal.set_in_love_time(5);
    assert_eq!(animal.in_love_time(), 5);
    assert!(animal.is_in_love());
}

#[test]
fn try_as_dyn_exposes_pig_item_steerable_behavior() {
    init_vanilla_registry();

    let pig = PigEntity::new(&vanilla_entities::PIG, 1, DVec3::ZERO, Weak::new());
    let entity = &pig as &dyn Entity;

    assert!(entity.is_item_steerable());
    let Some(steerable) = entity.as_item_steerable() else {
        panic!("pig should expose item-steerable behavior");
    };
    assert_eq!(steerable.boost_time_total(), 0);
}

#[test]
fn pig_item_steerable_boost_updates_synced_total_once() {
    init_vanilla_registry();

    let pig = PigEntity::new(&vanilla_entities::PIG, 1, DVec3::ZERO, Weak::new());

    assert!(ItemSteerable::boost(&pig));
    let boost_time_total = pig.boost_time_total();

    assert!((140..=980).contains(&boost_time_total));
    {
        let steering = pig.item_based_steering().lock();
        assert!(steering.is_boosting());
        assert_eq!(steering.boost_time(), 0);
    }
    assert!(!ItemSteerable::boost(&pig));
    assert_eq!(pig.boost_time_total(), boost_time_total);
}

#[test]
fn pig_ridden_speed_uses_item_steering_boost_factor() {
    init_vanilla_registry();

    let world = fresh_test_world("pig_ridden_speed");
    let pig = PigEntity::new(
        &vanilla_entities::PIG,
        1,
        DVec3::ZERO,
        Arc::downgrade(&world),
    );
    let controller = TestPlayerBuilder::new(world, Uuid::from_u128(2), "Controller", 2).build();
    let base_ridden_speed = 0.25_f32 * 0.225;

    assert_eq!(
        LivingEntity::ridden_speed(&pig, &controller).to_bits(),
        base_ridden_speed.to_bits()
    );

    assert!(ItemSteerable::boost(&pig));
    pig.tick_boost();

    assert!(LivingEntity::ridden_speed(&pig, &controller) > base_ridden_speed);
}

#[test]
fn pig_ridden_rotation_matches_controller_head_and_body_yaw() {
    init_vanilla_registry();

    let pig = PigEntity::new(&vanilla_entities::PIG, 1, DVec3::ZERO, Weak::new());
    pig.base().set_old_rotation((7.0, -12.0));

    pig.set_ridden_rotation(450.0, 120.0);

    assert_eq!(pig.rotation(), (90.0, 60.0));
    assert_eq!(pig.base().old_rotation(), (90.0, -12.0));
    assert_eq!(pig.y_body_rot().to_bits(), 90.0_f32.to_bits());
    assert_eq!(pig.y_head_rot().to_bits(), 90.0_f32.to_bits());
}

#[test]
fn pig_can_mate_with_same_type_when_both_in_love() {
    init_vanilla_registry();

    let pig = PigEntity::new(&vanilla_entities::PIG, 1, DVec3::ZERO, Weak::new());
    let partner = PigEntity::new(
        &vanilla_entities::PIG,
        2,
        DVec3::new(1.0, 0.0, 0.0),
        Weak::new(),
    );

    assert!(!pig.can_mate(&partner));

    pig.set_in_love_time(20);
    partner.set_in_love_time(20);

    assert!(pig.can_mate(&partner));
    assert!(!pig.can_mate(&pig));
}

#[test]
fn pig_uses_default_animal_love_mode() {
    init_vanilla_registry();

    let pig = PigEntity::new(&vanilla_entities::PIG, 1, DVec3::ZERO, Weak::new());

    assert!(pig.can_fall_in_love());

    pig.set_in_love(None);

    assert_eq!(pig.in_love_time(), 600);
    assert!(!pig.can_fall_in_love());
    assert!(pig.love_cause_uuid().is_none());
}

#[test]
fn pig_saddle_slot_requires_alive_adult() {
    init_vanilla_registry();

    let pig = PigEntity::new(&vanilla_entities::PIG, 1, DVec3::ZERO, Weak::new());
    let saddle = ItemStack::new(&vanilla_items::SADDLE);

    assert!(LivingEntity::is_equippable_in_slot(
        &pig,
        &saddle,
        EquipmentSlot::Saddle
    ));

    pig.set_baby(true);
    assert!(!LivingEntity::is_equippable_in_slot(
        &pig,
        &saddle,
        EquipmentSlot::Saddle
    ));

    pig.set_baby(false);
    pig.set_health(0.0);
    assert!(!LivingEntity::is_equippable_in_slot(
        &pig,
        &saddle,
        EquipmentSlot::Saddle
    ));
}

#[test]
fn pig_dispenser_can_equip_saddle_only_when_alive_adult_and_empty() {
    init_vanilla_registry();

    let pig = PigEntity::new(&vanilla_entities::PIG, 1, DVec3::ZERO, Weak::new());
    let saddle = ItemStack::new(&vanilla_items::SADDLE);

    assert!(LivingEntity::can_equip_with_dispenser(&pig, &saddle));

    pig.living_base.equipment().lock().set(
        EquipmentSlot::Saddle,
        ItemStack::new(&vanilla_items::SADDLE),
    );
    assert!(!LivingEntity::can_equip_with_dispenser(&pig, &saddle));

    let baby = PigEntity::new(&vanilla_entities::PIG, 2, DVec3::ZERO, Weak::new());
    baby.set_baby(true);
    assert!(!LivingEntity::can_equip_with_dispenser(&baby, &saddle));

    let dead = PigEntity::new(&vanilla_entities::PIG, 3, DVec3::ZERO, Weak::new());
    dead.set_health(0.0);
    assert!(!LivingEntity::can_equip_with_dispenser(&dead, &saddle));

    let unequippable_target = PigEntity::new(&vanilla_entities::PIG, 4, DVec3::ZERO, Weak::new());
    let stone = ItemStack::new(&vanilla_items::STONE);
    assert!(!LivingEntity::can_equip_with_dispenser(
        &unequippable_target,
        &stone
    ));
}

#[test]
fn pig_living_is_baby_uses_ageable_state() {
    init_vanilla_registry();

    let pig = PigEntity::new(&vanilla_entities::PIG, 1, DVec3::ZERO, Weak::new());

    assert!(!LivingEntity::is_baby(&pig));

    pig.set_baby(true);

    assert!(LivingEntity::is_baby(&pig));
}

#[test]
fn pig_saddled_state_reads_saddle_equipment() {
    init_vanilla_registry();

    let pig = PigEntity::new(&vanilla_entities::PIG, 1, DVec3::ZERO, Weak::new());

    assert!(!pig.is_saddled());

    pig.living_base.equipment().lock().set(
        EquipmentSlot::Saddle,
        ItemStack::new(&vanilla_items::CARROT),
    );
    assert!(!pig.is_saddled());

    pig.living_base.equipment().lock().set(
        EquipmentSlot::Saddle,
        ItemStack::new(&vanilla_items::SADDLE),
    );

    assert!(pig.is_saddled());
}

#[test]
fn pig_saddle_equip_sound_uses_vanilla_sound() {
    init_vanilla_registry();

    let pig = PigEntity::new(&vanilla_entities::PIG, 1, DVec3::ZERO, Weak::new());
    let saddle = ItemStack::new(&vanilla_items::SADDLE);

    assert_eq!(
        LivingEntity::equip_sound(&pig, EquipmentSlot::Saddle, &saddle)
            .map(|sound| sound.key.to_string()),
        Some("minecraft:entity.pig.saddle".to_owned())
    );
    assert!(LivingEntity::equip_sound(&pig, EquipmentSlot::Head, &saddle).is_none());
}

#[test]
fn pig_hurt_and_death_sounds_use_current_sound_variant() {
    init_vanilla_registry();

    let pig = PigEntity::new(&vanilla_entities::PIG, 1, DVec3::ZERO, Weak::new());
    let source = DamageSource::environment(&vanilla_damage_types::GENERIC);

    assert_eq!(
        LivingEntity::hurt_sound(&pig, &source).map(|sound| &sound.key),
        Some(&sound_events::ENTITY_PIG_HURT.key)
    );

    pig.set_sound_variant(&vanilla_pig_sound_variants::BIG);
    assert_eq!(
        LivingEntity::death_sound(&pig).map(|sound| &sound.key),
        Some(&sound_events::ENTITY_PIG_BIG_DEATH.key)
    );

    pig.set_baby(true);
    assert_eq!(
        LivingEntity::hurt_sound(&pig, &source).map(|sound| &sound.key),
        Some(&sound_events::ENTITY_BABY_PIG_HURT.key)
    );
}

#[test]
fn pig_ambient_sound_uses_current_sound_variant() {
    init_vanilla_registry();

    let pig = PigEntity::new(&vanilla_entities::PIG, 1, DVec3::ZERO, Weak::new());
    assert_eq!(Mob::ambient_sound_interval(&pig), 120);

    assert_eq!(
        Mob::ambient_sound(&pig).map(|sound| &sound.key),
        Some(&sound_events::ENTITY_PIG_AMBIENT.key)
    );

    pig.set_sound_variant(&vanilla_pig_sound_variants::BIG);
    assert_eq!(
        Mob::ambient_sound(&pig).map(|sound| &sound.key),
        Some(&sound_events::ENTITY_PIG_BIG_AMBIENT.key)
    );

    pig.set_baby(true);
    assert_eq!(
        Mob::ambient_sound(&pig).map(|sound| &sound.key),
        Some(&sound_events::ENTITY_BABY_PIG_AMBIENT.key)
    );
}

#[test]
fn pig_uses_vanilla_animal_experience_reward() {
    init_vanilla_registry();

    let pig = PigEntity::new(&vanilla_entities::PIG, 1, DVec3::ZERO, Weak::new());

    for _ in 0..16 {
        let reward = LivingEntity::base_experience_reward(&pig);
        assert!((1..=3).contains(&reward));
    }
}

#[test]
fn pig_baby_and_consumed_experience_follow_living_rules() {
    init_vanilla_registry();

    let pig = PigEntity::new(&vanilla_entities::PIG, 1, DVec3::ZERO, Weak::new());
    assert!(LivingEntity::should_drop_experience(&pig));
    assert!(!LivingEntity::was_experience_consumed(&pig));

    LivingEntity::skip_drop_experience(&pig);
    assert!(LivingEntity::was_experience_consumed(&pig));

    pig.living_base().reset_death_state();
    assert!(!LivingEntity::was_experience_consumed(&pig));

    pig.set_baby(true);
    assert!(!LivingEntity::should_drop_experience(&pig));
}

#[test]
fn mob_guaranteed_drop_marks_slot_preserved() {
    init_vanilla_registry();

    let pig = PigEntity::new(&vanilla_entities::PIG, 1, DVec3::ZERO, Weak::new());

    assert_eq!(
        pig.equipment_drop_chance(EquipmentSlot::Saddle).to_bits(),
        0.085_f32.to_bits()
    );
    assert!(!pig.is_equipment_drop_preserved(EquipmentSlot::Saddle));

    pig.set_guaranteed_drop(EquipmentSlot::Saddle);

    assert_eq!(
        pig.equipment_drop_chance(EquipmentSlot::Saddle).to_bits(),
        2.0_f32.to_bits()
    );
    assert!(pig.is_equipment_drop_preserved(EquipmentSlot::Saddle));
    assert_eq!(
        pig.equipment_drop_chance(EquipmentSlot::Head).to_bits(),
        0.085_f32.to_bits()
    );
}

#[test]
fn mob_death_loot_without_world_keeps_preserved_equipment() {
    init_vanilla_registry();

    let pig = PigEntity::new(&vanilla_entities::PIG, 1, DVec3::ZERO, Weak::new());
    pig.living_base.equipment().lock().set(
        EquipmentSlot::Saddle,
        ItemStack::new(&vanilla_items::SADDLE),
    );
    pig.set_guaranteed_drop(EquipmentSlot::Saddle);

    pig.drop_custom_death_loot_mob(
        &DamageSource::environment(&vanilla_damage_types::GENERIC),
        false,
    );

    assert!(pig.is_saddled());
    assert!(pig.is_equipment_drop_preserved(EquipmentSlot::Saddle));
}
