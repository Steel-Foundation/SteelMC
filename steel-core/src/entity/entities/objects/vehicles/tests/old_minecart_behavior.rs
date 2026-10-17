use std::sync::Weak;

use steel_registry::init_vanilla_registry;
use steel_utils::{ChunkPos, types::UpdateFlags};

use super::*;
use crate::behavior::init_behaviors;
use crate::entity::entities::objects::vehicles::MinecartEntity;
use crate::entity::{Entity, EntityEventSource};
use crate::test_support::{TestWorld, fresh_test_world, insert_ready_full_chunk};

fn assert_close(actual: f64, expected: f64) {
    assert!(
        (actual - expected).abs() <= 1.0e-12,
        "expected {expected}, got {actual}",
    );
}

fn assert_vec_close(actual: DVec3, expected: DVec3) {
    assert_close(actual.x, expected.x);
    assert_close(actual.y, expected.y);
    assert_close(actual.z, expected.z);
}

fn test_world(key: &'static str, chunk: ChunkPos) -> TestWorld {
    init_vanilla_registry();
    init_behaviors();

    let fixture = fresh_test_world(key);
    insert_ready_full_chunk(&fixture.world, chunk);
    fixture
}

fn run_flat_track_case(
    key: &'static str,
    powered: bool,
    initial_vx: f64,
    conductors: &[Direction],
) -> (DVec3, DVec3) {
    let fixture = test_world(key, ChunkPos::new(0, 0));
    let world = &fixture.world;
    let pos = BlockPos::new(8, 64, 8);
    let flags = UpdateFlags::UPDATE_NONE | UpdateFlags::UPDATE_SKIP_ON_PLACE;

    assert!(world.set_block(pos.below(), vanilla_blocks::STONE.default_state(), flags,));

    let state = vanilla_blocks::POWERED_RAIL
        .default_state()
        .set_value(RAIL_SHAPE, RailShape::EastWest)
        .set_value(POWERED, powered);
    assert!(world.set_block(pos, state, flags));

    for &direction in conductors {
        assert!(world.set_block(
            pos.relative(direction),
            vanilla_blocks::STONE.default_state(),
            flags,
        ));
    }
    assert_eq!(world.get_block_state(pos), state);

    let start = DVec3::new(8.5, 64.0625, 8.5);
    let cart = Arc::new(MinecartEntity::new(
        &vanilla_entities::MINECART,
        8_001,
        start,
        Arc::downgrade(world),
    ));
    let entity = Arc::clone(&cart).into_shared_entity();

    world
        .try_add_entity(Arc::clone(&entity))
        .expect("minecart should enter the loaded chunk");

    cart.set_old_position_to_current();
    cart.set_velocity(DVec3::new(initial_vx, 0.0, 0.0));

    OldMinecartBehavior.move_along_track(cart.as_ref(), &entity, world);

    (cart.position() - start, cart.velocity())
}

#[test]
fn known_movement_handles_nan_and_infinity_like_vanilla() {
    let behavior = OldMinecartBehavior;

    assert_eq!(
        behavior.known_movement(DVec3::new(5.0, -0.75, -7.0)),
        DVec3::new(0.4, -0.75, -0.4),
    );

    for movement in [
        DVec3::new(f64::NAN, 0.2, 0.3),
        DVec3::new(0.1, f64::NAN, 0.3),
        DVec3::new(0.1, 0.2, f64::NAN),
    ] {
        assert_eq!(behavior.known_movement(movement), DVec3::ZERO);
    }

    assert_eq!(
        behavior.known_movement(DVec3::new(f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY,)),
        DVec3::new(0.4, f64::INFINITY, -0.4),
    );
}

#[test]
fn unpowered_rail_braking_preserves_the_stop_threshold() {
    for sign in [-1.0, 1.0] {
        for (speed, expected_distance, expected_speed) in [(0.029, 0.0, 0.0), (0.03, 0.015, 0.0144)]
        {
            let (displacement, velocity) =
                run_flat_track_case("old_minecart_braking", false, sign * speed, &[]);

            assert_vec_close(displacement, DVec3::new(sign * expected_distance, 0.0, 0.0));
            assert_vec_close(velocity, DVec3::new(sign * expected_speed, 0.0, 0.0));
        }
    }
}

#[test]
fn powered_rail_accelerates_after_slowdown_and_caps_only_displacement() {
    for (initial_speed, expected_distance, expected_speed) in [
        (0.0104, 0.0104, 0.009_984),
        (0.0105, 0.0105, 0.070_08),
        (0.8, 0.4, 0.828),
    ] {
        let (displacement, velocity) = run_flat_track_case(
            "old_minecart_powered_acceleration",
            true,
            initial_speed,
            &[],
        );

        assert_vec_close(displacement, DVec3::new(expected_distance, 0.0, 0.0));
        assert_vec_close(velocity, DVec3::new(expected_speed, 0.0, 0.0));
    }
}

#[test]
fn rail_projection_handles_all_slopes_at_negative_coordinates() {
    let fixture = test_world("old_minecart_slope_projection", ChunkPos::new(-1, 0));
    let world = &fixture.world;
    let pos = BlockPos::new(-1, 64, 0);
    let flags = UpdateFlags::UPDATE_NONE | UpdateFlags::UPDATE_SKIP_ON_PLACE;

    assert!(world.set_block(pos.below(), vanilla_blocks::STONE.default_state(), flags,));

    for (shape, expected) in [
        (RailShape::AscendingEast, DVec3::new(-0.75, 64.3125, 0.5)),
        (RailShape::AscendingWest, DVec3::new(-0.75, 64.8125, 0.5)),
        (RailShape::AscendingNorth, DVec3::new(-0.5, 64.8125, 0.25)),
        (RailShape::AscendingSouth, DVec3::new(-0.5, 64.3125, 0.25)),
    ] {
        let state = vanilla_blocks::RAIL
            .default_state()
            .set_value(RAIL_SHAPE, shape);
        assert!(world.set_block(pos, state, flags));

        for input_y in [64.0, 65.0] {
            let projected = OldMinecartBehavior::get_pos(world, -0.75, input_y, 0.25, None)
                .expect("the position should project onto the rail");

            assert_vec_close(projected, expected);
        }
    }
}

#[test]
fn rotation_flip_boundaries_are_asymmetric() {
    init_vanilla_registry();

    for (initial_yaw, expected_yaw, expected_flipped) in [
        (-170.0, -170.0, false),
        (170.0, 350.0, true),
        (-170.25, 9.75, true),
        (169.75, 169.75, false),
    ] {
        let cart = MinecartEntity::new(&vanilla_entities::MINECART, 1, DVec3::ZERO, Weak::new());
        cart.set_old_position_to_current();
        cart.base().set_old_rotation((0.0, 0.0));
        cart.set_rotation((initial_yaw, 35.0));
        cart.minecart_base().set_flipped(false);

        OldMinecartBehavior::update_rotation_from_movement(&cart);

        assert_eq!(
            cart.is_flipped(),
            expected_flipped,
            "starting yaw: {initial_yaw}",
        );
        assert_eq!(cart.rotation(), (expected_yaw, 0.0));
    }
}
