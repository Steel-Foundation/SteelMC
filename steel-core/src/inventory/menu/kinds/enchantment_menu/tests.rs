use std::sync::Arc;

use glam::DVec3;
use steel_registry::data_components::vanilla_components::STORED_ENCHANTMENTS;
use steel_registry::{init_vanilla_registry, item_stack::ItemStack, vanilla_blocks, vanilla_items};
use steel_utils::types::{GameType, UpdateFlags};
use steel_utils::{BlockPos, ChunkPos, Downcast as _};

use super::{EnchantmentKind, ITEM_SLOT, LAPIS_SLOT, OFFER_COUNT, enchantment};
use crate::behavior::blocks::EnchantingTableBlock;
use crate::behavior::init_behaviors;
use crate::block_entity::init_block_entities;
use crate::entity::Entity as _;
use crate::inventory::click::{Click, MouseButton};
use crate::inventory::container::Container as _;
use crate::inventory::menu::Menu;
use crate::player::Player;
use crate::test_support::{
    TestPlayerBuilder, TestWorld, fresh_test_world, insert_ready_full_chunk,
};
use crate::world::World;

/// Two table slots and 27 main-inventory slots precede the hotbar in the menu.
const HOTBAR_FIRST_MENU_SLOT: usize = 29;
const SEED: i32 = 0x1234_5678;
const HIGH_OFFER_INDEX: usize = 2;
const FULL_POWER_LEVELS: i32 = 30;
const HIGH_OFFER_LAPIS_COST: i32 = 3;
const NO_BOOKSHELVES_MAX_COST: i32 = 8;
const EXPECTED_SEED_DATA: usize = 3;
const EXPECTED_ENCHANT_CLUE_DATA_START: usize = 4;
const EXPECTED_LEVEL_CLUE_DATA_START: usize = 7;
const EXPECTED_NO_CLUE: i32 = -1;

fn test_table(key: &'static str, seed: i32) -> (TestWorld, Arc<Player>, BlockPos, Menu) {
    init_vanilla_registry();
    init_behaviors();
    init_block_entities();
    let world_fixture = fresh_test_world(key);
    let world = &world_fixture.world;
    let pos = BlockPos::new(8, 64, 8);
    insert_ready_full_chunk(world, ChunkPos::from_block_pos(pos));
    assert!(world.set_block(
        pos,
        vanilla_blocks::ENCHANTING_TABLE.default_state(),
        UpdateFlags::UPDATE_ALL,
    ));
    let player = TestPlayerBuilder::new(Arc::clone(world), "EnchantTester", 1).build();
    player.base().set_position_local(DVec3::new(8.5, 64.0, 8.5));
    let menu = enchantment(Arc::clone(&player.inventory), 1, pos, world, seed);
    (world_fixture, player, pos, menu)
}

fn surround_with_bookshelves(world: &Arc<World>, pos: BlockPos) {
    for offset in EnchantingTableBlock::BOOKSHELF_OFFSETS {
        assert!(world.set_block(
            pos.offset(offset.x, offset.y, offset.z),
            vanilla_blocks::BOOKSHELF.default_state(),
            UpdateFlags::UPDATE_ALL,
        ));
    }
}

/// Puts `stack` in hotbar slot `hotbar_slot` and shift-clicks it into the table.
fn place_in_table(menu: &mut Menu, player: &Player, stack: ItemStack, hotbar_slot: usize) {
    player.inventory.lock().set_item(hotbar_slot, stack);
    menu.clicked(
        Click::QuickMove {
            slot: HOTBAR_FIRST_MENU_SLOT + hotbar_slot,
        },
        player,
    );
}

fn kind(menu: &Menu) -> &EnchantmentKind {
    menu.kind()
        .downcast_ref::<EnchantmentKind>()
        .expect("builder should create an enchantment menu")
}

fn table_item(menu: &Menu) -> ItemStack {
    kind(menu).enchant_slots.lock().get_item(ITEM_SLOT).clone()
}

fn table_lapis(menu: &Menu) -> ItemStack {
    kind(menu).enchant_slots.lock().get_item(LAPIS_SLOT).clone()
}

