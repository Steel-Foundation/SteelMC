use glam::DVec3;

use super::look_at_rotation;

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
