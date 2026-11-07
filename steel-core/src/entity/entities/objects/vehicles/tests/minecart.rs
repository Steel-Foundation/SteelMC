use steel_utils::ChunkPos;

use super::{AbstractMinecart, VehicleEntity};
use crate::entity::Entity;
use crate::entity::entities::objects::vehicles::test_support::{spawned_minecart, test_world};

#[test]
fn powered_activation_starts_the_hurt_animation_without_destroying_the_cart() {
    let fixture = test_world("minecart_powered_activation", ChunkPos::new(0, 0));
    let world = &fixture.world;
    let cart = spawned_minecart(world);

    cart.set_hurt_time(0);
    cart.set_hurt_dir(1);
    cart.set_damage(7.0);
    cart.clear_hurt_mark();

    cart.activate_minecart(world, cart.block_position(), true);

    assert_eq!(
        (cart.hurt_time(), cart.hurt_dir(), cart.damage()),
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

    cart.set_hurt_time(3);
    cart.set_hurt_dir(1);
    cart.set_damage(7.0);
    cart.clear_hurt_mark();

    cart.activate_minecart(world, cart.block_position(), true);

    assert_eq!(
        (cart.hurt_time(), cart.hurt_dir(), cart.damage()),
        (3, 1, 7.0),
    );
    assert!(!cart.hurt_marked());
    assert!(!cart.is_removed());
}
