use steel_registry::init_vanilla_registry;

use super::*;
use crate::entity::EntityFluidContact;
use std::sync::Weak;

fn squid() -> SquidEntity {
    init_vanilla_registry();
    SquidEntity::new(&vanilla_entities::SQUID, 1, DVec3::ZERO, Weak::new())
}

/// A nose-down squid inks behind itself, so Z comes out positive. Reaching for
/// `DVec3::rotate_x` mirrors it and the ink squirts forwards instead.
#[test]
fn rotate_vector_trails_ink_behind_a_pitched_squid() {
    let squid = squid();
    squid.swimming.lock().x_body_rot_o = 30.0;

    let rotated = squid.rotate_vector(DVec3::new(0.0, -1.0, 0.0));

    assert!(
        (rotated - DVec3::new(0.0, -0.866_025, 0.5)).length() < 1e-4,
        "ink must trail behind the squid, got {rotated:?}"
    );
}

#[test]
fn late_in_a_stroke_the_squid_swims_along_its_movement_vector() {
    let squid = squid();
    squid
        .base()
        .set_fluid_contact(EntityFluidContact::from_parts(1.0, 0.0, false, false));
    let movement_vector = DVec3::new(0.1, 0.0, -0.1);
    squid.set_movement_vector(movement_vector);
    {
        let mut swimming = squid.swimming.lock();
        swimming.tentacle_speed = 0.0;
        swimming.tentacle_movement = PI * 0.8;
    }

    squid.swim();

    assert_eq!(squid.velocity(), movement_vector);
}
