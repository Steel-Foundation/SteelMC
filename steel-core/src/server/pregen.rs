//! Startup pregeneration for the server default world.

use std::collections::VecDeque;
use std::env;
use std::sync::Arc;
use std::time::{Duration, Instant};

use steel_utils::{ChunkPos, SectionPos};
use tokio::time::sleep;
use tokio_util::sync::CancellationToken;

use crate::chunk::chunk_access::ChunkStatus;
use crate::chunk::chunk_pyramid::GENERATION_PYRAMID;
use crate::chunk::chunk_request::{
    ChunkRequest, ChunkRequestHandle, ChunkRequestState, ChunkTicketKind,
};
use crate::server::Server;
use crate::world::World;

#[cfg(feature = "slow_chunk_gen")]
use crate::chunk::chunk_holder::SLOW_CHUNK_GEN;
#[cfg(feature = "slow_chunk_gen")]
use std::sync::atomic::Ordering;

const PREGEN_SIZE_ENV: &str = "PREGEN_SIZE";
const VANILLA_PLAYER_SPAWN_SIZE_CHUNKS: i32 = 7;
const PREGEN_WINDOW_SIZE: i32 = 32;
const PREGEN_ACTIVE_WINDOWS: usize = 2;
const PREGEN_UNLOAD_BACKPRESSURE_HIGH: usize = 8192;
const PREGEN_UNLOAD_BACKPRESSURE_LOW: usize = 4096;
const FULL_DEPENDENCY_RADIUS: i32 = GENERATION_PYRAMID
    .get_step_to(ChunkStatus::Full)
    .accumulated_dependencies
    .get_radius_of(ChunkStatus::Empty) as i32;

#[derive(Clone, Copy, Debug)]
struct PregenWindow {
    min_x: i32,
    max_x: i32,
    min_z: i32,
    max_z: i32,
}

impl PregenWindow {
    fn positions(self) -> Vec<ChunkPos> {
        let mut positions = Vec::with_capacity(self.chunk_count());
        for z in self.min_z..=self.max_z {
            for x in self.min_x..=self.max_x {
                positions.push(ChunkPos::new(x, z));
            }
        }
        positions
    }

    const fn chunk_count(self) -> usize {
        (self.width() * self.height()) as usize
    }

    const fn width(self) -> i32 {
        self.max_x - self.min_x + 1
    }

    const fn height(self) -> i32 {
        self.max_z - self.min_z + 1
    }

    const fn protected_rect(self) -> PregenRect {
        PregenRect {
            min_x: self.min_x - FULL_DEPENDENCY_RADIUS,
            max_x: self.max_x + FULL_DEPENDENCY_RADIUS,
            min_z: self.min_z - FULL_DEPENDENCY_RADIUS,
            max_z: self.max_z + FULL_DEPENDENCY_RADIUS,
        }
    }
}

#[derive(Clone, Copy)]
struct PregenRect {
    min_x: i32,
    max_x: i32,
    min_z: i32,
    max_z: i32,
}

impl PregenRect {
    const fn overlaps(self, other: Self) -> bool {
        self.min_x <= other.max_x
            && self.max_x >= other.min_x
            && self.min_z <= other.max_z
            && self.max_z >= other.min_z
    }
}

struct ActivePregenWindow {
    window: PregenWindow,
    handle: ChunkRequestHandle,
    ready_chunks: usize,
    ready: bool,
    counted: bool,
}

impl ActivePregenWindow {
    fn new(world: &Arc<World>, window: PregenWindow) -> Self {
        let handle = world.chunk_map.request_chunks(ChunkRequest {
            status: ChunkStatus::Full,
            positions: window.positions(),
            ticket_kind: ChunkTicketKind::Pregen,
        });
        Self {
            window,
            handle,
            ready_chunks: 0,
            ready: false,
            counted: false,
        }
    }

