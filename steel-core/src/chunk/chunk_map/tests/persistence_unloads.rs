use super::*;
use std::thread;

#[test]
fn world_tick_spawns_dirty_unload_save_on_the_chunk_runtime() {
    let world = fresh_test_world("world_tick_dirty_unload");
    let pos = ChunkPos::new(2, 3);
    let holder = unloaded_light_holder(pos);
    let Some(chunk) = holder.try_chunk(ChunkStatus::Light) else {
        panic!("test holder should contain a light-status chunk");
    };
    chunk.mark_dirty();
    let _ = world
        .chunk_map
        .unloading_chunks
        .insert_sync(pos, Arc::clone(&holder));
    drop(holder);

    let tick_world = Arc::clone(&world);
    let tick = thread::spawn(move || tick_world.tick_game(1, false));
    assert!(
        tick.join().is_ok(),
        "a world tick outside Tokio must still enqueue unload saves"
    );

    stop_chunk_tasks(&world);
}

#[test]
fn save_retry_marks_same_unloading_holder_dirty() {
    let _chunk_map = test_chunk_map();
    let pos = ChunkPos::new(2, 3);
    let holder = unloaded_light_holder(pos);
    let chunk = holder
        .try_chunk(ChunkStatus::Light)
        .expect("test holder should contain a light-status chunk");
    chunk.clear_dirty();

    ChunkMap::mark_chunk_dirty_for_save_retry(&holder);

    let chunk = holder
        .try_chunk(ChunkStatus::Light)
        .expect("test holder should still contain a light-status chunk");
    assert!(chunk.is_dirty());
}

#[test]
fn revival_during_save_preparation_is_retried_at_the_next_lifecycle_boundary() {
    init_vanilla_registry();
    init_behaviors();
    let world = fresh_test_world("save_preparation_revival");
    let chunk_pos = ChunkPos::new(0, 0);
    let original = insert_ready_full_chunk(&world, chunk_pos);

    world.chunk_map.update_chunk_level(chunk_pos, None);
    let preparation = original
        .try_begin_save_preparation()
        .expect("the unloading holder should reserve save preparation");

    assert!(
        world
            .chunk_map
            .update_chunk_level(chunk_pos, Some(ChunkTicketLevel::FULL_CHUNK))
            .is_none(),
        "revival must be staged instead of blocking the lifecycle thread"
    );
    assert!(world.chunk_map.unloading_chunks.contains_sync(&chunk_pos));
    assert!(!world.chunk_map.chunks.contains_sync(&chunk_pos));

    drop(preparation);

    let mut changes = Vec::new();
    world.chunk_map.merge_deferred_revivals(&mut changes);
    assert_eq!(changes.len(), 1);
    let change = changes[0];
    let Some(revived) = world
        .chunk_map
        .update_chunk_level(change.pos, change.new_level)
    else {
        panic!("revival should retry after save preparation releases the holder");
    };

    assert!(Arc::ptr_eq(&original, &revived));
    assert!(world.chunk_map.chunks.contains_sync(&chunk_pos));
    assert!(!world.chunk_map.unloading_chunks.contains_sync(&chunk_pos));
}

#[test]
fn ticket_receipt_waits_for_deferred_holder_revival() {
    let world = fresh_test_world("deferred_revival_receipt");
    let pos = ChunkPos::new(0, 0);
    let holder = insert_ready_full_chunk(&world, pos);
    world.chunk_map.update_chunk_level(pos, None);
    let preparation = holder
        .try_begin_save_preparation()
        .expect("the unloading holder should reserve save preparation");

    let receipt = world
        .chunk_map
        .acquire_chunk_request_leases(&[pos], ChunkTicketLevel::MAX)
        .expect("one request lease should produce a receipt");
    world.chunk_map.advance_scheduling();

    assert!(!world.chunk_map.is_ticket_receipt_committed(receipt));
    assert!(!world.chunk_map.chunks.contains_sync(&pos));

    drop(preparation);
    world.chunk_map.advance_scheduling();

    assert!(world.chunk_map.is_ticket_receipt_committed(receipt));
    assert!(
        world
            .chunk_map
            .chunks
            .read_sync(&pos, |_, active| Arc::ptr_eq(active, &holder))
            .unwrap_or(false),
        "the receipt should publish only after the original holder revives"
    );

    stop_chunk_tasks(&world);
}

