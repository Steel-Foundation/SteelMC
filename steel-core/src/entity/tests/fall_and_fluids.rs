use super::*;

#[test]
fn fall_damage_sound_selects_vanilla_small_and_big_sounds() {
    init_test_registry();
    let entity = LivingFluidTestEntity::new(0.0, 0.0, true);

    assert_eq!(
        entity.fall_damage_sound(4),
        &sound_events::ENTITY_GENERIC_SMALL_FALL
    );
    assert_eq!(
        entity.fall_damage_sound(5),
        &sound_events::ENTITY_GENERIC_BIG_FALL
    );
}

#[test]
fn living_fall_damage_uses_shared_damage_path_from_entity_dispatch() {
    init_test_registry();
    let entity = LivingFluidTestEntity::new_in_world(0.0, 0.0, true, test_world())
        .with_entity_type(&vanilla_entities::PIG);

    assert!(entity.cause_fall_damage(
        8.0,
        1.0,
        &DamageSource::environment(&vanilla_damage_types::FALL),
    ));

    assert_f32_close(entity.get_health(), 15.0);
}

#[test]
fn living_fall_damage_caps_distance_from_current_impulse() {
    init_test_registry();
    let entity = LivingFluidTestEntity::new_in_world(0.0, 0.0, true, test_world());

    entity.set_ignore_fall_damage_from_current_impulse(true, DVec3::new(0.0, 4.0, 0.0));

    assert!(entity.cause_fall_damage(
        8.0,
        1.0,
        &DamageSource::environment(&vanilla_damage_types::FALL),
    ));

    assert_f32_close(entity.get_health(), 19.0);
    assert!(!entity.is_ignoring_fall_damage_from_current_impulse());
}

#[test]
fn living_fall_damage_resets_current_impulse_when_landing_above_impact() {
    init_test_registry();
    let entity = LivingFluidTestEntity::new(0.0, 0.0, true);

    entity.set_ignore_fall_damage_from_current_impulse(true, DVec3::new(0.0, -1.0, 0.0));

    assert!(!entity.cause_fall_damage(
        8.0,
        1.0,
        &DamageSource::environment(&vanilla_damage_types::FALL),
    ));

    assert_f32_close(entity.get_health(), 20.0);
    assert!(!entity.is_ignoring_fall_damage_from_current_impulse());
}

#[test]
fn stop_fall_flying_toggles_shared_state_back_to_false() {
    init_test_registry();
    let entity = LivingFluidTestEntity::new(0.0, 0.0, true);
    entity.set_fall_flying(true);

    entity.stop_fall_flying();

    assert!(!entity.is_fall_flying());
}

#[test]
fn fluid_falling_adjustment_matches_vanilla_special_falling_case() {
    init_test_registry();
    let entity = LivingFluidTestEntity::new(0.0, 0.0, true);

    let movement =
        entity.get_fluid_falling_adjusted_movement(0.16, true, DVec3::new(1.0, 0.01, 1.0));

    assert_vec3_close(movement, DVec3::new(1.0, -0.003, 1.0));
}

#[test]
fn fluid_falling_adjustment_is_skipped_while_sprinting() {
    init_test_registry();
    let entity = LivingFluidTestEntity::new(0.0, 0.0, true);
    entity.set_sprinting(true);

    let movement =
        entity.get_fluid_falling_adjusted_movement(0.16, true, DVec3::new(1.0, 0.01, 1.0));

    assert_vec3_close(movement, DVec3::new(1.0, 0.01, 1.0));
}

#[test]
fn water_float_while_ridden_uses_vanilla_entity_type_tag_and_threshold() {
    init_test_registry();
    let entity = LivingFluidTestEntity::new(0.5, 0.0, true)
        .with_entity_type(&vanilla_entities::HORSE)
        .with_vehicle();

    entity.float_in_water_while_ridden();

    assert_vec3_close(entity.velocity(), DVec3::new(0.0, f64::from(0.04_f32), 0.0));
}

#[test]
fn water_float_while_ridden_ignores_non_vehicle_tagged_entity() {
    init_test_registry();
    let entity =
        LivingFluidTestEntity::new(0.5, 0.0, true).with_entity_type(&vanilla_entities::HORSE);

    entity.float_in_water_while_ridden();

    assert_vec3_close(entity.velocity(), DVec3::ZERO);
}

#[test]
fn inside_bubble_column_pushes_up_and_resets_fall_distance() {
    init_test_registry();
    let entity = LivingFluidTestEntity::new(0.0, 0.0, true);
    entity.set_velocity(DVec3::new(0.1, 0.68, 0.2));
    entity.set_fall_distance(4.0);

    entity.on_inside_bubble_column(false);

    assert_vec3_close(entity.velocity(), DVec3::new(0.1, 0.7, 0.2));
    assert_f64_close(entity.fall_distance(), 0.0);
}

#[test]
fn inside_bubble_column_drags_down_and_resets_fall_distance() {
    init_test_registry();
    let entity = LivingFluidTestEntity::new(0.0, 0.0, true);
    entity.set_velocity(DVec3::new(0.1, -0.28, 0.2));
    entity.set_fall_distance(4.0);

    entity.on_inside_bubble_column(true);

    assert_vec3_close(entity.velocity(), DVec3::new(0.1, -0.3, 0.2));
    assert_f64_close(entity.fall_distance(), 0.0);
}

#[test]
fn above_bubble_column_uses_vanilla_stronger_velocity_limits() {
    init_test_registry();
    let entity = LivingFluidTestEntity::new(0.0, 0.0, true);
    entity.set_velocity(DVec3::new(0.1, 1.75, 0.2));
    entity.set_fall_distance(4.0);

    entity.on_above_bubble_column(false, BlockPos::ZERO);

    assert_vec3_close(entity.velocity(), DVec3::new(0.1, 1.8, 0.2));
    assert_f64_close(entity.fall_distance(), 4.0);
}

