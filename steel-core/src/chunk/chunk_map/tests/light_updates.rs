use super::*;

#[test]
fn light_update_center_is_available_in_unloading_chunks() {
    let chunk_map = test_chunk_map();
    let pos = ChunkPos::new(2, 3);
    let holder = unloaded_light_holder(pos);
    let _ = chunk_map.unloading_chunks.insert_sync(pos, holder);

    assert!(chunk_map.light_update_center_is_available(pos));
}

#[test]
fn light_changed_marks_unloading_chunk_dirty() {
    let chunk_map = test_chunk_map();
    let pos = ChunkPos::new(2, 3);
    let holder = unloaded_light_holder(pos);
    let _ = chunk_map
        .unloading_chunks
        .insert_sync(pos, Arc::clone(&holder));

    let chunk = holder
        .try_chunk(ChunkStatus::Light)
        .expect("test holder should contain a light-status chunk");
    chunk.clear_dirty();

    chunk_map.light_changed(LightLayer::Block, SectionPos::new(pos.0.x, 0, pos.0.y));

    let chunk = holder
        .try_chunk(ChunkStatus::Light)
        .expect("test holder should still contain a light-status chunk");
    assert!(chunk.is_dirty());
}

#[test]
fn drained_light_updates_remain_unload_blocking_until_applied() {
    let chunk_map = test_chunk_map();
    let center = ChunkPos::new(0, 0);
    chunk_map
        .light_updates
        .lock()
        .pending
        .queue_change(center, BlockPos::new(1, 2, 3), true, None);

    let Some((_tasks, in_flight_updates)) = chunk_map.drain_pending_light_updates() else {
        panic!("queued light update should drain");
    };

    assert!(chunk_map.light_updates.lock().pending.is_empty());
    assert!(chunk_map.has_pending_light_updates());

    drop(in_flight_updates);

    assert!(!chunk_map.has_pending_light_updates());
}

#[test]
fn light_update_unload_barrier_is_limited_to_cache_window() {
    let chunk_map = test_chunk_map();
    let center = ChunkPos::new(0, 0);
    let inside = ChunkPos::new(LIGHT_CACHE_RADIUS, -LIGHT_CACHE_RADIUS);
    let outside = ChunkPos::new(LIGHT_CACHE_RADIUS + 1, 0);
    chunk_map
        .light_updates
        .lock()
        .pending
        .queue_change(center, BlockPos::new(1, 2, 3), true, None);

    assert!(chunk_map.light_update_touches_chunk(inside));
    assert!(!chunk_map.light_update_touches_chunk(outside));
}

#[test]
fn drained_light_update_window_remains_unload_blocking_until_applied() {
    let chunk_map = test_chunk_map();
    let center = ChunkPos::new(0, 0);
    let inside = ChunkPos::new(LIGHT_CACHE_RADIUS, 0);
    chunk_map
        .light_updates
        .lock()
        .pending
        .queue_change(center, BlockPos::new(1, 2, 3), true, None);

    let Some((_tasks, in_flight_updates)) = chunk_map.drain_pending_light_updates() else {
        panic!("queued light update should drain");
    };

    assert!(chunk_map.light_update_touches_chunk(inside));

    drop(in_flight_updates);

    assert!(!chunk_map.light_update_touches_chunk(inside));
}

#[test]
fn pending_light_updates_coalesce_changes_by_chunk_in_queue_order() {
    let center = ChunkPos::new(0, 0);
    let east = ChunkPos::new(1, 0);
    let center_block = BlockPos::new(1, 2, 3);
    let center_section = SectionPos::new(0, 0, 0);
    let east_block = BlockPos::new(16, 4, 5);
    let mut pending = PendingLightUpdates::default();

    pending.queue_change(center, center_block, true, None);
    pending.queue_change(
        center,
        center_block,
        false,
        Some(LightSectionEmptinessChange {
            section_pos: center_section,
            empty: false,
        }),
    );
    pending.queue_change(east, east_block, true, None);

    let drained = pending.drain();

    assert!(pending.is_empty());
    assert_eq!(drained.len(), 2);
    assert_eq!(drained[0].0, center);
    assert_eq!(drained[1].0, east);
    assert!(drained[0].1.changed_positions.contains(&center_block));
    assert_eq!(
        drained[0].1.changed_sections.get(&center_section),
        Some(&false)
    );
    assert!(drained[1].1.changed_positions.contains(&east_block));
}

