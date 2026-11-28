//! How the parity tests order feature generation: one chunk at a time, in waves on several
//! threads, or through the chunk map's features gate with a serial replay.

use std::collections::VecDeque;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::thread;

use rayon::prelude::*;
use rustc_hash::{FxHashMap, FxHashSet};
use steel_core::chunk::status::ChunkStatus;
use steel_core::chunk::window_gate::ChunkWindowGate;
use steel_utils::locks::SyncMutex;
use steel_utils::{ChunkPos, Identifier};

use super::{FeatureGenerationInputs, generate_features_at};

/// Chunk orders a [`FeatureSchedule::Gate`] run recorded, by dimension.
pub(super) type RecordedFeatureOrders = FxHashMap<&'static str, VecDeque<Vec<(i32, i32)>>>;

/// Threads for the [`FeatureSchedule::Waves`] and [`FeatureSchedule::Gate`] tests.
pub(super) const PARALLEL_FEATURE_THREADS: usize = 4;

/// How [`generate_features_for_positions`] orders feature generation.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum FeatureSchedule {
    /// One chunk at a time, in vanilla's order.
    Serial,
    /// Waves of chunks that cannot observe each other, on several threads (see
    /// [`feature_waves`]).
    Waves,
    /// Several threads each taking the next chunk, kept apart only by the chunk map's features
    /// gate. Records the order chunks got their windows in.
    Gate,
    /// One chunk at a time, in the orders a [`Self::Gate`] run recorded.
    Replay,
}

impl FeatureSchedule {
    /// Whether chunks that can observe each other run in vanilla's order, so the result
    /// must match vanilla's hashes.
    pub(super) const fn follows_vanilla_order(self) -> bool {
        matches!(self, Self::Serial | Self::Waves)
    }
}

/// Feature scheduling state for one dimension.
pub(super) struct FeatureScheduler {
    pub(super) schedule: FeatureSchedule,
    /// Threads for [`FeatureSchedule::Waves`] and [`FeatureSchedule::Gate`].
    pub(super) pool: Option<rayon::ThreadPool>,
    /// The chunk map's features-step gate, for [`FeatureSchedule::Gate`].
    pub(super) gate: Option<Arc<ChunkWindowGate>>,
    /// Chunk orders, one per [`generate_features_for_positions`] call, that a
    /// [`FeatureSchedule::Gate`] run records and a [`FeatureSchedule::Replay`] run consumes.
    pub(super) orders: SyncMutex<VecDeque<Vec<(i32, i32)>>>,
    /// Failed window reservations in a [`FeatureSchedule::Gate`] run.
    pub(super) window_waits: AtomicUsize,
}

pub(super) fn generate_features_for_positions(
    positions: &[(i32, i32)],
    generated_positions: &mut FxHashSet<(i32, i32)>,
    inputs: FeatureGenerationInputs<'_>,
) {
    let pending = positions
        .iter()
        .copied()
        .filter(|&pos| generated_positions.insert(pos))
        .collect::<Vec<_>>();
    let scheduler = inputs.scheduler;
    match scheduler.schedule {
        FeatureSchedule::Serial => {
            for &pos in &pending {
                generate_features_at(pos, &inputs);
            }
        }
        FeatureSchedule::Waves => {
            let Some(pool) = &scheduler.pool else {
                panic!("wave feature schedule needs a thread pool");
            };
            for wave in feature_waves(&pending, &inputs) {
                pool.install(|| {
                    wave.par_iter()
                        .for_each(|&pos| generate_features_at(pos, &inputs));
                });
            }
        }
        FeatureSchedule::Gate => {
            let (Some(pool), Some(gate)) = (&scheduler.pool, &scheduler.gate) else {
                panic!("gated feature schedule needs a thread pool and a gate");
            };
            let order = SyncMutex::new(Vec::with_capacity(pending.len()));
            // Every thread takes the next chunk in order, so neighboring chunks are started
            // together and have to be kept apart by the gate.
            let next = AtomicUsize::new(0);
            pool.broadcast(|_| {
                while let Some(&(x, z)) = pending.get(next.fetch_add(1, Ordering::Relaxed)) {
                    let _reservation = loop {
                        if let Some(reservation) = gate.try_reserve_centered(ChunkPos::new(x, z)) {
                            break reservation;
                        }
                        scheduler.window_waits.fetch_add(1, Ordering::Relaxed);
                        thread::yield_now();
                    };
                    // Recorded while the window is held, so chunks with overlapping windows
                    // are recorded in the order they ran.
                    order.lock().push((x, z));
                    generate_features_at((x, z), &inputs);
                }
            });
            scheduler.orders.lock().push_back(order.into_inner());
        }
        FeatureSchedule::Replay => {
            let Some(order) = scheduler.orders.lock().pop_front() else {
                panic!("feature replay ran out of recorded orders");
            };
            let mut replayed = order.clone();
            replayed.sort_unstable();
            let mut expected = pending;
            expected.sort_unstable();
            assert_eq!(
                replayed, expected,
                "recorded feature order covers other chunks"
            );
            for pos in order {
                generate_features_at(pos, &inputs);
            }
        }
    }
}

/// Splits `positions`, in vanilla's generation order, into waves whose chunks can run
/// concurrently without changing any result vanilla's serial order would produce.
///
/// A chunk goes in a later wave than every earlier chunk it conflicts with. Two chunks
/// conflict when one's features may write where the other's read or write (centers within
/// twice the write radius), or when both place pieces of the same structure start, since
/// the first placement can update the start's shared state (e.g. a scattered feature's
/// height position). The live gate does not exclude shared starts, so this is stricter than
/// it; see `ChunkMap::window_gate_for_step`.
fn feature_waves(
    positions: &[(i32, i32)],
    inputs: &FeatureGenerationInputs<'_>,
) -> Vec<Vec<(i32, i32)>> {
    let conflict_distance = inputs.feature_step.block_state_write_radius * 2;
    let placed_starts = positions
        .iter()
        .map(|pos| {
            let Some(holder) = inputs.holders.get(pos) else {
                panic!("Missing feature center chunk {pos:?}");
            };
            let Some(chunk) = holder.try_chunk(ChunkStatus::Terrain) else {
                panic!("Feature center chunk {pos:?} missing");
            };
            chunk
                .structure_references()
                .iter()
                .flat_map(|(structure, sources)| {
                    sources
                        .iter()
                        .map(move |source| (structure.clone(), *source))
                })
                .collect::<FxHashSet<(Identifier, ChunkPos)>>()
        })
        .collect::<Vec<_>>();

    let mut wave_of = Vec::<usize>::with_capacity(positions.len());
    for (index, &(x, z)) in positions.iter().enumerate() {
        let wave = (0..index)
            .filter(|&earlier| {
                let (earlier_x, earlier_z) = positions[earlier];
                ((x - earlier_x).abs() <= conflict_distance
                    && (z - earlier_z).abs() <= conflict_distance)
                    || !placed_starts[index].is_disjoint(&placed_starts[earlier])
            })
            .map(|earlier| wave_of[earlier] + 1)
            .max()
            .unwrap_or(0);
        wave_of.push(wave);
    }

    let wave_count = wave_of.iter().max().map_or(0, |last| last + 1);
    let mut waves = vec![Vec::new(); wave_count];
    for (&pos, &wave) in positions.iter().zip(&wave_of) {
        waves[wave].push(pos);
    }
    waves
}
