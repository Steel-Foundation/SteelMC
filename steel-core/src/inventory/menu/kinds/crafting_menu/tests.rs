use std::sync::Arc;

use super::{CraftingKind, crafting};
use crate::{
    behavior::init_behaviors,
    entity::{Entity as _, entities::ItemEntity},
    inventory::{
        click::{Click, MouseButton, SwapTarget},
        container::Container as _,
        menu::Menu,
    },
    player::Player,
    test_support::{TestPlayerBuilder, TestWorld, fresh_test_world, insert_ready_full_chunk},
};
use glam::DVec3;
use steel_registry::{
    init_vanilla_registry, item_stack::ItemStack, items::Item, stat::vanilla_stat_types,
    vanilla_blocks, vanilla_entities, vanilla_items,
};
use steel_utils::types::UpdateFlags;
use steel_utils::{BlockPos, ChunkPos, Downcast as _, WorldAabb};

#[test]
fn partial_result_overflow_uses_the_default_drop_policy() {
    init_vanilla_registry();
    init_behaviors();
    let world_fixture = fresh_test_world("crafting_menu_partial_result_overflow");
    let world = &world_fixture.world;
    let pos = BlockPos::new(0, 64, 0);
    insert_ready_full_chunk(world, ChunkPos::from_block_pos(pos));
    assert!(world.set_block(
        pos,
        vanilla_blocks::CRAFTING_TABLE.default_state(),
        UpdateFlags::UPDATE_ALL,
    ));
    let player = TestPlayerBuilder::new(Arc::clone(world), "Crafter", 1).build();
    player.base().set_position_local(DVec3::new(0.5, 64.0, 0.5));
    let mut menu = crafting(Arc::clone(&player.inventory), 1, pos);
    let Some(kind) = menu.kind().downcast_ref::<CraftingKind>() else {
        panic!("crafting builder should create a crafting menu");
    };
    let (crafting_container, result_id) =
        (kind.handler.crafting_container(), kind.handler.result_id());

    *menu.behavior_mut().carried_mut() = ItemStack::new(&vanilla_items::OAK_LOG);
    menu.clicked(
        Click::Pickup {
            slot: 1,
            button: MouseButton::Left,
        },
        &player,
    );
    let result = menu
        .behavior()
        .lock_all_containers()
        .expect("container access should succeed")
        .get(result_id)
        .expect("result container is registered with the menu")
        .get_item(0)
        .clone();
    assert!(result.is(&vanilla_items::OAK_PLANKS));
    assert_eq!(result.count(), 4);

    let mut matching = result;
    matching.set_count(63);
    {
        let mut inventory = player.inventory.lock();
        for slot in 0..36 {
            inventory.set_item(slot, ItemStack::with_count(&vanilla_items::DIRT, 64));
        }
        inventory.set_item(8, matching);
    }

    menu.clicked(Click::QuickMove { slot: 0 }, &player);

    assert!(crafting_container.lock().get_item(0).is_empty());
    assert!(
        menu.behavior()
            .lock_all_containers()
            .expect("container access should succeed")
            .get(result_id)
            .expect("result container is registered with the menu")
            .get_item(0)
            .is_empty()
    );
    assert_eq!(player.inventory.lock().get_item(8).count(), 64);
    let dropped = world.get_entities_in_aabb_matching(
        &WorldAabb::new(-2.0, 62.0, -2.0, 2.0, 68.0, 2.0),
        |entity| entity.entity_type() == &vanilla_entities::ITEM,
    );
    assert_eq!(dropped.len(), 1);
    let Some(item) = dropped[0].as_ref().downcast_ref::<ItemEntity>() else {
        panic!("dropped entity should retain its concrete item type");
    };
    assert!(item.get_item().is(&vanilla_items::OAK_PLANKS));
    assert_eq!(item.get_item().count(), 3);
    assert_eq!(item.get_thrower(), None);
}

fn crafted_count(player: &Player, item: &'static Item) -> i32 {
    let crafted = vanilla_stat_types::ITEM_CRAFTED.get(item);
    player
        .stats()
        .into_iter()
        .find_map(|(stat, count)| (stat == crafted).then_some(count))
        .unwrap_or(0)
}

