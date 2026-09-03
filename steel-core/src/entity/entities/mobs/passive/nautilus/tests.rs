use std::io::Cursor;
use std::sync::Weak;

use glam::DVec3;
use simdnbt::borrow::read_compound as read_borrowed_compound;
use simdnbt::owned::NbtCompound;
use steel_registry::item_stack::ItemStack;
use steel_registry::{init_vanilla_registry, vanilla_attributes, vanilla_entities, vanilla_items};

use super::*;
use crate::entity::next_entity_id;
use crate::test_support::TestPlayerBuilder;

fn nautilus() -> NautilusEntity {
    init_vanilla_registry();
    NautilusEntity::new(
        &vanilla_entities::NAUTILUS,
        next_entity_id(),
        DVec3::ZERO,
        Weak::new(),
    )
}

#[test]
fn nautilus_initializes_vanilla_health_and_speed() {
    let nautilus = nautilus();
    assert_eq!(nautilus.get_health().to_bits(), 15.0_f32.to_bits());
    let attributes = nautilus.attributes().lock();
    assert_eq!(
        attributes
            .required_value(vanilla_attributes::MAX_HEALTH)
            .to_bits(),
        15.0_f64.to_bits()
    );
    assert_eq!(
        attributes
            .required_value(vanilla_attributes::MOVEMENT_SPEED)
            .to_bits(),
        1.0_f64.to_bits()
    );
}

#[test]
fn nautilus_exposes_animal_and_tamable_behavior() {
    let nautilus = nautilus();
    let entity = &nautilus as &dyn Entity;
    assert!(entity.is_animal());
    assert!(entity.is_tamable_animal());
    assert!(!nautilus.is_tame());
}

#[test]
fn nautilus_food_and_taming_items_match_extracted_tags() {
    init_vanilla_registry();
    let pufferfish = ItemStack::new(&vanilla_items::PUFFERFISH);
    let cod = ItemStack::new(&vanilla_items::COD);
    let carrot = ItemStack::new(&vanilla_items::CARROT);

    assert!(NautilusEntity::is_food(&pufferfish));
    assert!(NautilusEntity::is_taming_item(&pufferfish));
    assert!(NautilusEntity::is_food(&cod));
    assert!(!NautilusEntity::is_taming_item(&cod));
    assert!(!NautilusEntity::is_food(&carrot));
}

#[test]
fn nautilus_saddle_requires_tamed_adult() {
    let nautilus = nautilus();
    assert!(!nautilus.is_saddled());
    assert!(!LivingEntity::can_use_slot(
        &nautilus,
        EquipmentSlot::Saddle
    ));
    assert!(!LivingEntity::can_use_slot(&nautilus, EquipmentSlot::Body));

    let player = TestPlayerBuilder::new(
        crate::test_support::fresh_test_world("nautilus_saddle"),
        "Tamer",
        2,
    )
    .build();
    nautilus.tame(&player);

    assert!(LivingEntity::can_use_slot(&nautilus, EquipmentSlot::Saddle));
    assert!(LivingEntity::can_use_slot(&nautilus, EquipmentSlot::Body));
    assert!(nautilus.is_tame());
    assert!(nautilus.is_owned_by(&player));
    assert!(nautilus.is_persistence_required());

    nautilus.set_baby(true);
    assert!(!LivingEntity::can_use_slot(
        &nautilus,
        EquipmentSlot::Saddle
    ));
}

#[test]
fn nautilus_persists_owner_across_save_load() {
    let nautilus = nautilus();
    let player = TestPlayerBuilder::new(
        crate::test_support::fresh_test_world("nautilus_save"),
        "Owner",
        3,
    )
    .build();
    nautilus.tame(&player);

    let mut nbt = NbtCompound::new();
    nautilus.save_additional(&mut nbt);

    let mut bytes = Vec::new();
    nbt.write(&mut bytes);
    let borrowed = read_borrowed_compound(&mut Cursor::new(&bytes))
        .unwrap_or_else(|error| panic!("test nbt should reborrow: {error}"));

    let loaded = NautilusEntity::new(
        &vanilla_entities::NAUTILUS,
        next_entity_id(),
        DVec3::ZERO,
        Weak::new(),
    );
    loaded.load_additional((&borrowed).into());

    assert!(loaded.is_tame());
    assert_eq!(loaded.owner_uuid(), Some(player.uuid()));
}

#[test]
fn nautilus_saddled_state_reads_saddle_equipment() {
    let nautilus = nautilus();
    let player = TestPlayerBuilder::new(
        crate::test_support::fresh_test_world("nautilus_saddled"),
        "Rider",
        4,
    )
    .build();
    nautilus.tame(&player);

    nautilus.living_base.equipment().lock().set(
        EquipmentSlot::Saddle,
        ItemStack::new(&vanilla_items::SADDLE),
    );
    assert!(nautilus.is_saddled());
}

#[test]
fn nautilus_cannot_fall_in_love_until_tamed() {
    let nautilus = nautilus();
    assert!(!nautilus.can_fall_in_love());

    let player = TestPlayerBuilder::new(
        crate::test_support::fresh_test_world("nautilus_love"),
        "Breeder",
        5,
    )
    .build();
    nautilus.tame(&player);
    assert!(nautilus.can_fall_in_love());
}
