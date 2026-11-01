use super::*;
use crate::{
    behavior::{BLOCK_BEHAVIORS, PlacementOrientation, PlacementSource, init_behaviors},
    block_entity::{entities::CHEST_SLOTS, init_block_entities},
    inventory::{click::Click, container::Container as _},
    test_support::{TestPlayerBuilder, TestWorld, fresh_test_world, insert_ready_full_chunk},
};
use simdnbt::owned::{NbtCompound, NbtList, NbtTag};
use steel_registry::{
    data_components::vanilla_components::SeededContainerLoot, init_vanilla_registry,
    item_stack::ItemStack, vanilla_blocks, vanilla_items,
};
use steel_utils::{
    ChunkPos,
    types::{GameType, InteractionHand, UpdateFlags},
};

fn fixture(name: &'static str) -> TestWorld {
    init_vanilla_registry();
    init_behaviors();
    init_block_entities();
    let fixture = fresh_test_world(name);
    insert_ready_full_chunk(&fixture.world, ChunkPos::new(0, 0));
    fixture
}

fn behavior() -> ChestBlock {
    ChestBlock::new(
        &vanilla_blocks::CHEST,
        &sound_events::BLOCK_CHEST_OPEN,
        &sound_events::BLOCK_CHEST_CLOSE,
    )
}

fn state(facing: Direction, kind: ChestType) -> BlockStateId {
    vanilla_blocks::CHEST
        .default_state()
        .set_value(&BlockStateProperties::HORIZONTAL_FACING, facing)
        .set_value(&BlockStateProperties::CHEST_TYPE, kind)
}

fn pair(world: &Arc<World>, pos: BlockPos) {
    assert!(world.set_block(
        pos,
        state(Direction::North, ChestType::Left),
        UpdateFlags::UPDATE_NONE
    ));
    assert!(world.set_block(
        pos.east(),
        state(Direction::North, ChestType::Right),
        UpdateFlags::UPDATE_NONE
    ));
}

fn set_item(container: &ContainerRef, slot: usize, item: ItemStack) {
    let mut guard = ContainerLockGuard::lock_all(&[container]).expect("prepare storage");
    assert!(guard.set_item(container.container_id(), slot, item));
}

fn use_chest(world: &Arc<World>, pos: BlockPos, player: &Player) {
    let hit = BlockHitResult {
        location: DVec3::new(
            f64::from(pos.x()) + 0.5,
            f64::from(pos.y()) + 0.5,
            f64::from(pos.z()) + 0.5,
        ),
        direction: Direction::Up,
        block_pos: pos,
        miss: false,
        inside: false,
        world_border_hit: false,
    };
    let mut inv = InventoryAccess::new(Arc::clone(&player.inventory), InteractionHand::MainHand);
    BLOCK_BEHAVIORS
        .get_behavior(&vanilla_blocks::CHEST)
        .use_without_item(
            world.get_block_state(pos),
            world,
            pos,
            player,
            &hit,
            &mut inv,
        );
}

#[test]
fn placement_connects_single_neighbors_in_every_orientation_and_detaches_on_removal() {
    let fixture = fixture("chest_placement");
    let world = &fixture.world;
    let pos = BlockPos::new(5, 64, 5);
    let behavior = behavior();
    for facing in [
        Direction::North,
        Direction::East,
        Direction::South,
        Direction::West,
    ] {
        let partner = pos.relative(facing.rotate_y_clockwise());
        assert!(world.set_block(
            partner,
            state(facing, ChestType::Single),
            UpdateFlags::UPDATE_NONE
        ));
        let mut item = ItemStack::new(&vanilla_items::CHEST);
        let context =
            BlockPlaceContext::directional(world, pos, facing.opposite(), &mut item, Direction::Up);
        let placed = behavior
            .get_state_for_placement(&context)
            .expect("placement");
        assert_eq!(
            placed.get_value(&BlockStateProperties::CHEST_TYPE),
            ChestType::Left
        );
        let connected = behavior.update_shape(
            state(facing, ChestType::Single),
            world,
            partner,
            facing.rotate_y_counter_clockwise(),
            pos,
            placed,
        );
        assert_eq!(
            connected.get_value(&BlockStateProperties::CHEST_TYPE),
            ChestType::Right
        );
        let detached = behavior.update_shape(
            connected,
            world,
            partner,
            facing.rotate_y_counter_clockwise(),
            pos,
            vanilla_blocks::AIR.default_state(),
        );
        assert_eq!(
            detached.get_value(&BlockStateProperties::CHEST_TYPE),
            ChestType::Single
        );
        assert!(world.set_block(
            partner,
            vanilla_blocks::AIR.default_state(),
            UpdateFlags::UPDATE_NONE
        ));
    }
}

