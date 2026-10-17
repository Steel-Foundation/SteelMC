use std::io::Cursor;
use std::sync::Weak;

use simdnbt::borrow::read_compound;
use steel_registry::blocks::properties::BlockStateProperties;
use steel_registry::init_vanilla_registry;
use steel_utils::{ChunkPos, types::UpdateFlags};

use super::*;
use crate::behavior::init_behaviors;
use crate::entity::entities::{MinecartEntity, PigEntity};
use crate::entity::{EntityBase, EntityEventSource, EntityFluidContact};
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

fn test_cart(id: i32, position: DVec3) -> MinecartEntity {
    init_vanilla_registry();
    MinecartEntity::new(&vanilla_entities::MINECART, id, position, Weak::new())
}

fn test_world(key: &'static str) -> TestWorld {
    init_vanilla_registry();
    init_behaviors();

    let fixture = fresh_test_world(key);
    insert_ready_full_chunk(&fixture.world, ChunkPos::new(0, 0));
    fixture
}

fn load_nbt(cart: &MinecartEntity, nbt: &NbtCompound) {
    let mut bytes = Vec::new();
    nbt.write(&mut bytes);

    let borrowed = read_compound(&mut Cursor::new(bytes.as_slice()))
        .expect("test minecart NBT should be readable");

    cart.load_abstract_minecart_data((&borrowed).into());
}

#[test]
fn natural_slowdown_handles_passengers_water_and_vertical_motion() {
    for (occupied, in_water, expected_horizontal) in [
        (false, false, 0.96),
        (false, true, 0.911_999_988_555_908_1),
        (true, false, 0.997),
        (true, true, 0.947_149_988_114_833_9),
    ] {
        let cart = Arc::new(test_cart(1, DVec3::ZERO));
        let vehicle = Arc::clone(&cart).into_shared_entity();
        let passenger: SharedEntity = Arc::new(PigEntity::new(
            &vanilla_entities::PIG,
            2,
            DVec3::ZERO,
            Weak::new(),
        ));

        if occupied {
            EntityBase::restore_passenger_relationship(&vehicle, &passenger);
        }

        cart.base()
            .set_fluid_contact(EntityFluidContact::from_parts(
                if in_water { 0.5 } else { 0.0 },
                0.0,
                false,
                false,
            ));

        let result = cart.apply_natural_slowdown(DVec3::new(1.0, 0.5, -1.0));

        assert_vec_close(
            result,
            DVec3::new(expected_horizontal, 0.0, -expected_horizontal),
        );
    }
}

#[test]
fn derailing_clamps_before_ground_slowdown_and_applies_air_drag_after_moving() {
    for (grounded, initial_velocity, expected_displacement, expected_velocity) in [
        (
            false,
            DVec3::new(0.8, 0.2, -0.8),
            DVec3::new(0.4, 0.2, -0.4),
            DVec3::new(
                0.379_999_995_231_628_4,
                0.189_999_997_615_814_2,
                -0.379_999_995_231_628_4,
            ),
        ),
        (
            true,
            DVec3::new(0.8, -0.2, -0.8),
            DVec3::new(0.2, 0.0, -0.2),
            DVec3::new(0.2, 0.0, -0.2),
        ),
    ] {
        let fixture = test_world("abstract_minecart_derailing");
        let world = &fixture.world;

        if grounded {
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
        }

        let start = DVec3::new(8.5, 64.0, 8.5);
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

        cart.set_on_ground(grounded);
        cart.set_velocity(initial_velocity);
        cart.come_off_track(&entity, world);

        assert_vec_close(cart.position() - start, expected_displacement);
        assert_vec_close(cart.velocity(), expected_velocity);
        assert_eq!(cart.on_ground(), grounded);
    }
}

