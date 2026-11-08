use steel_utils::ChunkPos;

use super::abstract_minecart::Parts as _;
use super::vehicle_entity::Parts as _;
use crate::entity::entities::objects::vehicles::test_support::{spawned_minecart, test_world};
use crate::entity::prelude::*;

#[test]
fn powered_activation_starts_the_hurt_animation_without_destroying_the_cart() {
    let fixture = test_world("minecart_powered_activation", ChunkPos::new(0, 0));
    let world = &fixture.world;
    let cart = spawned_minecart(world);

    cart.vehicle_entity().set_hurt_time(0);
    cart.vehicle_entity().set_hurt_dir(1);
    cart.vehicle_entity().set_damage(7.0);
    cart.clear_hurt_mark();

    cart.abstract_minecart()
        .activate_minecart(world, cart.block_position(), true);

    assert_eq!(
        (
            cart.vehicle_entity().hurt_time(),
            cart.vehicle_entity().hurt_dir(),
            cart.vehicle_entity().damage()
        ),
        (10, -1, 50.0),
    );
    assert!(cart.hurt_marked());
    assert!(!cart.is_removed());
}

#[test]
fn powered_activation_preserves_an_existing_hurt_animation() {
    let fixture = test_world("minecart_existing_hurt_animation", ChunkPos::new(0, 0));
    let world = &fixture.world;
    let cart = spawned_minecart(world);

    cart.vehicle_entity().set_hurt_time(3);
    cart.vehicle_entity().set_hurt_dir(1);
    cart.vehicle_entity().set_damage(7.0);
    cart.clear_hurt_mark();

    cart.abstract_minecart()
        .activate_minecart(world, cart.block_position(), true);

    assert_eq!(
        (
            cart.vehicle_entity().hurt_time(),
            cart.vehicle_entity().hurt_dir(),
            cart.vehicle_entity().damage()
        ),
        (3, 1, 7.0),
    );
    assert!(!cart.hurt_marked());
    assert!(!cart.is_removed());
}
