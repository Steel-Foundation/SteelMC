use super::*;

#[test]
fn can_glide_using_matches_vanilla_component_gate() {
    init_test_registry();
    let entity = LivingFluidTestEntity::new(0.0, 0.0, true);
    let mut elytra = ItemStack::new(&vanilla_items::ELYTRA);

    assert!(entity.can_glide_using(&elytra, EquipmentSlot::Chest));
    assert!(!entity.can_glide_using(&elytra, EquipmentSlot::Head));

    elytra.set_damage_value(elytra.get_max_damage() - 1);

    assert!(elytra.next_damage_will_break());
    assert!(!entity.can_glide_using(&elytra, EquipmentSlot::Chest));
    assert!(!entity.can_glide_using(&ItemStack::new(&vanilla_items::STONE), EquipmentSlot::Chest));
}

#[test]
fn living_armor_cover_counts_non_empty_humanoid_armor_slots() {
    init_test_registry();
    let entity = LivingFluidTestEntity::new(0.0, 0.0, true);

    assert_f32_close(entity.get_armor_cover_percentage(), 0.0);

    entity.equip(EquipmentSlot::Head, ItemStack::new(&vanilla_items::STONE));
    entity.equip(EquipmentSlot::Feet, ItemStack::new(&vanilla_items::STONE));

    assert_f32_close(entity.get_armor_cover_percentage(), 0.5);
}

#[test]
fn living_visibility_percent_uses_discrete_and_invisible_scaling() {
    init_test_registry();
    let entity = LivingFluidTestEntity::new(0.0, 0.0, true);

    assert_f64_close(entity.get_visibility_percent(None), 1.0);

    EntitySyncedData::set_base_invisible_flag(&entity.entity_data, true);

    let invisible_without_armor = 0.7 * f64::from(0.1_f32);
    assert_f64_close(entity.get_visibility_percent(None), invisible_without_armor);

    entity.set_shared_shift_key_down(true);

    assert_f64_close(
        entity.get_visibility_percent(None),
        0.8 * invisible_without_armor,
    );
}

#[test]
fn living_visibility_percent_uses_matching_mob_head_disguise() {
    init_test_registry();
    let entity = LivingFluidTestEntity::new(0.0, 0.0, true);
    let skeleton =
        LivingFluidTestEntity::new(0.0, 0.0, true).with_entity_type(&vanilla_entities::SKELETON);

    entity.equip(
        EquipmentSlot::Head,
        ItemStack::new(&vanilla_items::SKELETON_SKULL),
    );

    assert_f64_close(entity.get_visibility_percent(Some(&skeleton)), 0.5);
}

#[test]
fn living_freeze_immunity_uses_armor_equipment() {
    init_test_registry();
    let entity = LivingFluidTestEntity::new(0.0, 0.0, true);

    assert!(entity.default_living_can_freeze());

    entity.equip(
        EquipmentSlot::Feet,
        ItemStack::new(&vanilla_items::LEATHER_BOOTS),
    );

    assert!(!entity.default_living_can_freeze());
}

#[test]
fn living_freeze_immunity_uses_body_armor_equipment() {
    init_test_registry();
    let entity = LivingFluidTestEntity::new(0.0, 0.0, true);

    entity.equip(
        EquipmentSlot::Body,
        ItemStack::new(&vanilla_items::LEATHER_HORSE_ARMOR),
    );

    assert!(!entity.default_living_can_freeze());
}

#[test]
fn living_freeze_immunity_ignores_non_armor_equipment() {
    init_test_registry();
    let entity = LivingFluidTestEntity::new(0.0, 0.0, true);
    entity.equip(
        EquipmentSlot::MainHand,
        ItemStack::new(&vanilla_items::LEATHER_BOOTS),
    );

    assert!(entity.default_living_can_freeze());
}

#[test]
fn living_freezing_decays_when_not_in_powder_snow() {
    init_test_registry();
    let entity = LivingFluidTestEntity::new(0.0, 0.0, true);
    entity.set_ticks_frozen(10);

    entity.tick_freezing();

    assert_eq!(entity.ticks_frozen(), 8);
}

#[test]
fn living_freezing_keeps_ticks_while_in_powder_snow() {
    init_test_registry();
    let entity = LivingFluidTestEntity::new(0.0, 0.0, true);
    entity.set_ticks_frozen(10);
    entity.apply_inside_block_effect(InsideBlockEffectType::Freeze);

    entity.tick_freezing();

    assert_eq!(entity.ticks_frozen(), 11);
}

#[test]
fn living_freezing_adds_powder_snow_speed_modifier() {
    init_test_registry();
    let entity = LivingFluidTestEntity::new(0.0, 0.0, true).with_non_air_frost_block();
    entity.set_ticks_frozen(DEFAULT_TICKS_REQUIRED_TO_FREEZE / 2);
    entity.apply_inside_block_effect(InsideBlockEffectType::Freeze);
    let base_speed = entity
        .attributes()
        .lock()
        .required_value(vanilla_attributes::MOVEMENT_SPEED);

    entity.tick_freezing();

    let attributes = entity.attributes().lock();
    assert!(attributes.has_modifier(
        vanilla_attributes::MOVEMENT_SPEED,
        &SPEED_MODIFIER_POWDER_SNOW_ID,
    ));
    assert_f64_close(
        attributes.required_value(vanilla_attributes::MOVEMENT_SPEED),
        base_speed - f64::from(0.05_f32 * entity.percent_frozen()),
    );
}

