use super::*;

#[test]
fn resolved_movement_application_matches_vanilla_threshold() {
    assert!(should_apply_resolved_movement(DVec3::ZERO, DVec3::ZERO));
    assert!(should_apply_resolved_movement(
        DVec3::new(1.0, 0.0, 0.0),
        DVec3::new(1.0e-3, 0.0, 0.0)
    ));
    assert!(!should_apply_resolved_movement(
        DVec3::new(1.0, 0.0, 0.0),
        DVec3::ZERO
    ));
}

#[test]
fn move_without_physics_returns_none_when_position_commit_rejects() {
    init_vanilla_registry();
    let entity = PushableTestEntity::shared(1, DVec3::ZERO);
    entity.set_no_physics(true);
    entity.set_level_callback(Arc::new(CommitRejectingCallback {
        entity_id: entity.id(),
    }));

    let result = entity.move_without_physics(DVec3::new(1.0, 0.0, 0.0));

    assert!(result.is_none());
    assert_vec3_close(entity.position(), DVec3::ZERO);
}

#[test]
fn fall_damage_reset_clip_target_matches_vanilla_thresholds() {
    let position = DVec3::new(1.0, 2.0, 3.0);

    assert_eq!(
        fall_damage_reset_clip_target(position, DVec3::new(1.0, 0.0, 0.0), 0.0),
        None
    );
    assert_eq!(
        fall_damage_reset_clip_target(position, DVec3::new(0.999, 0.0, 0.0), 2.0),
        None
    );
    assert_eq!(
        fall_damage_reset_clip_target(position, DVec3::new(1.0, 0.0, 0.0), 2.0),
        Some(DVec3::new(2.0, 2.0, 3.0))
    );
    assert_eq!(
        fall_damage_reset_clip_target(position, DVec3::new(10.0, 0.0, 0.0), 2.0),
        Some(DVec3::new(9.0, 2.0, 3.0))
    );
}

#[test]
fn input_vector_ignores_tiny_input_like_vanilla() {
    assert_vec3_close(
        get_input_vector(DVec3::new(1.0E-4, 0.0, 0.0), 0.02, 0.0),
        DVec3::ZERO,
    );
}

#[test]
fn input_vector_normalizes_large_input_and_rotates_by_yaw() {
    assert_vec3_close(
        get_input_vector(DVec3::new(2.0, 0.0, 0.0), 0.5, 0.0),
        DVec3::new(0.5, 0.0, 0.0),
    );
    assert_vec3_close(
        get_input_vector(DVec3::new(0.0, 0.0, 1.0), 0.5, 90.0),
        DVec3::new(-0.5, 0.0, 0.0),
    );
}

#[test]
fn look_angle_matches_vanilla_view_vector_axes() {
    let entity = PushableTestEntity::shared(1, DVec3::ZERO);

    entity.set_rotation((0.0, 0.0));
    assert_vec3_close(entity.look_angle(), DVec3::new(0.0, 0.0, 1.0));

    entity.set_rotation((90.0, 0.0));
    assert_vec3_close(entity.look_angle(), DVec3::new(-1.0, 0.0, 0.0));

    entity.set_rotation((0.0, 90.0));
    assert_vec3_close(entity.look_angle(), DVec3::new(0.0, -1.0, 0.0));
}

#[test]
fn fall_flying_movement_applies_vanilla_gravity_lift_and_drag() {
    init_vanilla_registry();
    let entity = LivingFluidTestEntity::new(0.0, 0.0, true);
    entity.set_rotation((0.0, 0.0));

    assert_vec3_close(
        entity.update_fall_flying_movement(DVec3::ZERO),
        DVec3::new(
            0.0,
            -0.018 * f64::from(0.98_f32),
            0.0018 * f64::from(0.99_f32),
        ),
    );
}

#[test]
fn fall_flying_movement_converts_upward_pitch_to_lift() {
    init_vanilla_registry();
    let entity = LivingFluidTestEntity::new(0.0, 0.0, true);
    entity.set_rotation((0.0, -45.0));

    let movement = entity.update_fall_flying_movement(DVec3::new(0.0, -0.2, 0.4));

    assert!(movement.y > -0.2);
    assert!(movement.z > 0.0);
}