#[test]
fn above_bubble_column_drag_down_uses_vanilla_stronger_velocity_limit() {
    init_test_registry();
    let entity = LivingFluidTestEntity::new(0.0, 0.0, true);
    entity.set_velocity(DVec3::new(0.1, -0.88, 0.2));

    entity.on_above_bubble_column(true, BlockPos::ZERO);

    assert_vec3_close(entity.velocity(), DVec3::new(0.1, -0.9, 0.2));
}

#[test]
fn flying_players_ignore_bubble_column_entity_hooks() {
    init_test_registry();
    let entity = LivingFluidTestEntity::new(0.0, 0.0, true).with_flying_player();
    let velocity = DVec3::new(0.1, 0.2, 0.3);
    entity.set_velocity(velocity);
    entity.set_fall_distance(4.0);

    entity.on_inside_bubble_column(false);
    entity.on_above_bubble_column(false, BlockPos::ZERO);

    assert_vec3_close(entity.velocity(), velocity);
    assert_f64_close(entity.fall_distance(), 4.0);
}

#[test]
fn dolphins_grace_water_travel_hook_uses_active_mob_effect_state() {
    init_test_registry();
    let entity = LivingFluidTestEntity::new(0.5, 0.0, true);

    assert!(!entity.has_dolphins_grace());
    entity.set_mob_effect_active(vanilla_mob_effects::DOLPHINS_GRACE, true);
    assert!(entity.has_dolphins_grace());
}

#[test]
fn living_air_supply_decrements_while_eye_in_water() {
    init_test_registry();
    let entity = LivingFluidTestEntity::new(0.5, 0.0, true).with_eye_in_water();

    entity.set_air_supply(entity.max_air_supply());
    entity.tick_living_air_supply();

    assert_eq!(entity.air_supply(), entity.max_air_supply() - 1);
}

#[test]
fn living_air_supply_drowning_damage_resets_air() {
    init_test_registry();
    let entity =
        LivingFluidTestEntity::new_in_world(0.5, 0.0, true, test_world()).with_eye_in_water();

    entity.set_air_supply(-19);
    entity.tick_living_air_supply();

    assert_eq!(entity.air_supply(), 0);
    assert_f32_close(entity.get_health(), 18.0);
}

#[test]
fn water_breathing_refills_air_underwater() {
    init_test_registry();
    let entity = LivingFluidTestEntity::new(0.5, 0.0, true).with_eye_in_water();

    entity.set_air_supply(entity.max_air_supply() - 8);
    entity.set_mob_effect_active(vanilla_mob_effects::WATER_BREATHING, true);
    entity.tick_living_air_supply();

    assert_eq!(entity.air_supply(), entity.max_air_supply() - 4);
}

#[test]
fn breath_of_the_nautilus_prevents_drowning_without_refilling_air() {
    init_test_registry();
    let entity = LivingFluidTestEntity::new(0.5, 0.0, true).with_eye_in_water();

    entity.set_air_supply(entity.max_air_supply() - 8);
    entity.set_mob_effect_active(vanilla_mob_effects::BREATH_OF_THE_NAUTILUS, true);
    entity.tick_living_air_supply();

    assert_eq!(entity.air_supply(), entity.max_air_supply() - 8);
    assert_f32_close(entity.get_health(), 20.0);
}

#[test]
fn entity_type_can_breathe_underwater_refills_air() {
    init_test_registry();
    let entity = LivingFluidTestEntity::new(0.5, 0.0, true)
        .with_eye_in_water()
        .with_entity_type(&vanilla_entities::ZOMBIE);

    entity.set_air_supply(entity.max_air_supply() - 8);
    entity.tick_living_air_supply();

    assert_eq!(entity.air_supply(), entity.max_air_supply() - 4);
}

#[test]
fn living_air_supply_refills_out_of_water() {
    init_test_registry();
    let entity = LivingFluidTestEntity::new(0.0, 0.0, true);

    entity.set_air_supply(entity.max_air_supply() - 8);
    entity.tick_living_air_supply();

    assert_eq!(entity.air_supply(), entity.max_air_supply() - 4);
}

#[test]
fn living_base_tick_damages_entities_in_wall() {
    init_test_registry();
    let entity = LivingFluidTestEntity::new_in_world(0.0, 0.0, true, test_world())
        .with_in_wall_for_base_tick();

    entity.base_tick_living_entity();

    assert_f32_close(entity.get_health(), 19.0);
}

#[test]
fn living_environmental_damage_applies_in_wall_before_drowning() {
    init_test_registry();
    let entity = LivingFluidTestEntity::new_in_world(0.5, 0.0, true, test_world())
        .with_eye_in_water()
        .with_in_wall_for_base_tick();

    entity.set_air_supply(-19);
    entity.tick_living_environmental_damage();

    assert_eq!(
        entity.damage_type_keys(),
        vec![
            vanilla_damage_types::IN_WALL.key.clone(),
            vanilla_damage_types::DROWN.key.clone(),
        ]
    );
}

#[test]
fn living_base_tick_skips_in_wall_damage_while_sleeping() {
    init_test_registry();
    let entity = LivingFluidTestEntity::new(0.0, 0.0, true).with_in_wall_for_base_tick();
    entity.set_sleeping_pos(BlockPos::ZERO);

    entity.base_tick_living_entity();

    assert_f32_close(entity.get_health(), 20.0);
}