#[test]
fn newer_ticket_change_replaces_a_deferred_revival() {
    init_vanilla_registry();
    init_behaviors();
    let world = fresh_test_world("save_preparation_revival_override");
    let chunk_pos = ChunkPos::new(0, 0);
    let holder = insert_ready_full_chunk(&world, chunk_pos);

    world.chunk_map.update_chunk_level(chunk_pos, None);
    let preparation = holder
        .try_begin_save_preparation()
        .expect("the unloading holder should reserve save preparation");
    assert!(
        world
            .chunk_map
            .update_chunk_level(chunk_pos, Some(ChunkTicketLevel::FULL_CHUNK))
            .is_none()
    );
    drop(preparation);

    let removal = LoadLevelChange {
        pos: chunk_pos,
        new_level: None,
    };
    let mut changes = vec![removal];
    world.chunk_map.merge_deferred_revivals(&mut changes);

    assert_eq!(changes, vec![removal]);
    assert!(world.chunk_map.unloading_chunks.contains_sync(&chunk_pos));
}

#[test]
fn final_full_chunk_unload_finalizes_chunk_owned_tick_queues() {
    init_vanilla_registry();
    init_behaviors();
    let world = fresh_test_world("chunk_owned_tick_unload");
    let chunk_pos = ChunkPos::new(0, 0);
    let holder = insert_ready_full_chunk(&world, chunk_pos);
    let Some(chunk) = holder.try_full_chunk() else {
        panic!("inserted test chunk must remain Full");
    };
    let block_entity_pos = BlockPos::new(1, 64, 1);
    let block_entity = add_test_comparator(chunk, block_entity_pos);
    let sign_pos = BlockPos::new(2, 64, 1);
    let sign = add_test_sign(chunk, sign_pos);
    chunk.schedule_block_tick(
        BlockPos::new(3, 64, 1),
        &vanilla_blocks::STONE,
        10,
        TickPriority::Normal,
        0,
    );
    chunk.common().take_dirty();
    assert!(world.has_registered_full_chunk_ticks(chunk_pos));
    assert!(world.has_indexed_scheduled_tick_head(chunk_pos));
    assert_eq!(world.block_entity_tickers().registered_len(), 1);

    world.chunk_map.update_chunk_level(chunk_pos, None);
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
    init_vanilla_registry();
    init_behaviors();
    let world = fresh_test_world("chunk_owned_tick_revival");
    let chunk_pos = ChunkPos::new(0, 0);
    let block_pos = BlockPos::new(1, 64, 1);
    let original = insert_ready_full_chunk(&world, chunk_pos);
    world.schedule_block_tick(block_pos, &vanilla_blocks::STONE, 3, TickPriority::Normal);
    assert!(world.has_indexed_scheduled_tick_head(chunk_pos));
    let Some(chunk) = original.try_full_chunk() else {
        panic!("inserted test chunk must remain Full");
    };
    let block_entity = add_test_comparator(chunk, block_pos);

    world.chunk_map.update_chunk_level(chunk_pos, None);
    assert!(world.has_registered_full_chunk_ticks(chunk_pos));
    let Some(revived) = world
        .chunk_map
        .update_chunk_level(chunk_pos, Some(ChunkTicketLevel::BLOCK_TICKING_CHUNK))
    else {
        panic!("restored ticket level must revive the unloading holder");
    };
    world.chunk_map.rebuild_ticking_chunk_snapshot();

    assert!(Arc::ptr_eq(&original, &revived));
    assert!(world.has_scheduled_block_tick(block_pos, &vanilla_blocks::STONE));
    assert!(world.has_indexed_scheduled_tick_head(chunk_pos));
    let Some(revived_chunk) = revived.try_full_chunk() else {
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
    init_vanilla_registry();
    init_behaviors();
    let world = fresh_test_world("weak_full_chunk_revival");
    let chunk_pos = ChunkPos::new(0, 0);
    let sign_pos = BlockPos::new(1, 64, 1);
    let original = insert_ready_full_chunk(&world, chunk_pos);

    world.chunk_map.update_chunk_level(chunk_pos, None);
    let Some(revived) = world
        .chunk_map
        .update_chunk_level(chunk_pos, Some(ChunkTicketLevel::MAX))
    else {
        panic!("a weak load level should revive the unloading holder");
    };
    assert!(Arc::ptr_eq(&original, &revived));

    let Some(chunk) = revived.try_full_chunk() else {
        panic!("weak revival should preserve the serialized Full chunk");
    };
    let _sign = add_test_sign(chunk, sign_pos);
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

    world
        .chunk_map
        .update_chunk_level(chunk_pos, Some(ChunkTicketLevel::BLOCK_TICKING_CHUNK));
    revived.set_simulation_level(Some(ChunkTicketLevel::BLOCK_TICKING_CHUNK));
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