#[test]
fn secondary_use_only_pairs_the_clicked_single_neighbor_and_never_makes_triples() {
    let fixture = fixture("chest_secondary_placement");
    let world = &fixture.world;
    let pos = BlockPos::new(5, 64, 5);
    let behavior = behavior();
    assert!(world.set_block(
        pos.east(),
        state(Direction::North, ChestType::Single),
        UpdateFlags::UPDATE_NONE
    ));
    let mut item = ItemStack::new(&vanilla_items::CHEST);
    let hit = BlockHitResult {
        location: DVec3::new(5.5, 64.5, 5.5),
        direction: Direction::West,
        block_pos: pos,
        miss: false,
        inside: false,
        world_border_hit: false,
    };
    let source = PlacementSource::direct(
        None,
        InteractionHand::MainHand,
        &mut item,
        PlacementOrientation::Directional {
            direction: Direction::North,
        },
        true,
    );
    let context = BlockPlaceContext::new(world, source, &hit);
    let placed = behavior
        .get_state_for_placement(&context)
        .expect("secondary placement");
    assert_eq!(
        placed.get_value(&BlockStateProperties::HORIZONTAL_FACING),
        Direction::North
    );
    assert_eq!(
        placed.get_value(&BlockStateProperties::CHEST_TYPE),
        ChestType::Left
    );
    assert!(world.set_block(
        pos.east(),
        state(Direction::North, ChestType::Right),
        UpdateFlags::UPDATE_NONE
    ));
    let mut item = ItemStack::new(&vanilla_items::CHEST);
    let context =
        BlockPlaceContext::directional(world, pos, Direction::South, &mut item, Direction::Up);
    assert_eq!(
        behavior
            .get_state_for_placement(&context)
            .expect("third chest")
            .get_value(&BlockStateProperties::CHEST_TYPE),
        ChestType::Single
    );
}

#[test]
fn either_half_combines_right_first_and_obstruction_blocks_both() {
    let fixture = fixture("double_chest_order");
    let world = &fixture.world;
    let pos = BlockPos::new(5, 64, 5);
    pair(world, pos);
    let left = ChestBlock::get_container(world.get_block_state(pos), world.as_ref(), pos, false)
        .expect("left access");
    let right = ChestBlock::get_container(
        world.get_block_state(pos.east()),
        world.as_ref(),
        pos.east(),
        false,
    )
    .expect("right access");
    assert_eq!(
        left.owners
            .iter()
            .map(|owner| owner.get_block_pos())
            .collect::<Vec<_>>(),
        [pos.east(), pos]
    );
    assert_eq!(
        left.containers
            .iter()
            .map(ContainerRef::container_id)
            .collect::<Vec<_>>(),
        right
            .containers
            .iter()
            .map(ContainerRef::container_id)
            .collect::<Vec<_>>()
    );
    assert!(world.set_block(
        pos.above(),
        vanilla_blocks::STONE.default_state(),
        UpdateFlags::UPDATE_NONE
    ));
    assert!(
        ChestBlock::get_container(
            world.get_block_state(pos.east()),
            world.as_ref(),
            pos.east(),
            false
        )
        .is_none()
    );
    assert_eq!(
        ChestBlock::get_container(
            world.get_block_state(pos.east()),
            world.as_ref(),
            pos.east(),
            true
        )
        .expect("automation ignores lid")
        .containers
        .len(),
        2
    );
}

#[test]
fn double_chest_menu_maps_real_slots_and_merges_stacks_across_both_halves() {
    let fixture = fixture("double_chest_transfer");
    let world = &fixture.world;
    let pos = BlockPos::new(5, 64, 5);
    pair(world, pos);
    let chest = ChestBlock::get_container(world.get_block_state(pos), world.as_ref(), pos, false)
        .expect("double");
    set_item(
        &chest.containers[1],
        0,
        ItemStack::with_count(&vanilla_items::STONE, 60),
    );
    let player = TestPlayerBuilder::new(Arc::clone(world), "ChestTester", 1).build();
    player
        .inventory
        .lock()
        .set_item(9, ItemStack::with_count(&vanilla_items::STONE, 10));
    let ids: Vec<_> = chest
        .containers
        .iter()
        .map(ContainerRef::container_id)
        .collect();
    let mut menu = block_chest(
        Arc::clone(&player.inventory),
        1,
        chest.owners,
        chest.containers,
    );
    assert_eq!(menu.behavior().slot_count(), 90);
    menu.clicked(Click::QuickMove { slot: 54 }, &player);
    let guard = menu.behavior().lock_all_containers().expect("menu storage");
    assert_eq!(guard.get(ids[0]).expect("first").get_item(0).count(), 6);
    assert_eq!(guard.get(ids[1]).expect("second").get_item(0).count(), 64);
    assert!(
        player.inventory.try_lock().is_none(),
        "menu guard includes player inventory"
    );
    assert_eq!(menu.behavior().slots()[27].get_item(&guard).count(), 64);
}

