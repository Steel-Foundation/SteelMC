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
fn incremental_activation_reports_the_stable_layout_slot() {
    init_vanilla_registry();
    init_behaviors();
    let world = fresh_test_world("incremental_scheduled_tick_layout_slot");
    for pos in [
        ChunkPos::new(0, 0),
        ChunkPos::new(1, 0),
        ChunkPos::new(2, 0),
    ] {
        insert_ready_full_chunk(&world, pos);
    }

    let initial = world.chunk_map.ticking_chunks.load_full();
    let target_slot = 1;
    let target = &initial.layout.entries[target_slot];
    let target_pos = target.pos;
    let target_holder = Arc::clone(&target.holder);
    target_holder.set_simulation_level(None);
    world.chunk_map.rebuild_ticking_chunk_snapshot();

    let tick_pos = BlockPos::new(target_pos.0.x * 16 + 1, 64, target_pos.0.y * 16 + 1);
    world.schedule_block_tick(tick_pos, &vanilla_blocks::STONE, 0, TickPriority::Normal);
    let before_activation = world.chunk_map.ticking_chunks.load_full();
    assert!(!before_activation.block.contains(target_slot));
    for entry in &before_activation.layout.entries {
        let Some(chunk) = entry.holder.try_chunk(ChunkStatus::Full) else {
            panic!("layout entry should remain Full");
        };
        chunk.take_dirty();
    }

    let _ = world.chunk_map.add_chunk_ticket(
        target_pos,
        ChunkTicket::full_chunks_with_entity_ticking(0, 0),
    );
    world.chunk_map.flush_simulation_updates();
    let after_activation = world.chunk_map.ticking_chunks.load_full();
    assert!(Arc::ptr_eq(
        &before_activation.layout,
        &after_activation.layout
    ));
    assert!(after_activation.block.contains(target_slot));

    let collected = ChunkMap::collect_scheduled_block_ticks(&world, &after_activation, 0);
    assert_eq!(collected.len(), 1);
    assert_eq!(collected[0].pos, tick_pos);
    for entry in &after_activation.layout.entries {
        let Some(chunk) = entry.holder.try_chunk(ChunkStatus::Full) else {
            panic!("layout entry should remain Full");
        };
        assert_eq!(chunk.is_dirty(), entry.pos == target_pos);
    }
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
    let layout_generation = world
        .chunk_map
        .ticking_chunks
        .load()
        .layout
        .scheduler_generation;
    let blocks = world
        .begin_scheduled_tick_phase(layout_generation, 20, MAX_SCHEDULED_TICKS_PER_TICK)
        .expect("test snapshot generation should remain current");
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

    let fluids = world
        .collect_scheduled_fluid_tick_batch(layout_generation, 20, MAX_SCHEDULED_TICKS_PER_TICK)
        .expect("test snapshot generation should remain current");
    assert_eq!(fluids.ticks.len(), 1);
    assert_eq!(fluids.ticks[0].pos, callback_fluid_pos);
    assert!(world.has_scheduled_block_tick(callback_block_pos, &vanilla_blocks::STONE));

    let next_blocks = world
        .begin_scheduled_tick_phase(layout_generation, 21, MAX_SCHEDULED_TICKS_PER_TICK)
        .expect("test snapshot generation should remain current");
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

    let layout_generation = match world
        .reconcile_active_scheduled_tick_chunks([second_chunk_pos, first_chunk_pos].into_iter())
    {
        Ok(generation) => generation,
        Err(error) => panic!("test scheduler invariant failed: {error:?}"),
    };
    let batch = world
        .begin_scheduled_tick_phase(layout_generation, 1, MAX_SCHEDULED_TICKS_PER_TICK)
        .expect("test snapshot generation should remain current");
    assert_eq!(
        batch.ticks.iter().map(|tick| tick.pos).collect::<Vec<_>>(),
        [second_tick_pos, first_tick_pos]
    );
}