#[test]
fn pending_light_updates_prepend_blocked_drained_tasks() {
    let center = ChunkPos::new(0, 0);
    let east = ChunkPos::new(1, 0);
    let south = ChunkPos::new(0, 1);
    let center_block = BlockPos::new(1, 2, 3);
    let east_block = BlockPos::new(16, 4, 5);
    let south_block = BlockPos::new(1, 6, 16);
    let mut pending = PendingLightUpdates::default();

    pending.queue_change(south, south_block, true, None);
    pending.prepend_drained(vec![
        (
            center,
            PendingChunkLightUpdates {
                changed_positions: FxHashSet::from_iter([center_block]),
                changed_sections: FxHashMap::default(),
            },
        ),
        (
            east,
            PendingChunkLightUpdates {
                changed_positions: FxHashSet::from_iter([east_block]),
                changed_sections: FxHashMap::default(),
            },
        ),
    ]);

    let drained = pending.drain();

    assert_eq!(
        drained
            .iter()
            .map(|(chunk_pos, _)| *chunk_pos)
            .collect::<Vec<_>>(),
        vec![center, east, south]
    );
    assert!(drained[0].1.changed_positions.contains(&center_block));
    assert!(drained[1].1.changed_positions.contains(&east_block));
    assert!(drained[2].1.changed_positions.contains(&south_block));
}

#[test]
fn pending_light_updates_merge_requeued_task_with_existing_pending_task() {
    let center = ChunkPos::new(0, 0);
    let old_block = BlockPos::new(1, 2, 3);
    let new_block = BlockPos::new(4, 5, 6);
    let section_pos = SectionPos::new(0, 1, 0);
    let mut pending = PendingLightUpdates::default();

    pending.queue_change(
        center,
        new_block,
        true,
        Some(LightSectionEmptinessChange {
            section_pos,
            empty: false,
        }),
    );
    pending.prepend_drained(vec![(
        center,
        PendingChunkLightUpdates {
            changed_positions: FxHashSet::from_iter([old_block]),
            changed_sections: FxHashMap::from_iter([(section_pos, true)]),
        },
    )]);

    let drained = pending.drain();

    assert_eq!(drained.len(), 1);
    assert_eq!(drained[0].0, center);
    assert!(drained[0].1.changed_positions.contains(&old_block));
    assert!(drained[0].1.changed_positions.contains(&new_block));
    assert_eq!(
        drained[0].1.changed_sections.get(&section_pos),
        Some(&false)
    );
}

#[test]
fn pending_chunk_light_updates_sort_empty_section_changes_deterministically() {
    let mut task = PendingChunkLightUpdates::default();
    task.changed_sections.insert(SectionPos::new(0, 1, 0), true);
    task.changed_sections
        .insert(SectionPos::new(0, 3, 0), false);
    task.changed_sections
        .insert(SectionPos::new(0, 2, -1), true);
    task.changed_sections
        .insert(SectionPos::new(-1, 0, 0), false);

    let changes = task.empty_section_changes();

    assert_eq!(
        changes,
        vec![
            LightSectionEmptinessChange {
                section_pos: SectionPos::new(-1, 0, 0),
                empty: false,
            },
            LightSectionEmptinessChange {
                section_pos: SectionPos::new(0, 2, -1),
                empty: true,
            },
            LightSectionEmptinessChange {
                section_pos: SectionPos::new(0, 3, 0),
                empty: false,
            },
            LightSectionEmptinessChange {
                section_pos: SectionPos::new(0, 1, 0),
                empty: true,
            },
        ]
    );
}
