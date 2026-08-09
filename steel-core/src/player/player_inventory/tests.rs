use std::{
    ptr,
    sync::{
        Arc, Barrier,
        atomic::{AtomicBool, AtomicU8, AtomicUsize, Ordering},
    },
    thread,
};

use rustc_hash::FxHashMap;
use simdnbt::owned::{NbtList, NbtTag};
use steel_protocol::{
    packet_traits::{CompressionInfo, EncodedPacket},
    packets::game::{ClickType, HashedStack, SContainerClick, SSetCreativeModeSlot},
};
use steel_registry::{
    init_vanilla_registry, item_stack::ItemStack, vanilla_entities, vanilla_items,
    vanilla_menu_types,
};
use steel_utils::{
    ChunkPos, Downcast as _, DowncastType, DowncastTypeKey, Identifier, WorldAabb,
    locks::{IntoShared as _, Shared},
    types::{GameType, InteractionHand},
};
use text_components::TextComponent;
use uuid::Uuid;

use crate::{
    entity::{Entity, LivingEntity as _, RemovalReason, entities::ItemEntity, next_entity_id},
    inventory::{
        click::{Click, ClickOutcome, DragKind, MouseButton, QuickCraft},
        container::{Container, SimpleContainer},
        equipment::{EntityEquipment, EquipmentSlot},
        lock::ContainerLockGuard,
        menu::{Menu, MenuBehavior, MenuBuilder, MenuKind, kinds::BasicKind},
    },
    player::{Player, PlayerConnection, ResetReason, connection::NetworkConnection},
    test_support::{TestPlayerBuilder, fresh_test_world, insert_ready_full_chunk, test_world},
    world::World,
};

use super::{
    EquipmentSwapResult, InvalidHotbarSlot, MenuItemDisposition, MenuRemovalStatus, PlayerInventory,
};

