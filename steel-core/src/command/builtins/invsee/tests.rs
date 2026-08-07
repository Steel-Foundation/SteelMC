use std::{
    io::Cursor,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

use steel_protocol::packet_traits::{CompressionInfo, EncodedPacket};
use steel_registry::packets::play::C_SET_PLAYER_INVENTORY;
use steel_registry::{
    init_vanilla_registry, item_stack::ItemStack, vanilla_items, vanilla_menu_types,
};
use steel_utils::codec::VarInt;
use steel_utils::locks::{IntoShared as _, Shared, SyncMutex};
use steel_utils::serial::ReadFrom as _;
use steel_utils::types::GameType;
use text_components::TextComponent;
use uuid::Uuid;

use super::*;
use crate::{
    entity::PendingWorldChangeToken,
    inventory::{
        container::{Container as _, SimpleContainer},
        menu::kinds::BasicKind,
        prelude::{Click, DragKind, MenuBuilder, MouseButton, QuickCraft, SectionKind},
    },
    permission::{PermissionEntry, PermissionMetadataSet, PermissionSet},
    player::{PlayerConnection, player_inventory::PlayerInventory},
    test_support::{
        TestPlayerBuilder, fresh_test_world_in_domain, test_runtime_config, test_world,
    },
};

const TARGET_HOTBAR_START: usize = 27;
const TARGET_ARMOR_START: usize = 36;
const TARGET_CRAFTING_START: usize = 41;
const VIEWER_INVENTORY_START: usize = 45;

struct RecordingConnection {
    packets: Arc<SyncMutex<Vec<EncodedPacket>>>,
    inventories: Arc<SyncMutex<Vec<Shared<PlayerInventory>>>>,
    callbacks_saw_unlocked_inventories: Arc<AtomicBool>,
}

impl NetworkConnection for RecordingConnection {
    fn compression(&self) -> Option<CompressionInfo> {
        None
    }

    fn send_encoded(&self, packet: EncodedPacket) {
        let inventories = self.inventories.lock().clone();
        if inventories
            .iter()
            .any(|inventory| inventory.try_lock().is_none())
        {
            self.callbacks_saw_unlocked_inventories
                .store(false, Ordering::Release);
        }
        self.packets.lock().push(packet);
    }

    fn send_encoded_bundle(&self, packets: Vec<EncodedPacket>) {
        self.packets.lock().extend(packets);
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

struct RecordingPlayer {
    player: Arc<Player>,
    packets: Arc<SyncMutex<Vec<EncodedPacket>>>,
    inventories: Arc<SyncMutex<Vec<Shared<PlayerInventory>>>>,
    callbacks_saw_unlocked_inventories: Arc<AtomicBool>,
}

fn recording_player(uuid: u128, name: &str, entity_id: i32) -> RecordingPlayer {
    init_vanilla_registry();
    let packets = Arc::new(SyncMutex::new(Vec::new()));
    let inventories = Arc::new(SyncMutex::new(Vec::new()));
    let callbacks_saw_unlocked_inventories = Arc::new(AtomicBool::new(true));
    let connection = Arc::new(PlayerConnection::Other(Box::new(RecordingConnection {
        packets: Arc::clone(&packets),
        inventories: Arc::clone(&inventories),
        callbacks_saw_unlocked_inventories: Arc::clone(&callbacks_saw_unlocked_inventories),
    })));
    let player = TestPlayerBuilder::new(
        Arc::clone(test_world()),
        Uuid::from_u128(uuid),
        name,
        entity_id,
    )
    .detached_config(test_runtime_config(2))
    .connection(connection)
    .build();
    RecordingPlayer {
        player,
        packets,
        inventories,
        callbacks_saw_unlocked_inventories,
    }
}

fn player_inventory_updates(packets: &SyncMutex<Vec<EncodedPacket>>) -> Vec<(i32, ItemStack)> {
    packets
        .lock()
        .iter()
        .filter_map(|packet| {
            let mut cursor = Cursor::new(packet.encoded_data.as_slice());
            let length = VarInt::read(&mut cursor);
            assert!(length.is_ok(), "packet length should decode");
            let packet_id = match VarInt::read(&mut cursor) {
                Ok(packet_id) => packet_id.0,
                Err(error) => panic!("packet id should decode: {error}"),
            };
            if packet_id != C_SET_PLAYER_INVENTORY {
                return None;
            }
            let slot = match VarInt::read(&mut cursor) {
                Ok(slot) => slot.0,
                Err(error) => panic!("logical inventory slot should decode: {error}"),
            };
            let item_stack = match ItemStack::read(&mut cursor) {
                Ok(item_stack) => item_stack,
                Err(error) => panic!("logical inventory item should decode: {error}"),
            };
            Some((slot, item_stack))
        })
        .collect()
}

fn test_player(uuid: u128, name: &str, entity_id: i32) -> Arc<Player> {
    init_vanilla_registry();
    TestPlayerBuilder::new(
        Arc::clone(test_world()),
        Uuid::from_u128(uuid),
        name,
        entity_id,
    )
    .detached_config(test_runtime_config(2))
    .build()
}

fn permission_key(value: &str) -> PermissionKey {
    match PermissionKey::parse(value) {
        Ok(key) => key,
        Err(error) => panic!("test permission key should parse: {error}"),
    }
}

fn set_permissions(player: &Player, effective: PermissionSet) {
    player.set_permission_state(
        Vec::new(),
        PermissionSet::new(),
        PermissionMetadataSet::new(),
        effective,
        PermissionMetadataSet::new(),
    );
}

fn begin_domain_switch(player: &Player) -> PendingWorldChangeToken {
    let Some(token) = player.begin_pending_world_change() else {
        panic!("test player should accept a pending world change");
    };
    assert!(player.begin_domain_switch(token));
    token
}

fn finish_domain_switch(player: &Player, token: PendingWorldChangeToken) {
    assert!(player.finish_domain_switch(token));
    assert!(player.finish_pending_world_change(token));
}

#[test]
fn base_and_modify_permissions_grant_the_expected_access() {
    let Ok((access, modify)) = invsee_permissions() else {
        panic!("built-in invsee permissions should parse");
    };
    let readonly = PermissionSet::from_entries([
        PermissionEntry::allow(permission_key(INVSEE_PERMISSION)),
        PermissionEntry::deny(permission_key(MODIFY_PERMISSION)),
    ]);
    assert!(readonly.allows(&access));
    assert!(!readonly.allows(&modify));

    let modifier = PermissionSet::from_entries([
        PermissionEntry::deny(permission_key(INVSEE_PERMISSION)),
        PermissionEntry::allow(permission_key(MODIFY_PERMISSION)),
    ]);
    assert!(modifier.allows(&access));
    assert!(modifier.allows(&modify));
}

#[test]
fn invsee_rejects_players_in_different_domains() {
    let source = test_player(24, "Viewer", 24);
    let target = test_player(25, "Target", 25);
    assert!(ensure_same_domain(&source, &target).is_ok());

    let switch_token = begin_domain_switch(&target);
    assert!(ensure_same_domain(&source, &target).is_err());
    finish_domain_switch(&target, switch_token);
    assert!(ensure_same_domain(&source, &target).is_ok());

    target.set_world(fresh_test_world_in_domain("other", "invsee_target"));

    assert!(ensure_same_domain(&source, &target).is_err());
}

#[test]
fn readonly_target_slots_reject_pickup_and_creative_clone() {
    let source = test_player(1, "Viewer", 1);
    let target = test_player(2, "Target", 2);
    source.restore_game_modes(GameType::Creative, None);
    target
        .inventory
        .lock()
        .set_item(0, ItemStack::with_count(&vanilla_items::STONE, 5));
    let mut menu = invsee(1, &source, &target, false);

    menu.clicked(
        Click::Pickup {
            slot: TARGET_HOTBAR_START,
            button: MouseButton::Left,
        },
        &source,
    );
    menu.clicked(
        Click::Clone {
            slot: TARGET_HOTBAR_START,
        },
        &source,
    );

    assert_eq!(target.inventory.lock().get_item(0).count(), 5);
    assert!(menu.behavior().carried().is_empty());
}

#[test]
fn modify_view_edits_armor_slots_within_equipment_rules() {
    let source = test_player(8, "Viewer", 8);
    let target = test_player(9, "Target", 9);
    let mut menu = invsee(1, &source, &target, true);

    *menu.behavior_mut().carried_mut() = ItemStack::new(&vanilla_items::STONE);
    menu.clicked(
        Click::Pickup {
            slot: TARGET_ARMOR_START,
            button: MouseButton::Left,
        },
        &source,
    );
    assert!(
        target.inventory.lock().get_item(39).is_empty(),
        "a non-equippable item must not enter the head slot"
    );
    assert!(menu.behavior().carried().is(&vanilla_items::STONE));

    *menu.behavior_mut().carried_mut() = ItemStack::new(&vanilla_items::IRON_HELMET);
    menu.clicked(
        Click::Pickup {
            slot: TARGET_ARMOR_START,
            button: MouseButton::Left,
        },
        &source,
    );

    assert!(
        target
            .inventory
            .lock()
            .get_item(39)
            .is(&vanilla_items::IRON_HELMET)
    );
    assert!(menu.behavior().carried().is_empty());
}

#[test]
fn modify_view_synchronizes_target_armor_without_inventory_locks() {
    let source = test_player(10, "Viewer", 10);
    let target = recording_player(11, "Target", 11);
    target
        .inventories
        .lock()
        .extend([source.inventory.clone(), target.player.inventory.clone()]);
    let mut menu = invsee(1, &source, &target.player, true);
    *menu.behavior_mut().carried_mut() = ItemStack::new(&vanilla_items::IRON_HELMET);

    menu.clicked(
        Click::Pickup {
            slot: TARGET_ARMOR_START,
            button: MouseButton::Left,
        },
        &source,
    );

    assert!(
        target
            .player
            .inventory
            .lock()
            .get_item(39)
            .is(&vanilla_items::IRON_HELMET)
    );
    target.player.tick();
    assert_eq!(
        player_inventory_updates(&target.packets),
        vec![(39, ItemStack::new(&vanilla_items::IRON_HELMET))]
    );
    assert!(
        target
            .callbacks_saw_unlocked_inventories
            .load(Ordering::Acquire),
        "direct inventory packets must be sent after releasing source and target inventories"
    );
}

#[test]
fn self_invsee_synchronizes_own_armor_slot() {
    let recording = recording_player(12, "SelfViewer", 12);
    recording
        .inventories
        .lock()
        .push(recording.player.inventory.clone());
    let mut menu = invsee(1, &recording.player, &recording.player, true);
    *menu.behavior_mut().carried_mut() = ItemStack::new(&vanilla_items::IRON_HELMET);

    menu.clicked(
        Click::Pickup {
            slot: TARGET_ARMOR_START,
            button: MouseButton::Left,
        },
        &recording.player,
    );

    recording.player.tick();
    assert_eq!(
        player_inventory_updates(&recording.packets),
        vec![(39, ItemStack::new(&vanilla_items::IRON_HELMET))]
    );
    assert!(
        recording
            .callbacks_saw_unlocked_inventories
            .load(Ordering::Acquire)
    );
}

#[test]
fn modify_view_synchronizes_empty_offhand_after_removal() {
    let source = test_player(13, "Viewer", 13);
    let target = recording_player(14, "Target", 14);
    target
        .player
        .inventory
        .lock()
        .set_item(40, ItemStack::new(&vanilla_items::SHIELD));
    let mut menu = invsee(1, &source, &target.player, true);

    menu.clicked(
        Click::Pickup {
            slot: TARGET_ARMOR_START + 4,
            button: MouseButton::Left,
        },
        &source,
    );

    assert!(target.player.inventory.lock().get_item(40).is_empty());
    target.player.tick();
    assert_eq!(
        player_inventory_updates(&target.packets),
        vec![(40, ItemStack::empty())]
    );
}

#[test]
fn modify_view_synchronizes_target_hotbar_slot() {
    let source = test_player(20, "Viewer", 20);
    let target = recording_player(21, "Target", 21);
    let mut menu = invsee(1, &source, &target.player, true);
    *menu.behavior_mut().carried_mut() = ItemStack::new(&vanilla_items::STONE);

    menu.clicked(
        Click::Pickup {
            slot: TARGET_HOTBAR_START,
            button: MouseButton::Left,
        },
        &source,
    );
    target.player.tick();

    assert_eq!(
        player_inventory_updates(&target.packets),
        vec![(0, ItemStack::new(&vanilla_items::STONE))]
    );
}

#[test]
fn modify_view_coalesces_to_latest_target_inventory_value() {
    let source = test_player(15, "Viewer", 15);
    let target = recording_player(16, "Target", 16);
    let mut menu = invsee(1, &source, &target.player, true);

    *menu.behavior_mut().carried_mut() = ItemStack::new(&vanilla_items::IRON_HELMET);
    menu.clicked(
        Click::Pickup {
            slot: TARGET_ARMOR_START,
            button: MouseButton::Left,
        },
        &source,
    );
    *menu.behavior_mut().carried_mut() = ItemStack::new(&vanilla_items::DIAMOND_HELMET);
    menu.clicked(
        Click::Pickup {
            slot: TARGET_ARMOR_START,
            button: MouseButton::Left,
        },
        &source,
    );

    target.player.tick();

    assert_eq!(
        player_inventory_updates(&target.packets),
        vec![(39, ItemStack::new(&vanilla_items::DIAMOND_HELMET))]
    );
}

#[test]
fn modify_view_drag_queues_each_changed_target_slot() {
    let source = test_player(17, "Viewer", 17);
    let target = recording_player(18, "Target", 18);
    let mut menu = invsee(1, &source, &target.player, true);
    *menu.behavior_mut().carried_mut() = ItemStack::with_count(&vanilla_items::STONE, 2);

    for action in [
        QuickCraft::Start {
            kind: DragKind::Left,
        },
        QuickCraft::AddSlot {
            slot: TARGET_HOTBAR_START,
        },
        QuickCraft::AddSlot {
            slot: TARGET_HOTBAR_START + 1,
        },
        QuickCraft::End,
    ] {
        menu.clicked(Click::QuickCraft(action), &source);
    }
    target.player.tick();

    assert_eq!(
        player_inventory_updates(&target.packets),
        vec![
            (0, ItemStack::new(&vanilla_items::STONE)),
            (1, ItemStack::new(&vanilla_items::STONE)),
        ]
    );
}

#[test]
fn overriding_menu_defers_main_inventory_sync_until_close() {
    let recording = recording_player(19, "OverlayViewer", 19);
    recording
        .inventories
        .lock()
        .push(recording.player.inventory.clone());
    recording
        .player
        .inventory
        .lock()
        .set_item(0, ItemStack::new(&vanilla_items::STONE));
    recording
        .player
        .inventory
        .lock()
        .set_item(39, ItemStack::new(&vanilla_items::DIAMOND_HELMET));

    let fake_slots = SimpleContainer::new(72).into_shared();
    recording.player.open_menu("Overlay", move |context| {
        let mut builder = MenuBuilder::new(&vanilla_menu_types::GENERIC_9X4, context.container_id);
        builder.section_with(fake_slots, 72, SectionKind::Display);
        builder.override_player_slots();
        builder.build(BasicKind {})
    });
    recording.packets.lock().clear();
    recording.player.request_inventory_resync([0, 39]);

    recording.player.tick();

    assert_eq!(
        player_inventory_updates(&recording.packets),
        vec![(39, ItemStack::new(&vanilla_items::DIAMOND_HELMET))]
    );

    recording.packets.lock().clear();
    recording.player.do_close_container();
    recording.player.tick();

    let updates = player_inventory_updates(&recording.packets);
    assert_eq!(updates.len(), PlayerInventory::INVENTORY_SIZE);
    assert_eq!(updates[0], (0, ItemStack::new(&vanilla_items::STONE)));
    assert_eq!(
        updates.iter().map(|(slot, _)| *slot).collect::<Vec<_>>(),
        (0..PlayerInventory::INVENTORY_SIZE as i32).collect::<Vec<_>>()
    );
}

#[test]
fn replacing_overriding_menu_keeps_main_inventory_sync_deferred() {
    let recording = recording_player(23, "ReplacementOverlayViewer", 23);
    recording
        .player
        .inventory
        .lock()
        .set_item(0, ItemStack::new(&vanilla_items::STONE));

    for title in ["First overlay", "Second overlay"] {
        let fake_slots = SimpleContainer::new(72).into_shared();
        recording.player.open_menu(title, move |context| {
            let mut builder =
                MenuBuilder::new(&vanilla_menu_types::GENERIC_9X4, context.container_id);
            builder.section_with(fake_slots, 72, SectionKind::Display);
            builder.override_player_slots();
            builder.build(BasicKind {})
        });
        recording.player.request_inventory_resync([0]);
    }
    recording.packets.lock().clear();

    recording.player.tick();
    assert!(player_inventory_updates(&recording.packets).is_empty());

    recording.player.do_close_container();
    recording.player.tick();

    let updates = player_inventory_updates(&recording.packets);
    assert_eq!(updates.len(), PlayerInventory::INVENTORY_SIZE);
    assert_eq!(updates[0], (0, ItemStack::new(&vanilla_items::STONE)));
}

#[test]
fn normal_menu_does_not_defer_main_inventory_sync() {
    let recording = recording_player(22, "NormalMenuViewer", 22);
    recording
        .player
        .inventory
        .lock()
        .set_item(0, ItemStack::new(&vanilla_items::STONE));

    let menu_slots = SimpleContainer::new(9).into_shared();
    let inventory = recording.player.inventory.clone();
    recording.player.open_menu("Normal", move |context| {
        let mut builder = MenuBuilder::new(&vanilla_menu_types::GENERIC_9X1, context.container_id);
        builder.section_with(menu_slots, 9, SectionKind::Display);
        builder.player_inventory(&inventory);
        builder.build(BasicKind {})
    });
    recording.packets.lock().clear();
    recording.player.request_inventory_resync([0]);

    recording.player.tick();

    assert_eq!(
        player_inventory_updates(&recording.packets),
        vec![(0, ItemStack::new(&vanilla_items::STONE))]
    );
}

#[test]
fn modify_view_moves_inventory_items_in_both_directions() {
    let source = test_player(8, "Viewer", 8);
    let target = test_player(9, "Target", 9);
    target
        .inventory
        .lock()
        .set_item(0, ItemStack::with_count(&vanilla_items::STONE, 5));
    let mut menu = invsee(1, &source, &target, true);

    menu.clicked(
        Click::QuickMove {
            slot: TARGET_HOTBAR_START,
        },
        &source,
    );
    assert!(target.inventory.lock().get_item(0).is_empty());
    assert_eq!(source.inventory.lock().get_item(8).count(), 5);

    menu.clicked(
        Click::QuickMove {
            slot: VIEWER_INVENTORY_START + 35,
        },
        &source,
    );
    assert!(source.inventory.lock().get_item(8).is_empty());
    assert_eq!(target.inventory.lock().get_item(9).count(), 5);
}

#[test]
fn modify_view_extracts_but_cannot_insert_crafting_items() {
    let source = test_player(3, "Viewer", 3);
    let target = test_player(4, "Target", 4);
    let handler = target.inventory_crafting_handler();
    handler
        .crafting_container()
        .lock()
        .set_item(0, ItemStack::new(&vanilla_items::OAK_LOG));
    let mut menu = invsee(1, &source, &target, true);
    menu.on_open(&source);

    {
        let guard = menu.behavior().lock_all_containers();
        let result = guard
            .get(handler.result_id())
            .expect("result container is registered with the menu");
        assert!(result.get_item(0).is(&vanilla_items::OAK_PLANKS));
        assert_eq!(result.get_item(0).count(), 4);
    }

    menu.clicked(
        Click::Pickup {
            slot: TARGET_CRAFTING_START,
            button: MouseButton::Left,
        },
        &source,
    );
    assert!(handler.crafting_container().lock().get_item(0).is_empty());
    {
        let guard = menu.behavior().lock_all_containers();
        let result = guard
            .get(handler.result_id())
            .expect("result container is registered with the menu");
        assert!(result.get_item(0).is_empty());
    }
    assert!(menu.behavior().carried().is(&vanilla_items::OAK_LOG));

    menu.clicked(
        Click::Pickup {
            slot: TARGET_CRAFTING_START,
            button: MouseButton::Left,
        },
        &source,
    );
    assert!(handler.crafting_container().lock().get_item(0).is_empty());
    assert!(menu.behavior().carried().is(&vanilla_items::OAK_LOG));
}

#[test]
fn self_invsee_quick_move_does_not_rearrange_the_aliased_inventory() {
    let player = test_player(5, "SelfViewer", 5);
    player
        .inventory
        .lock()
        .set_item(0, ItemStack::with_count(&vanilla_items::STONE, 5));
    let mut menu = invsee(1, &player, &player, true);

    menu.clicked(
        Click::QuickMove {
            slot: TARGET_HOTBAR_START,
        },
        &player,
    );
    menu.clicked(
        Click::QuickMove {
            slot: VIEWER_INVENTORY_START + TARGET_HOTBAR_START,
        },
        &player,
    );

    let inventory = player.inventory.lock();
    assert_eq!(inventory.get_item(0).count(), 5);
    assert!(
        (1..=40).all(|slot| inventory.get_item(slot).is_empty()),
        "self quick-move must not relocate the source stack"
    );
}

#[test]
fn open_menu_keeps_captured_access_and_tracks_target_lifecycle() {
    let source = test_player(6, "Viewer", 6);
    let target = test_player(7, "Target", 7);

    set_permissions(
        &source,
        PermissionSet::from_entries([PermissionEntry::allow(permission_key(MODIFY_PERMISSION))]),
    );
    let modify_menu = invsee(1, &source, &target, true);
    assert!(modify_menu.still_valid(&source));

    set_permissions(&source, PermissionSet::new());
    assert!(
        modify_menu.still_valid(&source),
        "an opened menu keeps the access and modify mode authorized by its command"
    );

    let readonly_menu = invsee(2, &source, &target, false);
    assert!(readonly_menu.still_valid(&source));
    let source_switch_token = begin_domain_switch(&source);
    assert!(!readonly_menu.still_valid(&source));
    finish_domain_switch(&source, source_switch_token);
    assert!(readonly_menu.still_valid(&source));

    source.set_world(fresh_test_world_in_domain("other", "invsee_viewer"));
    assert!(!readonly_menu.still_valid(&source));
    source.set_world(Arc::clone(test_world()));
    assert!(readonly_menu.still_valid(&source));

    let target_switch_token = begin_domain_switch(&target);
    assert!(!readonly_menu.still_valid(&source));
    finish_domain_switch(&target, target_switch_token);
    assert!(readonly_menu.still_valid(&source));

    target.set_world(fresh_test_world_in_domain("other", "invsee_domain"));
    assert!(!readonly_menu.still_valid(&source));
    target.set_world(Arc::clone(test_world()));
    assert!(readonly_menu.still_valid(&source));

    target.close_connection();
    assert!(!readonly_menu.still_valid(&source));
}