#[test]
fn comparator_measures_both_halves_and_returns_zero_when_blocked() {
    let fixture = fixture("double_chest_comparator");
    let world = &fixture.world;
    let pos = BlockPos::new(5, 64, 5);
    pair(world, pos);
    let behavior = behavior();
    let chest = ChestBlock::get_container(world.get_block_state(pos), world.as_ref(), pos, false)
        .expect("double");
    for slot in 0..CHEST_SLOTS {
        set_item(
            &chest.containers[0],
            slot,
            ItemStack::with_count(&vanilla_items::STONE, 64),
        );
    }
    assert_eq!(
        behavior.get_analog_output_signal(
            world.get_block_state(pos),
            world.as_ref(),
            pos,
            Direction::North
        ),
        8
    );
    assert_eq!(
        behavior.get_analog_output_signal(
            world.get_block_state(pos.east()),
            world.as_ref(),
            pos.east(),
            Direction::North
        ),
        8
    );
    assert!(world.set_block(
        pos.east().above(),
        vanilla_blocks::STONE.default_state(),
        UpdateFlags::UPDATE_NONE
    ));
    assert_eq!(
        behavior.get_analog_output_signal(
            world.get_block_state(pos),
            world.as_ref(),
            pos,
            Direction::North
        ),
        0
    );
}

#[test]
fn double_chest_loot_stays_separate_and_opening_is_lazy_and_spectator_safe() {
    let fixture = fixture("double_chest_loot");
    let world = &fixture.world;
    let pos = BlockPos::new(5, 64, 5);
    pair(world, pos);
    let chest = ChestBlock::get_container(world.get_block_state(pos), world.as_ref(), pos, false)
        .expect("double");
    for (owner, seed) in chest.owners.iter().zip([12_i64, 34]) {
        owner
            .as_randomizable_container()
            .expect("loot")
            .set_loot_table(Some(SeededContainerLoot::new(
                steel_utils::Identifier::vanilla_static("chests/spawn_bonus_chest"),
                seed,
            )));
        assert_eq!(
            owner
                .save_custom_only()
                .get("LootTableSeed")
                .and_then(NbtTag::long),
            Some(seed)
        );
    }
    let player = TestPlayerBuilder::new(Arc::clone(world), "Opener", 1).build();
    player.set_client_loaded(true);
    player.restore_game_modes(GameType::Spectator, None);
    use_chest(world, pos, &player);
    assert!(!player.has_container_open());
    assert!(chest.owners.iter().all(|owner| {
        owner
            .as_randomizable_container()
            .expect("loot")
            .loot_table()
            .is_some()
    }));
    player.restore_game_modes(GameType::Survival, None);
    use_chest(world, pos, &player);
    assert!(player.has_container_open());
    for owner in &chest.owners {
        assert!(
            owner
                .as_randomizable_container()
                .expect("loot")
                .loot_table()
                .is_none()
        );
        let saved = owner.save_custom_only();
        assert!(saved.get("Items").is_some());
        owner
            .load_with_owned_components(&saved)
            .expect("reload generated loot");
        assert_eq!(owner.save_custom_only(), saved);
    }
    player.do_close_container();
}

#[test]
fn lock_on_either_half_protects_the_whole_menu_and_custom_names_persist() {
    let fixture = fixture("double_chest_locks");
    let world = &fixture.world;
    let pos = BlockPos::new(5, 64, 5);
    pair(world, pos);
    let chest = ChestBlock::get_container(world.get_block_state(pos), world.as_ref(), pos, false)
        .expect("double");
    let mut lock = NbtCompound::new();
    lock.insert("items", "minecraft:diamond");
    let mut nbt = NbtCompound::new();
    nbt.insert("lock", lock);
    nbt.insert(
        "CustomName",
        TextComponent::plain("Treasure").to_codec_nbt(),
    );
    chest.owners[1]
        .load_with_owned_components(&nbt)
        .expect("load lock");
    assert_eq!(
        ChestBlock::display_name(&chest),
        TextComponent::plain("Treasure")
    );
    let saved = chest.owners[1].save_custom_only();
    chest.owners[1]
        .load_with_owned_components(&saved)
        .expect("reload lock/name");
    let player = TestPlayerBuilder::new(Arc::clone(world), "LockedOut", 1).build();
    player.set_client_loaded(true);
    use_chest(world, pos.east(), &player);
    assert!(!player.has_container_open());
    player
        .inventory
        .lock()
        .set_selected_item(ItemStack::new(&vanilla_items::DIAMOND));
    use_chest(world, pos, &player);
    assert!(player.has_container_open());
    player.do_close_container();
    player
        .inventory
        .lock()
        .set_selected_item(ItemStack::empty());
    player.restore_game_modes(GameType::Spectator, None);
    use_chest(world, pos, &player);
    assert!(
        player.has_container_open(),
        "spectators bypass locks once loot is generated"
    );
    player.do_close_container();
}