#[test]
fn vanilla_inventory_nbt_contains_main_slots_only() {
    init_vanilla_registry();
    let mut inventory = PlayerInventory::new();
    inventory.items[2] = ItemStack::new(&vanilla_items::STONE);
    inventory.set(
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
    init_vanilla_registry();

    let mut inventory = PlayerInventory::new();
    inventory.items[0] = ItemStack::with_count(&vanilla_items::OAK_LOG, 63);
    let before = inventory.get_times_changed();

    let mut stack = ItemStack::new(&vanilla_items::OAK_LOG);
    assert!(inventory.add(&mut stack));

    assert!(stack.is_empty());
    assert_eq!(inventory.items[0].count(), 64);
    assert_ne!(inventory.get_times_changed(), before);
}

#[test]
fn add_to_selected_existing_slot_marks_inventory_changed() {
    init_vanilla_registry();

    let mut inventory = PlayerInventory::new();
    inventory.items[0] = ItemStack::with_count(&vanilla_items::OAK_LOG, 63);
    let before = inventory.get_times_changed();

    let mut stack = ItemStack::new(&vanilla_items::OAK_LOG);
    assert!(inventory.add(&mut stack));

    assert_eq!(inventory.get_selected_item().count(), 64);
    assert_ne!(inventory.get_times_changed(), before);
}

#[test]
fn add_to_empty_selected_slot_marks_inventory_changed() {
    init_vanilla_registry();

    let mut inventory = PlayerInventory::new();
    let before = inventory.get_times_changed();

    let mut stack = ItemStack::with_count(&vanilla_items::OAK_LOG, 3);
    assert!(inventory.add(&mut stack));

    assert_eq!(inventory.get_selected_item().count(), 3);
    assert_ne!(inventory.get_times_changed(), before);
}

#[test]
fn add_merges_into_existing_offhand_stack_before_main_inventory() {
    init_vanilla_registry();

    let mut inventory = PlayerInventory::new();
    inventory.set(
        EquipmentSlot::OffHand,
        ItemStack::with_count(&vanilla_items::OAK_LOG, 63),
    );

    let mut stack = ItemStack::new(&vanilla_items::OAK_LOG);
    assert!(inventory.add(&mut stack));

    assert!(stack.is_empty());
    assert_eq!(inventory.get_ref(EquipmentSlot::OffHand).count(), 64);
    assert!(inventory.get_items().iter().all(ItemStack::is_empty));
}

#[test]
fn contains_stack_compares_components() {
    init_vanilla_registry();

    let mut inventory = PlayerInventory::new();
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
    init_vanilla_registry();

    let mut inventory = PlayerInventory::new();
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
    init_vanilla_registry();

    let mut inventory = PlayerInventory::new();
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
    init_vanilla_registry();

    let mut inventory = PlayerInventory::new();
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
    init_vanilla_registry();

    let mut inventory = PlayerInventory::new();
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
    init_vanilla_registry();

    let mut inventory = PlayerInventory::new();
    inventory.set_selected_item(ItemStack::new(&vanilla_items::BUCKET));

    let overflow =
        inventory.apply_filled_result(InteractionHand::MainHand, ItemStack::empty(), false, true);

    assert!(overflow.is_empty());
    assert!(inventory.get_selected_item().is_empty());
}

#[test]
fn filled_result_creative_unlimited_discards_unadded_result() {
    init_vanilla_registry();

    let mut inventory = PlayerInventory::new();
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
    init_vanilla_registry();

    let mut inventory = PlayerInventory::new();
    inventory.items[0] = ItemStack::with_count(&vanilla_items::OAK_LOG, 3);
    inventory.set(
        EquipmentSlot::Head,
        ItemStack::new(&vanilla_items::DIAMOND_HELMET),
    );

    assert_eq!(inventory.clear_content(), 4);
    assert!(inventory.is_empty());
}

#[test]
fn container_traversal_matches_visible_slot_indices() {
    init_vanilla_registry();

    let mut inventory = PlayerInventory::new();
    let size = inventory.get_container_size();
    for slot in 0..size {
        inventory.set_item(
            slot,
            ItemStack::with_count(&vanilla_items::OAK_LOG, slot as i32 + 1),
        );
    }

    let iterated_counts: Vec<_> = inventory.iter().map(ItemStack::count).collect();
    let indexed_counts: Vec<_> = (0..size)
        .map(|slot| inventory.get_item(slot).count())
        .collect();
    assert_eq!(iterated_counts, indexed_counts);
    assert_eq!(iterated_counts.len(), size);

    let mut mutable_count = 0;
    for (slot, item) in inventory.iter_mut().enumerate() {
        mutable_count += 1;
        item.set_count((size - slot) as i32);
    }
    assert_eq!(mutable_count, size);
    for slot in 0..size {
        assert_eq!(inventory.get_item(slot).count(), (size - slot) as i32);
    }

    let mut predicate_visits = 0;
    inventory.clear_content_matching(&mut |_| {
        predicate_visits += 1;
        false
    });
    assert_eq!(predicate_visits, size);
}

#[test]
fn equipment_trait_aliases_vanilla_container_indices() {
    let inventory = PlayerInventory::new();
    assert_eq!(inventory.items().len(), PlayerInventory::CONTAINER_SIZE);
    assert_eq!(
        inventory.get_container_size(),
        PlayerInventory::CONTAINER_SIZE
    );
    assert!(ptr::eq(
        inventory.get_ref(EquipmentSlot::MainHand),
        inventory.get_item(0)
    ));

    for (equipment_slot, container_slot) in [
        (EquipmentSlot::Feet, 36),
        (EquipmentSlot::Legs, 37),
        (EquipmentSlot::Chest, 38),
        (EquipmentSlot::Head, 39),
        (EquipmentSlot::OffHand, 40),
        (EquipmentSlot::Body, 41),
        (EquipmentSlot::Saddle, 42),
    ] {
        assert!(ptr::eq(
            inventory.get_ref(equipment_slot),
            inventory.get_item(container_slot)
        ));
    }
}

#[test]
fn mutable_container_slice_exposes_all_logical_slots() {
    init_vanilla_registry();

    let mut inventory = PlayerInventory::new();
    {
        let items = inventory.items_mut();
        items[0] = ItemStack::new(&vanilla_items::STICK);
        items[39] = ItemStack::new(&vanilla_items::DIAMOND_HELMET);
    }
    inventory.set_changed();

    assert!(
        inventory
            .get_ref(EquipmentSlot::MainHand)
            .is(&vanilla_items::STICK)
    );
    assert!(
        inventory
            .get_ref(EquipmentSlot::Head)
            .is(&vanilla_items::DIAMOND_HELMET)
    );
}

#[test]
fn main_inventory_search_does_not_use_equipment_slots() {
    init_vanilla_registry();

    let mut inventory = PlayerInventory::new();
    for slot in 0..PlayerInventory::INVENTORY_SIZE {
        inventory.items[slot] = ItemStack::with_count(&vanilla_items::OAK_LOG, 64);
    }
    inventory.set(EquipmentSlot::Head, ItemStack::new(&vanilla_items::STONE));

    assert_eq!(inventory.get_free_slot(), -1);
    assert_eq!(
        inventory.find_slot_matching_item(&ItemStack::new(&vanilla_items::STONE)),
        -1
    );
}

#[test]
fn equipment_main_hand_follows_selected_slot() {
    init_vanilla_registry();

    let mut inventory = PlayerInventory::new();
    inventory.items[0] = ItemStack::new(&vanilla_items::STICK);
    inventory.items[1] = ItemStack::new(&vanilla_items::OAK_LOG);

    assert!(
        inventory
            .get_ref(EquipmentSlot::MainHand)
            .is(&vanilla_items::STICK)
    );
    inventory.set_selected_slot(1);
    assert!(
        inventory
            .get_ref(EquipmentSlot::MainHand)
            .is(&vanilla_items::OAK_LOG)
    );
    assert!(inventory.items[0].is(&vanilla_items::STICK));
}

#[test]
fn non_empty_equipment_items_uses_selected_item_as_main_hand() {
    init_vanilla_registry();

    let mut inventory = PlayerInventory::new();
    let main_hand = ItemStack::with_count(&vanilla_items::OAK_LOG, 2);
    let head = ItemStack::new(&vanilla_items::DIAMOND_HELMET);
    inventory.items[0] = main_hand.clone();
    inventory.set(EquipmentSlot::Head, head.clone());

    let items = inventory.non_empty_items();

    assert_eq!(items.len(), 2);
    assert!(items.contains(&(EquipmentSlot::MainHand, main_hand)));
    assert!(items.contains(&(EquipmentSlot::Head, head)));
}

#[test]
fn packet_selected_slot_rejects_invalid_values_without_wrapping() {
    let mut inventory = PlayerInventory::new();

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
fn shrink_item_in_hand_marks_inventory_changed() {
    init_vanilla_registry();

    let mut inventory = PlayerInventory::new();
    inventory.set_selected_item(ItemStack::with_count(&vanilla_items::OAK_LOG, 3));
    inventory.set_offhand_item(ItemStack::with_count(&vanilla_items::SHIELD, 2));

    let before = inventory.get_times_changed();
    inventory.shrink_item_in_hand(InteractionHand::MainHand, 1);

    assert_eq!(inventory.get_selected_item().count(), 2);
    assert_ne!(inventory.get_times_changed(), before);

    let before = inventory.get_times_changed();
    inventory.shrink_item_in_hand(InteractionHand::OffHand, 1);

    assert_eq!(inventory.get_offhand_item().count(), 1);
    assert_ne!(inventory.get_times_changed(), before);
}

#[test]
fn split_item_in_hand_marks_inventory_changed() {
    init_vanilla_registry();

    let mut inventory = PlayerInventory::new();
    inventory.set_selected_item(ItemStack::with_count(&vanilla_items::OAK_LOG, 3));

    let before = inventory.get_times_changed();
    let split = inventory.split_item_in_hand(InteractionHand::MainHand, 1);

    assert_eq!(split, ItemStack::with_count(&vanilla_items::OAK_LOG, 1));
    assert_eq!(inventory.get_selected_item().count(), 2);
    assert_ne!(inventory.get_times_changed(), before);
}

#[test]
fn mutating_only_held_item_components_marks_inventory_changed() {
    init_vanilla_registry();

    let mut inventory = PlayerInventory::new();
    inventory.set_selected_item(ItemStack::new(&vanilla_items::DIAMOND_SWORD));

    let before = inventory.get_times_changed();
    inventory.mutate_item_in_hand(InteractionHand::MainHand, |stack| {
        stack.set_enchantments(&[(Identifier::vanilla_static("sharpness"), 1)], false);
    });

    assert_ne!(inventory.get_times_changed(), before);
    assert_eq!(
        inventory
            .get_selected_item()
            .get_enchantment_level(&Identifier::vanilla_static("sharpness")),
        1
    );
}

#[test]
fn hurt_item_in_hand_marks_inventory_changed() {
    init_vanilla_registry();

    let mut inventory = PlayerInventory::new();
    inventory.set_selected_item(ItemStack::new(&vanilla_items::SHEARS));

    let before = inventory.get_times_changed();
    inventory.hurt_item_in_hand(InteractionHand::MainHand, 1, false);

    let main_hand = inventory.get_selected_item();
    assert!(main_hand.is(&vanilla_items::SHEARS));
    assert_eq!(main_hand.get_damage_value(), 1);
    assert_ne!(inventory.get_times_changed(), before);
}

#[test]
fn hurt_and_convert_item_in_hand_damages_without_breaking() {
    init_vanilla_registry();

    let mut inventory = PlayerInventory::new();
    inventory.set_offhand_item(ItemStack::new(&vanilla_items::CARROT_ON_A_STICK));

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
    assert_ne!(inventory.get_times_changed(), before);
}

#[test]
fn hurt_and_convert_item_in_hand_replaces_broken_item() {
    init_vanilla_registry();

    let mut inventory = PlayerInventory::new();
    inventory.set_selected_item(ItemStack::new(&vanilla_items::CARROT_ON_A_STICK));
    let max_damage = inventory.get_selected_item().get_max_damage();
    inventory
        .get_selected_item_mut()
        .set_damage_value(max_damage - 1);

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
    assert_ne!(inventory.get_times_changed(), before);
}

#[test]
fn swap_hands_swaps_selected_and_offhand() {
    init_vanilla_registry();

    let mut inventory = PlayerInventory::new();
    let main_hand = ItemStack::with_count(&vanilla_items::OAK_LOG, 3);
    let offhand = ItemStack::new(&vanilla_items::SHIELD);
    inventory.set_selected_item(main_hand.clone());
    inventory.set_offhand_item(offhand.clone());

    assert!(inventory.swap_hands());

    assert_eq!(inventory.get_selected_item(), &offhand);
    assert_eq!(inventory.get_offhand_item(), &main_hand);
}

#[test]
fn equippable_single_item_moves_to_empty_armor_slot() {
    init_vanilla_registry();

    let mut inventory = PlayerInventory::new();
    inventory.set_selected_item(ItemStack::new(&vanilla_items::DIAMOND_HELMET));

    let result = inventory.try_swap_with_equipment_slot(
        InteractionHand::MainHand,
        EquipmentSlot::Head,
        false,
    );

    assert_eq!(result, EquipmentSwapResult::Success(ItemStack::empty()));
    assert!(inventory.get_selected_item().is_empty());
    assert_eq!(
        inventory.get_ref(EquipmentSlot::Head),
        &ItemStack::new(&vanilla_items::DIAMOND_HELMET)
    );
}

#[test]
fn equippable_swap_respects_prevent_armor_change_effect() {
    init_vanilla_registry();

    let mut inventory = PlayerInventory::new();
    let mut bound_helmet = ItemStack::new(&vanilla_items::DIAMOND_HELMET);
    bound_helmet.set_enchantments(&[(Identifier::vanilla_static("binding_curse"), 1)], false);
    inventory.set_selected_item(ItemStack::new(&vanilla_items::CARVED_PUMPKIN));
    inventory.set(EquipmentSlot::Head, bound_helmet.copy_with_count(1));

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
    assert_eq!(inventory.get_ref(EquipmentSlot::Head), &bound_helmet);
}

#[test]
fn repair_with_xp_repairs_damaged_mending_item() {
    init_vanilla_registry();

    let mut inventory = PlayerInventory::new();
    let mut pickaxe = ItemStack::new(&vanilla_items::DIAMOND_PICKAXE);
    pickaxe.set_damage_value(10);
    pickaxe.set_enchantments(&[(Identifier::vanilla_static("mending"), 1)], false);
    inventory.set_selected_item(pickaxe);
    let before = inventory.get_times_changed();

    let remaining = inventory.repair_random_equipped_item_with_xp(3);

    assert_eq!(remaining, 0);
    assert_eq!(inventory.get_selected_item().get_damage_value(), 4);
    assert_ne!(inventory.get_times_changed(), before);
}

#[test]
fn repair_with_xp_returns_leftover_when_item_is_fully_repaired() {
    init_vanilla_registry();

    let mut inventory = PlayerInventory::new();
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
    init_vanilla_registry();

    let mut inventory = PlayerInventory::new();
    inventory.set_selected_item(ItemStack::with_count(&vanilla_items::CARVED_PUMPKIN, 2));
    inventory.set(
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
        inventory.get_ref(EquipmentSlot::Head),
        &ItemStack::new(&vanilla_items::CARVED_PUMPKIN)
    );
    assert!(
        inventory
            .get_items()
            .iter()
            .any(|stack| stack.is(&vanilla_items::DIAMOND_HELMET))
    );
}

fn test_player(world: Arc<World>) -> Arc<Player> {
    let player = TestPlayerBuilder::new(world, Uuid::from_u128(1), "TestPlayer", 1).build();
    player.set_client_loaded(true);
    player
}

struct LockProbeState {
    armed: AtomicBool,
    saw_packet: AtomicBool,
    all_callbacks_saw_container_unlocked: AtomicBool,
}

struct LockProbeConnection {
    state: Arc<LockProbeState>,
    container: Shared<SimpleContainer>,
}

impl LockProbeConnection {
    fn record_if_armed(&self) {
        if !self.state.armed.load(Ordering::Acquire) {
            return;
        }
        self.state.saw_packet.store(true, Ordering::Release);
        if self.container.try_lock().is_none() {
            self.state
                .all_callbacks_saw_container_unlocked
                .store(false, Ordering::Release);
        }
    }
}

impl NetworkConnection for LockProbeConnection {
    fn compression(&self) -> Option<CompressionInfo> {
        None
    }

    fn send_encoded(&self, _packet: EncodedPacket) {
        self.record_if_armed();
    }

    fn send_encoded_bundle(&self, _packets: Vec<EncodedPacket>) {
        self.record_if_armed();
    }

    fn disconnect_with_reason(&self, _reason: TextComponent) {}

    fn tick(&self) {}

    fn latency(&self) -> i32 {
        0
    }

    fn close(&self) {}

    fn closed(&self) -> bool {
        false
    }
}

struct CloseOnTick;

macro_rules! impl_test_menu_kind_downcast {
    ($type:ty, $name:literal) => {
        // SAFETY: This test-owned key uniquely identifies the concrete menu
        // kind within the test process.
        unsafe impl DowncastType for $type {
            const TYPE_KEY: DowncastTypeKey = DowncastTypeKey::new($name);
        }
    };
}

impl_test_menu_kind_downcast!(
    CloseOnTick,
    "steel:test/menu/player_inventory/close_on_tick"
);

impl MenuKind for CloseOnTick {
    fn on_tick(
        &mut self,
        _behavior: &mut MenuBehavior,
        _guard: &mut ContainerLockGuard,
        player: &Player,
    ) {
        player.close_container();
    }
}

struct CloseOnClick;

impl_test_menu_kind_downcast!(
    CloseOnClick,
    "steel:test/menu/player_inventory/close_on_click"
);

impl MenuKind for CloseOnClick {
    fn on_slot_clicked(
        &mut self,
        _behavior: &mut MenuBehavior,
        _guard: &mut ContainerLockGuard,
        _click: Click,
        player: &Player,
    ) -> ClickOutcome {
        player.close_container();
        ClickOutcome::Consume
    }
}

struct CloseOnOpen;

impl_test_menu_kind_downcast!(
    CloseOnOpen,
    "steel:test/menu/player_inventory/close_on_open"
);

impl MenuKind for CloseOnOpen {
    fn on_open(
        &mut self,
        _behavior: &mut MenuBehavior,
        _guard: &mut ContainerLockGuard,
        player: &Player,
    ) {
        player.close_container();
    }
}

struct OpenReplacementOnOpen {
    own_removals: Arc<AtomicUsize>,
    replacement_removals: Arc<AtomicUsize>,
}

struct ReopenSameContainerOnOpen {
    container: Shared<SimpleContainer>,
    factory_saw_unlocked: Arc<AtomicBool>,
}

impl_test_menu_kind_downcast!(
    ReopenSameContainerOnOpen,
    "steel:test/menu/player_inventory/reopen_same_container_on_open"
);

impl MenuKind for ReopenSameContainerOnOpen {
    fn on_open(
        &mut self,
        _behavior: &mut MenuBehavior,
        _guard: &mut ContainerLockGuard,
        player: &Player,
    ) {
        let container = Arc::clone(&self.container);
        let factory_container = Arc::clone(&container);
        let factory_saw_unlocked = Arc::clone(&self.factory_saw_unlocked);
        player.open_menu("Same container", move |context| {
            factory_saw_unlocked.store(factory_container.try_lock().is_some(), Ordering::Relaxed);
            let mut builder =
                MenuBuilder::new(&vanilla_menu_types::GENERIC_9X1, context.container_id);
            builder.section(container, 9);
            builder.player_inventory(&context.player.inventory);
            builder.build(BasicKind {})
        });
    }
}

impl_test_menu_kind_downcast!(
    OpenReplacementOnOpen,
    "steel:test/menu/player_inventory/open_replacement_on_open"
);

impl MenuKind for OpenReplacementOnOpen {
    fn on_open(
        &mut self,
        _behavior: &mut MenuBehavior,
        _guard: &mut ContainerLockGuard,
        player: &Player,
    ) {
        let replacement_removals = Arc::clone(&self.replacement_removals);
        player.open_menu("Replacement", move |context| {
            empty_test_menu(
                context.player,
                context.container_id,
                CountRemovals {
                    count: replacement_removals,
                },
            )
        });
    }

    fn removed(&mut self, _behavior: &mut MenuBehavior, _player: &Player) {
        self.own_removals.fetch_add(1, Ordering::Relaxed);
    }
}

struct ReopenOnRemoved {
    replacement_removals: Arc<AtomicUsize>,
}

impl_test_menu_kind_downcast!(
    ReopenOnRemoved,
    "steel:test/menu/player_inventory/reopen_on_removed"
);

impl MenuKind for ReopenOnRemoved {
    fn removed(&mut self, _behavior: &mut MenuBehavior, player: &Player) {
        let replacement_removals = Arc::clone(&self.replacement_removals);
        player.open_menu("Replacement", move |context| {
            empty_test_menu(
                context.player,
                context.container_id,
                CountRemovals {
                    count: replacement_removals,
                },
            )
        });
    }
}

struct QueueDrainedReplacementThenRemoveAllOnOpen {
    transient: Shared<SimpleContainer>,
}

impl_test_menu_kind_downcast!(
    QueueDrainedReplacementThenRemoveAllOnOpen,
    "steel:test/menu/player_inventory/queue_drained_replacement_then_remove_all_on_open"
);

impl MenuKind for QueueDrainedReplacementThenRemoveAllOnOpen {
    fn on_open(
        &mut self,
        _behavior: &mut MenuBehavior,
        _guard: &mut ContainerLockGuard,
        player: &Player,
    ) {
        let transient = Arc::clone(&self.transient);
        let inventory = Arc::clone(&player.inventory);
        player.open_menu("Replacement", move |context| {
            let mut builder =
                MenuBuilder::new(&vanilla_menu_types::GENERIC_9X1, context.container_id);
            let transient = builder.section(transient, 9);
            builder.player_inventory(&inventory);
            builder.drain([transient]);
            builder.build(BasicKind {})
        });

        assert_eq!(
            player.remove_all_menus(),
            MenuRemovalStatus::Pending,
            "the on_open callback owns the current menu"
        );
    }
}

struct DropAllMenusOnOpen;

impl_test_menu_kind_downcast!(
    DropAllMenusOnOpen,
    "steel:test/menu/player_inventory/drop_all_menus_on_open"
);

impl MenuKind for DropAllMenusOnOpen {
    fn on_open(
        &mut self,
        _behavior: &mut MenuBehavior,
        _guard: &mut ContainerLockGuard,
        player: &Player,
    ) {
        assert_eq!(
            player.remove_all_menus_with_disposition(MenuItemDisposition::Drop),
            MenuRemovalStatus::Pending,
            "the on_open callback owns the current menu"
        );
    }
}

struct RemoveAllOnRemoved;

impl_test_menu_kind_downcast!(
    RemoveAllOnRemoved,
    "steel:test/menu/player_inventory/remove_all_on_removed"
);

impl MenuKind for RemoveAllOnRemoved {
    fn removed(&mut self, _behavior: &mut MenuBehavior, player: &Player) {
        assert_eq!(
            player.remove_all_menus_with_disposition(MenuItemDisposition::Drop),
            MenuRemovalStatus::Pending,
            "the removal callback owns the current menu dispatch"
        );
    }
}

struct OpenTerminalReplacementOnRemoved;

impl_test_menu_kind_downcast!(
    OpenTerminalReplacementOnRemoved,
    "steel:test/menu/player_inventory/open_terminal_replacement_on_removed"
);

impl MenuKind for OpenTerminalReplacementOnRemoved {
    fn removed(&mut self, _behavior: &mut MenuBehavior, player: &Player) {
        player.open_menu("Terminal replacement", |context| {
            empty_test_menu(context.player, context.container_id, RemoveAllOnRemoved)
        });
    }
}

struct QueueReplacementOnOpenAndRemoveAllOnRemoved {
    transient: Shared<SimpleContainer>,
}

impl_test_menu_kind_downcast!(
    QueueReplacementOnOpenAndRemoveAllOnRemoved,
    "steel:test/menu/player_inventory/queue_replacement_on_open_and_remove_all_on_removed"
);

impl MenuKind for QueueReplacementOnOpenAndRemoveAllOnRemoved {
    fn on_open(
        &mut self,
        _behavior: &mut MenuBehavior,
        _guard: &mut ContainerLockGuard,
        player: &Player,
    ) {
        let transient = Arc::clone(&self.transient);
        let inventory = Arc::clone(&player.inventory);
        player.open_menu("Queued replacement", move |context| {
            let mut builder =
                MenuBuilder::new(&vanilla_menu_types::GENERIC_9X1, context.container_id);
            let transient = builder.section(transient, 9);
            builder.player_inventory(&inventory);
            builder.drain([transient]);
            builder.build(BasicKind {})
        });
    }

    fn removed(&mut self, _behavior: &mut MenuBehavior, player: &Player) {
        assert_eq!(
            player.remove_all_menus_with_disposition(MenuItemDisposition::Drop),
            MenuRemovalStatus::Pending,
            "the removal callback owns the current menu dispatch"
        );
    }
}

struct CountRemovals {
    count: Arc<AtomicUsize>,
}

impl_test_menu_kind_downcast!(
    CountRemovals,
    "steel:test/menu/player_inventory/count_removals"
);

impl MenuKind for CountRemovals {
    fn removed(&mut self, _behavior: &mut MenuBehavior, _player: &Player) {
        self.count.fetch_add(1, Ordering::Relaxed);
    }
}

struct BlockTerminalMenuRemoval {
    entered: Arc<Barrier>,
    release: Arc<Barrier>,
    returned_to_inventory: Arc<AtomicBool>,
}

impl_test_menu_kind_downcast!(
    BlockTerminalMenuRemoval,
    "steel:test/menu/player_inventory/block_terminal_menu_removal"
);

impl MenuKind for BlockTerminalMenuRemoval {
    fn removed(&mut self, _behavior: &mut MenuBehavior, player: &Player) {
        self.entered.wait();
        self.release.wait();
        self.returned_to_inventory
            .store(player.returns_menu_items_to_inventory(), Ordering::Release);
    }
}

fn empty_test_menu(player: &Player, container_id: u8, kind: impl MenuKind + 'static) -> Menu {
    let mut builder = MenuBuilder::new(&vanilla_menu_types::GENERIC_9X1, container_id);
    builder.section(SimpleContainer::new(9).into_shared(), 9);
    builder.player_inventory(&player.inventory);
    builder.build(kind)
}
#[test]
fn disconnected_menu_removal_drops_transient_items() {
    init_vanilla_registry();
    let world = fresh_test_world("disconnected_menu_close");
    insert_ready_full_chunk(&world, ChunkPos::new(0, 0));
    let player = test_player(Arc::clone(&world));
    let transient = SimpleContainer::new(9).into_shared();
    transient
        .lock()
        .set_item(0, ItemStack::new(&vanilla_items::STONE));

    let probe_state = Arc::new(LockProbeState {
        armed: AtomicBool::new(false),
        saw_packet: AtomicBool::new(false),
        all_callbacks_saw_container_unlocked: AtomicBool::new(true),
    });
    let observer_connection = Arc::new(PlayerConnection::Other(Box::new(LockProbeConnection {
        state: Arc::clone(&probe_state),
        container: Arc::clone(&transient),
    })));
    let observer = TestPlayerBuilder::new(
        Arc::clone(&world),
        Uuid::from_u128(2),
        "Observer",
        next_entity_id(),
    )
    .connection(observer_connection)
    .build();
    assert!(world.add_player(Arc::clone(&observer), ResetReason::InitialJoin));
    let _ = observer.mark_joined_world();
    observer.set_client_loaded(true);
    observer
        .chunk_sender
        .lock()
        .mark_chunk_sent_for_test(ChunkPos::new(0, 0));

    let menu_container = Arc::clone(&transient);
    let inventory = Arc::clone(&player.inventory);
    player.open_menu("Transient", move |context| {
        let mut builder = MenuBuilder::new(&vanilla_menu_types::GENERIC_9X1, context.container_id);
        let transient_slots = builder.section(menu_container, 9);
        builder.player_inventory(&inventory);
        builder.drain([transient_slots]);
        builder.build(BasicKind {})
    });

    probe_state.armed.store(true, Ordering::Release);
    player.close_connection();
    assert_eq!(player.remove_all_menus(), MenuRemovalStatus::Complete);

    assert!(probe_state.saw_packet.load(Ordering::Acquire));
    assert!(
        probe_state
            .all_callbacks_saw_container_unlocked
            .load(Ordering::Acquire)
    );
    assert!(transient.lock().get_item(0).is_empty());
    assert!(
        player
            .inventory
            .lock()
            .items()
            .iter()
            .all(|item| !item.is(&vanilla_items::STONE))
    );

    let dropped = world.get_entities_in_aabb_matching(
        &WorldAabb::new(-2.0, -1.0, -2.0, 2.0, 3.0, 2.0),
        |entity| entity.entity_type() == &vanilla_entities::ITEM,
    );
    assert_eq!(dropped.len(), 1);
    let Some(item) = dropped[0].as_ref().downcast_ref::<ItemEntity>() else {
        panic!("dropped entity should retain its concrete item type");
    };
    assert!(item.get_item().is(&vanilla_items::STONE));

    probe_state.armed.store(false, Ordering::Release);
    world.remove_player_for_world_change(&observer);
}

#[test]
fn drained_items_return_without_player_inventory_slots() {
    init_vanilla_registry();
    let player = test_player(Arc::clone(test_world()));
    let transient = SimpleContainer::new(1).into_shared();
    transient
        .lock()
        .set_item(0, ItemStack::with_count(&vanilla_items::STONE, 3));
    let mut builder = MenuBuilder::new(None, 1);
    let transient_slots = builder.section(Arc::clone(&transient), 1);
    builder.drain([transient_slots]);
    let mut menu = builder.build(BasicKind {});

    menu.removed(&player);

    assert!(transient.lock().get_item(0).is_empty());
    assert_eq!(player.inventory.lock().get_item(0).count(), 3);
}

#[test]
fn menu_item_return_policy_preserves_world_changes_only() {
    init_vanilla_registry();
    let connected = test_player(Arc::clone(test_world()));
    assert!(connected.returns_menu_items_to_inventory());

    let changing_world = test_player(Arc::clone(test_world()));
    changing_world.set_removed(RemovalReason::ChangedWorld);
    assert!(changing_world.returns_menu_items_to_inventory());

    let killed = test_player(Arc::clone(test_world()));
    killed.set_removed(RemovalReason::Killed);
    assert!(!killed.returns_menu_items_to_inventory());
}

#[test]
fn menu_tick_hook_can_close_the_current_menu() {
    init_vanilla_registry();
    let player = test_player(Arc::clone(test_world()));
    player.open_menu("Close on tick", |context| {
        empty_test_menu(context.player, context.container_id, CloseOnTick)
    });

    player.tick_open_menu();

    assert!(!player.has_container_open());
}

#[test]
fn menu_click_hook_can_close_the_current_menu() {
    init_vanilla_registry();
    let player = test_player(Arc::clone(test_world()));
    let opened_container_id = Arc::new(AtomicU8::new(0));
    let factory_container_id = Arc::clone(&opened_container_id);
    player.open_menu("Close on click", move |context| {
        factory_container_id.store(context.container_id, Ordering::Relaxed);
        empty_test_menu(context.player, context.container_id, CloseOnClick)
    });

    player.handle_container_click(SContainerClick {
        container_id: i32::from(opened_container_id.load(Ordering::Relaxed)),
        state_id: 0,
        slot_num: 0,
        button_num: 0,
        click_type: ClickType::Pickup,
        changed_slots: FxHashMap::default(),
        carried_item: HashedStack::Empty,
    });

    assert!(!player.has_container_open());
}

#[test]
fn dead_player_container_click_only_resynchronizes() {
    init_vanilla_registry();
    let player = test_player(Arc::clone(test_world()));
    player
        .inventory
        .lock()
        .set_item(0, ItemStack::new(&vanilla_items::STONE));
    player.set_health(0.0);

    player.handle_container_click(SContainerClick {
        container_id: 0,
        state_id: 0,
        slot_num: 36,
        button_num: 0,
        click_type: ClickType::Pickup,
        changed_slots: FxHashMap::default(),
        carried_item: HashedStack::Empty,
    });

    assert!(
        player
            .inventory
            .lock()
            .get_item(0)
            .is(&vanilla_items::STONE)
    );
    assert!(player.inventory_menu.lock().behavior().carried().is_empty());
}

#[test]
fn malformed_quickcraft_encoding_resets_active_drag() {
    init_vanilla_registry();
    let player = test_player(Arc::clone(test_world()));
    {
        let mut menu = player.inventory_menu.lock();
        *menu.behavior_mut().carried_mut() = ItemStack::new(&vanilla_items::STONE);
        menu.clicked(
            Click::QuickCraft(QuickCraft::Start {
                kind: DragKind::Left,
            }),
            &player,
        );
        assert_eq!(menu.behavior().quickcraft(), Some(DragKind::Left));
    }

    player.handle_container_click(SContainerClick {
        container_id: 0,
        state_id: 0,
        slot_num: -999,
        button_num: 3,
        click_type: ClickType::QuickCraft,
        changed_slots: FxHashMap::default(),
        carried_item: HashedStack::Empty,
    });

    assert_eq!(player.inventory_menu.lock().behavior().quickcraft(), None);
}

#[test]
fn malformed_non_quickcraft_click_resets_active_drag() {
    init_vanilla_registry();
    let probe_state = Arc::new(LockProbeState {
        armed: AtomicBool::new(false),
        saw_packet: AtomicBool::new(false),
        all_callbacks_saw_container_unlocked: AtomicBool::new(true),
    });
    let connection = Arc::new(PlayerConnection::Other(Box::new(LockProbeConnection {
        state: Arc::clone(&probe_state),
        container: SimpleContainer::new(1).into_shared(),
    })));
    let player = TestPlayerBuilder::new(
        Arc::clone(test_world()),
        Uuid::from_u128(1),
        "TestPlayer",
        1,
    )
    .connection(connection)
    .build();
    player.set_client_loaded(true);
    let out_of_range_slot = {
        let mut menu = player.inventory_menu.lock();
        *menu.behavior_mut().carried_mut() = ItemStack::with_count(&vanilla_items::STONE, 2);
        menu.clicked(
            Click::QuickCraft(QuickCraft::Start {
                kind: DragKind::Left,
            }),
            &player,
        );
        menu.clicked(Click::QuickCraft(QuickCraft::AddSlot { slot: 36 }), &player);
        assert_eq!(menu.behavior().quickcraft(), Some(DragKind::Left));
        i16::try_from(menu.behavior().slot_count()).expect("inventory menu should fit in i16")
    };

    probe_state.armed.store(true, Ordering::Release);
    player.handle_container_click(SContainerClick {
        container_id: 0,
        state_id: 1,
        slot_num: out_of_range_slot,
        button_num: 0,
        click_type: ClickType::Pickup,
        changed_slots: FxHashMap::default(),
        carried_item: HashedStack::Empty,
    });
    assert_eq!(
        player.inventory_menu.lock().behavior().quickcraft(),
        Some(DragKind::Left)
    );
    assert!(!probe_state.saw_packet.load(Ordering::Acquire));
    probe_state.armed.store(false, Ordering::Release);

    player.handle_container_click(SContainerClick {
        container_id: 0,
        state_id: 0,
        slot_num: 36,
        button_num: 2,
        click_type: ClickType::Pickup,
        changed_slots: FxHashMap::default(),
        carried_item: HashedStack::Empty,
    });
    assert_eq!(player.inventory_menu.lock().behavior().quickcraft(), None);
    player.handle_container_click(SContainerClick {
        container_id: 0,
        state_id: 0,
        slot_num: -999,
        button_num: 2,
        click_type: ClickType::QuickCraft,
        changed_slots: FxHashMap::default(),
        carried_item: HashedStack::Empty,
    });

    let menu = player.inventory_menu.lock();
    assert_eq!(menu.behavior().quickcraft(), None);
    assert_eq!(menu.behavior().carried().count(), 2);
    assert!(player.inventory.lock().get_item(0).is_empty());
}

#[test]
fn closing_menu_while_dead_does_not_return_items_to_inventory() {
    init_vanilla_registry();
    let world = fresh_test_world("dead_menu_close");
    insert_ready_full_chunk(&world, ChunkPos::new(0, 0));
    let player = test_player(Arc::clone(&world));
    let transient = SimpleContainer::new(9).into_shared();
    transient
        .lock()
        .set_item(0, ItemStack::with_count(&vanilla_items::STONE, 3));
    let menu_container = Arc::clone(&transient);
    let inventory = Arc::clone(&player.inventory);
    player.open_menu("Dead close", move |context| {
        let mut builder = MenuBuilder::new(&vanilla_menu_types::GENERIC_9X1, context.container_id);
        let transient_slots = builder.section(menu_container, 9);
        builder.player_inventory(&inventory);
        builder.drain([transient_slots]);
        builder.build(BasicKind {})
    });
    player.set_health(0.0);

    player.do_close_container();

    assert!(transient.lock().get_item(0).is_empty());
    assert!(
        player
            .inventory
            .lock()
            .items()
            .iter()
            .all(|item| !item.is(&vanilla_items::STONE))
    );
}

#[test]
fn programmatic_out_of_range_menu_click_is_ignored() {
    init_vanilla_registry();
    let player = test_player(Arc::clone(test_world()));
    let mut menu = empty_test_menu(&player, 1, BasicKind {});
    let invalid_slot = menu.behavior().slot_count();

    menu.clicked(
        Click::Pickup {
            slot: invalid_slot,
            button: MouseButton::Left,
        },
        &player,
    );

    assert!(menu.behavior().carried().is_empty());
}

#[test]
fn menu_open_hook_can_close_the_new_menu() {
    init_vanilla_registry();
    let player = test_player(Arc::clone(test_world()));
    player.open_menu("Close on open", |context| {
        empty_test_menu(context.player, context.container_id, CloseOnOpen)
    });

    assert!(!player.has_container_open());
}

#[test]
fn menu_open_hook_can_replace_the_new_menu() {
    init_vanilla_registry();
    let player = test_player(Arc::clone(test_world()));
    let own_removals = Arc::new(AtomicUsize::new(0));
    let replacement_removals = Arc::new(AtomicUsize::new(0));
    let factory_own_removals = Arc::clone(&own_removals);
    let factory_replacement_removals = Arc::clone(&replacement_removals);
    player.open_menu("Replace on open", move |context| {
        empty_test_menu(
            context.player,
            context.container_id,
            OpenReplacementOnOpen {
                own_removals: factory_own_removals,
                replacement_removals: factory_replacement_removals,
            },
        )
    });

    assert!(player.has_container_open());
    assert_eq!(own_removals.load(Ordering::Relaxed), 1);
    assert_eq!(replacement_removals.load(Ordering::Relaxed), 0);

    player.do_close_container();

    assert_eq!(replacement_removals.load(Ordering::Relaxed), 1);
}

#[test]
fn menu_hook_defers_a_factory_that_reuses_its_locked_container() {
    init_vanilla_registry();
    let player = test_player(Arc::clone(test_world()));
    let container = SimpleContainer::new(9).into_shared();
    let factory_saw_unlocked = Arc::new(AtomicBool::new(false));
    let menu_container = Arc::clone(&container);
    let kind_container = Arc::clone(&container);
    let kind_factory_saw_unlocked = Arc::clone(&factory_saw_unlocked);

    player.open_menu("Initial", move |context| {
        let mut builder = MenuBuilder::new(&vanilla_menu_types::GENERIC_9X1, context.container_id);
        builder.section(menu_container, 9);
        builder.player_inventory(&context.player.inventory);
        builder.build(ReopenSameContainerOnOpen {
            container: kind_container,
            factory_saw_unlocked: kind_factory_saw_unlocked,
        })
    });

    assert!(factory_saw_unlocked.load(Ordering::Relaxed));
    assert!(player.has_container_open());
}

#[test]
#[should_panic(expected = "open_menu factory returned container id")]
fn open_menu_rejects_a_factory_with_the_wrong_container_id() {
    init_vanilla_registry();
    let player = test_player(Arc::clone(test_world()));

    player.open_menu("Wrong id", |context| {
        empty_test_menu(
            context.player,
            context.container_id.wrapping_add(1),
            BasicKind {},
        )
    });
}

#[test]
fn menu_removed_hook_can_open_a_replacement() {
    init_vanilla_registry();
    let player = test_player(Arc::clone(test_world()));
    let replacement_removals = Arc::new(AtomicUsize::new(0));
    let factory_replacement_removals = Arc::clone(&replacement_removals);
    player.open_menu("Reopen on removal", move |context| {
        empty_test_menu(
            context.player,
            context.container_id,
            ReopenOnRemoved {
                replacement_removals: factory_replacement_removals,
            },
        )
    });

    player.do_close_container();

    assert!(player.has_container_open());
    assert_eq!(replacement_removals.load(Ordering::Relaxed), 0);
}

#[test]
fn terminal_menu_removal_returns_carried_item_and_rejects_replacement() {
    init_vanilla_registry();
    let player = test_player(Arc::clone(test_world()));
    player
        .inventory
        .lock()
        .set_item(0, ItemStack::with_count(&vanilla_items::STONE, 3));
    let replacement_removals = Arc::new(AtomicUsize::new(0));
    let opened_container_id = Arc::new(AtomicU8::new(0));
    let factory_container_id = Arc::clone(&opened_container_id);
    let factory_replacement_removals = Arc::clone(&replacement_removals);
    player.open_menu("Reopen on removal", move |context| {
        factory_container_id.store(context.container_id, Ordering::Relaxed);
        empty_test_menu(
            context.player,
            context.container_id,
            ReopenOnRemoved {
                replacement_removals: factory_replacement_removals,
            },
        )
    });

    player.handle_container_click(SContainerClick {
        container_id: i32::from(opened_container_id.load(Ordering::Relaxed)),
        state_id: 0,
        slot_num: 36,
        button_num: 0,
        click_type: ClickType::Pickup,
        changed_slots: FxHashMap::default(),
        carried_item: HashedStack::Empty,
    });
    assert!(player.inventory.lock().get_item(0).is_empty());

    assert_eq!(player.remove_all_menus(), MenuRemovalStatus::Complete);

    assert!(!player.has_container_open());
    assert_eq!(replacement_removals.load(Ordering::Relaxed), 0);
    let inventory = player.inventory.lock();
    let stone_count: i32 = inventory
        .items()
        .iter()
        .filter(|item| item.is(&vanilla_items::STONE))
        .map(ItemStack::count)
        .sum();
    assert_eq!(stone_count, 3);
}

#[test]
fn terminal_menu_removal_skips_queued_factory_and_drains_base_menu() {
    init_vanilla_registry();
    let player = test_player(Arc::clone(test_world()));
    let crafting = player.crafting_container();
    crafting
        .lock()
        .set_item(0, ItemStack::with_count(&vanilla_items::STONE, 2));
    *player.inventory_menu.lock().behavior_mut().carried_mut() =
        ItemStack::with_count(&vanilla_items::DIRT, 3);
    let transient = SimpleContainer::new(9).into_shared();
    transient
        .lock()
        .set_item(0, ItemStack::with_count(&vanilla_items::OAK_LOG, 4));

    let menu_transient = Arc::clone(&transient);
    player.open_menu("Terminal on open", move |context| {
        empty_test_menu(
            context.player,
            context.container_id,
            QueueDrainedReplacementThenRemoveAllOnOpen {
                transient: menu_transient,
            },
        )
    });

    assert!(!player.has_container_open());
    assert!(crafting.lock().get_item(0).is_empty());
    assert!(player.inventory_menu.lock().behavior().carried().is_empty());
    assert_eq!(transient.lock().get_item(0).count(), 4);

    let inventory = player.inventory.lock();
    for (item, expected) in [(&vanilla_items::STONE, 2), (&vanilla_items::DIRT, 3)] {
        let count: i32 = inventory
            .items()
            .iter()
            .filter(|stack| stack.is(item))
            .map(ItemStack::count)
            .sum();
        assert_eq!(count, expected);
    }
    assert!(
        inventory
            .items()
            .iter()
            .all(|stack| !stack.is(&vanilla_items::OAK_LOG))
    );
}

#[test]
fn pending_terminal_removal_preserves_drop_disposition() {
    init_vanilla_registry();
    let world = fresh_test_world("pending_terminal_menu_drop");
    insert_ready_full_chunk(&world, ChunkPos::new(0, 0));
    let player = test_player(Arc::clone(&world));
    *player.inventory_menu.lock().behavior_mut().carried_mut() =
        ItemStack::with_count(&vanilla_items::STONE, 2);

    player.open_menu("Terminal drop on open", |context| {
        empty_test_menu(context.player, context.container_id, DropAllMenusOnOpen)
    });

    assert!(!player.has_container_open());
    assert!(
        player
            .inventory
            .lock()
            .items()
            .iter()
            .all(ItemStack::is_empty)
    );
    let dropped = world.get_entities_in_aabb_matching(
        &WorldAabb::new(-2.0, -1.0, -2.0, 2.0, 3.0, 2.0),
        |entity| entity.entity_type() == &vanilla_entities::ITEM,
    );
    assert_eq!(dropped.len(), 1);
    let Some(item) = dropped[0].as_ref().downcast_ref::<ItemEntity>() else {
        panic!("dropped entity should retain its concrete item type");
    };
    assert!(item.get_item().is(&vanilla_items::STONE));
    assert_eq!(item.get_item().count(), 2);
}

#[test]
fn menu_open_stops_when_predecessor_removal_turns_terminal() {
    init_vanilla_registry();
    let player = test_player(Arc::clone(test_world()));
    player.open_menu("Terminal on removal", |context| {
        empty_test_menu(context.player, context.container_id, RemoveAllOnRemoved)
    });
    let factory_called = Arc::new(AtomicBool::new(false));
    let rejected_factory_called = Arc::clone(&factory_called);

    player.open_menu("Rejected", move |context| {
        rejected_factory_called.store(true, Ordering::Relaxed);
        empty_test_menu(context.player, context.container_id, BasicKind {})
    });

    assert!(!factory_called.load(Ordering::Relaxed));
    assert!(!player.has_container_open());
}

#[test]
fn prepared_menu_is_cleaned_when_replacement_removal_turns_terminal() {
    init_vanilla_registry();
    let world = fresh_test_world("prepared_menu_terminal_cleanup");
    insert_ready_full_chunk(&world, ChunkPos::new(0, 0));
    let player = test_player(Arc::clone(&world));
    player.open_menu("Open terminal replacement", |context| {
        empty_test_menu(
            context.player,
            context.container_id,
            OpenTerminalReplacementOnRemoved,
        )
    });
    let final_removals = Arc::new(AtomicUsize::new(0));
    let transient = SimpleContainer::new(9).into_shared();
    transient
        .lock()
        .set_item(0, ItemStack::with_count(&vanilla_items::STONE, 2));

    let menu_transient = Arc::clone(&transient);
    let menu_final_removals = Arc::clone(&final_removals);
    player.open_menu("Rejected after construction", move |context| {
        let mut builder = MenuBuilder::new(&vanilla_menu_types::GENERIC_9X1, context.container_id);
        let transient = builder.section(menu_transient, 9);
        builder.player_inventory(&context.player.inventory);
        builder.drain([transient]);
        builder.build(CountRemovals {
            count: menu_final_removals,
        })
    });

    assert!(!player.has_container_open());
    assert_eq!(final_removals.load(Ordering::Relaxed), 1);
    assert!(transient.lock().get_item(0).is_empty());
    assert!(
        player
            .inventory
            .lock()
            .items()
            .iter()
            .all(ItemStack::is_empty)
    );
    let dropped = world.get_entities_in_aabb_matching(
        &WorldAabb::new(-2.0, -1.0, -2.0, 2.0, 3.0, 2.0),
        |entity| entity.entity_type() == &vanilla_entities::ITEM,
    );
    assert_eq!(dropped.len(), 1);
    let Some(item) = dropped[0].as_ref().downcast_ref::<ItemEntity>() else {
        panic!("dropped entity should retain its concrete item type");
    };
    assert!(item.get_item().is(&vanilla_items::STONE));
    assert_eq!(item.get_item().count(), 2);
}

#[test]
fn deferred_factory_is_not_run_when_earlier_close_turns_terminal() {
    init_vanilla_registry();
    let world = fresh_test_world("deferred_open_terminal_cleanup");
    insert_ready_full_chunk(&world, ChunkPos::new(0, 0));
    let player = test_player(Arc::clone(&world));
    let transient = SimpleContainer::new(9).into_shared();
    transient
        .lock()
        .set_item(0, ItemStack::with_count(&vanilla_items::STONE, 4));

    let menu_transient = Arc::clone(&transient);
    player.open_menu("Queue then remove", move |context| {
        empty_test_menu(
            context.player,
            context.container_id,
            QueueReplacementOnOpenAndRemoveAllOnRemoved {
                transient: menu_transient,
            },
        )
    });

    assert!(!player.has_container_open());
    assert_eq!(transient.lock().get_item(0).count(), 4);
    assert!(
        player
            .inventory
            .lock()
            .items()
            .iter()
            .all(ItemStack::is_empty)
    );
    let dropped = world.get_entities_in_aabb_matching(
        &WorldAabb::new(-2.0, -1.0, -2.0, 2.0, 3.0, 2.0),
        |entity| entity.entity_type() == &vanilla_entities::ITEM,
    );
    assert!(dropped.is_empty());
}

#[test]
fn terminal_removal_stays_active_while_pending_menu_cleanup_runs() {
    init_vanilla_registry();
    let player = test_player(Arc::clone(test_world()));
    let factory_entered = Arc::new(Barrier::new(2));
    let factory_release = Arc::new(Barrier::new(2));
    let removal_entered = Arc::new(Barrier::new(2));
    let removal_release = Arc::new(Barrier::new(2));
    let returned_to_inventory = Arc::new(AtomicBool::new(true));

    let opener_player = Arc::clone(&player);
    let opener_factory_entered = Arc::clone(&factory_entered);
    let opener_factory_release = Arc::clone(&factory_release);
    let opener_removal_entered = Arc::clone(&removal_entered);
    let opener_removal_release = Arc::clone(&removal_release);
    let opener_returned_to_inventory = Arc::clone(&returned_to_inventory);
    let opener = thread::spawn(move || {
        opener_player.open_menu("Pending cleanup", move |context| {
            opener_factory_entered.wait();
            opener_factory_release.wait();
            empty_test_menu(
                context.player,
                context.container_id,
                BlockTerminalMenuRemoval {
                    entered: opener_removal_entered,
                    release: opener_removal_release,
                    returned_to_inventory: opener_returned_to_inventory,
                },
            )
        });
    });

    factory_entered.wait();
    assert_eq!(player.remove_all_menus(), MenuRemovalStatus::Pending);
    player.close_connection();
    assert_eq!(player.remove_all_menus(), MenuRemovalStatus::Pending);
    factory_release.wait();
    removal_entered.wait();

    player.retry_terminal_menu_removal_for_test();
    let replacement_factory_called = Arc::new(AtomicBool::new(false));
    let rejected_factory_called = Arc::clone(&replacement_factory_called);
    player.open_menu("Rejected during cleanup", move |context| {
        rejected_factory_called.store(true, Ordering::Relaxed);
        empty_test_menu(context.player, context.container_id, BasicKind {})
    });
    assert!(!replacement_factory_called.load(Ordering::Relaxed));

    removal_release.wait();
    assert!(opener.join().is_ok());
    assert!(!returned_to_inventory.load(Ordering::Acquire));
    assert!(!player.has_container_open());
}

#[test]
fn opening_a_menu_closes_a_replacement_created_during_removal() {
    init_vanilla_registry();
    let player = test_player(Arc::clone(test_world()));
    let replacement_removals = Arc::new(AtomicUsize::new(0));
    let factory_replacement_removals = Arc::clone(&replacement_removals);
    player.open_menu("Reopen on removal", move |context| {
        empty_test_menu(
            context.player,
            context.container_id,
            ReopenOnRemoved {
                replacement_removals: factory_replacement_removals,
            },
        )
    });

    player.open_menu("Final", |context| {
        empty_test_menu(context.player, context.container_id, BasicKind {})
    });

    assert!(player.has_container_open());
    assert_eq!(replacement_removals.load(Ordering::Relaxed), 1);
}

#[test]
fn creative_crafting_grid_updates_the_result_slot() {
    init_vanilla_registry();
    let player = test_player(Arc::clone(test_world()));
    assert!(player.change_game_mode_state(GameType::Creative));
    let crafting = player.inventory_crafting_handler();

    player.handle_set_creative_mode_slot(SSetCreativeModeSlot {
        slot_num: 1,
        item_stack: ItemStack::new(&vanilla_items::OAK_LOG),
    });

    {
        let menu = player.inventory_menu.lock();
        let guard = menu.behavior().lock_all_containers();
        let result = guard
            .get(crafting.result_id())
            .expect("result container is registered with the menu");
        assert!(result.get_item(0).is(&vanilla_items::OAK_PLANKS));
        assert_eq!(result.get_item(0).count(), 4);
    }

    player.handle_set_creative_mode_slot(SSetCreativeModeSlot {
        slot_num: 1,
        item_stack: ItemStack::empty(),
    });

    {
        let menu = player.inventory_menu.lock();
        let guard = menu.behavior().lock_all_containers();
        let result = guard
            .get(crafting.result_id())
            .expect("result container is registered with the menu");
        assert!(result.get_item(0).is_empty());
    }
}
