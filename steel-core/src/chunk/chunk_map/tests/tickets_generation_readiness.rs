use super::*;
use crate::chunk::chunk_pyramid::GENERATION_PYRAMID;

#[test]
fn ticket_changes_move_the_same_holder_only_at_boundary_commit() {
    let world = fresh_test_world("chunk_removal_boundary");
    let pos = ChunkPos::new(9, -11);
    let ticket_level = ChunkTicketLevel::MAX;
    let addition_receipt = world
        .chunk_map
        .acquire_chunk_request_leases(&[pos], ticket_level)
        .expect("one request lease should produce a receipt");
    advance_until_receipt(&world.chunk_map, addition_receipt);
    let holder = world
        .chunk_map
        .chunks
        .read_sync(&pos, |_, holder| Arc::clone(holder))
        .expect("committed ticket should create an active holder");

    let removal_receipt = world
        .chunk_map
        .release_chunk_request_leases(&[pos], ticket_level)
        .expect("one request lease release should produce a receipt");

    assert!(world.chunk_map.chunks.contains_sync(&pos));
    assert!(!world.chunk_map.unloading_chunks.contains_sync(&pos));

    advance_until_receipt(&world.chunk_map, removal_receipt);

    assert!(!world.chunk_map.chunks.contains_sync(&pos));
    assert!(
        world
            .chunk_map
            .unloading_chunks
            .read_sync(&pos, |_, unloading| Arc::ptr_eq(unloading, &holder))
            .unwrap_or(false)
    );

    let revival_receipt = world
        .chunk_map
        .acquire_chunk_request_leases(&[pos], ticket_level)
        .expect("one request lease should produce a receipt");
    assert!(!world.chunk_map.chunks.contains_sync(&pos));
    assert!(world.chunk_map.unloading_chunks.contains_sync(&pos));

    advance_until_receipt(&world.chunk_map, revival_receipt);

    assert!(
        world
            .chunk_map
            .chunks
            .read_sync(&pos, |_, active| Arc::ptr_eq(active, &holder))
            .unwrap_or(false)
    );
    assert!(!world.chunk_map.unloading_chunks.contains_sync(&pos));

    let _ = world
        .chunk_map
        .release_chunk_request_leases(&[pos], ticket_level);
    stop_chunk_tasks(&world);
}

#[test]
fn generation_priority_prefers_simulation_tickets() {
    let normal_strong =
        GenerationTaskPriority::for_levels(Some(ChunkTicketLevel::for_full_chunk_radius(8)), None);
    let simulated_weak = GenerationTaskPriority::for_levels(
        Some(ChunkTicketLevel::for_full_chunk_radius(1)),
        Some(ChunkTicketLevel::for_full_chunk_radius(1)),
    );

    assert!(simulated_weak < normal_strong);
}

#[test]
fn generation_priority_orders_simulation_by_simulation_level() {
    let weaker_simulation = GenerationTaskPriority::for_levels(
        Some(ChunkTicketLevel::for_full_chunk_radius(8)),
        Some(ChunkTicketLevel::for_full_chunk_radius(1)),
    );
    let stronger_simulation = GenerationTaskPriority::for_levels(
        Some(ChunkTicketLevel::for_full_chunk_radius(1)),
        Some(ChunkTicketLevel::for_full_chunk_radius(4)),
    );

    assert!(stronger_simulation < weaker_simulation);
}

#[test]
fn generation_priority_orders_normal_by_load_level() {
    let weaker_load =
        GenerationTaskPriority::for_levels(Some(ChunkTicketLevel::for_full_chunk_radius(1)), None);
    let stronger_load =
        GenerationTaskPriority::for_levels(Some(ChunkTicketLevel::for_full_chunk_radius(4)), None);

    assert!(stronger_load < weaker_load);
}

