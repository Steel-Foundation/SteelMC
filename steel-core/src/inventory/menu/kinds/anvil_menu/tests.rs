use std::sync::Arc;

use glam::DVec3;
use steel_registry::{
    REGISTRY, RegistryExt, RegistryReference,
    data_components::{
        components::PotionContents,
        vanilla_components::{CUSTOM_NAME, POTION_CONTENTS, REPAIR_COST},
    },
    item_stack::ItemStack,
    test_support::init_test_registry,
    vanilla_blocks, vanilla_enchantments, vanilla_entities, vanilla_items,
};
use steel_utils::Downcast as _;
use steel_utils::{
    BlockPos, ChunkPos, WorldAabb,
    types::{GameType, UpdateFlags},
};
use uuid::Uuid;

use super::{AnvilKind, anvil};
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

fn test_player(world: Arc<World>) -> Arc<Player> {
    TestPlayerBuilder::new(world, Uuid::from_u128(1), "AnvilTester", 1).build()
}

fn test_anvil(key: &'static str) -> (Arc<World>, Arc<Player>, BlockPos, Menu) {
    init_test_registry();
    init_behaviors();
    let world = fresh_test_world(key);
    let pos = BlockPos::new(0, 64, 0);
    insert_ready_full_chunk(&world, ChunkPos::from_block_pos(pos));
    assert!(world.set_block(
        pos,
        vanilla_blocks::ANVIL.default_state(),
        UpdateFlags::UPDATE_ALL,
    ));
    let player = test_player(Arc::clone(&world));
    player.base().set_position_local(DVec3::new(0.5, 64.0, 0.5));
    let menu = anvil(Arc::clone(&player.inventory), 1, pos, &world);
    (world, player, pos, menu)
}

#[test]
fn validity_requires_anvil_tag_and_interaction_range() {
    let (world, player, pos, menu) = test_anvil("anvil_menu_validity");
    assert!(menu.still_valid(&player));

    assert!(world.set_block(
        pos,
        vanilla_blocks::CHIPPED_ANVIL.default_state(),
        UpdateFlags::UPDATE_ALL,
    ));
    assert!(menu.still_valid(&player));

    let current_world = fresh_test_world("anvil_menu_validity_current_world");
    insert_ready_full_chunk(&current_world, ChunkPos::from_block_pos(pos));
    player.set_world(Arc::clone(&current_world));
    assert!(menu.still_valid(&player));

    assert!(world.set_block(
        pos,
        vanilla_blocks::AIR.default_state(),
        UpdateFlags::UPDATE_ALL,
    ));
    assert!(current_world.set_block(
        pos,
        vanilla_blocks::ANVIL.default_state(),
        UpdateFlags::UPDATE_ALL,
    ));
    assert!(!menu.still_valid(&player));

    assert!(world.set_block(
        pos,
        vanilla_blocks::ANVIL.default_state(),
        UpdateFlags::UPDATE_ALL,
    ));
    player
        .base()
        .set_position_local(DVec3::new(20.0, 64.0, 0.5));
    assert!(!menu.still_valid(&player));
}

#[test]
fn sacrifice_enchantments_conflict_with_earlier_merges() {
    let (_world, player, _pos, mut menu) = test_anvil("anvil_menu_enchantment_conflict");
    let Some(kind) = menu.kind().downcast_ref::<AnvilKind>() else {
        panic!("anvil builder should create an anvil menu");
    };
    let (input_container, result_container) = (
        Arc::clone(&kind.input_container),
        Arc::clone(&kind.result_container),
    );

    let mut book = ItemStack::new(&vanilla_items::ENCHANTED_BOOK);
    book.set_enchantments(
        &[
            (vanilla_enchantments::SHARPNESS.key.clone(), 1),
            (vanilla_enchantments::SMITE.key.clone(), 1),
        ],
        false,
    );
    {
        let mut input = input_container.lock();
        input.set_item(0, ItemStack::new(&vanilla_items::DIAMOND_SWORD));
        input.set_item(1, book);
    }

    menu.set_item_name("", &player);

    let result = result_container.lock().get_item(0).clone();
    let Some(enchantments) = result.get_enchantments_for_crafting() else {
        panic!("anvil result should contain one compatible damage enchantment");
    };
    let damage_enchantment_count = [
        &vanilla_enchantments::SHARPNESS.key,
        &vanilla_enchantments::SMITE.key,
    ]
    .into_iter()
    .filter(|key| enchantments.get_level(key) > 0)
    .count();
    assert_eq!(damage_enchantment_count, 1);
}

