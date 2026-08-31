use super::*;

#[test]
fn timed_simulation_expiration_follows_its_final_scheduled_tick() {
    init_vanilla_registry();
    init_behaviors();
    let world = fresh_test_world("timed_simulation_expiration");
    let chunk_pos = ChunkPos::new(0, 0);
    let block_pos = BlockPos::new(1, 64, 1);
    let holder = insert_ready_full_chunk(&world, chunk_pos);

    let load_receipt = world
        .chunk_map
        .add_chunk_ticket(chunk_pos, ChunkTicket::full_chunks(0));
    world.chunk_map.place_ender_pearl_ticket(chunk_pos);
    world.chunk_map.flush_simulation_updates();

    assert_eq!(
        holder.simulation_level(),
        Some(ChunkTicketLevel::ENTITY_TICKING_CHUNK)
    );
    assert!(
        world
            .chunk_map
            .is_block_ticking_full_chunk_simulated(chunk_pos)
    );

    for _ in 0..ENDER_PEARL_TICKET_TIMEOUT {
        world.chunk_map.tick_timed_tickets();
    }
    world.chunk_map.flush_simulation_updates();
    assert_eq!(
        holder.simulation_level(),
        Some(ChunkTicketLevel::ENTITY_TICKING_CHUNK),
        "the timed ticket should remain active through its final countdown tick"
    );

    world.schedule_block_tick(block_pos, &vanilla_blocks::STONE, 0, TickPriority::Normal);
    assert!(world.has_scheduled_block_tick(block_pos, &vanilla_blocks::STONE));

    world.tick_game(1, false);
    assert_eq!(
        holder.simulation_level(),
        Some(ChunkTicketLevel::ENTITY_TICKING_CHUNK),
        "frozen ticks must not age timed tickets"
    );
    assert!(world.has_scheduled_block_tick(block_pos, &vanilla_blocks::STONE));

    world.tick_game(2, true);

    assert_eq!(holder.simulation_level(), None);
    assert!(
        !world.has_scheduled_block_tick(block_pos, &vanilla_blocks::STONE),
        "the chunk must execute its final eligible scheduled tick before expiration"
    );
    assert!(
        !world
            .chunk_map
            .is_block_ticking_full_chunk_simulated(chunk_pos),
        "later chunk gameplay must observe the expired simulation ticket"
    );
    assert!(
        !world
            .chunk_map
            .tickable_full_chunk_positions()
            .contains(&chunk_pos),
        "the published ticking snapshot must exclude the expired chunk"
    );

    advance_until_receipt(&world.chunk_map, load_receipt);

    assert_eq!(holder.load_level(), Some(ChunkTicketLevel::FULL_CHUNK));
    assert!(
        world
            .chunk_map
            .chunks
            .read_sync(&chunk_pos, |_, active| Arc::ptr_eq(active, &holder))
            .unwrap_or(false),
        "the overlapping load-only ticket should keep the same holder active"
    );
    stop_chunk_tasks(&world);
}