#[test]
fn aligned_cart_collisions_exchange_horizontal_velocity() {
    for (yaw, distance, expected_first_x, expected_second_x) in [
        (0.0, 1.0, 0.109_999_999_254_941_93, 0.130_000_000_745_058_02),
        (
            180.0,
            1.0,
            0.109_999_999_254_941_93,
            0.130_000_000_745_058_02,
        ),
        (0.0, 2.0, 0.134_999_999_627_470_95, 0.105_000_000_372_529),
    ] {
        let first = test_cart(1, DVec3::new(0.0, 64.0, 0.0));
        let second = test_cart(2, DVec3::new(distance, 64.0, 0.0));

        first.set_rotation((yaw, 0.0));
        first.set_velocity(DVec3::new(0.3, 0.5, 0.0));
        second.set_velocity(DVec3::new(-0.1, -0.25, 0.0));

        first.minecart_push_entity(&second);

        assert_vec_close(first.velocity(), DVec3::new(expected_first_x, 0.5, 0.0));
        assert_vec_close(second.velocity(), DVec3::new(expected_second_x, -0.25, 0.0));
    }
}

#[test]
fn cart_push_distance_threshold_preserves_vanilla_float_precision() {
    for (distance, expected_impulse) in [
        (0.0099, 0.0),
        (f64::from(0.01_f32), 0.0),
        (0.009_999_999_9, 0.050_000_000_745_058_06),
        (0.01, 0.050_000_000_745_058_06),
    ] {
        let first = test_cart(1, DVec3::new(0.0, 64.0, 0.0));
        let second = test_cart(2, DVec3::new(distance, 64.0, 0.0));
        first.set_rotation((0.0, 0.0));

        first.minecart_push_entity(&second);

        assert_vec_close(first.velocity(), DVec3::new(-expected_impulse, 0.0, 0.0));
        assert_vec_close(second.velocity(), DVec3::new(expected_impulse, 0.0, 0.0));
    }
}

#[test]
fn display_settings_and_rotation_round_trip_with_vanilla_has_ticked_semantics() {
    init_vanilla_registry();

    let display = vanilla_blocks::POWERED_RAIL
        .default_state()
        .set_value(&BlockStateProperties::RAIL_SHAPE, RailShape::AscendingWest)
        .set_value(&BlockStateProperties::POWERED, true);

    for first_tick in [false, true] {
        let original = test_cart(1, DVec3::ZERO);
        original.set_custom_display_block(Some(display));
        original.set_display_offset(-7);
        original.minecart_base().set_flipped(true);
        original.set_first_tick(first_tick);

        let mut saved = NbtCompound::new();
        original.save_abstract_minecart_data(&mut saved);

        assert_eq!(saved.byte("HasTicked"), Some(i8::from(first_tick)));
        assert_eq!(saved.byte("FlippedRotation"), Some(1));
        assert_eq!(saved.int("DisplayOffset"), Some(-7));

        let loaded = test_cart(2, DVec3::ZERO);
        loaded.set_first_tick(!first_tick);
        load_nbt(&loaded, &saved);

        assert_eq!(loaded.custom_display_block(), Some(display));
        assert_eq!(loaded.display_offset(), -7);
        assert!(loaded.is_flipped());
        assert_eq!(loaded.is_first_tick(), first_tick);
    }
}

#[test]
fn loading_empty_nbt_clears_previous_settings_and_omits_default_display_fields() {
    let cart = test_cart(1, DVec3::ZERO);
    cart.set_custom_display_block(Some(vanilla_blocks::STONE.default_state()));
    cart.set_display_offset(-7);
    cart.minecart_base().set_flipped(true);
    cart.set_first_tick(true);

    load_nbt(&cart, &NbtCompound::new());

    assert_eq!(cart.custom_display_block(), None);
    assert_eq!(cart.display_offset(), 6);
    assert!(!cart.is_flipped());
    assert!(!cart.is_first_tick());

    let mut saved = NbtCompound::new();
    cart.save_abstract_minecart_data(&mut saved);

    assert!(saved.get("DisplayState").is_none());
    assert!(saved.get("DisplayOffset").is_none());
    assert_eq!(saved.byte("HasTicked"), Some(0));
}