#[test]
fn full_bookshelf_ring_rolls_three_offers_with_clues() {
    let (world_fixture, player, pos, mut menu) = test_table("enchant_menu_offers", SEED);
    let world = &world_fixture.world;
    surround_with_bookshelves(world, pos);
    place_in_table(
        &mut menu,
        &player,
        ItemStack::new(&vanilla_items::DIAMOND_SWORD),
        0,
    );

    assert!(table_item(&menu).is(&vanilla_items::DIAMOND_SWORD));
    let kind = kind(&menu);
    assert!(kind.costs[0] >= 1);
    assert!(
        kind.costs[HIGH_OFFER_INDEX] >= FULL_POWER_LEVELS,
        "third offer is at least twice the 15 bookshelves"
    );
    for slot in 0..OFFER_COUNT {
        if kind.costs[slot] > 0 {
            assert!(kind.enchant_clue[slot].is_some_and(|clue| clue >= 0));
            assert!(kind.level_clue[slot].is_some_and(|level| level >= 1));
        }
        assert_eq!(
            kind.cost_slots[slot].get(menu.behavior()),
            EnchantmentKind::client_value(kind.costs[slot])
        );
    }
    assert_eq!(
        kind.seed_slot.get(menu.behavior()),
        EnchantmentKind::client_value(SEED)
    );

    // Pin the absolute wire order: costs at 0..3, seed at 3, enchantment clues
    // at 4..7, level clues at 7..10. This is the one contract the client relies
    // on that no index-independent handle assertion above can catch.
    let behavior = menu.behavior();
    for offer in 0..OFFER_COUNT {
        assert_eq!(
            behavior.get_data(offer),
            Some(EnchantmentKind::client_value(kind.costs[offer]))
        );
        assert_eq!(
            behavior.get_data(EXPECTED_ENCHANT_CLUE_DATA_START + offer),
            Some(EnchantmentKind::client_value(
                kind.enchant_clue[offer].unwrap_or(EXPECTED_NO_CLUE)
            ))
        );
        assert_eq!(
            behavior.get_data(EXPECTED_LEVEL_CLUE_DATA_START + offer),
            Some(EnchantmentKind::client_value(
                kind.level_clue[offer].unwrap_or(EXPECTED_NO_CLUE)
            ))
        );
    }
    assert_eq!(
        behavior.get_data(EXPECTED_SEED_DATA),
        Some(EnchantmentKind::client_value(SEED))
    );
}

#[test]
fn offers_are_identical_for_the_same_seed_and_cleared_for_unenchantable_items() {
    let (world_fixture, player, pos, mut first) = test_table("enchant_menu_same_seed", SEED);
    let world = &world_fixture.world;
    surround_with_bookshelves(world, pos);
    place_in_table(
        &mut first,
        &player,
        ItemStack::new(&vanilla_items::DIAMOND_SWORD),
        0,
    );
    let mut second = enchantment(Arc::clone(&player.inventory), 2, pos, world, SEED);
    place_in_table(
        &mut second,
        &player,
        ItemStack::new(&vanilla_items::DIAMOND_SWORD),
        1,
    );
    assert_eq!(kind(&first).costs, kind(&second).costs);
    assert_eq!(kind(&first).enchant_clue, kind(&second).enchant_clue);
    assert_eq!(kind(&first).level_clue, kind(&second).level_clue);

    let mut stone = enchantment(Arc::clone(&player.inventory), 3, pos, world, SEED);
    place_in_table(
        &mut stone,
        &player,
        ItemStack::new(&vanilla_items::STONE),
        2,
    );
    assert!(table_item(&stone).is(&vanilla_items::STONE));
    assert_eq!(kind(&stone).costs, [0; OFFER_COUNT]);
    assert_eq!(kind(&stone).enchant_clue, [None; OFFER_COUNT]);
    assert_eq!(kind(&stone).level_clue, [None; OFFER_COUNT]);
    for offer in 0..OFFER_COUNT {
        assert_eq!(
            stone
                .behavior()
                .get_data(EXPECTED_ENCHANT_CLUE_DATA_START + offer),
            Some(EXPECTED_NO_CLUE as i16)
        );
        assert_eq!(
            stone
                .behavior()
                .get_data(EXPECTED_LEVEL_CLUE_DATA_START + offer),
            Some(EXPECTED_NO_CLUE as i16)
        );
    }
}

#[test]
fn button_is_rejected_without_lapis_or_levels() {
    let (world_fixture, player, pos, mut menu) = test_table("enchant_menu_rejections", SEED);
    let world = &world_fixture.world;
    surround_with_bookshelves(world, pos);
    place_in_table(
        &mut menu,
        &player,
        ItemStack::new(&vanilla_items::DIAMOND_SWORD),
        0,
    );
    player.experience.lock().set_levels(FULL_POWER_LEVELS);

    assert!(!menu.click_menu_button(0, &player), "no lapis");
    place_in_table(
        &mut menu,
        &player,
        ItemStack::with_count(&vanilla_items::LAPIS_LAZULI, HIGH_OFFER_LAPIS_COST),
        1,
    );
    player.experience.lock().set_levels(0);
    assert!(!menu.click_menu_button(0, &player), "no levels");
    assert!(
        !menu.click_menu_button(OFFER_COUNT as i32, &player),
        "invalid button id"
    );

    assert!(!table_item(&menu).is_enchanted());
    assert_eq!(table_lapis(&menu).count(), HIGH_OFFER_LAPIS_COST);
    assert_eq!(kind(&menu).enchantment_seed, SEED);
}

