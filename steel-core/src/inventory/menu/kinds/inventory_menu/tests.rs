use std::sync::Arc;

use super::{InventoryKind, inventory_menu};
use crate::{
    behavior::init_behaviors,
    entity::{Entity as _, entities::ItemEntity},
    inventory::{
        click::{Click, MouseButton},
        container::Container as _,
    },
    test_support::{TestPlayerBuilder, fresh_test_world, insert_ready_full_chunk},
};
use glam::DVec3;
use steel_registry::{
    init_vanilla_registry, item_stack::ItemStack, vanilla_entities, vanilla_items,
};
use steel_utils::{ChunkPos, Downcast as _, WorldAabb};

#[test]
fn partial_result_overflow_has_no_thrower() {
    init_vanilla_registry();
    init_behaviors();
    let world = fresh_test_world("inventory_menu_partial_result_overflow");
    insert_ready_full_chunk(&world, ChunkPos::new(0, 0));
    let player = TestPlayerBuilder::new(Arc::clone(&world), "Crafter", 1).build();
    player.base().set_position_local(DVec3::new(0.5, 64.0, 0.5));
    let mut menu = inventory_menu(Arc::clone(&player.inventory));
    let Some(InventoryKind { handler, .. }) = menu.kind().downcast_ref::<InventoryKind>() else {
        panic!("inventory_menu should create an inventory menu");
    };
    let (crafting_container, result_id) = (handler.crafting_container(), handler.result_id());

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
