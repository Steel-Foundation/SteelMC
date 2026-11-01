use steel_registry::{init_vanilla_registry, vanilla_blocks, vanilla_items};

use super::*;
use crate::inventory::container::Container as _;
use crate::loot::LootError;
use crate::{
    behavior::{BLOCK_BEHAVIORS, init_behaviors},
    block_entity::init_block_entities,
    inventory::lock::ContainerLockGuard,
    test_support::{fresh_test_world, insert_ready_full_chunk},
};
use simdnbt::owned::NbtTag;
use std::sync::Arc;
use steel_registry::data_components::vanilla_components::CONTAINER_LOOT;
use steel_utils::{ChunkPos, Direction, Identifier, types::UpdateFlags};

fn pending(seed: i64) -> SeededContainerLoot {
    SeededContainerLoot::new(Identifier::vanilla_static("chests/spawn_bonus_chest"), seed)
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
fn failed_generation_keeps_pending_loot_and_aborts_before_locking_any_inventory() {
    let fixture = fresh_test_world("failed_barrel_loot");
    let barrel = BarrelBlockEntity::new(
        Arc::downgrade(&fixture.world),
        BlockPos::new(1, 64, 1),
        vanilla_blocks::BARREL.default_state(),
    );
    let missing = SeededContainerLoot::new(Identifier::new_static("example", "missing_table"), 12);
    barrel.set_loot_table(Some(missing.clone()));
    let container = barrel.container_ref().expect("container");
    assert!(matches!(
        ContainerLockGuard::lock_all(&[&container]),
        Err(LootError::MissingTable(_))
    ));
    assert_eq!(barrel.loot_table(), Some(missing));
    assert!(
        barrel
            .storage
            .container
            .try_lock()
            .expect("no retained lock")
            .is_empty()
    );
    barrel.set_loot_table(Some(pending(12)));
    drop(ContainerLockGuard::lock_all(&[&container]).expect("retry valid loot"));
    assert!(barrel.loot_table().is_none());
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
    use crate::{
        behavior::{BlockHitResult, InventoryAccess},
        test_support::TestPlayerBuilder,
    };
    use steel_utils::types::{GameType, InteractionHand};

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
    let hit = BlockHitResult {
        location: glam::DVec3::new(1.5, 64.5, 1.5),
        direction: Direction::Up,
        block_pos: pos,
        miss: false,
        inside: false,
        world_border_hit: false,
    };
    let mut inv = InventoryAccess::new(Arc::clone(&player.inventory), InteractionHand::MainHand);
    let behavior = BLOCK_BEHAVIORS.get_behavior(&vanilla_blocks::BARREL);
    behavior.use_without_item(
        vanilla_blocks::BARREL.default_state(),
        world,
        pos,
        &player,
        &hit,
        &mut inv,
    );
    assert!(randomizable.loot_table().is_some());
    assert!(!player.has_container_open());

    player.restore_game_modes(GameType::Survival, None);
    behavior.use_without_item(
        vanilla_blocks::BARREL.default_state(),
        world,
        pos,
        &player,
        &hit,
        &mut inv,
    );
    assert!(randomizable.loot_table().is_none());
    assert!(player.has_container_open());
}

#[test]
fn placed_container_loot_component_remains_lazy() {
    use steel_registry::data_components::DataComponentPatch;
    let barrel = test_barrel();
    let mut patch = DataComponentPatch::new();
    patch.set(CONTAINER_LOOT, pending(42));
    barrel.apply_components_from_item(&ItemStack::with_count_and_patch(
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
