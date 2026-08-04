use super::*;

#[test]
fn jump_from_ground_uses_jump_strength_and_marks_velocity_sync() {
    init_test_registry();
    let entity = LivingFluidTestEntity::new(0.0, 0.0, true);
    let jump_strength = f64::from(vanilla_attributes::JUMP_STRENGTH.default_value as f32);

    entity.jump_from_ground();

    assert_vec3_close(entity.velocity(), DVec3::new(0.0, jump_strength, 0.0));
    assert!(entity.needs_velocity_sync());
}

#[test]
fn sprint_jump_from_ground_adds_vanilla_horizontal_impulse() {
    init_test_registry();
    let entity = LivingFluidTestEntity::new(0.0, 0.0, true);
    let jump_strength = f64::from(vanilla_attributes::JUMP_STRENGTH.default_value as f32);
    entity.set_sprinting(true);
    entity.set_rotation((0.0, 0.0));

    entity.jump_from_ground();

    assert_vec3_close(
        entity.velocity(),
        DVec3::new(0.0, jump_strength, f64::from(0.2_f32)),
    );
}

#[test]
fn living_jump_in_water_uses_fluid_jump_impulse_without_cooldown() {
    init_test_registry();
    let entity = LivingFluidTestEntity::new(0.5, 0.0, true);
    entity.set_jumping(true);

    entity.handle_living_jump();

    assert_vec3_close(entity.velocity(), DVec3::new(0.0, f64::from(0.04_f32), 0.0));
    assert_eq!(entity.no_jump_delay(), 0);
}

#[test]
fn living_jump_without_input_resets_jump_delay_like_vanilla() {
    init_test_registry();
    let entity = LivingFluidTestEntity::new(0.0, 0.0, true);
    entity.set_no_jump_delay(4);

    entity.handle_living_jump();

    assert_eq!(entity.no_jump_delay(), 0);
}

#[test]
fn living_ai_step_zeroes_tiny_player_velocity_like_vanilla() {
    init_test_registry();
    let entity = LivingFluidTestEntity::new(0.0, 0.0, true);
    entity.set_velocity(DVec3::new(0.002, 0.002, 0.002));

    entity.apply_living_velocity_thresholds();

    assert_vec3_close(entity.velocity(), DVec3::ZERO);
}

#[test]
fn living_ai_step_keeps_player_horizontal_velocity_above_combined_threshold() {
    init_test_registry();
    let entity = LivingFluidTestEntity::new(0.0, 0.0, true);
    let velocity = DVec3::new(0.002, 0.003, 0.0025);
    entity.set_velocity(velocity);

    entity.apply_living_velocity_thresholds();

    assert_vec3_close(entity.velocity(), velocity);
}

#[test]
fn default_ai_step_resets_idle_jump_delay_and_dampens_input_before_travel() {
    init_test_registry();
    let entity = LivingFluidTestEntity::new(0.0, 0.0, true);
    entity.set_no_jump_delay(2);
    entity.set_travel_input(LivingTravelInput::new(1.0, 0.5, -1.0));

    assert!(entity.default_ai_step().is_none());

    assert_eq!(entity.no_jump_delay(), 0);
    assert_eq!(
        entity.travel_input(),
        LivingTravelInput::new(0.98, 0.5, -0.98)
    );
}

#[test]
fn default_ai_step_resets_fall_distance_for_slow_falling_and_levitation() {
    init_test_registry();

    let slow_falling = LivingFluidTestEntity::new(0.0, 0.0, true);
    slow_falling.set_fall_distance(7.0);
    slow_falling.set_mob_effect_active(vanilla_mob_effects::SLOW_FALLING, true);
    slow_falling.default_ai_step();

    assert_f64_close(slow_falling.fall_distance(), 0.0);

    let levitating = LivingFluidTestEntity::new(0.0, 0.0, true);
    levitating.set_fall_distance(7.0);
    levitating.set_mob_effect_active(vanilla_mob_effects::LEVITATION, true);
    levitating.default_ai_step();

    assert_f64_close(levitating.fall_distance(), 0.0);
}

