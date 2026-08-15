use std::sync::Arc;

use super::{MenuBuilder, kinds::BasicKind};
use crate::{
    behavior::init_behaviors,
    block_entity::init_block_entities,
    chunk::{Chunk, status::ChunkStatus},
    entity::{Entity as _, entities::ItemEntity},
    inventory::{
        click::{Click, DragKind, QuickCraft, SwapTarget},
        container::{Container as _, SimpleContainer},
        lock::{ContainerLockGuard, ContainerRef},
        slots::{NormalSlot, Slot, SlotStorage},
    },
    player::Player,
    test_support::{TestPlayerBuilder, fresh_test_world, insert_ready_full_chunk},
    world::World,
};
use glam::DVec3;
use steel_registry::{
    init_vanilla_registry, item_stack::ItemStack, vanilla_blocks, vanilla_entities, vanilla_items,
    vanilla_menu_types,
};
use steel_utils::locks::{IntoShared as _, Shared};
use steel_utils::types::{GameType, UpdateFlags};
use steel_utils::{ChunkPos, Downcast as _, DowncastType, DowncastTypeKey, WorldAabb};

struct SingleItemSlot {
    base: NormalSlot,
}

// SAFETY: This test-only key uniquely identifies `SingleItemSlot`.
unsafe impl DowncastType for SingleItemSlot {
    const TYPE_KEY: DowncastTypeKey = DowncastTypeKey::new("steel:test/slot/single_item");
}

impl Slot for SingleItemSlot {
    fn storage(&self) -> &SlotStorage {
        self.base.storage()
    }

    fn get_item<'a>(&self, guard: &'a ContainerLockGuard) -> &'a ItemStack {
        self.base.get_item(guard)
    }

    fn get_item_mut<'a>(&self, guard: &'a mut ContainerLockGuard) -> &'a mut ItemStack {
        self.base.get_item_mut(guard)
    }

    fn set_item(&self, guard: &mut ContainerLockGuard, stack: ItemStack) {
        self.base.set_item(guard, stack);
    }

    fn get_max_stack_size(&self, _guard: &ContainerLockGuard) -> i32 {
        1
    }

    fn set_changed(&self, guard: &mut ContainerLockGuard) {
        self.base.set_changed(guard);
    }

    fn get_container_slot(&self) -> usize {
        self.base.get_container_slot()
    }
}

struct PartialSwapFixture {
    world: Arc<World>,
    player: Arc<Player>,
    target: Shared<SimpleContainer>,
}

fn perform_partial_swap(world_name: &'static str, game_mode: GameType) -> PartialSwapFixture {
    init_vanilla_registry();
    let world = fresh_test_world(world_name);
    insert_ready_full_chunk(&world, ChunkPos::new(0, 0));
    let player = TestPlayerBuilder::new(Arc::clone(&world), "SwapTester", 1).build();
    player.restore_game_modes(game_mode, None);
    player.base().set_position_local(DVec3::new(0.5, 64.0, 0.5));
    {
        let mut inventory = player.inventory.lock();
        for slot in 0..inventory.get_container_size() {
            inventory.set_item(slot, ItemStack::with_count(&vanilla_items::DIRT, 64));
        }
        inventory.set_item(0, ItemStack::with_count(&vanilla_items::DIRT, 2));
    }

    let target = SimpleContainer::new(1).into_shared();
    target
        .lock()
        .set_item(0, ItemStack::new(&vanilla_items::STONE));
    let target_ref = ContainerRef::from(Arc::clone(&target));
    let mut builder = MenuBuilder::new(None, 1);
    let target_slots = builder.custom_section([SingleItemSlot {
        base: NormalSlot::new(target_ref.clone(), 0),
    }]);
    let mut menu = builder.build(BasicKind {});
    menu.clicked(
        Click::Swap {
            slot: target_slots.start(),
            with: SwapTarget::Hotbar(0),
        },
        &player,
    );

    PartialSwapFixture {
        world,
        player,
        target,
    }
}

#[test]
fn swap_locks_player_inventory_when_menu_has_no_inventory_slots() {
    init_vanilla_registry();
    let world = fresh_test_world("menu_swap_without_inventory_slots");
    let player = TestPlayerBuilder::new(Arc::clone(&world), "SwapTester", 1).build();
    let container = SimpleContainer::new(45).into_shared();
    container
        .lock()
        .set_item(0, ItemStack::new(&vanilla_items::STONE));

    let mut builder = MenuBuilder::new(&vanilla_menu_types::GENERIC_9X1, 1);
    let menu_slots = builder.section(container.clone(), 45);
    let mut menu = builder.build(BasicKind {});

    menu.clicked(
        Click::Swap {
            slot: menu_slots.start(),
            with: SwapTarget::Hotbar(0),
        },
        &player,
    );

    assert!(container.lock().get_item(0).is_empty());
    assert!(
        player
            .inventory
            .lock()
            .get_item(0)
            .is(&vanilla_items::STONE)
    );
}

