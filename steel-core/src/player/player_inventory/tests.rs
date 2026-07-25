use std::sync::Weak;

use simdnbt::owned::{NbtList, NbtTag};
use steel_registry::{item_stack::ItemStack, test_support::init_test_registry, vanilla_items};
use steel_utils::{Identifier, types::InteractionHand};

use crate::inventory::{container::Container, equipment::EquipmentSlot};

use super::{EquipmentSwapResult, InvalidHotbarSlot, PlayerInventory};

#[test]
fn death_drops_destroy_vanishing_items() {
    init_test_registry();
    let mut inventory = PlayerInventory::new(Weak::new());
    let mut vanishing = ItemStack::new(&vanilla_items::DIAMOND_SWORD);
    vanishing.set_enchantments(&[(Identifier::vanilla_static("vanishing_curse"), 1)], false);
    inventory.set_item(0, ItemStack::new(&vanilla_items::STONE));
    inventory.set_item(1, vanishing);

    let drops = inventory.take_death_drops();

    assert!(inventory.is_empty());
    assert_eq!(drops, [ItemStack::new(&vanilla_items::STONE)]);
}

#[test]
fn vanilla_inventory_nbt_contains_main_slots_only() {
    init_test_registry();
    let mut inventory = PlayerInventory::new(Weak::new());
    inventory.items[2] = ItemStack::new(&vanilla_items::STONE);
    inventory.equipment_mut().set(
        EquipmentSlot::Head,
        ItemStack::new(&vanilla_items::DIAMOND_HELMET),
    );

    let NbtList::Compound(items) = inventory.to_vanilla_inventory_nbt() else {
        panic!("player inventory should serialize as a compound list");
    };

    assert_eq!(items.len(), 1);
    assert_eq!(items[0].get("Slot"), Some(&NbtTag::Byte(2)));
    assert_eq!(
        items[0].string("id").map(ToString::to_string),
        Some("minecraft:stone".to_owned())
    );
}

#[test]
fn add_marks_changed_when_stack_fills_existing_slot() {
    init_test_registry();

    let mut inventory = PlayerInventory::new(Weak::new());
    inventory.items[0] = ItemStack::with_count(&vanilla_items::OAK_LOG, 63);
    let before = inventory.get_times_changed();

    let mut stack = ItemStack::new(&vanilla_items::OAK_LOG);
    assert!(inventory.add(&mut stack));

    assert!(stack.is_empty());
    assert_eq!(inventory.items[0].count(), 64);
    assert_ne!(inventory.get_times_changed(), before);
}

#[test]
fn add_to_selected_existing_slot_marks_main_hand_dirty() {
    init_test_registry();

    let mut inventory = PlayerInventory::new(Weak::new());
    inventory.items[0] = ItemStack::with_count(&vanilla_items::OAK_LOG, 63);
    inventory.drain_dirty_equipment_items();

    let mut stack = ItemStack::new(&vanilla_items::OAK_LOG);
    assert!(inventory.add(&mut stack));

    assert_eq!(
        inventory.drain_dirty_equipment_items(),
        vec![(
            EquipmentSlot::MainHand,
            ItemStack::with_count(&vanilla_items::OAK_LOG, 64)
        )]
    );
}

#[test]
fn add_to_empty_selected_slot_marks_main_hand_dirty() {
    init_test_registry();

    let mut inventory = PlayerInventory::new(Weak::new());
    inventory.drain_dirty_equipment_items();

    let mut stack = ItemStack::with_count(&vanilla_items::OAK_LOG, 3);
    assert!(inventory.add(&mut stack));

    assert_eq!(
        inventory.drain_dirty_equipment_items(),
        vec![(
            EquipmentSlot::MainHand,
            ItemStack::with_count(&vanilla_items::OAK_LOG, 3)
        )]
    );
}

#[test]
fn contains_stack_compares_components() {
    init_test_registry();

    let mut inventory = PlayerInventory::new(Weak::new());
    let mut damaged_in_inventory = ItemStack::new(&vanilla_items::DIAMOND_PICKAXE);
    damaged_in_inventory.set_damage_value(3);
    inventory.items[0] = damaged_in_inventory;

    let mut damaged_search = ItemStack::new(&vanilla_items::DIAMOND_PICKAXE);
    damaged_search.set_damage_value(3);
    let undamaged_search = ItemStack::new(&vanilla_items::DIAMOND_PICKAXE);

    assert!(inventory.contains_stack(&damaged_search));
    assert!(!inventory.contains_stack(&undamaged_search));
}