#[test]
fn default_ai_step_jumps_from_ground_and_sets_vanilla_cooldown() {
    init_test_registry();
    let entity = LivingFluidTestEntity::new(0.0, 0.0, true);
    let jump_strength = f64::from(vanilla_attributes::JUMP_STRENGTH.default_value as f32);
    entity.set_on_ground(true);
    entity.set_jumping(true);

    assert!(entity.default_ai_step().is_none());

    assert_vec3_close(entity.velocity(), DVec3::new(0.0, jump_strength, 0.0));
    assert_eq!(entity.no_jump_delay(), 10);
    assert!(entity.needs_velocity_sync());
}

#[test]
fn living_travel_fluid_predicate_matches_vanilla_hooks() {
    init_test_registry();
    let water = FluidState::source(&vanilla_fluids::WATER);

    assert!(LivingFluidTestEntity::new(0.4, 0.0, true).should_travel_in_fluid(water));
    assert!(LivingFluidTestEntity::new(0.0, 0.4, true).should_travel_in_fluid(water));
    assert!(!LivingFluidTestEntity::new(0.0, 0.0, true).should_travel_in_fluid(water));
    assert!(!LivingFluidTestEntity::new(0.4, 0.0, false).should_travel_in_fluid(water));
    assert!(
        !LivingFluidTestEntity::new(0.4, 0.0, true)
            .with_standing_on_fluid()
            .should_travel_in_fluid(water)
    );
}

#[test]
fn open_trapdoor_matches_ladder_facing_for_climbable() {
    init_test_registry();

    let trapdoor = vanilla_blocks::OAK_TRAPDOOR
        .default_state()
        .set_value(&BlockStateProperties::OPEN, true)
        .set_value(&BlockStateProperties::FACING, BlockDirection::North);
    let ladder = vanilla_blocks::LADDER
        .default_state()
        .set_value(&BlockStateProperties::FACING, BlockDirection::North);

    assert!(trapdoor_usable_as_ladder_state(trapdoor, ladder));
}

#[test]
fn closed_trapdoor_is_not_usable_as_ladder() {
    init_test_registry();

    let trapdoor = vanilla_blocks::OAK_TRAPDOOR
        .default_state()
        .set_value(&BlockStateProperties::OPEN, false)
        .set_value(&BlockStateProperties::FACING, BlockDirection::North);
    let ladder = vanilla_blocks::LADDER
        .default_state()
        .set_value(&BlockStateProperties::FACING, BlockDirection::North);

    assert!(!trapdoor_usable_as_ladder_state(trapdoor, ladder));
}

#[test]
fn trapdoor_ladder_facing_must_match() {
    init_test_registry();

    let trapdoor = vanilla_blocks::OAK_TRAPDOOR
        .default_state()
        .set_value(&BlockStateProperties::OPEN, true)
        .set_value(&BlockStateProperties::FACING, BlockDirection::North);
    let ladder = vanilla_blocks::LADDER
        .default_state()
        .set_value(&BlockStateProperties::FACING, BlockDirection::South);

    assert!(!trapdoor_usable_as_ladder_state(trapdoor, ladder));
}

#[test]
fn vertical_collision_state_update_matches_vanilla_authority_gate() {
    assert!(
        EntityVerticalMovementStateUpdate::for_move(DVec3::new(0.0, -0.1, 0.0), false)
            .refreshes_state()
    );
    assert!(EntityVerticalMovementStateUpdate::for_move(DVec3::ZERO, true).refreshes_state());
    assert!(
        !EntityVerticalMovementStateUpdate::for_move(DVec3::new(0.1, 0.0, 0.0), false)
            .refreshes_state()
    );
}

#[test]
fn push_impulse_updates_velocity_and_marks_sync() {
    let entity = PushableTestEntity::shared(1, DVec3::ZERO);

    entity.push_impulse(DVec3::new(0.1, 0.2, 0.3));

    assert_vec3_close(entity.velocity(), DVec3::new(0.1, 0.2, 0.3));
    assert!(entity.needs_velocity_sync());

    entity.clear_velocity_sync();
    entity.push_impulse(DVec3::new(f64::INFINITY, 0.0, 0.0));

    assert_vec3_close(entity.velocity(), DVec3::new(0.1, 0.2, 0.3));
    assert!(!entity.needs_velocity_sync());
}
