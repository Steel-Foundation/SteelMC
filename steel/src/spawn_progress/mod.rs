//! Spawn chunk generation with optional terminal progress display.
//!
//! During server startup, generates chunks around the spawn position until
//! the 7×7 Full area is complete. When the `spawn_chunk_display` feature is
//! enabled, a colored ANSI grid shows real-time progress including the
//! surrounding dependency rings.
//!
//! Set `PREGEN_RADIUS` environment variable to generate a larger area (e.g., 128).

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

/// Gets the pregeneration radius from environment variable, or returns default spawn radius.
fn get_pregen_radius() -> i32 {
    use std::env;
    env::var("PREGEN_RADIUS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(SPAWN_RADIUS)
}

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
#[cfg(feature = "spawn_chunk_display")]
const TOTAL_SPAWN_CHUNKS: usize = ((SPAWN_RADIUS * 2 + 1) * (SPAWN_RADIUS * 2 + 1)) as usize;

/// Generates spawn chunks, optionally displaying progress in the terminal.
///
/// Adds a ticket at the world spawn position so that a 7×7 area of chunks
/// reaches `Full` status. The generation system is pumped in a loop until
/// completion. With the `spawn_chunk_display` feature, progress is shown as
/// a colored terminal grid that includes the surrounding dependency chunks.
///
/// Set `PREGEN_RADIUS` environment variable to generate a larger area.
pub async fn generate_spawn_chunks(
    server: &Arc<Server>,
    #[allow(unused)] logger: &Arc<CommandLogger>,
) {
    let world = &server.overworld();
    let pregen_radius = get_pregen_radius();

    // For large pregeneration, use center at 0,0; otherwise use spawn position
    let center_chunk = if pregen_radius > SPAWN_RADIUS {
        ChunkPos::new(0, 0)
    } else {
        let spawn_pos = world.level_data.read().data().spawn_pos();
        ChunkPos::new(
            SectionPos::block_to_section_coord(spawn_pos.0.x),
            SectionPos::block_to_section_coord(spawn_pos.0.z),
        )
    };

    let total_chunks = ((pregen_radius * 2 + 1) * (pregen_radius * 2 + 1)) as usize;

    log::info!(
        "Preparing spawn area: {} chunks (radius {}) around chunk ({}, {})",
        total_chunks,
        pregen_radius,
        center_chunk.0.x,
        center_chunk.0.y,
    );

    // For large pregeneration, add tickets for every chunk
    // Ticket level MAX_VIEW_DISTANCE - 3 ensures chunks within 3 of each ticket reach Full
    let ticket_level = MAX_VIEW_DISTANCE - 3;
    let ticket_positions: Vec<ChunkPos> = if pregen_radius > SPAWN_RADIUS {
        // Add tickets for all chunks in the pregeneration area
        let mut positions = Vec::with_capacity(total_chunks);
        for z in -pregen_radius..=pregen_radius {
            for x in -pregen_radius..=pregen_radius {
                positions.push(ChunkPos::new(center_chunk.0.x + x, center_chunk.0.y + z));
            }
        }
        positions
    } else {
        vec![center_chunk]
    };

    {
        let mut tickets = world.chunk_map.chunk_tickets.lock();
        for pos in &ticket_positions {
            tickets.add_ticket(*pos, ticket_level);
        }
    }

    #[cfg(feature = "slow_chunk_gen")]
    SLOW_CHUNK_GEN.store(true, Ordering::Relaxed);

    #[cfg(feature = "spawn_chunk_display")]
    let elapsed = if pregen_radius > SPAWN_RADIUS {
        let start = Instant::now();
        generate_pregen(world, center_chunk, pregen_radius).await;
        start.elapsed()
    } else {
        generate_with_display(world, center_chunk, logger).await
    };

    #[cfg(not(feature = "spawn_chunk_display"))]
    let elapsed = {
        let start = Instant::now();
        generate_pregen(world, center_chunk, pregen_radius).await;
        start.elapsed()
    };

    #[cfg(feature = "slow_chunk_gen")]
    SLOW_CHUNK_GEN.store(false, Ordering::Relaxed);

    // Remove tickets
    {
        let mut tickets = world.chunk_map.chunk_tickets.lock();
        for pos in &ticket_positions {
            tickets.remove_ticket(*pos, ticket_level);
        }
    }

    log::info!(
        "Spawn area prepared: {} chunks in {:.2}s ({:.1} chunks/s)",
        total_chunks,
        elapsed.as_secs_f64(),
        total_chunks as f64 / elapsed.as_secs_f64(),
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

/// Generates chunks with progress reporting for pregeneration.
async fn generate_pregen(world: &World, center_chunk: ChunkPos, radius: i32) {
    let total_chunks = ((radius * 2 + 1) * (radius * 2 + 1)) as usize;
    let mut tick_count: u64 = 1;
    let mut last_report = Instant::now();
    let mut last_completed = 0usize;
    let start = Instant::now();

    loop {
        world.chunk_map.tick_b(world, tick_count, 0, false);

        // Count completed chunks
        let completed = count_full_chunks(world, center_chunk, radius);

        // Report progress every 5 seconds for large pregen
        if radius > SPAWN_RADIUS && last_report.elapsed() >= Duration::from_secs(5) {
            let elapsed = start.elapsed().as_secs_f64();
            let chunks_per_sec = if elapsed > 0.0 {
                (completed.saturating_sub(last_completed)) as f64 / 5.0
            } else {
                0.0
            };
            let percent = (completed as f64 / total_chunks as f64) * 100.0;
            let eta = if chunks_per_sec > 0.0 {
                (total_chunks - completed) as f64 / chunks_per_sec
            } else {
                0.0
            };
            log::info!(
                "Progress: {completed}/{total_chunks} ({percent:.1}%), {chunks_per_sec:.1} chunks/s, ETA: {eta:.0}s",
            );
            last_report = Instant::now();
            last_completed = completed;
        }

        if completed == total_chunks {
            break;
        }

        sleep(Duration::from_millis(10)).await;
        tick_count += 1;
    }
}

/// Counts how many chunks in the area have reached Full status.
fn count_full_chunks(world: &World, center_chunk: ChunkPos, radius: i32) -> usize {
    let mut completed = 0;
    for dz in -radius..=radius {
        for dx in -radius..=radius {
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