#[test]
fn filled_result_replaces_single_survival_hand_stack() {
    init_test_registry();

    let mut inventory = PlayerInventory::new(Weak::new());
    inventory.set_selected_item(ItemStack::new(&vanilla_items::WATER_BUCKET));

    let overflow = inventory.apply_filled_result(
        InteractionHand::MainHand,
        ItemStack::new(&vanilla_items::BUCKET),
        false,
        true,
    );

    assert!(overflow.is_empty());
    assert_eq!(
        inventory.get_selected_item(),
        &ItemStack::new(&vanilla_items::BUCKET)
    );
}

#[test]
fn filled_result_adds_result_for_stacked_survival_hand_stack() {
    init_test_registry();

    let mut inventory = PlayerInventory::new(Weak::new());
    inventory.set_selected_item(ItemStack::with_count(&vanilla_items::BUCKET, 2));

    let overflow = inventory.apply_filled_result(
        InteractionHand::MainHand,
        ItemStack::new(&vanilla_items::WATER_BUCKET),
        false,
        true,
    );

    assert!(overflow.is_empty());
    assert_eq!(
        inventory.get_selected_item(),
        &ItemStack::new(&vanilla_items::BUCKET)
    );
    assert_eq!(
        inventory.get_item(1),
        &ItemStack::new(&vanilla_items::WATER_BUCKET)
    );
}

#[test]
fn filled_result_creative_limited_keeps_matching_held_stack() {
    init_test_registry();

    let mut inventory = PlayerInventory::new(Weak::new());
    inventory.set_selected_item(ItemStack::new(&vanilla_items::WATER_BUCKET));

    let overflow = inventory.apply_filled_result(
        InteractionHand::MainHand,
        ItemStack::new(&vanilla_items::WATER_BUCKET),
        true,
        true,
    );

    assert!(overflow.is_empty());
    assert_eq!(
        inventory.get_selected_item(),
        &ItemStack::new(&vanilla_items::WATER_BUCKET)
    );
    assert_eq!(
        (0..PlayerInventory::INVENTORY_SIZE)
            .filter(|&slot| !inventory.items[slot].is_empty())
            .count(),
        1
    );
}

#[test]
fn filled_result_creative_limited_adds_missing_result_without_consuming_hand() {
    init_test_registry();

    let mut inventory = PlayerInventory::new(Weak::new());
    inventory.set_selected_item(ItemStack::with_count(&vanilla_items::BUCKET, 16));

    let overflow = inventory.apply_filled_result(
        InteractionHand::MainHand,
        ItemStack::new(&vanilla_items::WATER_BUCKET),
        true,
        true,
    );

    assert!(overflow.is_empty());
    assert_eq!(
        inventory.get_selected_item(),
        &ItemStack::with_count(&vanilla_items::BUCKET, 16)
    );
    assert_eq!(
        inventory.get_item(1),
        &ItemStack::new(&vanilla_items::WATER_BUCKET)
    );
}

#[test]
fn filled_result_empty_result_still_consumes_survival_hand_stack() {
    init_test_registry();

    let mut inventory = PlayerInventory::new(Weak::new());
    inventory.set_selected_item(ItemStack::new(&vanilla_items::BUCKET));

    let overflow =
        inventory.apply_filled_result(InteractionHand::MainHand, ItemStack::empty(), false, true);

    assert!(overflow.is_empty());
    assert!(inventory.get_selected_item().is_empty());
}

#[test]
fn filled_result_creative_unlimited_discards_unadded_result() {
    init_test_registry();

    let mut inventory = PlayerInventory::new(Weak::new());
    inventory.set_selected_item(ItemStack::new(&vanilla_items::LAVA_BUCKET));
    for slot in 1..PlayerInventory::INVENTORY_SIZE {
        inventory.items[slot] = ItemStack::with_count(&vanilla_items::OAK_LOG, 64);
    }

    let overflow = inventory.apply_filled_result(
        InteractionHand::MainHand,
        ItemStack::new(&vanilla_items::WATER_BUCKET),
        true,
        false,
    );

    assert!(overflow.is_empty());
    assert_eq!(
        inventory.get_selected_item(),
        &ItemStack::new(&vanilla_items::LAVA_BUCKET)
    );
}

