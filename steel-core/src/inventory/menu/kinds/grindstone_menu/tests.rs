use std::sync::Arc;

use glam::DVec3;
use steel_registry::{
    init_vanilla_registry, item_stack::ItemStack, vanilla_blocks, vanilla_enchantments,
    vanilla_entities, vanilla_items,
};
use steel_utils::{BlockPos, ChunkPos, Downcast as _, WorldAabb, types::UpdateFlags};

use super::{GrindstoneKind, grindstone};
use crate::{
    behavior::init_behaviors,
    entity::Entity as _,
    inventory::{
        click::{Click, MouseButton},
        container::Container as _,
        menu::Menu,
    },
    player::Player,
    test_support::{TestPlayerBuilder, fresh_test_world, insert_ready_full_chunk},
    world::World,
};

fn test_grindstone(key: &'static str) -> (Arc<World>, Arc<Player>, BlockPos, Menu) {
    init_vanilla_registry();
    init_behaviors();
    let world = fresh_test_world(key);
    let pos = BlockPos::new(0, 64, 0);
    insert_ready_full_chunk(&world, ChunkPos::from_block_pos(pos));
    assert!(world.set_block(
        pos,
        vanilla_blocks::GRINDSTONE.default_state(),
        UpdateFlags::UPDATE_ALL,
    ));
    let player = TestPlayerBuilder::new(Arc::clone(&world), "GrindstoneTester", 1).build();
    player.base().set_position_local(DVec3::new(0.5, 64.0, 0.5));
    let menu = grindstone(Arc::clone(&player.inventory), 1, pos, &world);
    (world, player, pos, menu)
}

fn sharpened_sword() -> ItemStack {
    let mut sword = ItemStack::new(&vanilla_items::DIAMOND_SWORD);
    sword.set_enchantments(&[(vanilla_enchantments::SHARPNESS.key.clone(), 3)], false);
    sword
}

#[test]
fn placing_an_enchanted_item_computes_a_stripped_result() {
    let (_world, player, _pos, mut menu) = test_grindstone("grindstone_menu_compute");
    let Some(kind) = menu.kind().downcast_ref::<GrindstoneKind>() else {
        panic!("grindstone builder should create a grindstone menu");
    };
    let result_container = kind.handler.result_container_handle();

    *menu.behavior_mut().carried_mut() = sharpened_sword();
    menu.clicked(
        Click::Pickup {
            slot: 0,
            button: MouseButton::Left,
        },
        &player,
    );

    let result = result_container.lock().get_item(0).clone();
    assert!(result.is(&vanilla_items::DIAMOND_SWORD));
    assert!(
        result
            .get_enchantments_for_crafting()
            .is_none_or(|enchantments| enchantments
                .get_level(&vanilla_enchantments::SHARPNESS.key)
                == 0)
    );
}

#[test]
fn taking_the_result_awards_experience_and_clears_the_inputs() {
    let (world, player, pos, mut menu) = test_grindstone("grindstone_menu_take");
    let Some(kind) = menu.kind().downcast_ref::<GrindstoneKind>() else {
        panic!("grindstone builder should create a grindstone menu");
    };
    let input_container = kind.handler.input_container();

    *menu.behavior_mut().carried_mut() = sharpened_sword();
    menu.clicked(
        Click::Pickup {
            slot: 0,
            button: MouseButton::Left,
        },
        &player,
    );
    menu.clicked(
        Click::Pickup {
            slot: 2,
            button: MouseButton::Left,
        },
        &player,
    );

    assert!(menu.behavior().carried().is(&vanilla_items::DIAMOND_SWORD));
    assert!(input_container.lock().get_item(0).is_empty());
    assert!(input_container.lock().get_item(1).is_empty());

    let orbs = world.get_entities_in_aabb_matching(
        &WorldAabb::new(
            f64::from(pos.x()) - 2.0,
            f64::from(pos.y()) - 2.0,
            f64::from(pos.z()) - 2.0,
            f64::from(pos.x()) + 2.0,
            f64::from(pos.y()) + 2.0,
            f64::from(pos.z()) + 2.0,
        ),
        |entity| entity.entity_type() == &vanilla_entities::EXPERIENCE_ORB,
    );
    assert!(
        !orbs.is_empty(),
        "disenchanting should spawn experience orbs"
    );
}

#[test]
fn input_slots_reject_items_that_cannot_be_ground() {
    let (_world, player, _pos, mut menu) = test_grindstone("grindstone_menu_reject");

    *menu.behavior_mut().carried_mut() = ItemStack::with_count(&vanilla_items::STONE, 16);
    menu.clicked(
        Click::Pickup {
            slot: 0,
            button: MouseButton::Left,
        },
        &player,
    );

    let Some(kind) = menu.kind().downcast_ref::<GrindstoneKind>() else {
        panic!("grindstone builder should create a grindstone menu");
    };
    assert!(kind.handler.input_container().lock().get_item(0).is_empty());
    assert!(menu.behavior().carried().is(&vanilla_items::STONE));
}

#[test]
fn validity_follows_the_grindstone_block() {
    let (world, player, pos, menu) = test_grindstone("grindstone_menu_validity");
    assert!(menu.still_valid(&player));

    assert!(world.set_block(
        pos,
        vanilla_blocks::AIR.default_state(),
        UpdateFlags::UPDATE_ALL,
    ));
    assert!(!menu.still_valid(&player));
}
