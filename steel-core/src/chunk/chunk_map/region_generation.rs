//! Stage-ordered generation of a chunk rectangle into an unused chunk map.

use std::sync::{Arc, atomic::Ordering};

use futures::future::{BoxFuture, FutureExt, Shared, join_all};
use rustc_hash::FxHashMap;
use steel_utils::ChunkPos;
use thiserror::Error;

use super::ChunkMap;
use crate::chunk::{
    chunk_generation_task::StaticCache2D,
    chunk_holder::ChunkHolder,
    chunk_pyramid::{ChunkStep, GENERATION_PYRAMID},
    chunk_ticket_manager::ticket_level_for_status,
    light::LIGHT_CACHE_RADIUS,
    status::ChunkStatus,
};

const FULL_STEP: &ChunkStep = GENERATION_PYRAMID.get_step_to(ChunkStatus::Full);

type RegionTask = Shared<BoxFuture<'static, Result<(), RegionGenerationError>>>;

/// Why [`ChunkMap::generate_region`] could not generate a region.
#[derive(Debug, Clone, Error, PartialEq, Eq)]
pub enum RegionGenerationError {
    /// The map has active or unloading holders, or its generation refill loop is running.
    #[error("region generation needs an empty chunk map with its refill loop stopped")]
    MapInUse,
    /// `min` exceeds `max` on an axis, or the region's dependency halo leaves the valid chunk range.
    #[error("invalid region bounds {min:?}..={max:?}")]
    InvalidBounds {
        /// Requested minimum corner.
        min: ChunkPos,
        /// Requested maximum corner.
        max: ChunkPos,
    },
    /// A step was disallowed, cancelled or panicked.
    #[error("{status:?} generation failed at {pos:?}")]
    StepFailed {
        /// Status the failed step targeted.
        status: ChunkStatus,
        /// Chunk the step ran on.
        pos: ChunkPos,
    },
}

impl ChunkMap {
    /// Generates every chunk in `min..=max` to Full and returns their holders in X-then-Z order.
    ///
    /// Each status is finished across the region and its dependency halo before the next
    /// starts, so shared dependencies run once rather than once per requested chunk. Chunks
    /// whose steps may touch the same data run in a fixed order and the rest in parallel, so the
    /// result equals generating one chunk at a time and does not depend on thread scheduling.
    ///
    /// The map must be empty with its refill loop stopped, and nothing else may drive it until
    /// this returns. The holders have no tickets, so the map never unloads them.
    pub async fn generate_region(
        self: &Arc<Self>,
        min: ChunkPos,
        max: ChunkPos,
    ) -> Result<Vec<Arc<ChunkHolder>>, RegionGenerationError> {
        let halo = FULL_STEP.get_accumulated_radius_of(ChunkStatus::Empty) as i32;
        let limit = ChunkPos::MAX_COORDINATE_VALUE - halo;
        let in_range = |low: i32, high: i32| low <= high && low >= -limit && high <= limit;
        if !in_range(min.0.x, max.0.x) || !in_range(min.0.y, max.0.y) {
            return Err(RegionGenerationError::InvalidBounds { min, max });
        }
        self.insert_region_holders(min, max, halo)?;

        let mut status = Some(ChunkStatus::Empty);
        while let Some(current) = status {
            let radius = FULL_STEP.get_accumulated_radius_of(current) as i32;
            self.generate_region_stage(
                GENERATION_PYRAMID.get_step_to(current),
                region_positions(min, max, radius),
            )
            .await?;
            status = current.next();
        }

        Ok(region_positions(min, max, 0)
            .into_iter()
            .map(|pos| self.region_holder(pos))
            .collect())
    }

    /// Installs a holder for every region and halo chunk, allowing only the status it needs.
    fn insert_region_holders(
        &self,
        min: ChunkPos,
        max: ChunkPos,
        halo: i32,
    ) -> Result<(), RegionGenerationError> {
        let _source_phase_guard = self.source_phase_guard.lock();
        if !self.generation_refill_stopped.load(Ordering::Acquire)
            || !self.chunks.is_empty()
            || !self.unloading_chunks.is_empty()
        {
            return Err(RegionGenerationError::MapInUse);
        }

        for pos in region_positions(min, max, halo) {
            let dx = (min.0.x - pos.0.x).max(pos.0.x - max.0.x).max(0);
            let dz = (min.0.y - pos.0.y).max(pos.0.y - max.0.y).max(0);
            let distance = dx.max(dz) as usize;
            let needed_status = if distance == 0 {
                ChunkStatus::Full
            } else {
                let Some(status) = FULL_STEP.accumulated_dependencies.get(distance) else {
                    panic!("Full dependencies must cover their own radius");
                };
                status
            };
            let holder = Arc::new(ChunkHolder::new_with_full_publications(
                pos,
                ticket_level_for_status(needed_status),
                None,
                self.world_gen_context.min_y(),
                self.world_gen_context.height(),
                Arc::downgrade(&self.full_publications),
            ));
            if self.chunks.insert_sync(pos, holder).is_err() {
                return Err(RegionGenerationError::MapInUse);
            }
        }
        Ok(())
    }

