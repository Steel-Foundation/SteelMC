use super::*;
use crate::chunk::chunk_ticket_manager::ticket_level_for_status;

fn take_prepared_epoch(map: &ChunkMap) -> PreparedChunkSchedulingEpoch {
    for _ in 0..10_000 {
        match map.scheduling.take_boundary_step() {
            ChunkSchedulingBoundaryStep::Commit(epoch) => return epoch,
            ChunkSchedulingBoundaryStep::Running => thread::sleep(Duration::from_millis(1)),
            ChunkSchedulingBoundaryStep::Start { .. } => {
                panic!("scheduler must already be started")
            }
        }
    }
    panic!("scheduling epoch did not finish");
}

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
    map.update_chunk_level(neighbor_pos, None, None);
    let preparation = neighbor
        .try_begin_save_preparation()
        .expect("unloading neighbor should reserve save preparation");
    let level = ticket_level_for_status(ChunkStatus::Carvers);
    let ticket = ChunkTicket::loading(level);
    map.add_chunk_ticket(center, ticket);
    map.add_chunk_ticket(unrelated, ticket);
    map.advance_scheduling();
    map.commit_scheduling_epoch(take_prepared_epoch(map));

    let blocked = take_prepared_epoch(map);
    assert!(blocked.deferred_generation.contains(&center));
    assert!(!blocked.deferred_generation.contains(&unrelated));
    assert_eq!(blocked.changes, []);
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
    map.commit_scheduling_epoch(blocked);
    let resumed = take_prepared_epoch(map);
    assert!(!resumed.deferred_generation.contains(&center));
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
        map.update_chunk_level(neighbor_pos, None, None);
        let preparation = neighbor
            .try_begin_save_preparation()
            .expect("unloading neighbor should reserve save preparation");
        let ticket = ChunkTicket::loading(ticket_level_for_status(ChunkStatus::Carvers));
        map.add_chunk_ticket(center, ticket);
        map.advance_scheduling();
        map.commit_scheduling_epoch(take_prepared_epoch(map));
        let blocked = take_prepared_epoch(map);
        assert!(blocked.deferred_generation.contains(&center));

        map.remove_chunk_ticket(center, ticket);
        if let Some(level) = replacement {
            map.add_chunk_ticket(center, ChunkTicket::loading(level));
        }
        // One epoch propagates the changed tickets, then the boundary commits them.
        map.commit_scheduling_epoch(blocked);
        map.commit_scheduling_epoch(take_prepared_epoch(map));
        let changed = take_prepared_epoch(map);
        assert!(!changed.deferred_generation.contains(&center));
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
    }
}