#[test]
fn fall_flying_collision_damage_matches_vanilla_threshold() {
    assert!(fall_flying_collision_damage(1.0, 0.8) <= 0.0);
    assert!((fall_flying_collision_damage(1.0, 0.6) - 1.0).abs() < f32::EPSILON);
}

#[test]
fn in_wall_eye_box_requires_suffocating_state_and_shape_overlap() {
    init_vanilla_registry();
    init_behaviors();
    let pos = BlockPos::ZERO;
    let level = EmptyTestLevel;
    let inside_box = WorldAabb::new(0.1, 0.5, 0.1, 0.9, 0.500_001, 0.9);
    let outside_box = WorldAabb::new(1.1, 0.5, 0.1, 1.9, 0.500_001, 0.9);

    let stone = REGISTRY.blocks.get_default_state_id(&vanilla_blocks::STONE);
    let glass = REGISTRY.blocks.get_default_state_id(&vanilla_blocks::GLASS);
    let air = REGISTRY.blocks.get_default_state_id(&vanilla_blocks::AIR);

    assert!(block_state_suffocates_eye_box(
        stone, &level, pos, inside_box
    ));
    assert!(!block_state_suffocates_eye_box(
        glass, &level, pos, inside_box
    ));
    assert!(!block_state_suffocates_eye_box(
        air, &level, pos, inside_box
    ));
    assert!(!block_state_suffocates_eye_box(
        stone,
        &level,
        pos,
        outside_box
    ));
}

#[test]
fn fall_flying_free_fall_interval_matches_vanilla_cadence() {
    assert_eq!(fall_flying_free_fall_interval(8), None);
    assert_eq!(fall_flying_free_fall_interval(9), Some(1));
    assert_eq!(fall_flying_free_fall_interval(19), Some(2));
}

#[test]
fn jump_boost_power_uses_active_effect_amplifier() {
    init_vanilla_registry();
    let entity = LivingFluidTestEntity::new(0.0, 0.0, true);

    assert!(entity.get_jump_boost_power().abs() < f32::EPSILON);

    entity.set_mob_effect(vanilla_mob_effects::JUMP_BOOST, 2);

    assert!((entity.get_jump_boost_power() - 0.3).abs() < f32::EPSILON);
}

#[test]
fn levitation_travel_uses_active_effect_amplifier() {
    init_vanilla_registry();
    let entity = LivingFluidTestEntity::new(0.0, 0.0, true);

    assert!(entity.levitation_travel_y_delta(-0.2).is_none());

    entity.set_mob_effect(vanilla_mob_effects::LEVITATION, 1);

    assert!((entity.levitation_travel_y_delta(-0.2).unwrap_or(0.0) - 0.06).abs() < f64::EPSILON);
}

#[test]
fn slow_falling_caps_effective_gravity_only_while_falling() {
    init_vanilla_registry();
    let entity = LivingFluidTestEntity::new(0.0, 0.0, true);
    entity.set_mob_effect_active(vanilla_mob_effects::SLOW_FALLING, true);
    entity.set_velocity(DVec3::new(0.0, -0.1, 0.0));

    assert!((entity.get_effective_gravity() - 0.01).abs() < f64::EPSILON);

    entity.set_velocity(DVec3::new(0.0, 0.1, 0.0));

    assert!((entity.get_effective_gravity() - entity.get_gravity()).abs() < f64::EPSILON);
}

#[test]
fn fall_distance_accumulation_clamps_like_vanilla() {
    init_vanilla_registry();
    let entity = LivingFluidTestEntity::new(0.0, 0.0, true);
    entity.set_fall_distance(2.0);
    entity.set_velocity(DVec3::new(0.0, -0.4, 0.0));

    entity.check_fall_distance_accumulation();

    assert!((entity.fall_distance() - 1.0).abs() < f64::EPSILON);

    entity.set_fall_distance(2.0);
    entity.set_velocity(DVec3::new(0.0, -0.6, 0.0));

    entity.check_fall_distance_accumulation();

    assert!((entity.fall_distance() - 2.0).abs() < f64::EPSILON);
}
