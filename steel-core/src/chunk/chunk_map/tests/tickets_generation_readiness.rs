use super::*;

#[test]
fn ticket_changes_move_the_same_holder_only_at_boundary_commit() {
    let world = fresh_test_world("chunk_removal_boundary");
    let pos = ChunkPos::new(9, -11);
    let ticket = ChunkTicket::loading(ChunkTicketLevel::MAX);
    let addition_revision = world.chunk_map.add_chunk_ticket(pos, ticket);
    advance_until_revision(&world.chunk_map, addition_revision);
    let holder = world
        .chunk_map
        .chunks
        .read_sync(&pos, |_, holder| Arc::clone(holder))
        .expect("committed ticket should create an active holder");

    let removal_revision = world.chunk_map.remove_chunk_ticket(pos, ticket);

    assert!(world.chunk_map.chunks.contains_sync(&pos));
    assert!(!world.chunk_map.unloading_chunks.contains_sync(&pos));

    advance_until_revision(&world.chunk_map, removal_revision);

    assert!(!world.chunk_map.chunks.contains_sync(&pos));
    assert!(
        world
            .chunk_map
            .unloading_chunks
            .read_sync(&pos, |_, unloading| Arc::ptr_eq(unloading, &holder))
            .unwrap_or(false)
    );

    let revival_revision = world.chunk_map.add_chunk_ticket(pos, ticket);
    assert!(!world.chunk_map.chunks.contains_sync(&pos));
    assert!(world.chunk_map.unloading_chunks.contains_sync(&pos));

    advance_until_revision(&world.chunk_map, revival_revision);

    assert!(
        world
            .chunk_map
            .chunks
            .read_sync(&pos, |_, active| Arc::ptr_eq(active, &holder))
            .unwrap_or(false)
    );
    assert!(!world.chunk_map.unloading_chunks.contains_sync(&pos));

    world.chunk_map.remove_chunk_ticket(pos, ticket);
}

#[test]
fn staged_revival_keeps_map_only_unloading_holder_until_commit() {
    let world = fresh_test_world("staged_chunk_revival");
    let pos = ChunkPos::new(-4, 7);
    let level = ChunkTicketLevel::MAX;
    let ticket = ChunkTicket::loading(level);
    let holder = world
        .chunk_map
        .update_chunk_level(pos, Some(level), None)
        .expect("loaded level should create a holder");

    world.chunk_map.update_chunk_level(pos, None, None);
    let weak_holder = Arc::downgrade(&holder);
    drop(holder);

    assert_eq!(
        world
            .chunk_map
            .unloading_chunks
            .read_sync(&pos, |_, unloading| Arc::strong_count(unloading)),
        Some(1),
        "the unloading map should own the holder's only strong reference"
    );

    world.chunk_map.add_chunk_ticket(pos, ticket);
    let epoch = world.chunk_map.prepare_scheduling_epoch(
        ChunkTicketManager::new(),
        ChunkTicketRevision::default(),
        Vec::new(),
    );

    assert!(
        weak_holder.upgrade().is_some(),
        "a staged revival must reserve the unloading holder until commit"
    );
    assert!(world.chunk_map.unloading_chunks.contains_sync(&pos));

    let change = epoch
        .changes
        .into_iter()
        .find(|change| change.pos == pos)
        .expect("ticket propagation should stage the holder revival");
    let active = world
        .chunk_map
        .update_chunk_level(change.pos, change.new_level, change.new_simulation_level)
        .expect("revival commit should reactivate the holder");
    let original = weak_holder
        .upgrade()
        .expect("revival commit should preserve the original holder");

    assert!(Arc::ptr_eq(&active, &original));
    assert!(!world.chunk_map.unloading_chunks.contains_sync(&pos));
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
fn cached_holder_rechecks_publication_and_generation_permission() {
    init_test_registry();
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
    init_test_registry();
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
        .prepare_ticking_readiness_demotions(&[LevelChange {
            pos: ChunkPos::new(-2, -2),
            new_level: None,
            new_simulation_level: None,
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
        .prepare_ticking_readiness_demotions(&[LevelChange {
            pos: ChunkPos::new(-1, -1),
            new_level: None,
            new_simulation_level: None,
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
    init_test_registry();
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
fn ticking_snapshot_preserves_scc_order_and_distinct_readiness_gates() {
    init_test_registry();
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
    let snapshot = world.chunk_map.ticking_chunks.load();
    let mut scc_order = Vec::new();
    world.chunk_map.chunks.iter_sync(|pos, _| {
        scc_order.push(*pos);
        true
    });
    assert_eq!(
        snapshot
            .block
            .iter()
            .map(|chunk| chunk.pos)
            .collect::<Vec<_>>(),
        scc_order
    );

    let random_positions = snapshot
        .random_chunk_indices
        .iter()
        .map(|&index| snapshot.block[index].pos)
        .collect::<FxHashSet<_>>();
    assert_eq!(
        random_positions,
        FxHashSet::from_iter([random_pos, entity_pos])
    );
    let entity_positions = snapshot
        .entity_indices
        .iter()
        .map(|&index| snapshot.block[index].pos)
        .collect::<Vec<_>>();
    assert_eq!(entity_positions, [entity_pos]);
}

#[test]
fn simulation_changes_rebuild_only_eligible_snapshot_membership() {
    init_test_registry();
    init_behaviors();
    let world = fresh_test_world("simulation_snapshot_membership");
    let pos = ChunkPos::new(0, 0);
    let holder = insert_ready_full_chunk(&world, pos);

    let entity_ticking = LevelChange {
        pos,
        new_level: Some(ChunkTicketLevel::BLOCK_TICKING_CHUNK),
        new_simulation_level: Some(ChunkTicketLevel::ENTITY_TICKING_CHUNK),
    };
    assert!(
        world
            .chunk_map
            .simulation_changes_ticking_snapshot(&[entity_ticking]),
        "entering the random-tick set must republish the snapshot"
    );

    let unchanged = LevelChange {
        new_simulation_level: Some(ChunkTicketLevel::BLOCK_TICKING_CHUNK),
        ..entity_ticking
    };
    assert!(
        !world
            .chunk_map
            .simulation_changes_ticking_snapshot(&[unchanged]),
        "an unchanged simulation class must retain the snapshot"
    );

    holder.transition_ticking_readiness(TickingReadiness::Unready);
    assert!(
        !world
            .chunk_map
            .simulation_changes_ticking_snapshot(&[entity_ticking]),
        "simulation changes cannot add an unready holder to the snapshot"
    );
}

#[test]
fn full_load_activation_uses_packed_chunk_position_order() {
    init_test_registry();
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