    fn poll(&mut self, world: &Arc<World>) {
        match self.handle.poll() {
            ChunkRequestState::Ready => {
                self.ready_chunks = self.window.chunk_count();
                self.ready = true;
            }
            ChunkRequestState::Pending { ready, .. } => {
                self.ready_chunks = ready;
            }
            ChunkRequestState::Cancelled => {
                self.handle = world.chunk_map.request_chunks(ChunkRequest {
                    status: ChunkStatus::Full,
                    positions: self.window.positions(),
                    ticket_kind: ChunkTicketKind::Pregen,
                });
                self.ready_chunks = 0;
                self.ready = false;
                self.counted = false;
            }
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct PregenSize {
    side_length: i32,
    radius: i32,
}

impl PregenSize {
    fn from_side_length(side_length: i32) -> Result<Option<Self>, String> {
        if side_length == 0 {
            return Ok(None);
        }
        if side_length < 0 {
            return Err(format!(
                "{PREGEN_SIZE_ENV} must be 0 or a positive odd integer"
            ));
        }
        if side_length % 2 == 0 {
            return Err(format!(
                "{PREGEN_SIZE_ENV} must be odd so the area has a single center chunk"
            ));
        }

        Ok(Some(Self {
            side_length,
            radius: side_length / 2,
        }))
    }
}

impl Server {
    /// Generates the startup spawn area for the server default world.
    ///
    /// Set `PREGEN_SIZE` to an odd chunk side length, or `0` to skip custom pregen.
    pub async fn prepare_spawn_area(&self) -> bool {
        let overworld = self.overworld();
        let pregen_size = match get_pregen_size() {
            Ok(Some(size)) => size,
            Ok(None) => {
                log::info!("Skipping custom startup spawn-area pregeneration");
                return true;
            }
            Err(error) => {
                log::error!("{error}");
                return false;
            }
        };

        let center_chunk = if pregen_size.side_length > VANILLA_PLAYER_SPAWN_SIZE_CHUNKS {
            ChunkPos::new(0, 0)
        } else {
            let spawn_pos = overworld.level_data.read().data().spawn_pos();
            ChunkPos::new(
                SectionPos::block_to_section_coord(spawn_pos.0.x),
                SectionPos::block_to_section_coord(spawn_pos.0.z),
            )
        };

        pregen_overworld(overworld, center_chunk, pregen_size, &self.cancel_token).await
    }
}

fn get_pregen_size() -> Result<Option<PregenSize>, String> {
    let side_length = match env::var(PREGEN_SIZE_ENV) {
        Ok(value) => value
            .parse::<i32>()
            .map_err(|e| format!("{PREGEN_SIZE_ENV} must be 0 or a positive odd integer: {e}"))?,
        Err(env::VarError::NotPresent) => return Ok(None),
        Err(env::VarError::NotUnicode(_)) => {
            return Err(format!("{PREGEN_SIZE_ENV} must be valid unicode"));
        }
    };

    PregenSize::from_side_length(side_length)
}

async fn pregen_overworld(
    world: &Arc<World>,
    center_chunk: ChunkPos,
    pregen_size: PregenSize,
    cancel_token: &CancellationToken,
) -> bool {
    let total_chunks = total_chunks(pregen_size.side_length);

    log::info!(
        "Preparing spawn area: {} chunks ({}x{}) around chunk ({}, {})",
        total_chunks,
        pregen_size.side_length,
        pregen_size.side_length,
        center_chunk.0.x,
        center_chunk.0.y,
    );

    #[cfg(feature = "slow_chunk_gen")]
    SLOW_CHUNK_GEN.store(true, Ordering::Relaxed);

    let elapsed = {
        let start = Instant::now();
        let completed = generate_pregen(world, center_chunk, pregen_size, cancel_token).await;
        (start.elapsed(), completed)
    };

    #[cfg(feature = "slow_chunk_gen")]
    SLOW_CHUNK_GEN.store(false, Ordering::Relaxed);

    let elapsed_secs = elapsed.0.as_secs_f64();
    let chunks_per_second = if elapsed_secs > 0.0 {
        total_chunks as f64 / elapsed_secs
    } else {
        0.0
    };
    if elapsed.1 {
        log::info!(
            "Spawn area prepared: {total_chunks} chunks in {elapsed_secs:.2}s ({chunks_per_second:.1} chunks/s)",
        );
    } else {
        log::info!("Spawn area preparation cancelled after {elapsed_secs:.2}s");
    }
    elapsed.1
}

fn build_pregen_windows(center_chunk: ChunkPos, radius: i32) -> VecDeque<PregenWindow> {
    let min_x = center_chunk.0.x - radius;
    let max_x = center_chunk.0.x + radius;
    let min_z = center_chunk.0.y - radius;
    let max_z = center_chunk.0.y + radius;
    let mut windows = VecDeque::new();

    let mut z = min_z;
    while z <= max_z {
        let window_max_z = (z + PREGEN_WINDOW_SIZE - 1).min(max_z);
        let mut x = min_x;
        while x <= max_x {
            let window_max_x = (x + PREGEN_WINDOW_SIZE - 1).min(max_x);
            windows.push_back(PregenWindow {
                min_x: x,
                max_x: window_max_x,
                min_z: z,
                max_z: window_max_z,
            });
            x = window_max_x + 1;
        }
        z = window_max_z + 1;
    }

    windows
}

async fn generate_pregen(
    world: &Arc<World>,
    center_chunk: ChunkPos,
    pregen_size: PregenSize,
    cancel_token: &CancellationToken,
) -> bool {
    let total_chunks = total_chunks(pregen_size.side_length);
    let mut pending_windows = build_pregen_windows(center_chunk, pregen_size.radius);
    let mut active_windows = Vec::with_capacity(PREGEN_ACTIVE_WINDOWS + 1);
    let mut last_report = Instant::now();
    let mut last_completed = 0usize;
    let mut completed = 0usize;
    let mut unload_backpressure = false;
    let start = Instant::now();

    log::info!(
        "Pregeneration windowing: {PREGEN_WINDOW_SIZE}x{PREGEN_WINDOW_SIZE} target chunks, {PREGEN_ACTIVE_WINDOWS} active windows, dependency halo {FULL_DEPENDENCY_RADIUS} chunks",
    );

    fill_active_windows(world, &mut pending_windows, &mut active_windows);

    while completed < total_chunks {
        if cancel_token.is_cancelled() {
            release_all_windows(world, &mut active_windows);
            return false;
        }

        drain_pregen_broadcasts(world);
        world.chunk_map.tick_scheduling();
        update_unload_backpressure(world, &mut unload_backpressure);

        for active in &mut active_windows {
            active.poll(world);
        }

        if !unload_backpressure {
            let newly_ready_count = active_windows
                .iter()
                .filter(|active| active.ready && !active.counted)
                .count();
            for _ in 0..newly_ready_count {
                activate_next_window(world, &mut pending_windows, &mut active_windows);
            }
        }

        for active in &mut active_windows {
            if active.ready && !active.counted {
                completed += active.window.chunk_count();
                active.counted = true;
            }
        }
        if !unload_backpressure {
            fill_active_windows(world, &mut pending_windows, &mut active_windows);
        }
        drain_pregen_broadcasts(world);
        world.chunk_map.tick_scheduling();
        update_unload_backpressure(world, &mut unload_backpressure);
        release_unneeded_completed_windows(world, &mut active_windows);

        if completed == total_chunks {
            break;
        }

        if pregen_size.side_length > VANILLA_PLAYER_SPAWN_SIZE_CHUNKS
            && last_report.elapsed() >= Duration::from_secs(5)
        {
            let report_elapsed = last_report.elapsed().as_secs_f64();
            let ready_in_active = active_windows
                .iter()
                .filter(|active| !active.counted)
                .map(|active| active.ready_chunks)
                .sum::<usize>();
            let current_completed = (completed + ready_in_active).min(total_chunks);
            let elapsed = start.elapsed().as_secs_f64();
            let chunks_per_sec = if elapsed > 0.0 {
                (current_completed.saturating_sub(last_completed)) as f64 / report_elapsed
            } else {
                0.0
            };
            let percent = (current_completed as f64 / total_chunks as f64) * 100.0;
            let remaining = total_chunks.saturating_sub(current_completed);
            let eta = if chunks_per_sec > 0.0 && remaining > 0 {
                remaining as f64 / chunks_per_sec
            } else {
                0.0
            };
            log::info!(
                "Progress: {current_completed}/{total_chunks} ({percent:.1}%), {chunks_per_sec:.1} chunks/s, ETA: {eta:.0}s",
            );
            last_report = Instant::now();
            last_completed = current_completed;
        }

        tokio::select! {
            () = cancel_token.cancelled() => {
                release_all_windows(world, &mut active_windows);
                return false;
            }
            () = sleep(Duration::from_millis(10)) => {}
        }
    }

    release_all_windows(world, &mut active_windows);
    true
}

fn update_unload_backpressure(world: &Arc<World>, unload_backpressure: &mut bool) {
    let unloading_chunks = world.chunk_map.unloading_chunks.len();
    if *unload_backpressure {
        if unloading_chunks <= PREGEN_UNLOAD_BACKPRESSURE_LOW {
            *unload_backpressure = false;
            log::info!(
                "Pregen unload backpressure released: unloading_chunks={unloading_chunks}, low_watermark={PREGEN_UNLOAD_BACKPRESSURE_LOW}",
            );
        }
        return;
    }

    if unloading_chunks >= PREGEN_UNLOAD_BACKPRESSURE_HIGH {
        *unload_backpressure = true;
        log::info!(
            "Pregen unload backpressure active: unloading_chunks={unloading_chunks}, high_watermark={PREGEN_UNLOAD_BACKPRESSURE_HIGH}, low_watermark={PREGEN_UNLOAD_BACKPRESSURE_LOW}",
        );
    }
}

fn drain_pregen_broadcasts(world: &Arc<World>) {
    world.chunk_map.broadcast_changed_chunks();
}

fn total_chunks(side_length: i32) -> usize {
    let side_length = i64::from(side_length);
    (side_length * side_length) as usize
}

fn fill_active_windows(
    world: &Arc<World>,
    pending_windows: &mut VecDeque<PregenWindow>,
    active_windows: &mut Vec<ActivePregenWindow>,
) {
    while active_windows.iter().filter(|active| !active.ready).count() < PREGEN_ACTIVE_WINDOWS {
        if !activate_next_window(world, pending_windows, active_windows) {
            break;
        }
    }
}

fn activate_next_window(
    world: &Arc<World>,
    pending_windows: &mut VecDeque<PregenWindow>,
    active_windows: &mut Vec<ActivePregenWindow>,
) -> bool {
    let Some(window) = pending_windows.pop_front() else {
        return false;
    };

    active_windows.push(ActivePregenWindow::new(world, window));
    true
}

fn release_unneeded_completed_windows(
    world: &Arc<World>,
    active_windows: &mut Vec<ActivePregenWindow>,
) {
    let incomplete_windows = active_windows
        .iter()
        .filter(|active| !active.ready)
        .map(|active| active.window)
        .collect::<Vec<_>>();

    active_windows.retain(|active| {
        if !active.ready {
            return true;
        }

        let protected = active.window.protected_rect();

        incomplete_windows
            .iter()
            .any(|window| protected.overlaps(window.protected_rect()))
    });

    world.chunk_map.tick_scheduling();
}

fn release_all_windows(world: &Arc<World>, active_windows: &mut Vec<ActivePregenWindow>) {
    active_windows.clear();
    world.chunk_map.tick_scheduling();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pregen_size_accepts_zero_as_disabled() {
        assert_eq!(PregenSize::from_side_length(0), Ok(None));
    }

    #[test]
    fn pregen_size_accepts_odd_side_lengths() {
        assert_eq!(
            PregenSize::from_side_length(7),
            Ok(Some(PregenSize {
                side_length: 7,
                radius: 3,
            }))
        );
    }

    #[test]
    fn pregen_size_rejects_even_side_lengths() {
        assert!(PregenSize::from_side_length(2).is_err());
    }

    #[test]
    fn pregen_size_rejects_negative_side_lengths() {
        assert!(PregenSize::from_side_length(-1).is_err());
    }
}
