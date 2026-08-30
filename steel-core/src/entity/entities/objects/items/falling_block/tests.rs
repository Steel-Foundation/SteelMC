use steel_registry::init_vanilla_registry;
use steel_utils::ChunkPos;
use uuid::Uuid;

use super::*;
use crate::behavior::init_behaviors;
use crate::entity::{EntityBaseSaveData, EntityFireFreezeState};
use crate::test_support::{fresh_test_world, insert_ready_full_chunk};

fn falling_test_world(key: &'static str) -> Arc<World> {
    init_vanilla_registry();
    init_behaviors();
    let world = fresh_test_world(key);
    insert_ready_full_chunk(&world, ChunkPos::new(0, 0));
    world
}

fn set_test_block(world: &Arc<World>, pos: BlockPos, state: BlockStateId) {
    assert!(world.set_block(pos, state, UpdateFlags::UPDATE_ALL));
}

fn tick_until_settled(entities: &[&Arc<FallingBlockEntity>]) {
    for _ in 0..240 {
        for entity in entities {
            if entity.is_alive() {
                entity.set_old_position_to_current();
                entity.advance_tick_count();
                entity.tick();
            }
        }
        if entities.iter().all(|entity| entity.is_removed()) {
            return;
        }
    }
    panic!("falling block entities did not settle within the test limit");
}

fn start_falling(
    world: &Arc<World>,
    pos: BlockPos,
    state: BlockStateId,
) -> Arc<FallingBlockEntity> {
    set_test_block(world, pos, state);
    FallingBlockEntity::fall(world, pos, state)
}

#[test]
fn stacked_falling_blocks_settle_in_order_without_collapsing_into_one_state() {
    let world = falling_test_world("stacked_falling_blocks_settle");
    let ground = BlockPos::new(4, 64, 4);
    set_test_block(&world, ground, vanilla_blocks::STONE.default_state());
    let lower = start_falling(
        &world,
        BlockPos::new(4, 72, 4),
        vanilla_blocks::SAND.default_state(),
    );
    let upper = start_falling(
        &world,
        BlockPos::new(4, 73, 4),
        vanilla_blocks::SAND.default_state(),
    );

    tick_until_settled(&[&lower, &upper]);

    assert_eq!(
        world.get_block_state(ground.above()),
        vanilla_blocks::SAND.default_state()
    );
    assert_eq!(
        world.get_block_state(ground.above().above()),
        vanilla_blocks::SAND.default_state()
    );
}

#[test]
fn concrete_powder_distinguishes_dry_side_water_and_fast_water_entry_landings() {
    let world = falling_test_world("concrete_powder_water_landings");
    let powder = vanilla_blocks::WHITE_CONCRETE_POWDER.default_state();

    let dry_ground = BlockPos::new(3, 64, 3);
    let side_ground = BlockPos::new(7, 64, 3);
    for ground in [dry_ground, side_ground] {
        set_test_block(&world, ground, vanilla_blocks::STONE.default_state());
    }
    set_test_block(
        &world,
        side_ground.above().relative(Direction::East),
        vanilla_blocks::WATER.default_state(),
    );
    set_test_block(
        &world,
        BlockPos::new(11, 70, 3),
        vanilla_blocks::WATER.default_state(),
    );

    let dry = start_falling(&world, BlockPos::new(3, 78, 3), powder);
    let beside_water = start_falling(&world, BlockPos::new(7, 78, 3), powder);
    let enters_water = start_falling(&world, BlockPos::new(11, 78, 3), powder);
    // Cross the floating one-block water source in one movement. Without the
    // vanilla fast-concrete raycast the entity ends below it and keeps falling.
    enters_water.set_velocity(DVec3::new(0.0, -8.0, 0.0));

    tick_until_settled(&[&dry, &beside_water, &enters_water]);

    assert_eq!(world.get_block_state(dry_ground.above()), powder);
    assert_eq!(
        world.get_block_state(side_ground.above()),
        vanilla_blocks::WHITE_CONCRETE.default_state()
    );
    assert_eq!(
        world.get_block_state(BlockPos::new(11, 70, 3)),
        vanilla_blocks::WHITE_CONCRETE.default_state()
    );
}

#[test]
fn falling_block_persistence_and_spawn_data_preserve_the_carried_state() {
    init_vanilla_registry();
    let block_state = vanilla_blocks::RED_SAND.default_state();
    let entity = FallingBlockEntity::with_block_state(
        &vanilla_entities::FALLING_BLOCK,
        42,
        DVec3::new(1.5, 72.0, -2.5),
        block_state,
        Weak::new(),
    );
    {
        let mut state = entity.state.lock();
        state.time = 37;
        state.drop_item = false;
        state.cancel_drop = true;
        state.hurt_entities = true;
        state.fall_damage_per_distance = 2.25;
        state.fall_damage_max = 19;
        let mut block_data = NbtCompound::new();
        block_data.insert("payload", 7_i32);
        state.block_data = Some(block_data);
    }

    let mut saved = NbtCompound::new();
    entity.save_additional(&mut saved);
    let mut bytes = Vec::new();
    saved.write(&mut bytes);
    let Ok(borrowed) = read_compound(&mut Cursor::new(bytes.as_slice())) else {
        panic!("saved falling block NBT should reborrow");
    };
    let loaded = FallingBlockEntity::from_saved(
        &vanilla_entities::FALLING_BLOCK,
        EntityBaseLoad {
            id: 43,
            position: DVec3::ZERO,
            uuid: Uuid::nil(),
            velocity: DVec3::ZERO,
            rotation: (0.0, 0.0),
            fall_distance: 0.0,
            fire_freeze: EntityFireFreezeState::new(),
            on_ground: false,
            save_data: EntityBaseSaveData::new(),
            world: Weak::new(),
        },
    );
    loaded.load_additional((&borrowed).into());

    let loaded_state = loaded.state.lock();
    assert_eq!(loaded_state.block_state, block_state);
    assert_eq!(loaded_state.time, 37);
    assert!(!loaded_state.drop_item);
    assert!(loaded_state.cancel_drop);
    assert!(loaded_state.hurt_entities);
    assert_eq!(
        loaded_state.fall_damage_per_distance.to_bits(),
        2.25_f32.to_bits()
    );
    assert_eq!(loaded_state.fall_damage_max, 19);
    assert_eq!(
        loaded_state
            .block_data
            .as_ref()
            .and_then(|data| data.get("payload")),
        Some(&NbtTag::Int(7))
    );
    assert_eq!(entity.spawn_data(), i32::from(block_state.0));
    assert_eq!(entity.start_pos(), BlockPos::new(1, 72, -3));
}
