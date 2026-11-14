use steel_registry::{init_vanilla_registry, item_stack::ItemStack, vanilla_blocks, vanilla_items};

use super::*;
use crate::inventory::container::Container as _;
use crate::{
    behavior::{BLOCK_BEHAVIORS, init_behaviors},
    block_entity::init_block_entities,
    inventory::lock::ContainerLockGuard,
    test_support::{fresh_test_world, insert_ready_full_chunk},
};
use simdnbt::owned::NbtTag;
use std::sync::Arc;
use steel_registry::data_components::vanilla_components::CONTAINER_LOOT;
use steel_utils::random::{Random as _, RandomSource, legacy_random::LegacyRandom};
use steel_utils::{ChunkPos, Direction, Identifier, types::UpdateFlags};

fn pending(seed: i64) -> SeededContainerLoot {
    SeededContainerLoot::new(Identifier::vanilla_static("chests/spawn_bonus_chest"), seed)
}

#[test]
fn locked_barrel_preserves_custom_name_and_lock_on_reload() {
    use text_components::TextComponent;

    let source = test_barrel();
    let mut lock = NbtCompound::new();
    lock.insert("items", "minecraft:diamond");
    let mut nbt = NbtCompound::new();
    nbt.insert("lock", lock);
    nbt.insert(
        "CustomName",
        TextComponent::plain("Treasure").to_codec_nbt(),
    );
    source
        .load_with_owned_components(&nbt)
        .expect("load named locked barrel");

    let saved = source.save_custom_only();
    assert_eq!(saved.get("CustomName"), nbt.get("CustomName"));
    assert_eq!(saved.get("lock"), nbt.get("lock"));

    let restored = test_barrel();
    restored
        .load_with_owned_components(&saved)
        .expect("reload named locked barrel");
    assert_eq!(restored.save_custom_only(), saved);
}

#[test]
fn placed_name_and_lock_survive_reload_and_control_opening() {
    use crate::{
        behavior::BlockLootContext,
        test_support::{TestPlayerBuilder, use_block},
    };
    use steel_registry::{
        RegistryHolderSet,
        data_component_predicate::DataComponentMatchers,
        data_components::{
            DataComponentPatch,
            vanilla_components::{CUSTOM_NAME, LOCK},
        },
        item_predicate::{IntBounds, ItemPredicate, LockCode},
    };
    use text_components::TextComponent;

    init_vanilla_registry();
    init_behaviors();
    init_block_entities();
    let fixture = fresh_test_world("named_locked_barrel");
    let world = &fixture.world;
    let pos = BlockPos::new(1, 64, 1);
    insert_ready_full_chunk(world, ChunkPos::from_block_pos(pos));
    assert!(world.set_block(
        pos,
        vanilla_blocks::BARREL.default_state(),
        UpdateFlags::UPDATE_NONE
    ));
    let owner = world.get_block_entity(pos).expect("barrel");
    let name = TextComponent::plain("Treasure");
    let mut patch = DataComponentPatch::new();
    patch.set(CUSTOM_NAME, name.clone());
    patch.set(
        LOCK,
        LockCode::new(ItemPredicate::new(
            Some(RegistryHolderSet::Direct(vec![&vanilla_items::DIAMOND])),
            IntBounds::ANY,
            DataComponentMatchers::ANY,
        )),
    );
    owner.apply_components_from_item_stack(&ItemStack::with_count_and_patch(
        &vanilla_items::BARREL,
        1,
        patch,
    ));
    let saved = owner.save_without_metadata();
    owner
        .load_with_owned_components(&saved)
        .expect("reload barrel");
    assert_eq!(owner.as_nameable().expect("named barrel").name(), name);

    let player = TestPlayerBuilder::new(Arc::clone(world), "LockedOut", 1).build();
    player.set_client_loaded(true);
    use_block(world, pos, &player);
    assert!(!player.has_container_open());
    player
        .inventory
        .lock()
        .set_selected_item(ItemStack::new(&vanilla_items::STONE));
    use_block(world, pos, &player);
    assert!(!player.has_container_open());
    player
        .inventory
        .lock()
        .set_selected_item(ItemStack::new(&vanilla_items::DIAMOND));
    use_block(world, pos, &player);
    assert!(player.has_container_open());
    player.do_close_container();

    let items = BlockLootContext::new(world, pos).get_drops(world.get_block_state(pos));
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].get(CUSTOM_NAME), Some(&name));
}

#[test]
fn pending_nbt_suppresses_items_and_round_trips_without_unpacking() {
    for seed in [0, -12345] {
        let source = test_barrel();
        source
            .storage
            .container
            .lock()
            .set_item(0, ItemStack::new(&vanilla_items::STONE));
        source.set_loot_table(Some(pending(seed)));
        let nbt = source.save_custom_only();
        assert!(nbt.get("Items").is_none());
        assert_eq!(nbt.get("LootTableSeed").is_some(), seed != 0);

        let restored = test_barrel();
        restored
            .load_with_owned_components(&nbt)
            .expect("load pending loot");
        assert_eq!(restored.loot_table(), Some(pending(seed)));
        assert!(restored.storage.container.lock().is_empty());
        assert_eq!(restored.save_custom_only(), nbt);
    }
}