#[test]
fn clear_content_counts_equipment_items() {
    init_test_registry();

    let mut inventory = PlayerInventory::new(Weak::new());
    inventory.items[0] = ItemStack::with_count(&vanilla_items::OAK_LOG, 3);
    inventory.equipment.set(
        EquipmentSlot::Head,
        ItemStack::new(&vanilla_items::DIAMOND_HELMET),
    );

    assert_eq!(inventory.clear_content(), 4);
    assert!(inventory.is_empty());
}

#[test]
fn non_empty_equipment_items_uses_selected_item_as_main_hand() {
    init_test_registry();

    let mut inventory = PlayerInventory::new(Weak::new());
    let main_hand = ItemStack::with_count(&vanilla_items::OAK_LOG, 2);
    let head = ItemStack::new(&vanilla_items::DIAMOND_HELMET);
    inventory.items[0] = main_hand.clone();
    inventory.equipment.set(
        EquipmentSlot::MainHand,
        ItemStack::new(&vanilla_items::STICK),
    );
    inventory.equipment.set(EquipmentSlot::Head, head.clone());

    let items = inventory.non_empty_equipment_items();

    assert_eq!(items.len(), 2);
    assert!(items.contains(&(EquipmentSlot::MainHand, main_hand)));
    assert!(items.contains(&(EquipmentSlot::Head, head)));
}

#[test]
fn selected_slot_change_drains_main_hand_equipment_update_once() {
    init_test_registry();

    let mut inventory = PlayerInventory::new(Weak::new());
    let selected = ItemStack::new(&vanilla_items::OAK_LOG);
    inventory.items[1] = selected.clone();

    inventory.set_selected_slot(1);
    let dirty_items = inventory.drain_dirty_equipment_items();

    assert_eq!(dirty_items, vec![(EquipmentSlot::MainHand, selected)]);
    assert!(inventory.drain_dirty_equipment_items().is_empty());
}

#[test]
fn packet_selected_slot_rejects_invalid_values_without_wrapping() {
    let mut inventory = PlayerInventory::new(Weak::new());

    assert!(inventory.try_set_selected_slot_from_packet(8).is_ok());
    assert_eq!(inventory.get_selected_slot(), 8);

    assert_eq!(
        inventory.try_set_selected_slot_from_packet(9),
        Err(InvalidHotbarSlot)
    );
    assert_eq!(inventory.get_selected_slot(), 8);

    assert_eq!(
        inventory.try_set_selected_slot_from_packet(-1),
        Err(InvalidHotbarSlot)
    );
    assert_eq!(inventory.get_selected_slot(), 8);

    assert_eq!(
        inventory.try_set_selected_slot_from_packet(256),
        Err(InvalidHotbarSlot)
    );
    assert_eq!(inventory.get_selected_slot(), 8);
}

#[test]
fn shrink_item_in_hand_marks_changed_and_dirty_equipment() {
    init_test_registry();

    let mut inventory = PlayerInventory::new(Weak::new());
    inventory.set_selected_item(ItemStack::with_count(&vanilla_items::OAK_LOG, 3));
    inventory.set_offhand_item(ItemStack::with_count(&vanilla_items::SHIELD, 2));
    inventory.drain_dirty_equipment_items();

    let before = inventory.get_times_changed();
    inventory.shrink_item_in_hand(InteractionHand::MainHand, 1);

    assert_eq!(inventory.get_selected_item().count(), 2);
    assert_ne!(inventory.get_times_changed(), before);
    assert_eq!(
        inventory.drain_dirty_equipment_items(),
        vec![(
            EquipmentSlot::MainHand,
            ItemStack::with_count(&vanilla_items::OAK_LOG, 2)
        )]
    );

    let before = inventory.get_times_changed();
    inventory.shrink_item_in_hand(InteractionHand::OffHand, 1);

    assert_eq!(inventory.get_offhand_item().count(), 1);
    assert_ne!(inventory.get_times_changed(), before);
    assert_eq!(
        inventory.drain_dirty_equipment_items(),
        vec![(
            EquipmentSlot::OffHand,
            ItemStack::with_count(&vanilla_items::SHIELD, 1)
        )]
    );
}

#[test]
fn split_item_in_hand_marks_changed_and_dirty_equipment() {
    init_test_registry();

    let mut inventory = PlayerInventory::new(Weak::new());
    inventory.set_selected_item(ItemStack::with_count(&vanilla_items::OAK_LOG, 3));
    inventory.drain_dirty_equipment_items();

    let before = inventory.get_times_changed();
    let split = inventory.split_item_in_hand(InteractionHand::MainHand, 1);

    assert_eq!(split, ItemStack::with_count(&vanilla_items::OAK_LOG, 1));
    assert_eq!(inventory.get_selected_item().count(), 2);
    assert_ne!(inventory.get_times_changed(), before);
    assert_eq!(
        inventory.drain_dirty_equipment_items(),
        vec![(
            EquipmentSlot::MainHand,
            ItemStack::with_count(&vanilla_items::OAK_LOG, 2)
        )]
    );
}

