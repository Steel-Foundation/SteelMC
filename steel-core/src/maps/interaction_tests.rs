use std::{io::Cursor, sync::Arc};

use glam::DVec3;
use simdnbt::{borrow::read_compound, owned::NbtCompound};
use steel_registry::{
    data_components::vanilla_components::MAP_ID, init_vanilla_registry, item_stack::ItemStack,
    vanilla_blocks, vanilla_damage_types, vanilla_entities, vanilla_items,
};
use steel_utils::{
    BlockPos, ChunkPos, Direction,
    types::{InteractionHand, UpdateFlags},
};

use super::MapItemSavedData;
use crate::{
    behavior::{ITEM_BEHAVIORS, InteractionResult, UseItemContext, init_behaviors, items::MapItem},
    bootstrap::init_globals,
    entity::{
        BlockAttachedEntity, Entity, HangingEntity, ItemFrame, SharedEntity, damage::DamageSource,
        entities::ItemFrameEntity,
    },
    inventory::container::Container,
    player::Player,
    test_support::{TestPlayerBuilder, TestWorld, fresh_test_world, insert_ready_full_chunk},
};

const POS: BlockPos = BlockPos::new(8, 64, 8);

#[test]
fn framed_maps_consume_one_rotate_and_remove_the_shared_marker_on_damage_and_kill() {
    init_globals();
    let fixture = fresh_test_world("framed_maps");
    let world = &fixture.world;
    insert_ready_full_chunk(world, ChunkPos::from_block_pos(POS));
    assert!(world.set_block(
        POS.relative(Direction::West),
        vanilla_blocks::STONE.default_state(),
        UpdateFlags::UPDATE_NONE
    ));
    let frame = Arc::new(ItemFrameEntity::new_attached(
        &vanilla_entities::ITEM_FRAME,
        11,
        POS,
        Direction::East,
        Arc::downgrade(world),
    ));
    assert!(frame.survives());
    let entity: SharedEntity = Arc::<ItemFrameEntity>::clone(&frame);
    world.try_add_entity(entity).expect("frame");
    let player = TestPlayerBuilder::new(Arc::clone(world), "MapFramer", 12).build();
    assert!(world.players.insert(Arc::clone(&player)));
    let mut stack = MapItem::create(world, 8, 8, 0, true, false);
    let id = *stack.get(MAP_ID).expect("map identity");
    stack.set_count(2);
    player
        .inventory
        .lock()
        .set_item_in_hand(InteractionHand::MainHand, stack);
    assert_eq!(
        frame.interact(&player, InteractionHand::MainHand, DVec3::ZERO),
        InteractionResult::Success
    );
    assert_eq!(
        player
            .inventory
            .lock()
            .get_item_in_hand(InteractionHand::MainHand)
            .count(),
        1
    );
    assert_eq!(frame.item().count(), 1);
    assert_ne!(frame.bounding_box(), frame.pop_box());
    MapItemSavedData::synchronize_frame(frame.as_ref());
    let data = world.get_map_data(id).expect("map");
    assert_eq!(data.lock().frames.len(), 1);
    assert_eq!(
        frame.interact(&player, InteractionHand::MainHand, DVec3::ZERO),
        InteractionResult::Success
    );
    assert_eq!(frame.analog_output(), 2);
    assert!(frame.hurt(
        world,
        &DamageSource::environment(&vanilla_damage_types::PLAYER_ATTACK),
        1.0
    ));
    assert!(!frame.is_removed());
    assert!(frame.item().is_empty());
    assert!(data.lock().frames.is_empty());
    let stack = player
        .inventory
        .lock()
        .get_item_in_hand(InteractionHand::MainHand)
        .clone();
    frame.set_item(stack);
    MapItemSavedData::synchronize_frame(frame.as_ref());
    assert_eq!(data.lock().frames.len(), 1);
    frame.kill(world);
    assert!(frame.is_removed());
    assert!(data.lock().frames.is_empty());
}