#[test]
fn loot_table_tag_takes_precedence_over_stored_items() {
    let barrel = test_barrel();
    barrel
        .storage
        .container
        .lock()
        .set_item(0, ItemStack::new(&vanilla_items::STONE));
    let mut nbt = barrel.save_custom_only();
    nbt.insert("LootTable", "minecraft:chests/spawn_bonus_chest");
    nbt.insert("LootTableSeed", 12_i64);
    barrel.load_with_owned_components(&nbt).expect("load");
    assert!(barrel.storage.container.lock().is_empty());
    assert_eq!(barrel.loot_table(), Some(pending(12)));
}

#[test]
fn loot_seed_accepts_vanilla_numeric_tags() {
    for seed in [
        NbtTag::Int(-42),
        NbtTag::Float(-42.75),
        NbtTag::Double(f64::NAN),
    ] {
        let barrel = test_barrel();
        let mut nbt = NbtCompound::new();
        nbt.insert("LootTable", "minecraft:chests/spawn_bonus_chest");
        let expected = if matches!(seed, NbtTag::Double(_)) {
            0
        } else {
            -42
        };
        nbt.insert("LootTableSeed", seed);
        barrel
            .load_with_owned_components(&nbt)
            .expect("load numeric seed");
        assert_eq!(barrel.loot_table(), Some(pending(expected)));
    }
}

#[test]
fn first_storage_access_fills_once_and_preserves_generated_items_on_reload() {
    let fixture = fresh_test_world("lazy_barrel_access");
    let barrel = BarrelBlockEntity::new(
        Arc::downgrade(&fixture.world),
        BlockPos::new(1, 64, 1),
        vanilla_blocks::BARREL.default_state(),
    );
    barrel.set_loot_table(Some(pending(12345)));
    let container = barrel.container_ref().expect("container");
    assert!(
        barrel.loot_table().is_some(),
        "creating a reference must not generate loot"
    );
    assert!(barrel.storage.container.lock().is_empty());
    drop(ContainerLockGuard::lock_all(&[&container]).expect("lazy access"));
    assert!(barrel.loot_table().is_none());
    assert!(!barrel.storage.container.lock().is_empty());
    let generated = barrel.save_custom_only();
    drop(ContainerLockGuard::lock_all(&[&container]).expect("second access"));
    assert_eq!(barrel.save_custom_only(), generated);

    let restored = BarrelBlockEntity::new(
        Arc::downgrade(&fixture.world),
        BlockPos::new(2, 64, 1),
        vanilla_blocks::BARREL.default_state(),
    );
    restored
        .load_with_owned_components(&generated)
        .expect("reload inventory");
    drop(
        ContainerLockGuard::lock_all(&[restored.container_ref().expect("container")])
            .expect("restored access"),
    );
    assert_eq!(restored.save_custom_only(), generated);
    assert!(restored.loot_table().is_none());

    // A menu retaining its original reference must see loot assigned later.
    barrel.set_loot_table(Some(pending(54321)));
    drop(ContainerLockGuard::lock_all(&[&container]).expect("access after reassignment"));
    assert!(barrel.loot_table().is_none());
}

#[test]
fn missing_loot_table_fills_as_empty_once_and_preserves_existing_items() {
    for seed in [0_i64, 12] {
        let fixture = fresh_test_world("missing_barrel_loot");
        let barrel = BarrelBlockEntity::new(
            Arc::downgrade(&fixture.world),
            BlockPos::new(1, 64, 1),
            vanilla_blocks::BARREL.default_state(),
        );
        let mut nbt = NbtCompound::new();
        nbt.insert("LootTable", "example:missing_table");
        nbt.insert("LootTableSeed", seed);
        barrel
            .load_with_owned_components(&nbt)
            .expect("load stale loot key");
        barrel
            .storage
            .container
            .lock()
            .set_item(0, ItemStack::with_count(&vanilla_items::STONE, 3));
        *fixture.world.loot_random.lock() = RandomSource::Legacy(LegacyRandom::from_seed(42));
        let mut expected_random = LegacyRandom::from_seed(42);
        if seed == 0 {
            // Vanilla fill shuffles empty slots even when the table has no pools.
            for end in (2..BARREL_SLOTS).rev() {
                expected_random.next_i32_bounded(end as i32);
            }
        }

        let container = barrel.container_ref().expect("container");
        for _ in 0..2 {
            let guard = ContainerLockGuard::lock_all(&[&container]).expect("access stale loot");
            let inventory = guard
                .get(container.container_id())
                .expect("locked inventory");
            assert_eq!(
                inventory.get_item(0),
                &ItemStack::with_count(&vanilla_items::STONE, 3)
            );
            assert!(inventory.items()[1..].iter().all(ItemStack::is_empty));
        }
        assert!(barrel.loot_table().is_none());
        let saved = barrel.save_custom_only();
        assert!(saved.get("LootTable").is_none());
        assert!(saved.get("Items").is_some());
        assert_eq!(
            fixture.world.loot_random.lock().next_i64(),
            expected_random.next_i64()
        );

        barrel.set_loot_table(Some(pending(12)));
        drop(ContainerLockGuard::lock_all(&[&container]).expect("generate reassigned loot"));
        assert!(barrel.loot_table().is_none());
    }
}

