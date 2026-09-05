use super::*;
use crate::chunk::chunk_ticket_manager::ticket_level_for_status;

fn cancel_queued_tasks(map: &ChunkMap) {
    for task in map.pending_generation_tasks.lock().drain(..) {
        task.center_holder.cancel_generation_task();
    }
}

#[test]
fn deferred_dependency_retries_unchanged_center_without_blocking_other_chunks() {
    init_vanilla_registry();
    init_behaviors();
    let world = fresh_test_world("generation_deferred_dependency");
    let map = &world.chunk_map;
    // Inspect task creation without workers consuming the queue.
    map.stop_generation_refill_loop();
    let center = ChunkPos::new(0, 0);
    let neighbor_pos = ChunkPos::new(1, 0);
    let unrelated = ChunkPos::new(100, 100);
    let neighbor = insert_ready_full_chunk(&world, neighbor_pos);
    map.update_chunk_level(neighbor_pos, None);
    let preparation = neighbor
        .try_begin_save_preparation()
        .expect("unloading neighbor should reserve save preparation");
    let level = ticket_level_for_status(ChunkStatus::Carvers);
    let _center_lease = map
        .acquire_chunk_request_leases(&[center], level)
        .expect("center request lease should produce receipt");
    let _unrelated_lease = map
        .acquire_chunk_request_leases(&[unrelated], level)
        .expect("unrelated request lease should produce receipt");
    map.advance_scheduling();

    assert!(map.deferred_generation.lock().contains(&center));
    assert!(!map.deferred_generation.lock().contains(&unrelated));
    assert!(!map.chunks.contains_sync(&neighbor_pos));
    {
        let tasks = map.pending_generation_tasks.lock();
        assert!(
            tasks
                .iter()
                .any(|task| task.center_holder.get_pos() == unrelated)
        );
        assert!(
            !tasks
                .iter()
                .any(|task| task.center_holder.get_pos() == center)
        );
    }

    drop(preparation);
    map.advance_scheduling();
    assert!(!map.deferred_generation.lock().contains(&center));
    {
        let tasks = map.pending_generation_tasks.lock();
        let task = tasks
            .iter()
            .find(|task| task.center_holder.get_pos() == center)
            .expect("unchanged center should retry after its neighbor revives");
        assert_eq!(Some(task.target_status), generation_status(Some(level)));
        assert!(Arc::ptr_eq(
            task.cache.get(neighbor_pos.0.x, neighbor_pos.0.y),
            &neighbor
        ));
    }
    cancel_queued_tasks(map);
    stop_chunk_tasks(&world);
}

#[test]
fn deferred_generation_uses_current_ticket_after_removal_or_demotion() {
    init_vanilla_registry();
    init_behaviors();
    for replacement in [
        None,
        Some(ticket_level_for_status(ChunkStatus::StructureStarts)),
    ] {
        let world = fresh_test_world("generation_deferred_ticket_change");
        let map = &world.chunk_map;
        map.stop_generation_refill_loop();
        let center = ChunkPos::new(0, 0);
        let neighbor_pos = ChunkPos::new(1, 0);
        let neighbor = insert_ready_full_chunk(&world, neighbor_pos);
        map.update_chunk_level(neighbor_pos, None);
        let preparation = neighbor
            .try_begin_save_preparation()
            .expect("unloading neighbor should reserve save preparation");
        let initial_level = ticket_level_for_status(ChunkStatus::Carvers);
        let _initial_lease = map
            .acquire_chunk_request_leases(&[center], initial_level)
            .expect("request lease should produce receipt");
        map.advance_scheduling();
        assert!(map.deferred_generation.lock().contains(&center));

        let _ = map.release_chunk_request_leases(&[center], initial_level);
        if let Some(level) = replacement {
            let _ = map.acquire_chunk_request_leases(&[center], level);
        }
        map.advance_scheduling();
        assert!(!map.deferred_generation.lock().contains(&center));
        {
            let tasks = map.pending_generation_tasks.lock();
            let target = tasks
                .iter()
                .find(|task| task.center_holder.get_pos() == center)
                .map(|task| task.target_status);
            assert_eq!(target, generation_status(replacement));
        }
        drop(preparation);
        cancel_queued_tasks(map);
        stop_chunk_tasks(&world);
    }
}
