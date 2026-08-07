use super::*;

#[test]
fn sparse_scheduler_collects_a_registered_chunk_owned_tick() {
    init_vanilla_registry();
    init_behaviors();
    let world = fresh_test_world("chunk_owned_tick_collection");
    let chunk_pos = ChunkPos::new(0, 0);
    let block_pos = BlockPos::new(1, 64, 1);
    insert_ready_full_chunk(&world, chunk_pos);
    world.schedule_block_tick(block_pos, &vanilla_blocks::STONE, 1, TickPriority::Normal);
    assert!(world.has_scheduled_block_tick(block_pos, &vanilla_blocks::STONE));

    // This focused test enters `ChunkMap` directly, so mirror the world
    // phase that advances game time before scheduled-tick collection.
    world.level_data.write().set_game_time(1);
    world.chunk_map.tick_game(&world, 1, 0, true);

    assert!(!world.has_scheduled_block_tick(block_pos, &vanilla_blocks::STONE));
}

#[test]
fn block_callback_ticks_respect_the_block_fluid_phase_boundary() {
    init_vanilla_registry();
    init_behaviors();
    let world = fresh_test_world("scheduled_tick_phase_boundary");
    let chunk_pos = ChunkPos::new(0, 0);
    let initial_block_pos = BlockPos::new(1, 64, 1);
    let callback_block_pos = BlockPos::new(2, 64, 1);
    let callback_fluid_pos = BlockPos::new(3, 64, 1);
    insert_ready_full_chunk(&world, chunk_pos);
    world.level_data.write().set_game_time(20);
    world.schedule_block_tick(
        initial_block_pos,
        &vanilla_blocks::STONE,
        0,
        TickPriority::Normal,
    );
    let blocks = world.begin_scheduled_tick_phase(20, MAX_SCHEDULED_TICKS_PER_TICK);
    assert_eq!(blocks.ticks.len(), 1);
    assert_eq!(blocks.ticks[0].pos, initial_block_pos);

    // Simulate the selected block callback. Block collection has already
    // closed, while the same game tick's fluid phase has not yet started.
    world.schedule_block_tick(
        callback_block_pos,
        &vanilla_blocks::STONE,
        0,
        TickPriority::Normal,
    );
    world.schedule_fluid_tick(
        callback_fluid_pos,
        &vanilla_fluids::WATER,
        0,
        TickPriority::Normal,
    );

    let fluids = world.collect_scheduled_fluid_tick_batch(20, MAX_SCHEDULED_TICKS_PER_TICK);
    assert_eq!(fluids.ticks.len(), 1);
    assert_eq!(fluids.ticks[0].pos, callback_fluid_pos);
    assert!(world.has_scheduled_block_tick(callback_block_pos, &vanilla_blocks::STONE));

    let next_blocks = world.begin_scheduled_tick_phase(21, MAX_SCHEDULED_TICKS_PER_TICK);
    assert_eq!(next_blocks.ticks.len(), 1);
    assert_eq!(next_blocks.ticks[0].pos, callback_block_pos);
}

#[test]
fn earlier_live_insertion_replaces_the_sparse_container_head() {
    init_vanilla_registry();
    init_behaviors();
    let world = fresh_test_world("scheduled_tick_head_replacement");
    let chunk_pos = ChunkPos::new(0, 0);
    let later_pos = BlockPos::new(1, 64, 1);
    let earlier_pos = BlockPos::new(2, 64, 1);
    insert_ready_full_chunk(&world, chunk_pos);

    world.schedule_block_tick(later_pos, &vanilla_blocks::STONE, 10, TickPriority::Normal);
    world.schedule_block_tick(earlier_pos, &vanilla_blocks::STONE, 1, TickPriority::Normal);
    world.schedule_block_tick(earlier_pos, &vanilla_blocks::STONE, 20, TickPriority::High);
    world.level_data.write().set_game_time(1);
    world.chunk_map.tick_game(&world, 1, 0, true);

    assert!(!world.has_scheduled_block_tick(earlier_pos, &vanilla_blocks::STONE));
    assert!(world.has_scheduled_block_tick(later_pos, &vanilla_blocks::STONE));
}

#[test]
fn registered_full_chunks_use_active_order_for_equal_explicit_tick_heads() {
    init_vanilla_registry();
    init_behaviors();
    let world = fresh_test_world("registered_explicit_tick_tie");
    let first_chunk_pos = ChunkPos::new(0, 0);
    let second_chunk_pos = ChunkPos::new(1, 0);
    let first_tick_pos = BlockPos::new(1, 64, 1);
    let second_tick_pos = BlockPos::new(17, 64, 1);
    let first = insert_ready_full_chunk(&world, first_chunk_pos);
    let second = insert_ready_full_chunk(&world, second_chunk_pos);

    for (holder, tick_pos) in [(&first, first_tick_pos), (&second, second_tick_pos)] {
        let Some(chunk) = holder.try_full_chunk() else {
            panic!("inserted test chunk must remain Full");
        };
        chunk.schedule_block_tick(tick_pos, &vanilla_blocks::STONE, 1, TickPriority::Normal, 0);
    }

    if let Err(error) = world
        .reconcile_active_scheduled_tick_chunks([second_chunk_pos, first_chunk_pos].into_iter())
    {
        panic!("test scheduler invariant failed: {error:?}");
    }
    let batch = world.begin_scheduled_tick_phase(1, MAX_SCHEDULED_TICKS_PER_TICK);
    assert_eq!(
        batch.ticks.iter().map(|tick| tick.pos).collect::<Vec<_>>(),
        [second_tick_pos, first_tick_pos]
    );
}
