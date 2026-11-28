use simdnbt::owned::NbtCompound;

use super::*;
use simdnbt::borrow::read_compound;
use std::io::Cursor;
use std::string::ToString;
use steel_registry::{vanilla_entities, vanilla_items};

fn frame_loaded_from(nbt: &NbtCompound) -> ItemFrameEntity {
    let frame = ItemFrameEntity::new_attached(
        &vanilla_entities::ITEM_FRAME,
        1,
        BlockPos::new(12, 80, 14),
        Direction::West,
        Weak::new(),
    );
    let mut bytes = Vec::new();
    nbt.write(&mut bytes);
    let borrowed =
        read_compound(&mut Cursor::new(bytes.as_slice())).expect("test NBT should reborrow");
    frame.load_additional((&borrowed).into());
    frame
}

#[test]
fn load_wraps_facing_and_keeps_rotation_sign_like_java() {
    let mut nbt = NbtCompound::new();
    nbt.insert("Facing", 7_i8);
    nbt.insert("ItemRotation", -1_i8);
    let frame = frame_loaded_from(&nbt);

    let entity_data = frame.entity_data.lock();
    assert_eq!(*entity_data.hanging_entity.direction.get(), Direction::Up);
    assert_eq!(*entity_data.rotation.get(), -1);
}

#[test]
fn load_defaults_missing_facing_to_down() {
    let frame = frame_loaded_from(&NbtCompound::new());

    let entity_data = frame.entity_data.lock();
    assert_eq!(*entity_data.hanging_entity.direction.get(), Direction::Down);
}

#[test]
fn item_frame_persists_structure_marker_state() {
    let frame = ItemFrameEntity::new_attached(
        &vanilla_entities::ITEM_FRAME,
        1,
        BlockPos::new(12, 80, 14),
        Direction::West,
        Weak::new(),
    );
    frame.set_item(ItemStack::new(&vanilla_items::ELYTRA));

    let mut nbt = NbtCompound::new();
    frame.save_additional(&mut nbt);

    assert_eq!(nbt.byte("Facing"), Some(4));
    assert_eq!(nbt.byte("ItemRotation"), Some(0));
    assert_eq!(nbt.float("ItemDropChance"), Some(1.0));
    assert_eq!(nbt.byte("Invisible"), Some(0));
    assert_eq!(nbt.byte("Fixed"), Some(0));
    let Some(item) = nbt.compound("Item") else {
        panic!("item frame should save framed item");
    };
    assert_eq!(
        item.string("id").map(ToString::to_string),
        Some("minecraft:elytra".to_owned())
    );
    assert_eq!(item.int("count"), Some(1));
}

#[test]
fn item_frame_is_pickable_like_vanilla() {
    let frame = ItemFrameEntity::new_attached(
        &vanilla_entities::ITEM_FRAME,
        1,
        BlockPos::new(12, 80, 14),
        Direction::West,
        Weak::new(),
    );

    assert!(frame.is_pickable());
}

#[test]
fn analog_output_uses_item_presence_and_rotation() {
    let frame = ItemFrameEntity::new_attached(
        &vanilla_entities::ITEM_FRAME,
        1,
        BlockPos::new(12, 80, 14),
        Direction::West,
        Weak::new(),
    );
    assert_eq!(frame.analog_output(), 0);

    frame.set_item_with_update(ItemStack::new(&vanilla_items::ELYTRA), false);
    assert_eq!(frame.analog_output(), 1);
    frame.entity_data.lock().rotation.set(7);
    assert_eq!(frame.analog_output(), 8);
}