#[test]
fn rename_only_result_preserves_unused_second_input() {
    let (_world, player, _pos, mut menu) = test_anvil("anvil_menu_rename_only");
    player.restore_game_modes(GameType::Creative, None);
    let Some(kind) = menu.kind().downcast_ref::<AnvilKind>() else {
        panic!("anvil builder should create an anvil menu");
    };
    let input_container = Arc::clone(&kind.input_container);
    {
        let mut input = input_container.lock();
        input.set_item(0, ItemStack::new(&vanilla_items::DIAMOND_SWORD));
        input.set_item(1, ItemStack::new(&vanilla_items::DIAMOND_SWORD));
    }

    menu.set_item_name("Renamed", &player);
    menu.clicked(
        Click::Pickup {
            slot: 2,
            button: MouseButton::Left,
        },
        &player,
    );

    let input = input_container.lock();
    assert!(input.get_item(0).is_empty());
    assert!(input.get_item(1).is(&vanilla_items::DIAMOND_SWORD));
    assert!(menu.behavior().carried().is(&vanilla_items::DIAMOND_SWORD));
}

#[test]
fn rename_only_result_restores_default_repair_cost_component() {
    let (_world, player, _pos, mut menu) = test_anvil("anvil_menu_default_repair_cost");
    let Some(kind) = menu.kind().downcast_ref::<AnvilKind>() else {
        panic!("anvil builder should create an anvil menu");
    };
    let (input_container, result_container) = (
        Arc::clone(&kind.input_container),
        Arc::clone(&kind.result_container),
    );

    let mut input = ItemStack::new(&vanilla_items::DIAMOND_SWORD);
    input.remove(REPAIR_COST);
    assert!(input.components_patch().is_removed(REPAIR_COST.key()));
    input_container.lock().set_item(0, input);

    menu.set_item_name("Renamed", &player);

    let result = result_container.lock().get_item(0).clone();
    assert_eq!(result.get(REPAIR_COST), Some(&0));
    assert!(!result.components_patch().is_removed(REPAIR_COST.key()));
}

#[test]
fn rename_validation_counts_filtered_java_utf16_units() {
    let maximum = "😀".repeat(25);
    assert_eq!(
        AnvilKind::validate_item_name(maximum.clone()),
        Some(maximum.clone())
    );
    assert_eq!(AnvilKind::validate_item_name("😀".repeat(26)), None);
    assert_eq!(
        AnvilKind::validate_item_name(format!("{maximum}§")),
        Some(maximum)
    );
}

#[test]
fn rename_uses_java_blank_rules() {
    let (_world, player, _pos, mut menu) = test_anvil("anvil_menu_java_blank_rename");
    let Some(kind) = menu.kind().downcast_ref::<AnvilKind>() else {
        panic!("anvil builder should create an anvil menu");
    };
    let (input_container, result_container) = (
        Arc::clone(&kind.input_container),
        Arc::clone(&kind.result_container),
    );
    input_container
        .lock()
        .set_item(0, ItemStack::new(&vanilla_items::DIAMOND_SWORD));

    menu.set_item_name("\u{0085}", &player);

    let result = result_container.lock().get_item(0).clone();
    assert_eq!(
        result.get(CUSTOM_NAME).map(ToString::to_string).as_deref(),
        Some("\u{0085}")
    );
}