#[test]
fn hurt_item_in_hand_marks_changed_and_dirty_equipment() {
    init_test_registry();

    let mut inventory = PlayerInventory::new(Weak::new());
    inventory.set_selected_item(ItemStack::new(&vanilla_items::SHEARS));
    inventory.drain_dirty_equipment_items();

    let before = inventory.get_times_changed();
    inventory.hurt_item_in_hand(InteractionHand::MainHand, 1, false);

    let main_hand = inventory.get_selected_item();
    assert!(main_hand.is(&vanilla_items::SHEARS));
    assert_eq!(main_hand.get_damage_value(), 1);
    let expected = main_hand.copy_with_count(1);
    assert_ne!(inventory.get_times_changed(), before);
    assert_eq!(
        inventory.drain_dirty_equipment_items(),
        vec![(EquipmentSlot::MainHand, expected)]
    );
}

#[test]
fn hurt_and_convert_item_in_hand_damages_without_breaking() {
    init_test_registry();

    let mut inventory = PlayerInventory::new(Weak::new());
    inventory.set_offhand_item(ItemStack::new(&vanilla_items::CARROT_ON_A_STICK));
    inventory.drain_dirty_equipment_items();

    let before = inventory.get_times_changed();
    inventory.hurt_and_convert_item_in_hand_on_break(
        InteractionHand::OffHand,
        1,
        &vanilla_items::FISHING_ROD,
        false,
    );

    let offhand = inventory.get_offhand_item();
    assert!(offhand.is(&vanilla_items::CARROT_ON_A_STICK));
    assert_eq!(offhand.get_damage_value(), 1);
    let expected = offhand.copy_with_count(1);
    assert_ne!(inventory.get_times_changed(), before);
    assert_eq!(
        inventory.drain_dirty_equipment_items(),
        vec![(EquipmentSlot::OffHand, expected)]
    );
}

#[test]
fn hurt_and_convert_item_in_hand_replaces_broken_item() {
    init_test_registry();

    let mut inventory = PlayerInventory::new(Weak::new());
    inventory.set_selected_item(ItemStack::new(&vanilla_items::CARROT_ON_A_STICK));
    let max_damage = inventory.get_selected_item().get_max_damage();
    inventory
        .get_selected_item_mut()
        .set_damage_value(max_damage - 1);
    inventory.drain_dirty_equipment_items();

    let before = inventory.get_times_changed();
    inventory.hurt_and_convert_item_in_hand_on_break(
        InteractionHand::MainHand,
        7,
        &vanilla_items::FISHING_ROD,
        false,
    );

    let main_hand = inventory.get_selected_item();
    assert!(main_hand.is(&vanilla_items::FISHING_ROD));
    assert_eq!(main_hand.count(), 1);
    assert_eq!(main_hand.get_damage_value(), 0);
    let expected = main_hand.copy_with_count(1);
    assert_ne!(inventory.get_times_changed(), before);
    assert_eq!(
        inventory.drain_dirty_equipment_items(),
        vec![(EquipmentSlot::MainHand, expected)]
    );
}

#[test]
fn swap_hands_swaps_selected_and_offhand() {
    init_test_registry();

    let mut inventory = PlayerInventory::new(Weak::new());
    let main_hand = ItemStack::with_count(&vanilla_items::OAK_LOG, 3);
    let offhand = ItemStack::new(&vanilla_items::SHIELD);
    inventory.set_selected_item(main_hand.clone());
    inventory.set_offhand_item(offhand.clone());
    inventory.drain_dirty_equipment_items();

    assert!(inventory.swap_hands());

    assert_eq!(inventory.get_selected_item(), &offhand);
    assert_eq!(inventory.get_offhand_item(), &main_hand);
    let dirty_items = inventory.drain_dirty_equipment_items();
    assert!(dirty_items.contains(&(EquipmentSlot::MainHand, offhand)));
    assert!(dirty_items.contains(&(EquipmentSlot::OffHand, main_hand)));
}