#[test]
fn cancelled_generation_task_keeps_cached_holders_pinned_for_in_flight_steps() {
    init_vanilla_registry();
    let world = fresh_test_world("pending_generation_save_dependency");
    world.chunk_map.stop_generation_refill_loop();

    let center_pos = ChunkPos::new(0, 0);
    let target_status = ChunkStatus::Biomes;
    let cache_radius = GENERATION_PYRAMID
        .get_step_to(target_status)
        .accumulated_dependencies
        .get_radius_of(ChunkStatus::Empty) as i32;
    let timed_ticket_pos = ChunkPos::new(cache_radius, 0);
    let mut cached_holders = Vec::new();

    for z in -cache_radius..=cache_radius {
        for x in -cache_radius..=cache_radius {
            let pos = ChunkPos::new(x, z);
            let holder = Arc::new(ChunkHolder::new(
                pos,
                ChunkTicketLevel::STRONGEST,
                None,
                world.chunk_map.world_gen_context.min_y(),
                world.chunk_map.world_gen_context.height(),
            ));
            let _ = world.chunk_map.chunks.insert_sync(pos, Arc::clone(&holder));
            cached_holders.push(holder);
        }
    }

    let center = world
        .chunk_map
        .chunks
        .read_sync(&center_pos, |_, holder| Arc::clone(holder))
        .expect("the center holder should be cached");
    assert!(center.schedule_chunk_generation_task_b(target_status, &world.chunk_map));
    let in_flight_cache = {
        let pending = world.chunk_map.pending_generation_tasks.lock();
        assert_eq!(pending.len(), 1);
        Arc::clone(&pending[0].cache)
    };
    assert!(
        cached_holders
            .iter()
            .all(|holder| !holder.is_ready_for_saving())
    );

    world.chunk_map.place_ender_pearl_ticket(timed_ticket_pos);
    assert_eq!(
        world.chunk_map.scheduling.timed_ticket_expirations().len(),
        1
    );
    assert!(
        world
            .chunk_map
            .eligible_timed_ticket_expirations()
            .is_empty(),
        "a holder cached by a pending generation task must not age timed tickets"
    );

    world.chunk_map.stop_generation_refill_loop();
    assert!(world.chunk_map.pending_generation_tasks.lock().is_empty());
    assert!(
        cached_holders
            .iter()
            .all(|holder| !holder.is_ready_for_saving())
    );
    assert!(
        world
            .chunk_map
            .eligible_timed_ticket_expirations()
            .is_empty(),
        "cancellation must not release holders still used by a spawned generation step"
    );

    drop(in_flight_cache);
    assert!(
        cached_holders
            .iter()
            .all(|holder| holder.is_ready_for_saving())
    );
    assert_eq!(
        world.chunk_map.eligible_timed_ticket_expirations().len(),
        1,
        "dropping the cancelled task must release every cached save dependency"
    );

    stop_chunk_tasks(&world);
}

#[test]
fn cached_holder_rechecks_publication_and_generation_permission() {
    init_vanilla_registry();
    let world = fresh_test_world("cached_holder_status_recheck");
    let pos = ChunkPos::new(4, -3);
    let load_level = ChunkTicketLevel::FULL_CHUNK;
    let min_y = world.chunk_map.world_gen_context.min_y();
    let height = world.chunk_map.world_gen_context.height();
    let holder = Arc::new(ChunkHolder::new_with_full_publications(
        pos,
        load_level,
        None,
        min_y,
        height,
        Arc::downgrade(&world.chunk_map.full_publications),
    ));
    let _ = world.chunk_map.chunks.insert_sync(pos, Arc::clone(&holder));
    let scope = GameplayChunkLookupCacheScope::enter(&world.chunk_map);

    assert!(
        world
            .chunk_map
            .with_chunk_at_status(pos, ChunkStatus::Empty, |_| ())
            .is_none(),
        "an unpublished status must remain unavailable after the holder is cached"
    );

    let sections = (0..height / 16)
        .map(|_| ChunkSection::new_empty())
        .collect::<Vec<_>>()
        .into_boxed_slice();
    holder.insert_chunk(
        Chunk::new(
            Sections::from_owned(sections),
            pos,
            min_y,
            height,
            Arc::downgrade(&world),
        ),
        ChunkStatus::Empty,
    );
    assert!(
        world
            .chunk_map
            .with_chunk_at_status(pos, ChunkStatus::Empty, |_| ())
            .is_some(),
        "publication must become visible through a cached holder"
    );

    holder.update_highest_allowed_status(None);
    assert!(
        world
            .chunk_map
            .with_chunk_at_status(pos, ChunkStatus::Empty, |_| ())
            .is_none(),
        "a cached holder must still honor generation permission revocation"
    );

    holder.update_highest_allowed_status(Some(load_level));
    assert_eq!(
        world
            .chunk_map
            .with_chunk_at_status(pos, ChunkStatus::Empty, |_| {
                world
                    .chunk_map
                    .with_chunk_at_status(pos, ChunkStatus::Empty, |_| ())
                    .is_some()
            }),
        Some(true),
        "callbacks must run after releasing the cache's RefCell borrow"
    );

    let stats = scope.finish();
    assert_eq!(stats.scc_lookups, 1);
    assert_eq!(stats.holder_hits, 4);
}

