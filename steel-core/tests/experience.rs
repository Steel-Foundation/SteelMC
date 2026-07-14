//! Tests for vanilla player experience state.

use steel_core::player::experience::Experience;

#[test]
fn total_point_constructor_builds_a_coherent_vanilla_state() {
    let experience = Experience::new(11);

    assert_eq!(experience.level(), 1);
    assert_eq!(experience.points(), 4);
    assert_eq!(experience.total_points(), 11);
    assert!(experience.dirty);
}

#[test]
fn coherent_level_boundaries_round_trip() {
    for level in 0..=100 {
        let total_points = Experience::total_points_at_level(level);
        let experience = Experience::new(total_points);

        assert_eq!(experience.level(), level);
        assert_eq!(experience.points(), 0);
        assert_eq!(experience.progress().to_bits(), 0.0_f32.to_bits());
    }
}

#[test]
fn vanilla_fields_are_preserved_independently() {
    let experience = Experience::from_parts(7, 0.5, 32);

    assert_eq!(experience.level(), 7);
    assert_eq!(experience.progress().to_bits(), 0.5_f32.to_bits());
    assert_eq!(experience.total_points(), 32);
    assert_eq!(experience.points(), 10);
}

#[test]
fn points_for_level_matches_vanilla_boundaries() {
    for (level, points) in [
        (0, 7),
        (1, 9),
        (14, 35),
        (15, 37),
        (16, 42),
        (29, 107),
        (30, 112),
        (31, 121),
        (50, 292),
    ] {
        assert_eq!(Experience::points_for_level(level), points);
    }
}

#[test]
fn adding_points_updates_all_vanilla_point_fields() {
    let mut experience = Experience::default();

    experience.add_points(7);
    assert_eq!(experience.level(), 1);
    assert_eq!(experience.progress().to_bits(), 0.0_f32.to_bits());
    assert_eq!(experience.total_points(), 7);

    experience.add_points(4);
    assert_eq!(experience.level(), 1);
    assert!((experience.progress() - 4.0_f32 / 9.0).abs() < f32::EPSILON);
    assert_eq!(experience.total_points(), 11);

    experience.add_points(-5);
    assert_eq!(experience.level(), 0);
    assert!((experience.progress() - 6.0_f32 / 7.0).abs() < 2.0 * f32::EPSILON);
    assert_eq!(experience.total_points(), 6);
}

#[test]
fn adding_levels_leaves_total_and_progress_independent() {
    let mut experience = Experience::from_parts(5, 0.5, 123);

    experience.add_levels(2);
    assert_eq!(experience.level(), 7);
    assert_eq!(experience.progress().to_bits(), 0.5_f32.to_bits());
    assert_eq!(experience.total_points(), 123);

    experience.add_levels(-20);
    assert_eq!(experience.level(), 0);
    assert_eq!(experience.progress().to_bits(), 0.0_f32.to_bits());
    assert_eq!(experience.total_points(), 0);
}

#[test]
fn setting_levels_changes_only_the_level() {
    let mut experience = Experience::from_parts(5, 0.5, 123);

    experience.set_levels(20);

    assert_eq!(experience.level(), 20);
    assert_eq!(experience.progress().to_bits(), 0.5_f32.to_bits());
    assert_eq!(experience.total_points(), 123);
}

#[test]
fn setting_points_changes_only_progress_and_rejects_a_full_level() {
    for level in [0, 1, 14, 15, 16, 29, 30, 31, 50] {
        let points_for_level = Experience::points_for_level(level);
        let mut experience = Experience::from_parts(level, 0.25, 500);

        assert!(experience.can_set_points(0));
        assert!(experience.can_set_points(points_for_level - 1));
        assert!(!experience.can_set_points(points_for_level));
        experience.set_points(points_for_level - 1);

        assert_eq!(experience.points(), points_for_level - 1);
        assert_eq!(experience.level(), level);
        assert_eq!(experience.total_points(), 500);
    }
}

#[test]
fn point_setter_clamps_like_vanilla_server_player() {
    let mut experience = Experience::from_parts(20, 0.25, 500);

    experience.set_points(-5);
    assert_eq!(experience.progress().to_bits(), 0.0_f32.to_bits());

    experience.set_points(100);
    assert_eq!(experience.progress().to_bits(), (61.0_f32 / 62.0).to_bits());
}

#[test]
fn point_addition_uses_java_overflow_before_clamping_total() {
    let mut experience = Experience::from_parts(0, 0.0, i32::MAX - 10);

    experience.add_points(100);

    assert_eq!(experience.total_points(), 0);

    let mut experience = Experience::default();
    experience.add_points(i32::MIN);
    assert_eq!(experience.total_points(), 0);
    assert_eq!(experience.level(), 0);
    assert_eq!(experience.progress().to_bits(), 0.0_f32.to_bits());
}

#[test]
fn clearing_experience_resets_all_three_fields() {
    let mut experience = Experience::from_parts(8, 0.25, 200);

    experience.clear();
    assert_eq!(experience.level(), 0);
    assert_eq!(experience.progress().to_bits(), 0.0_f32.to_bits());
    assert_eq!(experience.total_points(), 0);
}

#[test]
fn death_reward_uses_the_stored_level() {
    assert_eq!(Experience::from_parts(5, 0.0, 0).death_xp_reward(), 35);
    assert_eq!(Experience::from_parts(20, 0.0, 0).death_xp_reward(), 100);
}
