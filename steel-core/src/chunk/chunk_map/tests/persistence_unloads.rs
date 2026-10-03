use super::*;
use crate::chunk::chunk_holder::ChunkSavePreparationGuard;
use crate::chunk::chunk_pyramid::GENERATION_PYRAMID;
use crate::chunk::chunk_request::{ChunkRequest, ChunkTicketKind};
use std::thread;

/// Unloads a ready Full holder at `pos` and reserves its save preparation.
fn unloading_holder_with_save_preparation(
    world: &Arc<World>,
    pos: ChunkPos,
) -> (Arc<ChunkHolder>, ChunkSavePreparationGuard) {
    let holder = insert_ready_full_chunk(world, pos);
    world.chunk_map.update_chunk_level(pos, None);
    let preparation = holder
        .try_begin_save_preparation()
        .expect("the unloading holder should reserve save preparation");
    (holder, preparation)
}

fn revive_at_full(world: &Arc<World>, pos: ChunkPos) -> Arc<ChunkHolder> {
    world
        .chunk_map
        .update_chunk_level(pos, Some(ChunkTicketLevel::FULL_CHUNK))
        .expect("revival must win the race against an in-flight save preparation")
}

fn assert_snapshot_discarded(preparation: ChunkSavePreparationGuard) {
    assert!(
        preparation.finish(()).is_none(),
        "the preparation that lost the race must discard its snapshot"
    );
}

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
fn revival_during_save_preparation_activates_the_holder_immediately() {
    init_vanilla_registry();
    init_behaviors();
    let world = fresh_test_world("save_preparation_revival");
    let chunk_pos = ChunkPos::new(0, 0);
    let (original, preparation) = unloading_holder_with_save_preparation(&world, chunk_pos);

    let revived = revive_at_full(&world, chunk_pos);

    assert!(Arc::ptr_eq(&original, &revived));
    assert!(world.chunk_map.chunks.contains_sync(&chunk_pos));
    assert!(!world.chunk_map.unloading_chunks.contains_sync(&chunk_pos));
    assert_snapshot_discarded(preparation);
}

#[test]
fn ticket_receipt_commits_while_the_holder_is_still_preparing_a_save() {
    let world = fresh_test_world("save_preparation_receipt");
    let pos = ChunkPos::new(0, 0);
    let (holder, preparation) = unloading_holder_with_save_preparation(&world, pos);

    let receipt = world
        .chunk_map
        .acquire_chunk_request_leases(&[pos], ChunkTicketLevel::MAX)
        .expect("one request lease should produce a receipt");
    world.chunk_map.advance_scheduling();

    assert!(world.chunk_map.is_ticket_receipt_committed(receipt));
    assert!(
        world
            .chunk_map
            .chunks
            .read_sync(&pos, |_, active| Arc::ptr_eq(active, &holder))
            .unwrap_or(false),
        "the original holder must be active again without waiting for the save"
    );
    assert_snapshot_discarded(preparation);

    stop_chunk_tasks(&world);
}

#[test]
fn revival_during_save_preparation_keeps_the_generation_neighborhood_complete() {
    init_vanilla_registry();
    init_behaviors();
    let world = fresh_test_world("save_preparation_revival_neighborhood");
    let pinned = ChunkPos::new(0, 0);
    let neighbor = ChunkPos::new(1, 0);
    let target_status = ChunkStatus::Biomes;
    let radius = GENERATION_PYRAMID
        .get_step_to(target_status)
        .accumulated_dependencies
        .get_radius_of(ChunkStatus::Empty) as i32;

    // Every position in the neighbor's dependency square needs a live holder.
    for x in (neighbor.0.x - radius)..=(neighbor.0.x + radius) {
        for z in (neighbor.0.y - radius)..=(neighbor.0.y + radius) {
            let pos = ChunkPos::new(x, z);
            if pos != pinned {
                world
                    .chunk_map
                    .update_chunk_level(pos, Some(ChunkTicketLevel::MAX));
            }
        }
    }

    let (holder, preparation) = unloading_holder_with_save_preparation(&world, pinned);
    let revived = revive_at_full(&world, pinned);
    assert!(Arc::ptr_eq(&holder, &revived));

    // Before the fix the pinned position was a hole in `chunks` and this panicked.
    let task = world
        .chunk_map
        .schedule_generation_task_b(target_status, neighbor);
    assert!(Arc::ptr_eq(task.cache.get(pinned.0.x, pinned.0.y), &holder));

    task.cancel();
    assert_snapshot_discarded(preparation);
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
    world.chunk_map.process_unloads();

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

#[test]
fn gameplay_cache_scopes_observe_a_full_holder_revived_between_phases() {
    let world = fresh_test_world("cache_scope_revival");
    let pos = ChunkPos::new(0, 0);
    let holder = insert_ready_full_chunk(&world, pos);
    world.chunk_map.update_chunk_level(pos, None);

    let scheduled_scope = GameplayChunkLookupCacheScope::enter(&world.chunk_map);
    assert!(world.chunk_map.active_full_chunk_holder(pos).is_none());
    assert!(world.chunk_map.active_full_chunk_holder(pos).is_none());
    assert_eq!(scheduled_scope.finish().missing_hits, 1);

    let request = world.chunk_map.request_chunks(ChunkRequest {
        positions: vec![pos],
        status: ChunkStatus::Full,
        ticket_kind: ChunkTicketKind::Player,
    });
    // Includes the nested readiness cache and the later gameplay scopes.
    world.tick_game(1, false);

    let gameplay_scope = GameplayChunkLookupCacheScope::enter(&world.chunk_map);
    let revived = world
        .chunk_map
        .active_full_chunk_holder(pos)
        .expect("later gameplay must see the revived Full holder");
    assert!(Arc::ptr_eq(&holder, &revived));
    assert_eq!(gameplay_scope.finish().missing_hits, 0);
    drop(request);
    stop_chunk_tasks(&world);
}