#[test]
fn offers_only_reroll_when_the_table_slots_change() {
    let (world_fixture, player, pos, mut menu) = test_table("enchant_menu_unrelated_clicks", SEED);
    let world = &world_fixture.world;
    surround_with_bookshelves(world, pos);
    place_in_table(
        &mut menu,
        &player,
        ItemStack::new(&vanilla_items::DIAMOND_SWORD),
        0,
    );
    let full_ring_costs = kind(&menu).costs;
    assert!(full_ring_costs[HIGH_OFFER_INDEX] >= FULL_POWER_LEVELS);

    // Vanilla only recomputes from `enchantSlots.setChanged`, so a click that
    // touches nothing but the player inventory keeps the stale offers even
    // after the bookshelves are gone.
    for offset in EnchantingTableBlock::BOOKSHELF_OFFSETS {
        assert!(world.set_block(
            pos.offset(offset.x, offset.y, offset.z),
            vanilla_blocks::AIR.default_state(),
            UpdateFlags::UPDATE_ALL,
        ));
    }
    player
        .inventory
        .lock()
        .set_item(1, ItemStack::new(&vanilla_items::STONE));
    menu.clicked(
        Click::Pickup {
            slot: HOTBAR_FIRST_MENU_SLOT + 1,
            button: MouseButton::Left,
        },
        &player,
    );
    assert_eq!(kind(&menu).costs, full_ring_costs);

    // Moving lapis into the table changes `enchantSlots`, which rerolls against
    // the now-empty ring.
    place_in_table(
        &mut menu,
        &player,
        ItemStack::with_count(&vanilla_items::LAPIS_LAZULI, HIGH_OFFER_LAPIS_COST),
        2,
    );
    assert!(
        kind(&menu).costs[HIGH_OFFER_INDEX] <= NO_BOOKSHELVES_MAX_COST,
        "no bookshelves caps the third offer"
    );
}

#[test]
fn enchanting_spends_levels_and_lapis_and_rerolls_the_seed() {
    let (world_fixture, player, pos, mut menu) = test_table("enchant_menu_enchant", SEED);
    let world = &world_fixture.world;
    surround_with_bookshelves(world, pos);
    place_in_table(
        &mut menu,
        &player,
        ItemStack::new(&vanilla_items::DIAMOND_SWORD),
        0,
    );
    place_in_table(
        &mut menu,
        &player,
        ItemStack::with_count(&vanilla_items::LAPIS_LAZULI, HIGH_OFFER_LAPIS_COST),
        1,
    );
    player.experience.lock().set_levels(FULL_POWER_LEVELS);

    assert!(menu.click_menu_button(HIGH_OFFER_INDEX as i32, &player));

    assert!(table_item(&menu).is_enchanted());
    assert!(table_lapis(&menu).is_empty());
    let experience = player.experience.lock();
    assert_eq!(
        experience.level(),
        FULL_POWER_LEVELS - HIGH_OFFER_LAPIS_COST
    );
    let rerolled_seed = experience.enchantment_seed();
    drop(experience);
    let kind = kind(&menu);
    // Vanilla draws the new seed with `random.nextInt()`, which may repeat the
    // old value, so only the hand-off from player to menu is asserted.
    assert_eq!(kind.enchantment_seed, rerolled_seed);
    assert_eq!(
        kind.costs, [0; OFFER_COUNT],
        "an enchanted item has no further offers"
    );
    assert_eq!(
        kind.seed_slot.get(menu.behavior()),
        EnchantmentKind::client_value(kind.enchantment_seed)
    );
}

#[test]
fn creative_players_skip_lapis_but_still_spend_levels() {
    let (world_fixture, player, pos, mut menu) = test_table("enchant_menu_creative", SEED);
    let world = &world_fixture.world;
    surround_with_bookshelves(world, pos);
    player.restore_game_modes(GameType::Creative, None);
    place_in_table(
        &mut menu,
        &player,
        ItemStack::new(&vanilla_items::DIAMOND_SWORD),
        0,
    );
    player.experience.lock().set_levels(FULL_POWER_LEVELS);

    assert!(menu.click_menu_button(0, &player));
    assert!(table_item(&menu).is_enchanted());
    assert_eq!(player.experience.lock().level(), FULL_POWER_LEVELS - 1);
}

#[test]
fn books_become_enchanted_books_with_stored_enchantments() {
    let (world_fixture, player, pos, mut menu) = test_table("enchant_menu_book", SEED);
    let world = &world_fixture.world;
    surround_with_bookshelves(world, pos);
    place_in_table(&mut menu, &player, ItemStack::new(&vanilla_items::BOOK), 0);
    place_in_table(
        &mut menu,
        &player,
        ItemStack::with_count(&vanilla_items::LAPIS_LAZULI, HIGH_OFFER_LAPIS_COST),
        1,
    );
    player.experience.lock().set_levels(FULL_POWER_LEVELS);

    assert!(menu.click_menu_button(HIGH_OFFER_INDEX as i32, &player));
    let result = table_item(&menu);
    assert!(result.is(&vanilla_items::ENCHANTED_BOOK));
    assert!(
        result
            .get(STORED_ENCHANTMENTS)
            .is_some_and(|stored| !stored.is_empty())
    );
}
