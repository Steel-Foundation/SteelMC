use glam::DVec3;

use super::{landing_particle_count, landing_particle_position, look_at_rotation};
use steel_utils::BlockPos;

#[test]
fn look_at_rotation_matches_vanilla_axes() {
    assert_eq!(
        look_at_rotation(DVec3::ZERO, DVec3::new(0.0, 0.0, 1.0)),
        (0.0, 0.0)
    );
    assert_eq!(
        look_at_rotation(DVec3::ZERO, DVec3::new(1.0, 0.0, 0.0)),
        (-90.0, 0.0)
    );
    assert_eq!(
        look_at_rotation(DVec3::ZERO, DVec3::new(0.0, 1.0, 1.0)),
        (0.0, -45.0)
    );
    assert_eq!(
        look_at_rotation(DVec3::ZERO, DVec3::new(-1.0, 0.0, -1.0)),
        (135.0, 0.0)
    );
}

#[test]
fn landing_particles_use_vanilla_fall_power_threshold_and_cap() {
    assert_eq!(landing_particle_count(3.0, 3.0), None);
    assert_eq!(landing_particle_count(4.0, 3.0), Some(40));
    assert_eq!(landing_particle_count(38.0, 3.0), Some(375));
}

#[test]
fn landing_particles_shift_to_the_landed_on_block_edge() {
    let position = landing_particle_position(DVec3::new(1.25, 5.0, 0.75), BlockPos::new(0, 4, 0));
    assert_eq!(position.y, 5.0);
    assert!((position.x - 1.0).abs() < f64::EPSILON);
    assert!((position.z - 2.0 / 3.0).abs() < f64::EPSILON);
}