    /// Applies `step` to `positions` as if one chunk at a time in [`region_step_order`].
    async fn generate_region_stage(
        self: &Arc<Self>,
        step: &'static ChunkStep,
        positions: Vec<ChunkPos>,
    ) -> Result<(), RegionGenerationError> {
        let (read_radius, _) = region_step_radii(step);
        let order = region_step_order(step, positions);
        let starts = order
            .iter()
            .map(|&pos| {
                if step.target_status == ChunkStatus::Features {
                    self.referenced_structure_starts(pos)
                } else {
                    Vec::new()
                }
            })
            .collect::<Vec<_>>();
        let dependencies =
            region_step_dependencies(region_conflict_distance(step), &order, &starts);

        let mut tasks = Vec::<RegionTask>::with_capacity(order.len());
        for (&pos, dependencies) in order.iter().zip(dependencies) {
            let earlier = dependencies
                .into_iter()
                .map(|index| tasks[index].clone())
                .collect::<Vec<_>>();
            let chunk_map = Arc::clone(self);
            let task = async move {
                for task in earlier {
                    task.await?;
                }
                chunk_map.apply_region_step(step, pos, read_radius).await
            };
            tasks.push(task.boxed().shared());
        }
        join_all(tasks).await.into_iter().collect()
    }

    /// Runs one step on one chunk through the same path as ticket-driven generation.
    async fn apply_region_step(
        self: Arc<Self>,
        step: &'static ChunkStep,
        pos: ChunkPos,
        cache_radius: i32,
    ) -> Result<(), RegionGenerationError> {
        let chunk_map = Arc::clone(&self);
        let cache = Arc::new(StaticCache2D::create(
            pos.0.x,
            pos.0.y,
            cache_radius,
            move |x, z| chunk_map.region_holder(ChunkPos::new(x, z)),
        ));
        let holder = cache.get(pos.0.x, pos.0.y);
        let ready = holder.apply_step(step, &self, &cache, Arc::clone(&self.generation_pool));
        let failed = RegionGenerationError::StepFailed {
            status: step.target_status,
            pos,
        };
        match ready {
            Some(ready) => ready.await.ok_or(failed),
            None => Err(failed),
        }
    }

    /// Returns the chunks holding structure starts that Features at `pos` places pieces from.
    fn referenced_structure_starts(&self, pos: ChunkPos) -> Vec<ChunkPos> {
        let holder = self.region_holder(pos);
        let Some(chunk) = holder.try_chunk(ChunkStatus::StructureReferences) else {
            panic!("Features dependencies must have structure references at {pos:?}");
        };
        let references = chunk.structure_references();
        references
            .values()
            .flat_map(|sources| sources.iter().copied())
            .collect()
    }

    /// Returns a holder installed by [`Self::insert_region_holders`].
    fn region_holder(&self, pos: ChunkPos) -> Arc<ChunkHolder> {
        let Some(holder) = self.chunks.read_sync(&pos, |_, holder| Arc::clone(holder)) else {
            panic!("region generation installs a holder for every dependency, missing {pos:?}");
        };
        holder
    }
}

/// Returns how far around its center `step` reads and writes chunks.
///
/// Light worksets span a cache window wider than the Light step's declared dependencies.
const fn region_step_radii(step: &ChunkStep) -> (i32, i32) {
    let read = step.direct_dependencies.get_radius() as i32;
    match step.target_status {
        ChunkStatus::Light if read < LIGHT_CACHE_RADIUS => (LIGHT_CACHE_RADIUS, LIGHT_CACHE_RADIUS),
        ChunkStatus::Light => (read, LIGHT_CACHE_RADIUS),
        _ => (read, step.block_state_write_radius),
    }
}

/// Returns the distance within which one chunk's step may write what another's reads or writes.
///
/// Steps that only write their own chunk read neighbors at lower statuses, so they never conflict.
const fn region_conflict_distance(step: &ChunkStep) -> i32 {
    let (read, write) = region_step_radii(step);
    if write <= 0 {
        0
    } else if read > write {
        write + read
    } else {
        write * 2
    }
}