#[test]
fn chest_block_drop_copies_the_custom_name() {
    use crate::behavior::BlockLootContext;
    use steel_registry::data_components::vanilla_components::CUSTOM_NAME;
    let fixture = fixture("named_chest_drop");
    let world = &fixture.world;
    let pos = BlockPos::new(5, 64, 5);
    assert!(world.set_block(
        pos,
        vanilla_blocks::CHEST.default_state(),
        UpdateFlags::UPDATE_NONE
    ));
    let owner = world.get_block_entity(pos).expect("chest");
    let mut nbt = NbtCompound::new();
    nbt.insert(
        "CustomName",
        TextComponent::plain("Treasure").to_codec_nbt(),
    );
    owner.load_with_owned_components(&nbt).expect("named chest");
    let items = BlockLootContext::new(world, pos).get_drops(world.get_block_state(pos));
    assert_eq!(items.len(), 1);
    assert!(items[0].is(&vanilla_items::CHEST));
    assert_eq!(
        items[0].get(CUSTOM_NAME),
        Some(&TextComponent::plain("Treasure"))
    );
}

#[test]
fn storage_honors_component_stack_limits_and_vanilla_slot_number_decoding() {
    use simdnbt::ToNbtTag as _;
    use steel_registry::data_components::{DataComponentPatch, vanilla_components::MAX_STACK_SIZE};
    let fixture = fixture("chest_storage_codecs");
    let world = &fixture.world;
    let pos = BlockPos::new(5, 64, 5);
    assert!(world.set_block(
        pos,
        vanilla_blocks::CHEST.default_state(),
        UpdateFlags::UPDATE_NONE
    ));
    let owner = world.get_block_entity(pos).expect("chest");
    let container = owner.container_ref().expect("container");
    let mut patch = DataComponentPatch::new();
    patch.set(MAX_STACK_SIZE, 99);
    set_item(
        &container,
        0,
        ItemStack::with_count_and_patch(&vanilla_items::STONE, 99, patch),
    );
    {
        let guard = ContainerLockGuard::lock_all(&[&container]).expect("storage");
        assert_eq!(
            guard
                .get(container.container_id())
                .expect("container")
                .get_item(0)
                .count(),
            99
        );
    }
    let items: Vec<_> = [None, Some(NbtTag::Int(257)), Some(NbtTag::Double(2.9))]
        .into_iter()
        .map(|slot| {
            let NbtTag::Compound(mut item) = ItemStack::new(&vanilla_items::DIAMOND).to_nbt_tag()
            else {
                panic!("item encodes as compound");
            };
            if let Some(slot) = slot {
                item.insert("Slot", slot);
            }
            item
        })
        .collect();
    let mut nbt = NbtCompound::new();
    nbt.insert("Items", NbtList::Compound(items));
    owner
        .load_with_owned_components(&nbt)
        .expect("load codec slots");
    let guard = ContainerLockGuard::lock_all(&[&container]).expect("loaded storage");
    let inventory = guard.get(container.container_id()).expect("inventory");
    for slot in 0..3 {
        assert!(inventory.get_item(slot).is(&vanilla_items::DIAMOND));
    }
}

#[test]
fn breaking_one_half_preserves_the_other_inventory_and_detaches_it() {
    let fixture = fixture("double_chest_breaking");
    let world = &fixture.world;
    let pos = BlockPos::new(5, 64, 5);
    pair(world, pos);
    let chest = ChestBlock::get_container(world.get_block_state(pos), world.as_ref(), pos, false)
        .expect("double");
    set_item(
        &chest.containers[0],
        0,
        ItemStack::with_count(&vanilla_items::STONE, 7),
    );
    set_item(
        &chest.containers[1],
        0,
        ItemStack::with_count(&vanilla_items::DIAMOND, 5),
    );
    assert!(world.destroy_block(pos, true));
    let survivor = ChestBlock::get_container(
        world.get_block_state(pos.east()),
        world.as_ref(),
        pos.east(),
        false,
    )
    .expect("survivor");
    assert_eq!(survivor.containers.len(), 1);
    assert_eq!(
        world
            .get_block_state(pos.east())
            .get_value(&BlockStateProperties::CHEST_TYPE),
        ChestType::Single
    );
    let guard = ContainerLockGuard::lock_all(&survivor.containers).expect("survivor storage");
    assert_eq!(
        guard
            .get(survivor.containers[0].container_id())
            .expect("storage")
            .get_item(0),
        &ItemStack::with_count(&vanilla_items::STONE, 7)
    );
}
