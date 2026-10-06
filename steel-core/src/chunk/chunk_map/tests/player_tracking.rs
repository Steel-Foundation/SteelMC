use super::*;
use crate::chunk::chunk_scheduler::PlayerTicketOperation;
use uuid::Uuid;

#[test]
fn players_at_same_position_share_loading_and_simulation_sources() {
    let world = fresh_test_world("shared_player_ticket_sources");
    let pos = ChunkPos::new(0, 0);
    let load_level = ChunkTicketLevel::ENTITY_TICKING_CHUNK;
    let simulation_level = ChunkTicketLevel::for_entity_ticking_radius(world.simulation_distance);
    let first_player = Uuid::from_u128(1);
    let second_player = Uuid::from_u128(2);

    world
        .chunk_map
        .scheduling
        .queue_player_ticket_operation(PlayerTicketOperation::Add {
            pos,
            player_id: first_player,
        });
    let second_addition =
        world
            .chunk_map
            .scheduling
            .queue_player_ticket_operation(PlayerTicketOperation::Add {
                pos,
                player_id: second_player,
            });
    advance_until_receipt(&world.chunk_map, second_addition);

    let holder = world
        .chunk_map
        .chunks
        .read_sync(&pos, |_, holder| Arc::clone(holder))
        .expect("the shared player source should keep its center active");
    assert_eq!(holder.load_level(), Some(load_level));
    assert_eq!(holder.simulation_level(), Some(simulation_level));

    let first_removal =
        world
            .chunk_map
            .scheduling
            .queue_player_ticket_operation(PlayerTicketOperation::Remove {
                pos,
                player_id: first_player,
            });
    advance_until_receipt(&world.chunk_map, first_removal);

    assert!(
        world
            .chunk_map
            .chunks
            .read_sync(&pos, |_, active| Arc::ptr_eq(active, &holder))
            .unwrap_or(false),
        "removing one player must not weaken the remaining source"
    );
    assert_eq!(holder.load_level(), Some(load_level));
    assert_eq!(holder.simulation_level(), Some(simulation_level));

    let second_removal =
        world
            .chunk_map
            .scheduling
            .queue_player_ticket_operation(PlayerTicketOperation::Remove {
                pos,
                player_id: second_player,
            });
    advance_until_receipt(&world.chunk_map, second_removal);

    assert!(!world.chunk_map.chunks.contains_sync(&pos));
    assert_eq!(world.chunk_map.scheduling.simulation_level(pos), None);
    stop_chunk_tasks(&world);
}

#[test]
fn player_simulation_removal_applies_at_the_next_world_tick() {
    init_vanilla_registry();
    init_behaviors();
    let world = fresh_test_world("synchronous_player_simulation_removal");
    let center = ChunkPos::new(0, 0);
    let mut holder = None;
    for z in -2..=2 {
        for x in -2..=2 {
            let inserted = insert_ready_full_chunk(&world, ChunkPos::new(x, z));
            if x == 0 && z == 0 {
                holder = Some(inserted);
            }
        }
    }
    let holder = holder.expect("the center holder should be inserted");
    let _ = world
        .chunk_map
        .acquire_chunk_request_leases(&[center], ChunkTicketLevel::FULL_CHUNK);

    let player = TestPlayerBuilder::new(Arc::clone(&world), "SimulationPlayer", 1).build();
    assert!(world.add_player(Arc::clone(&player), ResetReason::InitialJoin));
    world.tick_game(1, false);
    assert_eq!(world.chunk_map.tickable_full_chunk_positions(), [center]);

    world.remove_player_for_world_change(&player);
    world.tick_game(2, false);

    assert_eq!(world.chunk_map.tickable_full_chunk_positions(), []);
    assert_eq!(holder.simulation_level(), None);
    assert!(world.chunk_map.chunks.contains_sync(&center));
    let _ = world
        .chunk_map
        .release_chunk_request_leases(&[center], ChunkTicketLevel::FULL_CHUNK);
    stop_chunk_tasks(&world);
}

#[test]
fn light_changed_does_not_broadcast_unloading_full_chunk() {
    let chunk_map = test_chunk_map();
    let pos = ChunkPos::new(2, 3);
    let holder = unloaded_full_holder(pos);
    let _ = chunk_map
        .unloading_chunks
        .insert_sync(pos, Arc::clone(&holder));

    let chunk = holder
        .try_chunk(ChunkStatus::Full)
        .expect("test holder should contain a full chunk");
    chunk.clear_dirty();

    chunk_map.light_changed(LightLayer::Block, SectionPos::new(pos.0.x, 0, pos.0.y));

    let chunk = holder
        .try_chunk(ChunkStatus::Full)
        .expect("test holder should still contain a full chunk");
    assert!(chunk.is_dirty());

    assert!(chunk_map.chunks_to_broadcast.lock().is_empty());
    assert!(!holder.has_changes_to_broadcast());
}