#[test]
fn draining_a_block_entity_slot_marks_its_chunk_dirty() {
    init_vanilla_registry();
    init_behaviors();
    init_block_entities();
    let world = fresh_test_world("persistent_menu_drain");
    let pos = steel_utils::BlockPos::new(0, 64, 0);
    let holder = insert_ready_full_chunk(&world, ChunkPos::from_block_pos(pos));
    assert!(world.set_block(
        pos,
        vanilla_blocks::BARREL.default_state(),
        UpdateFlags::UPDATE_NONE,
    ));
    let container = ContainerRef::from_block_entity(
        world
            .get_block_entity(pos)
            .expect("barrel should create its block entity"),
    )
    .expect("barrel should expose a container");
    {
        let mut guard = ContainerLockGuard::lock_all(&[&container]);
        assert!(guard.set_item(
            container.container_id(),
            0,
            ItemStack::new(&vanilla_items::STONE),
        ));
    }
    holder
        .try_chunk(ChunkStatus::Full)
        .expect("full chunk should remain loaded")
        .clear_dirty();

    let player = TestPlayerBuilder::new(Arc::clone(&world), "DrainTester", 1).build();
    player.base().set_position_local(DVec3::new(0.5, 64.0, 0.5));
    let mut builder = MenuBuilder::new(None, 1);
    let drained = builder.section(container, 1);
    builder.drain(drained);
    let mut menu = builder.build(BasicKind {});

    menu.removed(&player);

    assert!(
        holder
            .try_chunk(ChunkStatus::Full)
            .is_some_and(Chunk::is_dirty)
    );
}

#[test]
fn one_slot_creative_clone_drag_is_a_vanilla_noop() {
    init_vanilla_registry();
    let world = fresh_test_world("one_slot_clone_drag");
    let player = TestPlayerBuilder::new(Arc::clone(&world), "CloneTester", 1).build();
    player.restore_game_modes(GameType::Creative, None);
    let container = SimpleContainer::new(1).into_shared();
    container
        .lock()
        .set_item(0, ItemStack::new(&vanilla_items::STONE));
    let mut builder = MenuBuilder::new(None, 1);
    let slots = builder.section(Arc::clone(&container), 1);
    let mut menu = builder.build(BasicKind {});
    *menu.behavior_mut().carried_mut() = ItemStack::new(&vanilla_items::STONE);

    menu.clicked(
        Click::QuickCraft(QuickCraft::Start {
            kind: DragKind::Clone,
        }),
        &player,
    );
    menu.clicked(
        Click::QuickCraft(QuickCraft::AddSlot {
            slot: slots.start(),
        }),
        &player,
    );
    menu.clicked(Click::QuickCraft(QuickCraft::End), &player);

    assert_eq!(container.lock().get_item(0).count(), 1);
    assert_eq!(menu.behavior().carried().count(), 1);
    assert_eq!(menu.behavior().quickcraft(), None);
}

#[test]
fn partial_swap_overflow_marks_displaced_item_as_thrown() {
    let fixture = perform_partial_swap("menu_partial_swap_overflow", GameType::Survival);
    // The exact UUID is irrelevant; verify the dropped item records this player.
    let player_id = fixture.player.gameprofile.id;

    assert_eq!(fixture.player.inventory.lock().get_item(0).count(), 1);
    let target_item = fixture.target.lock().get_item(0).clone();
    assert!(target_item.is(&vanilla_items::DIRT));
    assert_eq!(target_item.count(), 1);
    let dropped = fixture.world.get_entities_in_aabb_matching(
        &WorldAabb::new(-2.0, 62.0, -2.0, 2.0, 68.0, 2.0),
        |entity| entity.entity_type() == &vanilla_entities::ITEM,
    );
    assert_eq!(dropped.len(), 1);
    let Some(item) = dropped[0].as_ref().downcast_ref::<ItemEntity>() else {
        panic!("dropped entity should retain its concrete item type");
    };
    assert!(item.get_item().is(&vanilla_items::STONE));
    assert_eq!(item.get_thrower(), Some(player_id));
}

#[test]
fn partial_swap_overflow_is_discarded_in_creative() {
    let fixture = perform_partial_swap("creative_menu_partial_swap_overflow", GameType::Creative);

    assert_eq!(fixture.player.inventory.lock().get_item(0).count(), 1);
    let target_item = fixture.target.lock().get_item(0).clone();
    assert!(target_item.is(&vanilla_items::DIRT));
    assert_eq!(target_item.count(), 1);
    assert!(
        fixture
            .player
            .inventory
            .lock()
            .items()
            .iter()
            .all(|item| !item.is(&vanilla_items::STONE))
    );
    assert!(
        fixture
            .world
            .get_entities_in_aabb_matching(
                &WorldAabb::new(-2.0, 62.0, -2.0, 2.0, 68.0, 2.0),
                |entity| entity.entity_type() == &vanilla_entities::ITEM,
            )
            .is_empty()
    );
}
