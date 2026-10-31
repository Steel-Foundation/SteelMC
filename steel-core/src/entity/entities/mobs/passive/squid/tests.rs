use steel_registry::{init_vanilla_registry, vanilla_entities};

use super::*;

fn test_squid() -> SquidEntity {
    init_vanilla_registry();
    SquidEntity::new(&vanilla_entities::SQUID, 1, DVec3::ZERO, Weak::new())
}

/// A nose-down squid inks behind itself, so Z comes out positive. Reaching for
/// `DVec3::rotate_x` mirrors it and the ink squirts forwards instead.
#[test]
fn rotate_vector_trails_ink_behind_a_pitched_squid() {
    let squid = test_squid();
    squid.state.lock().x_body_rot_old = 30.0;

    let rotated = squid.rotate_vector(DVec3::new(0.0, -1.0, 0.0));

    assert!(
        (rotated - DVec3::new(0.0, -0.866_025, 0.5)).length() < 1e-4,
        "ink must trail behind the squid, got {rotated:?}"
    );
}