#[test]
fn taking_the_result_awards_item_crafted() {
    init_vanilla_registry();
    init_behaviors();
    let world_fixture = fresh_test_world("crafting_menu_item_crafted");
    let world = &world_fixture.world;
    let pos = BlockPos::new(0, 64, 0);
    insert_ready_full_chunk(world, ChunkPos::from_block_pos(pos));
    assert!(world.set_block(
        pos,
        vanilla_blocks::CRAFTING_TABLE.default_state(),
        UpdateFlags::UPDATE_ALL,
    ));
    let player = TestPlayerBuilder::new(Arc::clone(world), "Crafter", 1).build();
    player.base().set_position_local(DVec3::new(0.5, 64.0, 0.5));
    let mut menu = crafting(Arc::clone(&player.inventory), 1, pos);

    *menu.behavior_mut().carried_mut() = ItemStack::new(&vanilla_items::OAK_LOG);
    menu.clicked(
        Click::Pickup {
            slot: 1,
            button: MouseButton::Left,
        },
        &player,
    );
    menu.clicked(
        Click::Pickup {
            slot: 0,
            button: MouseButton::Left,
        },
        &player,
    );
    assert_eq!(crafted_count(&player, &vanilla_items::OAK_PLANKS), 4);

    *menu.behavior_mut().carried_mut() = ItemStack::new(&vanilla_items::OAK_LOG);
    menu.clicked(
        Click::Pickup {
            slot: 1,
            button: MouseButton::Left,
        },
        &player,
    );
    menu.clicked(
        Click::Swap {
            slot: 0,
            with: SwapTarget::Hotbar(0),
        },
        &player,
    );
    assert_eq!(crafted_count(&player, &vanilla_items::OAK_PLANKS), 8);

    *menu.behavior_mut().carried_mut() = ItemStack::new(&vanilla_items::OAK_LOG);
    menu.clicked(
        Click::Pickup {
            slot: 1,
            button: MouseButton::Left,
        },
        &player,
    );
    menu.clicked(Click::QuickMove { slot: 0 }, &player);
    assert_eq!(crafted_count(&player, &vanilla_items::OAK_PLANKS), 12);
}

fn crafting_fixture(key: &'static str) -> (TestWorld, Arc<Player>, Menu) {
    init_vanilla_registry();
    init_behaviors();
    let world_fixture = fresh_test_world(key);
    let world = &world_fixture.world;
    let pos = BlockPos::new(0, 64, 0);
    insert_ready_full_chunk(world, ChunkPos::from_block_pos(pos));
    assert!(world.set_block(
        pos,
        vanilla_blocks::CRAFTING_TABLE.default_state(),
        UpdateFlags::UPDATE_ALL,
    ));
    let player = TestPlayerBuilder::new(Arc::clone(world), "Crafter", 1).build();
    player.base().set_position_local(DVec3::new(0.5, 64.0, 0.5));
    let menu = crafting(Arc::clone(&player.inventory), 1, pos);
    (world_fixture, player, menu)
}

/// Places `logs` oak logs in the first grid slot and refreshes the result.
fn seed_crafting_input(menu: &mut Menu, player: &Player, logs: i32) {
    *menu.behavior_mut().carried_mut() = ItemStack::with_count(&vanilla_items::OAK_LOG, logs);
    menu.clicked(
        Click::Pickup {
            slot: 1,
            button: MouseButton::Left,
        },
        player,
    );
}

fn result_stack(menu: &Menu) -> ItemStack {
    let Some(kind) = menu.kind().downcast_ref::<CraftingKind>() else {
        panic!("crafting builder should create a crafting menu");
    };
    menu.behavior()
        .lock_all_containers()
        .expect("crafting menu containers should be accessible")
        .get(kind.handler.result_id())
        .expect("result container is registered with the menu")
        .get_item(0)
        .clone()
}

