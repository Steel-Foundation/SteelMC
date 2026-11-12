use std::sync::Arc;

use glam::DVec3;
use steel_registry::{vanilla_blocks, vanilla_entities};
use steel_utils::types::UpdateFlags;
use steel_utils::{BlockPos, ChunkPos};

use super::abstract_minecart::Parts as _;
use super::vehicle_entity::Parts as _;
use crate::entity::entities::PigEntity;
use crate::entity::entities::objects::vehicles::test_support::{spawned_minecart, test_world};
use crate::entity::prelude::*;
use crate::entity::{EntityBase, SharedEntity, next_entity_id};

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

#[test]
fn passenger_dismounts_to_the_right_of_the_motion_direction() {
    let fixture = test_world("minecart_dismount_location", ChunkPos::new(0, 0));
    let world = &fixture.world;
    let flags = UpdateFlags::UPDATE_NONE | UpdateFlags::UPDATE_SKIP_ON_PLACE;
    for x in 7..=9 {
        for z in 7..=9 {
            assert!(world.set_block(
                BlockPos::new(x, 63, z),
                vanilla_blocks::STONE.default_state(),
                flags,
            ));
        }
    }
    let cart = spawned_minecart(world);
    // A cart facing south moves west, so its right-hand side is north.
    cart.set_rotation((0.0, 0.0));
    let vehicle: SharedEntity = Arc::clone(&cart) as SharedEntity;
    let passenger: SharedEntity = PigEntity::new(
        &vanilla_entities::PIG,
        next_entity_id(),
        cart.position(),
        Arc::downgrade(world),
    );
    world
        .try_add_entity(Arc::clone(&passenger))
        .expect("pig should join the world");
    EntityBase::restore_passenger_relationship(&vehicle, &passenger);

    passenger.stop_riding();

    assert_eq!(passenger.position(), DVec3::new(8.5, 64.0, 7.5));
}
