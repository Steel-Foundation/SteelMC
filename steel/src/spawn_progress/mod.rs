//! Spawn chunk generation with optional terminal progress display.
//!
//! During server startup, generates chunks around the spawn position until
//! the 7×7 Full area is complete. When the `spawn_chunk_display` feature is
//! enabled, a colored ANSI grid shows real-time progress including the
//! surrounding dependency rings.

use std::sync::Arc;
use std::time::{Duration, Instant};

use steel_core::chunk::chunk_pyramid::GENERATION_PYRAMID;
use tokio::time::sleep;

use steel_core::chunk::chunk_access::ChunkStatus;
use steel_core::chunk::chunk_ticket_manager::MAX_VIEW_DISTANCE;
use steel_core::server::Server;
use steel_core::world::World;
use steel_utils::{ChunkPos, SectionPos};

#[cfg(feature = "slow_chunk_gen")]
use std::sync::atomic::Ordering;
#[cfg(feature = "slow_chunk_gen")]
use steel_core::chunk::chunk_holder::SLOW_CHUNK_GEN;

use crate::logger::CommandLogger;

/// Vanilla spawn chunk radius — chunks within this radius reach Full status.
const SPAWN_RADIUS: i32 = 3;

/// Dependency margin: extra rings required for Full chunk generation.
const DEPENDENCY_MARGIN: i32 = GENERATION_PYRAMID
    .get_step_to(ChunkStatus::Full)
    .accumulated_dependencies
    .get_radius_of(ChunkStatus::Empty) as i32;

/// Display radius: Full radius + dependency margin.
pub const DISPLAY_RADIUS: i32 = SPAWN_RADIUS + DEPENDENCY_MARGIN;

/// Display diameter covering Full area + dependencies.
#[cfg(feature = "spawn_chunk_display")]
pub const DISPLAY_DIAMETER: usize = (DISPLAY_RADIUS * 2 + 1) as usize;

/// Number of chunks that must reach Full status (7×7).
const TOTAL_SPAWN_CHUNKS: usize = ((SPAWN_RADIUS * 2 + 1) * (SPAWN_RADIUS * 2 + 1)) as usize;

/// Generates spawn chunks, optionally displaying progress in the terminal.
///
/// Adds a ticket at the world spawn position so that a 7×7 area of chunks
/// reaches `Full` status. The generation system is pumped in a loop until
/// completion. With the `spawn_chunk_display` feature, progress is shown as
/// a colored terminal grid that includes the surrounding dependency chunks.
pub async fn generate_spawn_chunks(
    server: &Arc<Server>,
    #[allow(unused)] logger: &Arc<CommandLogger>,
) {
    let world = &server.worlds[0];

    let spawn_pos = world.level_data.read().data().spawn_pos();
    let center_chunk = ChunkPos::new(
        SectionPos::block_to_section_coord(spawn_pos.0.x),
        SectionPos::block_to_section_coord(spawn_pos.0.z),
    );

    log::info!(
        "Preparing spawn area: {TOTAL_SPAWN_CHUNKS} chunks around chunk ({}, {})",
        center_chunk.0.x,
        center_chunk.0.y,
    );

    // Add a ticket at the center chunk. Ticket level MAX_VIEW_DISTANCE - SPAWN_RADIUS
    // ensures that chunks within radius SPAWN_RADIUS reach Full status:
    //   center: level 29, is_full(29) = true
    //   distance 3: level 32, is_full(32) = true (32 <= MAX_VIEW_DISTANCE)
    //   distance 4: level 33, is_full(33) = false
    let ticket_level = MAX_VIEW_DISTANCE - SPAWN_RADIUS as u8;
    {
        let mut tickets = world.chunk_map.chunk_tickets.lock();
        tickets.add_ticket(center_chunk, ticket_level);
    }

    #[cfg(feature = "slow_chunk_gen")]
    SLOW_CHUNK_GEN.store(true, Ordering::Relaxed);

    #[cfg(feature = "spawn_chunk_display")]
    let elapsed = generate_with_display(world, center_chunk, logger).await;

    #[cfg(not(feature = "spawn_chunk_display"))]
    let elapsed = {
        let start = Instant::now();
        generate_without_display(world, center_chunk).await;
        start.elapsed()
    };

    #[cfg(feature = "slow_chunk_gen")]
    SLOW_CHUNK_GEN.store(false, Ordering::Relaxed);

    // Remove the ticket now that generation is complete (spawn chunks no longer stay loaded)
    {
        let mut tickets = world.chunk_map.chunk_tickets.lock();
        tickets.remove_ticket(center_chunk, ticket_level);
    }

    log::info!(
        "Spawn area prepared: {TOTAL_SPAWN_CHUNKS} chunks in {:.2}s",
        elapsed.as_secs_f64(),
    );
}