#[test]
fn comparator_access_unpacks_pending_loot_without_recursive_locking() {
    init_vanilla_registry();
    init_behaviors();
    init_block_entities();
    let fixture = fresh_test_world("barrel_loot_comparator");
    let world = &fixture.world;
    let pos = BlockPos::new(1, 64, 1);
    insert_ready_full_chunk(world, ChunkPos::from_block_pos(pos));
    assert!(world.set_block(
        pos,
        vanilla_blocks::BARREL.default_state(),
        UpdateFlags::UPDATE_NONE
    ));
    let entity = world.get_block_entity(pos).expect("barrel entity");
    let loot = entity.as_randomizable_container().expect("loot capability");
    loot.set_loot_table(Some(pending(12345)));
    let signal = BLOCK_BEHAVIORS
        .get_behavior(&vanilla_blocks::BARREL)
        .get_analog_output_signal(
            vanilla_blocks::BARREL.default_state(),
            world.as_ref(),
            pos,
            Direction::North,
        );
    assert!(signal > 0);
    assert!(loot.loot_table().is_none());
}

#[test]
fn opening_generates_loot_but_spectators_leave_pending_barrels_closed() {
    use crate::test_support::{TestPlayerBuilder, use_block};
    use steel_utils::types::GameType;

    init_vanilla_registry();
    init_behaviors();
    init_block_entities();
    let fixture = fresh_test_world("barrel_player_opening");
    let world = &fixture.world;
    let pos = BlockPos::new(1, 64, 1);
    insert_ready_full_chunk(world, ChunkPos::from_block_pos(pos));
    assert!(world.set_block(
        pos,
        vanilla_blocks::BARREL.default_state(),
        UpdateFlags::UPDATE_NONE
    ));
    let entity = world.get_block_entity(pos).expect("barrel");
    let randomizable = entity.as_randomizable_container().expect("loot capability");
    randomizable.set_loot_table(Some(pending(12345)));
    let player = TestPlayerBuilder::new(Arc::clone(world), "Opener", 1).build();
    player.set_client_loaded(true);
    player.restore_game_modes(GameType::Spectator, None);
    use_block(world, pos, &player);
    assert!(randomizable.loot_table().is_some());
    assert!(!player.has_container_open());

    player.restore_game_modes(GameType::Survival, None);
    use_block(world, pos, &player);
    assert!(randomizable.loot_table().is_none());
    assert!(player.has_container_open());
}

#[test]
fn placed_container_loot_component_remains_lazy() {
    use steel_registry::data_components::DataComponentPatch;
    let barrel = test_barrel();
    let mut patch = DataComponentPatch::new();
    patch.set(CONTAINER_LOOT, pending(42));
    barrel.apply_components_from_item_stack(&ItemStack::with_count_and_patch(
        &vanilla_items::BARREL,
        1,
        patch,
    ));
    assert_eq!(barrel.loot_table(), Some(pending(42)));
    assert!(barrel.storage.container.lock().is_empty());
}

fn test_barrel() -> BarrelBlockEntity {
    init_vanilla_registry();
    BarrelBlockEntity::new(
        Weak::new(),
        BlockPos::new(1, 2, 3),
        vanilla_blocks::BARREL.default_state(),
    )
}

#[test]
fn set_item_limits_stack_to_vanilla_container_maximum() {
    let barrel = test_barrel();
    barrel
        .storage
        .container
        .lock()
        .set_item(0, ItemStack::with_count(&vanilla_items::STONE, 100));

    assert_eq!(barrel.storage.container.lock().get_item(0).count(), 64);
}

#[test]
fn pre_remove_preserves_slots_for_existing_menu_references() {
    let barrel = test_barrel();
    barrel
        .storage
        .container
        .lock()
        .set_item(0, ItemStack::new(&vanilla_items::STONE));

    barrel.pre_remove_side_effects(
        BlockPos::new(1, 2, 3),
        vanilla_blocks::BARREL.default_state(),
    );

    let container = barrel.storage.container.lock();
    assert_eq!(container.items().len(), BARREL_SLOTS);
    assert!(container.items().iter().all(ItemStack::is_empty));
}
