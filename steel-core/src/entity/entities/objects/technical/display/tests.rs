use std::sync::{Arc, Weak};

use glam::{DVec3, Quat, Vec3};
use simdnbt::owned::NbtCompound;
use steel_registry::{init_vanilla_registry, vanilla_entities};

use super::block_display::BlockDisplayEntity;
use super::*;
use crate::entity::entities::MarkerEntity;
use crate::entity::{EntityBase, RemovalReason, SharedEntity};
use crate::test_support::load_additional;

fn block_display(id: i32) -> Arc<BlockDisplayEntity> {
    BlockDisplayEntity::new(
        &vanilla_entities::BLOCK_DISPLAY,
        id,
        DVec3::ZERO,
        Weak::new(),
    )
}

#[test]
fn display_leaves_a_removed_vehicle() {
    init_vanilla_registry();
    let display: SharedEntity = block_display(1);
    let vehicle: SharedEntity = block_display(2);
    EntityBase::restore_passenger_relationship(&vehicle, &display);
    vehicle.set_removed(RemovalReason::Discarded);

    display.tick();
    assert!(display.vehicle().is_none());
}

#[test]
fn shared_display_state_round_trips_through_nbt() {
    init_vanilla_registry();
    let entity = block_display(1);
    let display = entity.display();
    let transformation = Transformation {
        translation: Vec3::new(1.0, 2.0, 3.0),
        left_rotation: Quat::from_rotation_y(0.5),
        scale: Vec3::new(2.0, 2.0, 0.5),
        right_rotation: Quat::from_rotation_x(-0.25),
    };
    let brightness = Brightness { block: 7, sky: 12 };
    display.set_transformation(transformation);
    display.set_transformation_interpolation_duration(20);
    display.set_pos_rot_interpolation_duration(10);
    display.set_billboard_constraints(BillboardConstraints::Center);
    display.set_brightness_override(Some(brightness));
    display.set_view_range(2.5);
    display.set_shadow_radius(0.75);
    display.set_shadow_strength(0.25);
    display.set_width(3.0);
    display.set_height(4.0);
    display.set_glow_color_override(Some(0x0000_ff00));

    let mut nbt = NbtCompound::new();
    entity.save_additional(&mut nbt);
    let loaded_entity = block_display(2);
    load_additional(&*loaded_entity, &nbt);
    let loaded = loaded_entity.display();

    assert_eq!(loaded.transformation(), transformation);
    assert_eq!(loaded.transformation_interpolation_duration(), 20);
    assert_eq!(loaded.pos_rot_interpolation_duration(), 10);
    assert_eq!(loaded.billboard_constraints(), BillboardConstraints::Center);
    assert_eq!(loaded.brightness_override(), Some(brightness));
    assert_eq!(loaded.view_range().to_bits(), 2.5_f32.to_bits());
    assert_eq!(loaded.shadow_radius().to_bits(), 0.75_f32.to_bits());
    assert_eq!(loaded.shadow_strength().to_bits(), 0.25_f32.to_bits());
    assert_eq!(loaded.width().to_bits(), 3.0_f32.to_bits());
    assert_eq!(loaded.height().to_bits(), 4.0_f32.to_bits());
    assert_eq!(loaded.glow_color_override(), Some(0x0000_ff00));
}

#[test]
fn loading_clamps_the_teleport_duration_and_drops_invalid_brightness() {
    init_vanilla_registry();
    let entity = block_display(1);
    let display = entity.display();
    display.set_brightness_override(Some(Brightness { block: 1, sky: 1 }));

    let mut brightness = NbtCompound::new();
    brightness.insert("block", 16);
    brightness.insert("sky", 0);
    let mut nbt = NbtCompound::new();
    nbt.insert("teleport_duration", 100);
    nbt.insert("brightness", brightness);
    load_additional(&*entity, &nbt);

    assert_eq!(display.pos_rot_interpolation_duration(), 59);
    assert_eq!(display.brightness_override(), None);
    assert_eq!(display.glow_color_override(), None);
}

#[test]
fn only_display_entities_are_seen_as_displays() {
    init_vanilla_registry();
    let display: SharedEntity = block_display(1);
    let marker: SharedEntity =
        MarkerEntity::new(&vanilla_entities::MARKER, 2, DVec3::ZERO, Weak::new());

    assert!(Display::of(&*display).is_some());
    assert!(Display::of(&*marker).is_none());
}