/// Returns the elapsed generation time (excluding the final display delay).
#[cfg(feature = "spawn_chunk_display")]
async fn generate_with_display(
    world: &World,
    center_chunk: ChunkPos,
    logger: &Arc<CommandLogger>,
) -> Duration {
    use crate::spawn_progress::{DISPLAY_DIAMETER, DISPLAY_RADIUS};

    let _ = logger.activate_spawn_display().await;
    let start = Instant::now();
    let mut tick_count: u64 = 1;
    let mut grid = [[None; DISPLAY_DIAMETER]; DISPLAY_DIAMETER];
    let mut last_render = Instant::now();

    loop {
        world.chunk_map.tick_b(world, tick_count, 0, false);

        let mut completed = 0;
        let mut pending_dependencies = false;

        for dz in -DISPLAY_RADIUS..=DISPLAY_RADIUS {
            for dx in -DISPLAY_RADIUS..=DISPLAY_RADIUS {
                let pos = ChunkPos::new(center_chunk.0.x + dx, center_chunk.0.y + dz);
                let status = world
                    .chunk_map
                    .chunks
                    .read_sync(&pos, |_, holder| holder.persisted_status())
                    .flatten();

                let gx = (dx + DISPLAY_RADIUS) as usize;
                let gz = (dz + DISPLAY_RADIUS) as usize;
                grid[gz][gx] = status;

                let in_spawn_area = dx.abs() <= SPAWN_RADIUS && dz.abs() <= SPAWN_RADIUS;
                if in_spawn_area && status == Some(ChunkStatus::Full) {
                    completed += 1;
                } else if !in_spawn_area && status.is_none() {
                    pending_dependencies = true;
                }
            }
        }

        // Always update grid state; throttle rendering to ~10fps
        let should_render = last_render.elapsed() >= Duration::from_millis(100);
        let _ = logger.update_spawn_grid(&grid, should_render).await;
        if should_render {
            last_render = Instant::now();
        }

        if completed == TOTAL_SPAWN_CHUNKS && !pending_dependencies {
            break;
        }

        sleep(Duration::from_millis(10)).await;
        tick_count += 1;
    }

    let elapsed = start.elapsed();

    // Render final state
    let _ = logger.update_spawn_grid(&grid, true).await;
    // Show completed grid briefly before clearing
    #[cfg(feature = "slow_chunk_gen")]
    sleep(Duration::from_secs(1)).await;
    logger.deactivate_spawn_display().await;

    elapsed
}

/// Counts how many chunks in the spawn area have reached Full status.
#[cfg(not(feature = "spawn_chunk_display"))]
fn count_full_spawn_chunks(world: &World, center_chunk: ChunkPos) -> usize {
    let mut completed = 0;
    for dz in -SPAWN_RADIUS..=SPAWN_RADIUS {
        for dx in -SPAWN_RADIUS..=SPAWN_RADIUS {
            let pos = ChunkPos::new(center_chunk.0.x + dx, center_chunk.0.y + dz);
            let status = world
                .chunk_map
                .chunks
                .read_sync(&pos, |_, holder| holder.persisted_status())
                .flatten();
            if status == Some(ChunkStatus::Full) {
                completed += 1;
            }
        }
    }
    completed
}

#[cfg(not(feature = "spawn_chunk_display"))]
async fn generate_without_display(world: &World, center_chunk: ChunkPos) {
    let mut tick_count: u64 = 1;

    loop {
        world.chunk_map.tick_b(world, tick_count, 0, false);

        if count_full_spawn_chunks(world, center_chunk) == TOTAL_SPAWN_CHUNKS {
            break;
        }

        sleep(Duration::from_millis(10)).await;
        tick_count += 1;
    }
}
