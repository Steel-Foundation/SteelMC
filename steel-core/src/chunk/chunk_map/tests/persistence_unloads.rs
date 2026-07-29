use super::*;

#[test]
fn save_retry_marks_same_unloading_holder_dirty() {
    let _chunk_map = test_chunk_map();
    let pos = ChunkPos::new(2, 3);
    let holder = unloaded_light_holder(pos);
    let chunk = holder
        .try_chunk(ChunkStatus::Light)
        .expect("test holder should contain a light-status chunk");
    chunk.clear_dirty();
    drop(chunk);

    ChunkMap::mark_chunk_dirty_for_save_retry(&holder);

    let chunk = holder
        .try_chunk(ChunkStatus::Light)
        .expect("test holder should still contain a light-status chunk");
    assert!(chunk.is_dirty());
}

#[test]
fn final_full_chunk_unload_finalizes_chunk_owned_tick_queues() {
    init_test_registry();
    init_behaviors();
    let world = fresh_test_world("chunk_owned_tick_unload");
    let chunk_pos = ChunkPos::new(0, 0);
    let holder = insert_ready_full_chunk(&world, chunk_pos);
    let Some(chunk) = holder.try_chunk(ChunkStatus::Full) else {
        panic!("inserted test chunk must remain Full");
    };
    let block_entity_pos = BlockPos::new(1, 64, 1);
    let block_entity = add_test_comparator(&chunk, block_entity_pos);
    let sign_pos = BlockPos::new(2, 64, 1);
    let sign = add_test_sign(&chunk, sign_pos);
    chunk.schedule_block_tick(
        BlockPos::new(3, 64, 1),
        &vanilla_blocks::STONE,
        10,
        TickPriority::Normal,
        0,
    );
    chunk.take_dirty();
    drop(chunk);
    assert!(world.has_registered_full_chunk_ticks(chunk_pos));
    assert!(world.has_indexed_scheduled_tick_head(chunk_pos));
    assert_eq!(world.block_entity_tickers().registered_len(), 1);

    world.chunk_map.update_chunk_level(chunk_pos, None, None);
    world.chunk_map.rebuild_ticking_chunk_snapshot();
    drop(holder);
    let _runtime_guard = world.chunk_map.chunk_runtime.enter();
    world.chunk_map.process_unloads(&FxHashSet::default());

    assert!(!world.chunk_map.unloading_chunks.contains_sync(&chunk_pos));
    assert!(!world.has_registered_full_chunk_ticks(chunk_pos));
    assert!(!world.has_indexed_scheduled_tick_head(chunk_pos));
    assert!(block_entity.is_removed());
    assert!(sign.is_removed());
    assert_eq!(world.block_entity_tickers().registered_len(), 1);

    world.chunk_map.finish_block_entity_unloads();
    assert_eq!(world.block_entity_tickers().registered_len(), 0);
}

#[test]
fn unloading_full_chunk_revival_keeps_chunk_owned_tick_queues() {
    init_test_registry();
    init_behaviors();
    let world = fresh_test_world("chunk_owned_tick_revival");
    let chunk_pos = ChunkPos::new(0, 0);
    let block_pos = BlockPos::new(1, 64, 1);
    let original = insert_ready_full_chunk(&world, chunk_pos);
    world.schedule_block_tick(block_pos, &vanilla_blocks::STONE, 3, TickPriority::Normal);
    assert!(world.has_indexed_scheduled_tick_head(chunk_pos));
    let Some(chunk) = original.try_chunk(ChunkStatus::Full) else {
        panic!("inserted test chunk must remain Full");
    };
    let block_entity = add_test_comparator(&chunk, block_pos);
    drop(chunk);

    world.chunk_map.update_chunk_level(chunk_pos, None, None);
    assert!(world.has_registered_full_chunk_ticks(chunk_pos));
    let Some(revived) = world.chunk_map.update_chunk_level(
        chunk_pos,
        Some(ChunkTicketLevel::BLOCK_TICKING_CHUNK),
        Some(ChunkTicketLevel::BLOCK_TICKING_CHUNK),
    ) else {
        panic!("restored ticket level must revive the unloading holder");
    };
    world.chunk_map.rebuild_ticking_chunk_snapshot();

    assert!(Arc::ptr_eq(&original, &revived));
    assert!(world.has_scheduled_block_tick(block_pos, &vanilla_blocks::STONE));
    assert!(world.has_indexed_scheduled_tick_head(chunk_pos));
    let Some(revived_chunk) = revived.try_chunk(ChunkStatus::Full) else {
        panic!("revived chunk must remain Full");
    };
    let Some(revived_block_entity) = revived_chunk.get_block_entity(block_pos) else {
        panic!("revival should preserve the block entity");
    };
    assert!(Arc::ptr_eq(&block_entity, &revived_block_entity));
    assert!(!block_entity.is_removed());
}

#[test]
fn weak_revival_stays_dormant_until_the_same_holder_returns_to_full() {
    init_test_registry();
    init_behaviors();
    let world = fresh_test_world("weak_full_chunk_revival");
    let chunk_pos = ChunkPos::new(0, 0);
    let sign_pos = BlockPos::new(1, 64, 1);
    let original = insert_ready_full_chunk(&world, chunk_pos);

    world.chunk_map.update_chunk_level(chunk_pos, None, None);
    let Some(revived) =
        world
            .chunk_map
            .update_chunk_level(chunk_pos, Some(ChunkTicketLevel::MAX), None)
    else {
        panic!("a weak load level should revive the unloading holder");
    };
    assert!(Arc::ptr_eq(&original, &revived));

    let Some(chunk) = revived.try_chunk(ChunkStatus::Full) else {
        panic!("weak revival should preserve the serialized Full chunk");
    };
    let _sign = add_test_sign(&chunk, sign_pos);
    drop(chunk);
    assert_eq!(world.block_entity_tickers().registered_len(), 0);

    insert_active_full_holder(
        &world,
        ChunkPos::new(8, 8),
        ChunkTicketLevel::FULL_CHUNK,
        Vec::new(),
    );
    let snapshot_changed = world
        .chunk_map
        .reconcile_ticking_readiness(&[])
        .expect("the unrelated Full publication should reconcile");
    assert!(
        !snapshot_changed,
        "a Full publication without a readiness transition must keep the snapshot"
    );
    assert_eq!(
        world.block_entity_tickers().registered_len(),
        0,
        "another holder's publication must not activate a weakly loaded chunk"
    );

    world.chunk_map.update_chunk_level(
        chunk_pos,
        Some(ChunkTicketLevel::BLOCK_TICKING_CHUNK),
        Some(ChunkTicketLevel::BLOCK_TICKING_CHUNK),
    );
    world
        .chunk_map
        .reconcile_ticking_readiness(&[])
        .expect("the promoted holder's Full publication should reconcile");
    assert_eq!(
        world.block_entity_tickers().registered_len(),
        1,
        "promotion back to Full must activate the holder's staged ticker"
    );
}