#[test]
fn unchanged_extended_potion_name_does_not_create_rename_result() {
    let (_world, player, _pos, mut menu) = test_anvil("anvil_dynamic_potion_name");
    let Some(kind) = menu.kind().downcast_ref::<AnvilKind>() else {
        panic!("anvil builder should create an anvil menu");
    };
    let (input_container, result_container, level_cost) = (
        Arc::clone(&kind.input_container),
        Arc::clone(&kind.result_container),
        kind.level_cost,
    );
    let long_swiftness = REGISTRY
        .potions
        .by_key(&steel_utils::Identifier::vanilla_static("long_swiftness"))
        .expect("long swiftness potion should be registered");
    let mut potion = ItemStack::new(&vanilla_items::POTION);
    potion.set(
        POTION_CONTENTS,
        PotionContents::new(
            Some(RegistryReference::new(long_swiftness)),
            None,
            Vec::new(),
            None,
        ),
    );
    input_container.lock().set_item(0, potion);

    menu.set_item_name("Potion of Swiftness", &player);

    assert!(result_container.lock().get_item(0).is_empty());
    assert_eq!(level_cost.get(menu.behavior()), 0);
}

#[test]
fn full_inputs_do_not_fallback_to_the_hotbar() {
    let (_world, player, _pos, mut menu) = test_anvil("anvil_menu_full_input_quick_move");
    let Some(kind) = menu.kind().downcast_ref::<AnvilKind>() else {
        panic!("anvil builder should create an anvil menu");
    };
    let input_container = Arc::clone(&kind.input_container);
    {
        let mut input = input_container.lock();
        input.set_item(0, ItemStack::with_count(&vanilla_items::STONE, 64));
        input.set_item(1, ItemStack::with_count(&vanilla_items::STONE, 64));
    }
    player
        .inventory
        .lock()
        .set_item(9, ItemStack::with_count(&vanilla_items::DIRT, 5));

    menu.clicked(Click::QuickMove { slot: 3 }, &player);

    let inventory = player.inventory.lock();
    assert_eq!(inventory.get_item(9).count(), 5);
    assert!((0..9).all(|slot| inventory.get_item(slot).is_empty()));
}

#[test]
fn client_level_cost_uses_protocol_short_wrapping() {
    for (cost, expected) in [
        (32_767, 32_767),
        (32_768, -32_768),
        (32_769, -32_767),
        (65_536, 0),
    ] {
        assert_eq!(AnvilKind::client_cost(cost), expected);
    }
}

#[test]
fn partial_result_overflow_is_discarded() {
    let (world, player, _pos, mut menu) = test_anvil("anvil_menu_partial_result_overflow");
    player.restore_game_modes(GameType::Creative, None);
    let Some(kind) = menu.kind().downcast_ref::<AnvilKind>() else {
        panic!("anvil builder should create an anvil menu");
    };
    let (input_container, result_container) = (
        Arc::clone(&kind.input_container),
        Arc::clone(&kind.result_container),
    );
    input_container
        .lock()
        .set_item(0, ItemStack::with_count(&vanilla_items::STONE, 64));
    menu.set_item_name("Renamed", &player);

    let result = result_container.lock().get_item(0).clone();
    assert_eq!(result.count(), 64);
    let mut matching = result.clone();
    matching.set_count(63);
    {
        let mut inventory = player.inventory.lock();
        for slot in 0..36 {
            inventory.set_item(slot, ItemStack::with_count(&vanilla_items::DIRT, 64));
        }
        inventory.set_item(8, matching);
    }

    menu.clicked(Click::QuickMove { slot: 2 }, &player);

    assert!(input_container.lock().get_item(0).is_empty());
    assert!(result_container.lock().get_item(0).is_empty());
    assert_eq!(player.inventory.lock().get_item(8).count(), 64);
    let dropped = world.get_entities_in_aabb_matching(
        &WorldAabb::new(-2.0, 62.0, -2.0, 2.0, 68.0, 2.0),
        |entity| entity.entity_type() == &vanilla_entities::ITEM,
    );
    assert!(dropped.is_empty());
}