#[test]
fn broadcast_changed_chunks_does_not_defer_blocks_while_light_work_is_blocked() {
    init_vanilla_registry();
    init_behaviors();
    let world = fresh_test_world("blocked_light_block_publication");
    let center = ChunkPos::new(0, 0);
    let holder = insert_ready_full_chunk(&world, center);
    for z in -LIGHT_CACHE_RADIUS..=LIGHT_CACHE_RADIUS {
        for x in -LIGHT_CACHE_RADIUS..=LIGHT_CACHE_RADIUS {
            if x != 0 || z != 0 {
                insert_ready_full_chunk(&world, ChunkPos::new(x, z));
            }
        }
    }
    let pos = BlockPos::new(1, 2, 3);
    assert!(world.set_block(
        pos,
        vanilla_blocks::STONE.default_state(),
        UpdateFlags::UPDATE_ALL,
    ));
    world.chunk_map.broadcast_changed_chunks();
    assert!(!world.chunk_map.light_update_touches_chunk(center));

    let (player, packets) = recording_player(&world);
    assert!(world.add_player(Arc::clone(&player), ResetReason::InitialJoin));
    // Keep player-ticket generation from competing with the light-work fixture.
    world.chunk_map.stop_generation_refill_loop();
    let _ = player.mark_joined_world();
    player.set_client_loaded(true);
    player.chunk_sender.lock().mark_chunk_sent_for_test(center);
    packets.lock().clear();

    let Some(reservation) = world
        .chunk_map
        .light_work_window_gate
        .try_reserve_centered(center)
    else {
        panic!("test should reserve the light work window");
    };

    assert!(world.set_block(
        pos,
        vanilla_blocks::AIR.default_state(),
        UpdateFlags::UPDATE_ALL,
    ));
    assert!(holder.has_changes_to_broadcast());
    assert!(world.chunk_map.light_update_touches_chunk(center));
    player.ack_block_changes_up_to(1);

    world.tick_game(1, true);

    assert!(world.chunk_map.chunks_to_broadcast.lock().is_empty());
    assert!(!holder.has_changes_to_broadcast());
    assert_eq!(holder.take_changed_blocks().len(), 0);
    assert!(world.chunk_map.light_update_touches_chunk(center));
    let relevant_packet_ids = packets
        .lock()
        .iter()
        .map(packet_id)
        .filter(|id| matches!(*id, C_BLOCK_UPDATE | C_BLOCK_CHANGED_ACK))
        .collect::<Vec<_>>();
    assert_eq!(relevant_packet_ids, [C_BLOCK_UPDATE, C_BLOCK_CHANGED_ACK]);

    drop(reservation);
    world.chunk_map.broadcast_changed_chunks();

    assert!(!world.chunk_map.light_update_touches_chunk(center));
    assert!(world.chunk_map.chunks_to_broadcast.lock().is_empty());
    world.remove_player_for_world_change(&player);
    stop_chunk_tasks(&world);
}

#[test]
fn frozen_tick_broadcasts_block_changes_before_acknowledging_them() {
    init_vanilla_registry();
    init_behaviors();
    let world = fresh_test_world("frozen_block_change_publication");
    let chunk_pos = ChunkPos::new(0, 0);
    let holder = insert_ready_full_chunk(&world, chunk_pos);
    let pos = BlockPos::new(1, 64, 1);
    assert!(world.set_block(
        pos,
        vanilla_blocks::STONE.default_state(),
        UpdateFlags::UPDATE_ALL,
    ));
    world.chunk_map.broadcast_changed_chunks();

    let (player, packets) = recording_player(&world);
    assert!(world.add_player(Arc::clone(&player), ResetReason::InitialJoin));
    let _ = player.mark_joined_world();
    player.set_client_loaded(true);
    player
        .chunk_sender
        .lock()
        .mark_chunk_sent_for_test(chunk_pos);
    packets.lock().clear();

    assert!(world.set_block(
        pos,
        vanilla_blocks::AIR.default_state(),
        UpdateFlags::UPDATE_ALL,
    ));
    assert!(holder.has_changes_to_broadcast());
    player.ack_block_changes_up_to(1);

    world.tick_game(1, false);

    assert!(!holder.has_changes_to_broadcast());
    let relevant_packet_ids = packets
        .lock()
        .iter()
        .map(packet_id)
        .filter(|id| matches!(*id, C_BLOCK_UPDATE | C_BLOCK_CHANGED_ACK))
        .collect::<Vec<_>>();
    assert_eq!(relevant_packet_ids, [C_BLOCK_UPDATE, C_BLOCK_CHANGED_ACK]);
    world.remove_player_for_world_change(&player);
    stop_chunk_tasks(&world);
}