#[test]
fn loading_frame_flags_preserves_fixed_invisible_and_drop_chance_and_removes_replaced_maps() {
    init_globals();
    let fixture = fresh_test_world("fixed_map_frame");
    let world = &fixture.world;
    insert_ready_full_chunk(world, ChunkPos::from_block_pos(POS));
    let frame = ItemFrameEntity::new_attached(
        &vanilla_entities::GLOW_ITEM_FRAME,
        21,
        POS,
        Direction::Up,
        Arc::downgrade(world),
    );
    let player = TestPlayerBuilder::new(Arc::clone(world), "FixedFrame", 22).build();
    assert!(world.players.insert(Arc::clone(&player)));
    let stack = MapItem::create(world, 8, 8, 0, true, false);
    let id = *stack.get(MAP_ID).expect("map ID");
    frame.set_item(stack);
    MapItemSavedData::synchronize_frame(&frame);
    let data = world.get_map_data(id).expect("map");
    assert_eq!(data.lock().frames.len(), 1);
    let mut nbt = NbtCompound::new();
    nbt.insert("Fixed", 1_i8);
    nbt.insert("Invisible", 1_i8);
    nbt.insert("ItemDropChance", 0.25_f32);
    let mut bytes = Vec::new();
    nbt.write(&mut bytes);
    let borrowed = read_compound(&mut Cursor::new(bytes.as_slice())).expect("frame NBT");
    frame.load_additional((&borrowed).into());
    assert!(data.lock().frames.is_empty());
    assert!(frame.survives());
    assert!(frame.is_invisible());
    assert_eq!(
        frame.interact(&player, InteractionHand::MainHand, DVec3::ZERO),
        InteractionResult::Pass
    );
    assert!(!frame.hurt(
        world,
        &DamageSource::environment(&vanilla_damage_types::PLAYER_ATTACK),
        1.0
    ));
    let before = frame.position();
    frame.push_impulse(DVec3::X);
    assert_eq!(frame.position(), before);
    assert!(!frame.is_removed());
    let mut saved = NbtCompound::new();
    frame.save_additional(&mut saved);
    assert_eq!(saved.byte("Fixed"), Some(1));
    assert_eq!(saved.byte("Invisible"), Some(1));
    assert_eq!(saved.float("ItemDropChance"), Some(0.25));
}

fn map_item_fixture(name: &'static str) -> (TestWorld, Arc<Player>) {
    init_vanilla_registry();
    init_behaviors();
    let fixture = fresh_test_world(name);
    let player = TestPlayerBuilder::new(Arc::clone(&fixture.world), "Cartographer", 1).build();
    (fixture, player)
}

#[test]
fn empty_maps_consume_one_and_replace_the_last_map_in_the_used_hand() {
    let (fixture, player) = map_item_fixture("empty_map_use");
    let world = &fixture.world;
    player.inventory.lock().set_item_in_hand(
        InteractionHand::OffHand,
        ItemStack::with_count(&vanilla_items::MAP, 2),
    );
    let mut context = UseItemContext::new(
        &player,
        InteractionHand::OffHand,
        world,
        Arc::clone(&player.inventory),
    );
    let behavior = ITEM_BEHAVIORS.get_behavior(&vanilla_items::MAP);
    assert_eq!(behavior.use_item(&mut context), InteractionResult::Success);
    let first = {
        let inventory = player.inventory.lock();
        assert_eq!(
            inventory.get_item_in_hand(InteractionHand::OffHand).count(),
            1
        );
        let stack = inventory.get_item(0);
        assert!(stack.is(&vanilla_items::FILLED_MAP));
        *stack.get(MAP_ID).expect("created map ID")
    };
    assert_eq!(behavior.use_item(&mut context), InteractionResult::Success);
    let second = {
        let inventory = player.inventory.lock();
        let stack = inventory.get_item_in_hand(InteractionHand::OffHand);
        assert!(stack.is(&vanilla_items::FILLED_MAP));
        *stack.get(MAP_ID).expect("replacement map ID")
    };
    assert_ne!(first, second);
    assert!(world.get_map_data(first).is_some());
    assert!(world.get_map_data(second).is_some());
}