#[test]
fn living_freezing_removes_stale_powder_snow_speed_modifier() {
    init_test_registry();
    let entity = LivingFluidTestEntity::new(0.0, 0.0, true);
    entity.attributes().lock().add_modifier(
        vanilla_attributes::MOVEMENT_SPEED,
        AttributeModifier {
            id: SPEED_MODIFIER_POWDER_SNOW_ID,
            amount: -0.05,
            operation: AttributeModifierOperation::AddValue,
        },
        false,
    );

    entity.tick_freezing();

    assert!(!entity.attributes().lock().has_modifier(
        vanilla_attributes::MOVEMENT_SPEED,
        &SPEED_MODIFIER_POWDER_SNOW_ID,
    ));
}

#[test]
fn living_freezing_damages_fully_frozen_entities_on_frequency() {
    init_test_registry();
    let entity = LivingFluidTestEntity::new_in_world(0.0, 0.0, true, test_world());
    entity.set_ticks_frozen(DEFAULT_TICKS_REQUIRED_TO_FREEZE);
    entity.apply_inside_block_effect(InsideBlockEffectType::Freeze);
    for _ in 0..40 {
        entity.advance_tick_count();
    }

    entity.tick_freezing();

    assert_f32_close(entity.get_health(), 19.0);
}

#[test]
fn default_ai_step_ticks_freezing_after_travel() {
    init_test_registry();
    init_behaviors();
    let entity = LivingFluidTestEntity::new_in_world(0.0, 0.0, true, test_world());
    entity.set_ticks_frozen(DEFAULT_TICKS_REQUIRED_TO_FREEZE);
    entity.apply_inside_block_effect(InsideBlockEffectType::Freeze);
    for _ in 0..40 {
        entity.advance_tick_count();
    }

    entity.default_ai_step();

    assert_eq!(
        entity.damage_type_keys(),
        vec![vanilla_damage_types::FREEZE.key.clone()]
    );
    assert_f32_close(entity.get_health(), 19.0);
}

#[test]
fn entity_cramming_damage_threshold_matches_vanilla_push_entities() {
    assert!(!should_apply_entity_cramming_damage(0, 100, 100, 0));
    assert!(!should_apply_entity_cramming_damage(24, 23, 23, 0));
    assert!(!should_apply_entity_cramming_damage(24, 24, 23, 0));
    assert!(!should_apply_entity_cramming_damage(24, 24, 24, 1));
    assert!(should_apply_entity_cramming_damage(24, 24, 24, 0));
}

#[test]
fn freezing_damage_hurts_extra_tagged_entity_types() {
    init_test_registry();
    let entity =
        LivingFluidTestEntity::new(0.0, 0.0, true).with_entity_type(&vanilla_entities::BLAZE);

    assert!(entity.hurt(
        test_world(),
        &DamageSource::environment(&vanilla_damage_types::FREEZE),
        1.0,
    ));

    assert_f32_close(entity.get_health(), 15.0);
}

#[test]
fn living_powder_snow_walkability_uses_feet_equipment() {
    init_test_registry();
    let entity = LivingFluidTestEntity::new(0.0, 0.0, true);

    assert!(!entity.default_living_can_walk_on_powder_snow());

    entity.equip(
        EquipmentSlot::Feet,
        ItemStack::new(&vanilla_items::LEATHER_BOOTS),
    );

    assert!(entity.default_living_can_walk_on_powder_snow());
}

#[test]
fn living_powder_snow_walkability_ignores_non_feet_equipment() {
    init_test_registry();
    let entity = LivingFluidTestEntity::new(0.0, 0.0, true);
    entity.equip(
        EquipmentSlot::MainHand,
        ItemStack::new(&vanilla_items::LEATHER_BOOTS),
    );

    assert!(!entity.default_living_can_walk_on_powder_snow());
}

#[test]
fn default_can_glide_uses_living_equipment() {
    init_test_registry();
    let entity = LivingFluidTestEntity::new(0.0, 0.0, true);
    entity.set_on_ground(false);

    assert!(!entity.can_glide());

    entity.equip(EquipmentSlot::Chest, ItemStack::new(&vanilla_items::ELYTRA));

    assert!(entity.can_glide());
}

#[test]
fn try_to_start_fall_flying_uses_vanilla_glider_gate() {
    init_test_registry();
    let entity = LivingFluidTestEntity::new(0.0, 0.0, true);
    entity.equip(EquipmentSlot::Chest, ItemStack::new(&vanilla_items::ELYTRA));
    entity.set_on_ground(false);

    assert!(entity.try_to_start_fall_flying());
    assert!(entity.is_fall_flying());
}

#[test]
fn try_to_start_fall_flying_rejects_levitation() {
    init_test_registry();
    let entity = LivingFluidTestEntity::new(0.0, 0.0, true);
    entity.equip(EquipmentSlot::Chest, ItemStack::new(&vanilla_items::ELYTRA));
    entity.set_on_ground(false);
    entity.set_mob_effect_active(vanilla_mob_effects::LEVITATION, true);

    assert!(!entity.try_to_start_fall_flying());
    assert!(!entity.is_fall_flying());
}

#[test]
fn update_fall_flying_damages_glider_every_second_event_interval() {
    init_test_registry();
    let entity = LivingFluidTestEntity::new(0.0, 0.0, true);
    entity.equip(EquipmentSlot::Chest, ItemStack::new(&vanilla_items::ELYTRA));
    entity.set_on_ground(false);
    for _ in 0..19 {
        entity.living_base.tick_fall_flying_state(true);
    }

    entity.update_fall_flying();

    assert_eq!(
        entity
            .living_base
            .equipment()
            .lock()
            .get_ref(EquipmentSlot::Chest)
            .get_damage_value(),
        1
    );
}

#[test]
fn update_fall_flying_stops_when_glider_gate_fails() {
    init_test_registry();
    let entity = LivingFluidTestEntity::new(0.0, 0.0, true);
    entity.set_fall_flying(true);

    entity.update_fall_flying();

    assert!(!entity.is_fall_flying());
}