fn crafting_input_count(menu: &Menu) -> i32 {
    let Some(kind) = menu.kind().downcast_ref::<CraftingKind>() else {
        panic!("crafting builder should create a crafting menu");
    };
    kind.handler.crafting_container().lock().get_item(0).count()
}

// Vanilla's `ResultSlot` counts `min(amount, stack size)` for every removed
// portion, including the half stack taken by a secondary click.
#[test]
fn right_click_pickup_awards_vanilla_half_count() {
    let (_world, player, mut menu) = crafting_fixture("crafting_menu_right_click");
    seed_crafting_input(&mut menu, &player, 1);

    menu.clicked(
        Click::Pickup {
            slot: 0,
            button: MouseButton::Right,
        },
        &player,
    );

    assert_eq!(crafted_count(&player, &vanilla_items::OAK_PLANKS), 2);
}

// Both shift-click buttons send `QUICK_MOVE`; `Click::parse` folds button 0 and
// 1 into the same `QuickMove`, so one path covers both.
#[test]
fn shift_click_awards_the_number_of_items_moved() {
    let (_world, player, mut menu) = crafting_fixture("crafting_menu_shift_click");
    seed_crafting_input(&mut menu, &player, 1);

    menu.clicked(Click::QuickMove { slot: 0 }, &player);

    assert_eq!(crafted_count(&player, &vanilla_items::OAK_PLANKS), 4);
    assert!(result_stack(&menu).is_empty());
}

// A slot that can only accept part of the result awards and consumes a full
// recipe for the moved portion, exactly like vanilla's `onQuickCraft` path.
#[test]
fn shift_click_awards_only_the_planks_that_fit() {
    let (_world, player, mut menu) = crafting_fixture("crafting_menu_shift_click_partial");
    seed_crafting_input(&mut menu, &player, 64);
    {
        let mut inventory = player.inventory.lock();
        for slot in 1..36 {
            inventory.set_item(slot, ItemStack::with_count(&vanilla_items::DIRT, 64));
        }
        inventory.set_item(0, ItemStack::with_count(&vanilla_items::OAK_PLANKS, 63));
    }

    menu.clicked(Click::QuickMove { slot: 0 }, &player);

    assert_eq!(crafted_count(&player, &vanilla_items::OAK_PLANKS), 1);
    assert_eq!(player.inventory.lock().get_item(0).count(), 64);
    assert_eq!(result_stack(&menu).count(), 4);
    assert_eq!(crafting_input_count(&menu), 63);
}

// With one free inventory slot, shift-clicking keeps crafting until that slot
// is full; the leftover result stack stays in the result slot.
#[test]
fn shift_click_crafts_until_the_inventory_is_full() {
    let (_world, player, mut menu) = crafting_fixture("crafting_menu_shift_click_full");
    seed_crafting_input(&mut menu, &player, 64);
    {
        let mut inventory = player.inventory.lock();
        for slot in 1..36 {
            inventory.set_item(slot, ItemStack::with_count(&vanilla_items::DIRT, 64));
        }
    }

    menu.clicked(Click::QuickMove { slot: 0 }, &player);

    assert_eq!(crafted_count(&player, &vanilla_items::OAK_PLANKS), 64);
    assert_eq!(player.inventory.lock().get_item(0).count(), 64);
    assert_eq!(result_stack(&menu).count(), 4);
    assert_eq!(crafting_input_count(&menu), 48);
}

// Q takes one result item and Ctrl+Q takes the whole result stack; vanilla
// records each taken portion through `ResultSlot::remove`.
#[test]
fn throwing_the_result_awards_vanilla_counts() {
    let (_world, player, mut menu) = crafting_fixture("crafting_menu_throw");
    seed_crafting_input(&mut menu, &player, 2);

    menu.clicked(
        Click::Throw {
            slot: 0,
            whole_stack: false,
        },
        &player,
    );
    assert_eq!(crafted_count(&player, &vanilla_items::OAK_PLANKS), 1);

    menu.clicked(
        Click::Throw {
            slot: 0,
            whole_stack: true,
        },
        &player,
    );
    assert_eq!(crafted_count(&player, &vanilla_items::OAK_PLANKS), 5);
}