#[test]
fn equippable_single_item_moves_to_empty_armor_slot() {
    init_test_registry();

    let mut inventory = PlayerInventory::new(Weak::new());
    inventory.set_selected_item(ItemStack::new(&vanilla_items::DIAMOND_HELMET));

    let result = inventory.try_swap_with_equipment_slot(
        InteractionHand::MainHand,
        EquipmentSlot::Head,
        false,
    );

    assert_eq!(result, EquipmentSwapResult::Success(ItemStack::empty()));
    assert!(inventory.get_selected_item().is_empty());
    assert_eq!(
        inventory.equipment().get_ref(EquipmentSlot::Head),
        &ItemStack::new(&vanilla_items::DIAMOND_HELMET)
    );
}

#[test]
fn equippable_swap_respects_prevent_armor_change_effect() {
    init_test_registry();

    let mut inventory = PlayerInventory::new(Weak::new());
    let mut bound_helmet = ItemStack::new(&vanilla_items::DIAMOND_HELMET);
    bound_helmet.set_enchantments(&[(Identifier::vanilla_static("binding_curse"), 1)], false);
    inventory.set_selected_item(ItemStack::new(&vanilla_items::CARVED_PUMPKIN));
    inventory
        .equipment_mut()
        .set(EquipmentSlot::Head, bound_helmet.copy_with_count(1));

    let result = inventory.try_swap_with_equipment_slot(
        InteractionHand::MainHand,
        EquipmentSlot::Head,
        false,
    );

    assert_eq!(result, EquipmentSwapResult::Fail);
    assert_eq!(
        inventory.get_selected_item(),
        &ItemStack::new(&vanilla_items::CARVED_PUMPKIN)
    );
    assert_eq!(
        inventory.equipment().get_ref(EquipmentSlot::Head),
        &bound_helmet
    );
}

#[test]
fn repair_with_xp_repairs_damaged_mending_item() {
    init_test_registry();

    let mut inventory = PlayerInventory::new(Weak::new());
    let mut pickaxe = ItemStack::new(&vanilla_items::DIAMOND_PICKAXE);
    pickaxe.set_damage_value(10);
    pickaxe.set_enchantments(&[(Identifier::vanilla_static("mending"), 1)], false);
    inventory.set_selected_item(pickaxe);
    inventory.drain_dirty_equipment_items();
    let before = inventory.get_times_changed();

    let remaining = inventory.repair_random_equipped_item_with_xp(3);

    assert_eq!(remaining, 0);
    assert_eq!(inventory.get_selected_item().get_damage_value(), 4);
    assert_ne!(inventory.get_times_changed(), before);
    assert_eq!(
        inventory.drain_dirty_equipment_items(),
        vec![(
            EquipmentSlot::MainHand,
            inventory.get_selected_item().copy_with_count(1)
        )]
    );
}

#[test]
fn repair_with_xp_returns_leftover_when_item_is_fully_repaired() {
    init_test_registry();

    let mut inventory = PlayerInventory::new(Weak::new());
    let mut pickaxe = ItemStack::new(&vanilla_items::DIAMOND_PICKAXE);
    pickaxe.set_damage_value(3);
    pickaxe.set_enchantments(&[(Identifier::vanilla_static("mending"), 1)], false);
    inventory.set_selected_item(pickaxe);

    let remaining = inventory.repair_random_equipped_item_with_xp(5);

    assert_eq!(remaining, 4);
    assert_eq!(inventory.get_selected_item().get_damage_value(), 0);
}

#[test]
fn equippable_stack_moves_one_item_and_returns_old_equipment_to_inventory() {
    init_test_registry();

    let mut inventory = PlayerInventory::new(Weak::new());
    inventory.set_selected_item(ItemStack::with_count(&vanilla_items::CARVED_PUMPKIN, 2));
    inventory.equipment_mut().set(
        EquipmentSlot::Head,
        ItemStack::new(&vanilla_items::DIAMOND_HELMET),
    );

    let result = inventory.try_swap_with_equipment_slot(
        InteractionHand::MainHand,
        EquipmentSlot::Head,
        false,
    );

    assert_eq!(result, EquipmentSwapResult::Success(ItemStack::empty()));
    assert_eq!(inventory.get_selected_item().count(), 1);
    assert_eq!(
        inventory.equipment().get_ref(EquipmentSlot::Head),
        &ItemStack::new(&vanilla_items::CARVED_PUMPKIN)
    );
    assert!(
        inventory
            .get_items()
            .iter()
            .any(|stack| stack.is(&vanilla_items::DIAMOND_HELMET))
    );
}
