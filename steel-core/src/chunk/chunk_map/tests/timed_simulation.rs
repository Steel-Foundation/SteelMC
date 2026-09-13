use super::*;
use std::time::Duration;
use tokio::{task::yield_now, time::timeout};

#[test]
fn timed_simulation_expiration_follows_its_final_scheduled_tick() {
    init_vanilla_registry();
    init_behaviors();
    let world = fresh_test_world("timed_simulation_expiration");
    let chunk_pos = ChunkPos::new(0, 0);
    let block_pos = BlockPos::new(1, 64, 1);
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

    let load_receipt = world
        .chunk_map
        .acquire_chunk_request_leases(&[chunk_pos], ChunkTicketLevel::FULL_CHUNK)
        .expect("non-empty lease acquisition should produce a receipt");
    world.chunk_map.place_ender_pearl_ticket(chunk_pos);
    advance_until_receipt(&world.chunk_map, load_receipt);

    world.chunk_map.chunk_runtime.block_on(async {
        timeout(Duration::from_secs(5), async {
            while !holder.is_ready_for_saving() {
                yield_now().await;
            }
        })
        .await
        .expect("generation should release its save dependencies");
    });

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
    world.chunk_map.advance_scheduling();
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
    assert_eq!(holder.load_level(), Some(ChunkTicketLevel::FULL_CHUNK));
    assert!(
        world
            .chunk_map
            .chunks
            .read_sync(&chunk_pos, |_, active| Arc::ptr_eq(active, &holder))
            .unwrap_or(false),
        "the overlapping load-only ticket should keep the same holder active"
    );
    let _ = world
        .chunk_map
        .release_chunk_request_leases(&[chunk_pos], ChunkTicketLevel::FULL_CHUNK);
    stop_chunk_tasks(&world);
}
