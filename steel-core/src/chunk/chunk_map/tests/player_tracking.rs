use super::*;

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
}
