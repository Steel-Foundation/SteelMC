use super::*;
use crate::chunk::chunk_request::{ChunkRequestLease, ChunkTicketKind};
use futures::FutureExt;
use std::panic::{AssertUnwindSafe, catch_unwind};

#[test]
fn dropping_pending_radius_request_preserves_pearl_simulation() {
    let world = fresh_test_world("dropped_radius_request");
    let chunk_map = &world.chunk_map;
    let center = ChunkPos::new(0, 0);
    let neighbor = ChunkPos::new(1, 0);
    let edge = ChunkPos::new(3, 0);
    // Keep the request pending without depending on generation timing.
    chunk_map.stop_generation_refill_loop();
    let _runtime = chunk_map.chunk_runtime.enter();
    chunk_map.place_ender_pearl_ticket(center);

    let mut request = Box::pin(chunk_map.with_full_chunks_in_radius(center, 3, || {
        panic!("a pending request must not run its callback");
    }));
    assert!(request.as_mut().now_or_never().is_none());
    let edge_holder = chunk_map
        .chunks
        .read_sync(&edge, |_, holder| Arc::clone(holder))
        .expect("the radius request should load its outer edge");
    assert_eq!(edge_holder.load_level(), Some(ChunkTicketLevel::FULL_CHUNK));

    drop(request);
    chunk_map.advance_scheduling();

    // The pearl alone reaches only a generation status below Full at this edge.
    assert!(edge_holder.load_level().is_none_or(|level| !is_full(level)));
    let pearl_holder = chunk_map
        .chunks
        .read_sync(&center, |_, holder| Arc::clone(holder))
        .expect("the pearl ticket must retain its center");
    assert_eq!(
        pearl_holder.load_level(),
        Some(ChunkTicketLevel::ENTITY_TICKING_CHUNK)
    );
    assert_eq!(
        chunk_map.scheduling.simulation_level(center),
        Some(ChunkTicketLevel::ENTITY_TICKING_CHUNK)
    );
    assert_eq!(
        chunk_map.scheduling.simulation_level(neighbor),
        Some(ChunkTicketLevel::BLOCK_TICKING_CHUNK),
        "the neighboring bobber chunk must not become entity ticking"
    );
    stop_chunk_tasks(&world);
}

#[test]
fn panicking_radius_callback_releases_its_request() {
    let world = fresh_test_world("panicking_radius_request");
    let chunk_map = &world.chunk_map;
    let center = ChunkPos::new(0, 0);
    chunk_map.stop_generation_refill_loop();
    let holder = insert_ready_full_chunk(&world, center);

    let result = catch_unwind(AssertUnwindSafe(|| {
        chunk_map.chunk_runtime.block_on(
            chunk_map.with_full_chunks_in_radius(center, 0, || panic!("callback failed")),
        );
    }));
    assert!(result.is_err());
    let _runtime = chunk_map.chunk_runtime.enter();
    chunk_map.advance_scheduling();
    assert_eq!(holder.load_level(), None);
    stop_chunk_tasks(&world);
}

#[test]
fn cancelling_handle_releases_once_and_preserves_another_lease() {
    let world = fresh_test_world("cancelled_shared_request");
    let chunk_map = &world.chunk_map;
    let center = ChunkPos::new(0, 0);
    chunk_map.stop_generation_refill_loop();
    let _runtime = chunk_map.chunk_runtime.enter();
    let lease = ChunkRequestLease::new(
        Arc::clone(chunk_map),
        Box::new([center]),
        ChunkTicketLevel::FULL_CHUNK,
    );
    let mut request = chunk_map.request_chunk(center, ChunkStatus::Full, ChunkTicketKind::Command);
    chunk_map.advance_scheduling();
    let holder = chunk_map
        .chunks
        .read_sync(&center, |_, holder| Arc::clone(holder))
        .expect("the requests should load their center");

    request.cancel();
    request.cancel();
    drop(request);
    chunk_map.advance_scheduling();
    assert_eq!(holder.load_level(), Some(ChunkTicketLevel::FULL_CHUNK));

    drop(lease);
    chunk_map.advance_scheduling();
    assert_eq!(holder.load_level(), None);
    stop_chunk_tasks(&world);
}