#[test]
#[expect(
    clippy::too_many_lines,
    reason = "one lifecycle test documents both readiness radii and their transitions"
)]
fn full_publications_drive_block_and_entity_readiness_incrementally() {
    init_vanilla_registry();
    init_behaviors();
    let world = fresh_test_world("full_chunk_readiness_lifecycle");
    let center_pos = ChunkPos::new(0, 0);
    let marked_pos = BlockPos::new(
        center_pos.0.x * 16,
        world.chunk_map.world_gen_context.min_y(),
        center_pos.0.y * 16,
    );
    let packed = Chunk::pack_postprocessing_offset(marked_pos);
    let mut center = None;

    for z in -1..=1 {
        for x in -1..=1 {
            let pos = ChunkPos::new(x, z);
            let load_level = if pos == center_pos {
                ChunkTicketLevel::ENTITY_TICKING_CHUNK
            } else {
                ChunkTicketLevel::FULL_CHUNK
            };
            let postprocessing = if pos == center_pos {
                vec![vec![packed]]
            } else {
                Vec::new()
            };
            let holder = insert_active_full_holder(&world, pos, load_level, postprocessing);
            if pos == center_pos {
                center = Some(holder);
            }
        }
    }

    let readiness_result = world
        .chunk_map
        .reconcile_ticking_readiness_measured(&[])
        .expect("a unique 3x3 Full square should reconcile");
    assert_eq!(readiness_result.post_process_chunk_count, 1);
    assert_eq!(readiness_result.post_process_position_count, 1);
    let center = center.expect("the center holder should be inserted");
    assert_eq!(
        center.ticking_readiness_snapshot().readiness(),
        TickingReadiness::BlockTicking
    );
    assert!(
        !center.is_ready_for_saving(),
        "the pending entity transition should remain a save dependency"
    );
    assert_postprocessing_drained(&center);
    center.set_simulation_level(None);
    assert!(
        world
            .chunk_map
            .is_block_ticking_full_chunk_loaded(center_pos),
        "client publication follows load readiness, not simulation distance"
    );

    for z in -2_i32..=2 {
        for x in -2_i32..=2 {
            if x.abs() <= 1 && z.abs() <= 1 {
                continue;
            }
            insert_active_full_holder(
                &world,
                ChunkPos::new(x, z),
                ChunkTicketLevel::FULL_CHUNK,
                Vec::new(),
            );
        }
    }

    world
        .chunk_map
        .reconcile_ticking_readiness(&[])
        .expect("a unique 5x5 Full square should reconcile");
    assert_eq!(
        center.ticking_readiness_snapshot().readiness(),
        TickingReadiness::EntityTicking
    );
    assert!(center.is_ready_for_saving());
    assert!(
        !world
            .chunk_map
            .tickable_full_chunk_positions()
            .contains(&center_pos),
        "entity simulation remains separately gated"
    );

    world
        .chunk_map
        .prepare_ticking_readiness_demotions(&[LoadLevelChange {
            pos: ChunkPos::new(-2, -2),
            new_level: None,
        }])
        .expect("removing an indexed outer contributor should reconcile");
    assert_eq!(
        center.ticking_readiness_snapshot().readiness(),
        TickingReadiness::BlockTicking,
        "r2 must be revoked before the contributor's lifecycle mutation"
    );
    assert!(!center.is_ready_for_saving());

    world
        .chunk_map
        .prepare_ticking_readiness_demotions(&[LoadLevelChange {
            pos: ChunkPos::new(-1, -1),
            new_level: None,
        }])
        .expect("removing an indexed inner contributor should reconcile");
    assert_eq!(
        center.ticking_readiness_snapshot().readiness(),
        TickingReadiness::Unready,
        "r1 must be revoked before the contributor's lifecycle mutation"
    );
}