/// Orders positions so that chunks sharing a phase are beyond the step's conflict distance.
///
/// Phases group positions by coordinates modulo the conflict distance plus one, then sort by X
/// and Z. Any fixed order gives the same determinism; phases keep more chunks independent.
pub(super) fn region_step_order(step: &ChunkStep, mut positions: Vec<ChunkPos>) -> Vec<ChunkPos> {
    let period = region_conflict_distance(step) + 1;
    positions.sort_by_key(|pos| {
        (
            pos.0.x.rem_euclid(period),
            pos.0.y.rem_euclid(period),
            pos.0.x,
            pos.0.y,
        )
    });
    positions
}

/// Returns, for each position in `order`, the earlier positions it must wait for.
///
/// A position waits for earlier ones within `conflict` and for the previous user of each
/// structure-start chunk in its `starts` entry, which orders all users of that start.
fn region_step_dependencies(
    conflict: i32,
    order: &[ChunkPos],
    starts: &[Vec<ChunkPos>],
) -> Vec<Vec<usize>> {
    let indices = order
        .iter()
        .enumerate()
        .map(|(index, &pos)| (pos, index))
        .collect::<FxHashMap<_, _>>();
    let mut start_users = FxHashMap::<ChunkPos, usize>::default();
    order
        .iter()
        .zip(starts)
        .enumerate()
        .map(|(index, (pos, starts))| {
            let mut earlier = (pos.0.x - conflict..=pos.0.x + conflict)
                .flat_map(|x| {
                    (pos.0.y - conflict..=pos.0.y + conflict).map(move |z| ChunkPos::new(x, z))
                })
                .filter_map(|other| indices.get(&other).copied())
                .filter(|&other| other < index)
                .collect::<Vec<_>>();
            earlier.extend(
                starts
                    .iter()
                    .filter_map(|&start| start_users.insert(start, index))
                    .filter(|&user| user < index),
            );
            earlier
        })
        .collect()
}

/// Lists the rectangle expanded by `radius` in ascending X-then-Z order.
fn region_positions(min: ChunkPos, max: ChunkPos, radius: i32) -> Vec<ChunkPos> {
    (min.0.x - radius..=max.0.x + radius)
        .flat_map(|x| (min.0.y - radius..=max.0.y + radius).map(move |z| ChunkPos::new(x, z)))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Returns the positions `pos` waits for when the given positions run `status` in order.
    fn waits_for(
        status: ChunkStatus,
        positions: &[ChunkPos],
        starts: &[Vec<ChunkPos>],
        pos: ChunkPos,
    ) -> Vec<ChunkPos> {
        let step = GENERATION_PYRAMID.get_step_to(status);
        let order = positions.to_vec();
        let dependencies = region_step_dependencies(region_conflict_distance(step), &order, starts);
        let Some(index) = order.iter().position(|&other| other == pos) else {
            panic!("{pos:?} should be listed");
        };
        dependencies[index]
            .iter()
            .map(|&other| order[other])
            .collect()
    }

    #[test]
    fn features_wait_for_their_full_read_footprint() {
        let positions = [
            ChunkPos::new(0, 0),
            ChunkPos::new(19, 0),
            ChunkPos::new(9, 0),
        ];
        let starts = vec![Vec::new(); positions.len()];

        assert_eq!(
            waits_for(
                ChunkStatus::Features,
                &positions,
                &starts,
                ChunkPos::new(9, 0)
            ),
            [ChunkPos::new(0, 0)]
        );
    }

    #[test]
    fn light_waits_for_overlapping_cache_windows() {
        let positions = [
            ChunkPos::new(0, 0),
            ChunkPos::new(9, 0),
            ChunkPos::new(4, 0),
        ];
        let starts = vec![Vec::new(); positions.len()];

        assert_eq!(
            waits_for(ChunkStatus::Light, &positions, &starts, ChunkPos::new(4, 0)),
            [ChunkPos::new(0, 0)]
        );
    }

    #[test]
    fn features_sharing_a_structure_start_wait_regardless_of_distance() {
        let start = ChunkPos::new(10, 0);
        let positions = [ChunkPos::new(0, 0), ChunkPos::new(20, 0)];
        // Several structures can start in one chunk, so a source may be listed twice.
        let starts = vec![vec![start], vec![start, start]];

        assert_eq!(
            waits_for(
                ChunkStatus::Features,
                &positions,
                &starts,
                ChunkPos::new(20, 0)
            ),
            [ChunkPos::new(0, 0)]
        );
    }
}