#[test]
fn first_block_readiness_anchors_pending_ticks_once() {
    init_vanilla_registry();
    init_behaviors();
    let world = fresh_test_world("pending_tick_readiness_anchor");
    world.level_data.write().set_game_time(100);
    let center_pos = ChunkPos::new(0, 0);
    let tick_pos = BlockPos::new(1, 64, 1);
    let mut center = None;

    for z in -1..=1 {
        for x in -1..=1 {
            let pos = ChunkPos::new(x, z);
            let load_level = if pos == center_pos {
                ChunkTicketLevel::ENTITY_TICKING_CHUNK
            } else {
                ChunkTicketLevel::FULL_CHUNK
            };
            let block_ticks = if pos == center_pos {
                BlockTickList::from_saved_ticks(vec![SavedTick {
                    tick_type: &vanilla_blocks::STONE,
                    pos: tick_pos,
                    delay: 5,
                    priority: TickPriority::Normal,
                }])
            } else {
                BlockTickList::new()
            };
            let holder = insert_active_full_holder_with_ticks(
                &world,
                pos,
                load_level,
                Vec::new(),
                block_ticks,
            );
            if pos == center_pos {
                center = Some(holder);
            }
        }
    }

    world
        .chunk_map
        .reconcile_ticking_readiness(&[])
        .expect("a unique 3x3 Full square should reconcile");
    let center = center.expect("the center holder should be inserted");
    assert_eq!(
        center.ticking_readiness_snapshot().readiness(),
        TickingReadiness::BlockTicking
    );
    let full = center
        .try_full_chunk()
        .expect("the center should remain Full");
    assert_eq!(full.scheduled_tick_snapshot().block[0].delay, 5);

    world.level_data.write().set_game_time(200);
    world
        .unpack_scheduled_ticks(center_pos)
        .expect("repeated readiness unpack should remain valid");
    assert_eq!(full.scheduled_tick_snapshot().block[0].delay, -95);
}

#[test]
fn entity_tickability_requires_simulation_and_entity_readiness() {
    init_vanilla_registry();
    init_behaviors();
    let world = fresh_test_world("ticking_chunk_snapshot");
    let block_only_pos = ChunkPos::new(0, 0);
    let random_pos = ChunkPos::new(1, 0);
    let entity_pos = ChunkPos::new(2, 0);

    insert_ready_full_chunk(&world, block_only_pos);
    let random = insert_ready_full_chunk(&world, random_pos);
    random.set_simulation_level(Some(ChunkTicketLevel::ENTITY_TICKING_CHUNK));
    let entity = insert_ready_full_chunk(&world, entity_pos);
    entity.set_simulation_level(Some(ChunkTicketLevel::ENTITY_TICKING_CHUNK));
    entity.transition_ticking_readiness(TickingReadiness::EntityTicking);

    world.chunk_map.rebuild_ticking_chunk_snapshot();
    assert!(
        world
            .chunk_map
            .is_block_ticking_full_chunk_simulated(block_only_pos)
    );
    assert!(
        world
            .chunk_map
            .is_block_ticking_full_chunk_simulated(random_pos)
    );
    assert_eq!(
        world.chunk_map.tickable_full_chunk_positions(),
        [entity_pos]
    );
}

#[test]
fn full_load_activation_uses_packed_chunk_position_order() {
    init_vanilla_registry();
    init_behaviors();
    let world = fresh_test_world("packed_full_activation_order");
    let first_chunk = ChunkPos::new(0, 0);
    let second_chunk = ChunkPos::new(1, 0);
    let first_sign = BlockPos::new(1, 64, 1);
    let second_sign = BlockPos::new(17, 64, 1);

    let second = insert_active_full_holder(
        &world,
        second_chunk,
        ChunkTicketLevel::FULL_CHUNK,
        Vec::new(),
    );
    let Some(second) = second.try_full_chunk() else {
        panic!("inserted second chunk should remain Full");
    };
    add_test_sign(second, second_sign);

    let first = insert_active_full_holder(
        &world,
        first_chunk,
        ChunkTicketLevel::FULL_CHUNK,
        Vec::new(),
    );
    let Some(first) = first.try_full_chunk() else {
        panic!("inserted first chunk should remain Full");
    };
    add_test_sign(first, first_sign);

    world
        .chunk_map
        .reconcile_ticking_readiness(&[])
        .expect("the Full publications should reconcile");

    assert_eq!(
        world.block_entity_tickers().active_positions(),
        [first_sign, second_sign]
    );
}
