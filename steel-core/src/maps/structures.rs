use std::{io, sync::Arc};

use futures::FutureExt;
use steel_registry::structure::StructureRef;
use steel_utils::{BlockPos, ChunkPos, Identifier};
use tokio_util::task::AbortOnDropHandle;

use super::chunk_io::MapChunk;
use crate::{
    chunk::{chunk_holder::ChunkHolder, status::ChunkStatus},
    chunk_saver::ChunkStructureInfo,
    world::World,
    worldgen::{
        generator::ChunkGenerator,
        structure::{StructureLocateCandidate, StructureLocatePlan, squared_distance},
    },
};

mod lookahead;
use lookahead::MapStructureLookahead;

pub(super) struct MapStructureSearch {
    world: Arc<World>,
    plan: StructureLocatePlan,
    origin: BlockPos,
    create_reference: bool,
}

enum SavedStructureInfo {
    Unqueried,
    Unknown,
    Known(ChunkStructureInfo),
    LoadRequired,
}

impl SavedStructureInfo {
    async fn read(world: &World, pos: ChunkPos) -> Self {
        let storage = Arc::clone(&world.chunk_map.storage);
        let pool = Arc::clone(&world.chunk_map.generation_pool);
        // Keep cache hits eager. Suspended I/O runs on its runtime so file
        // operations can progress between game-tick polls of the loot job.
        let mut query = Box::pin(async move { storage.read_structure_info(pos, &pool).await });
        let result = if let Some(result) = query.as_mut().now_or_never() {
            result
        } else {
            let task = AbortOnDropHandle::new(world.chunk_map.chunk_runtime.spawn(query));
            match task.await {
                Ok(result) => result,
                Err(error) => Err(io::Error::other(format!(
                    "structure scan task failed: {error}"
                ))),
            }
        };
        match result {
            Ok(Some(info)) => Self::Known(info),
            Ok(None) => Self::Unknown,
            Err(error) => {
                tracing::warn!(chunk = ?pos, %error,
                    "Structure scan failed; using the normal chunk load and recovery path");
                Self::LoadRequired
            }
        }
    }

    fn presence(&self, key: &Identifier, require_unreferenced: bool) -> Option<bool> {
        match self {
            Self::Known(info) => Some(info.contains(key, require_unreferenced)),
            _ => None,
        }
    }
}

impl MapStructureSearch {
    pub(super) fn new(
        world: &Arc<World>,
        structures: &[StructureRef],
        origin: BlockPos,
        create_reference: bool,
    ) -> Option<Self> {
        let generator = world
            .chunk_map
            .world_gen_context
            .generator
            .structure_generator()?;
        let keys = structures
            .iter()
            .map(|structure| structure.key.clone())
            .collect::<Vec<_>>();
        let plan = generator.locate_plan_for_structures(&keys)?;
        Some(Self {
            world: Arc::clone(world),
            plan,
            origin,
            create_reference,
        })
    }

    fn current_structure_chunk(&self, pos: ChunkPos) -> Option<Arc<ChunkHolder>> {
        for chunks in [
            &self.world.chunk_map.chunks,
            &self.world.chunk_map.unloading_chunks,
        ] {
            if let Some(holder) = chunks.read_sync(&pos, |_, holder| Arc::clone(holder))
                && holder.try_chunk(ChunkStatus::StructureStarts).is_some()
            {
                return Some(holder);
            }
        }
        None
    }

    fn known_presence(&self, holder: &ChunkHolder, key: &Identifier) -> Option<bool> {
        let chunk = holder.try_chunk(ChunkStatus::StructureStarts)?;
        Some(chunk.structure_starts().get(key).is_some_and(|start| {
            !start.pieces.is_empty() && (!self.create_reference || start.references == 0)
        }))
    }

    async fn generated_at(
        &self,
        candidate: StructureLocateCandidate,
        lookahead: &mut MapStructureLookahead,
        remaining: &[StructureLocateCandidate],
    ) -> io::Result<bool> {
        let Some(structures) = self.plan.structures_for_candidate(candidate) else {
            return Err(io::Error::other("structure candidate lost its placement"));
        };
        let mut loaded: Option<MapChunk> = None;
        let mut saved = SavedStructureInfo::Unqueried;
        for key in structures {
            let mut known = self
                .current_structure_chunk(candidate.chunk_pos)
                .and_then(|holder| self.known_presence(&holder, key));
            if known.is_none() && loaded.is_none() {
                if matches!(saved, SavedStructureInfo::Unqueried) {
                    saved = SavedStructureInfo::read(&self.world, candidate.chunk_pos).await;
                }
                // A chunk may have finished loading or been claimed during disk I/O.
                known = self
                    .current_structure_chunk(candidate.chunk_pos)
                    .and_then(|holder| self.known_presence(&holder, key))
                    .or_else(|| saved.presence(key, self.create_reference));
            }
            if known == Some(false) {
                continue;
            }
            if known == Some(true) && !self.create_reference {
                return Ok(true);
            }
            if known.is_none() && !matches!(saved, SavedStructureInfo::LoadRequired) {
                let generator = self
                    .world
                    .chunk_map
                    .world_gen_context
                    .generator
                    .structure_generator()
                    .ok_or_else(|| io::Error::other("structure search lost its generator"))?;
                if !generator
                    .locate_candidate_is_structure_chunk(&self.plan, candidate)
                    .ok_or_else(|| io::Error::other("structure candidate lost its placement"))?
                {
                    return Ok(false);
                }
            }
            if loaded.is_none() {
                loaded = Some(
                    lookahead
                        .load(&self.world, candidate, remaining, &self.plan)
                        .await?,
                );
            }
            let chunk = loaded
                .as_ref()
                .and_then(|loaded| loaded.holder.try_chunk(ChunkStatus::StructureStarts))
                .ok_or_else(|| io::Error::other("structure candidate lost readiness"))?;
            let mut starts = chunk.structure_starts_mut();
            let Some(start) = starts.get_mut(key) else {
                continue;
            };
            if start.pieces.is_empty() || (self.create_reference && !start.can_be_referenced()) {
                continue;
            }
            if self.create_reference {
                start.add_reference();
                chunk.mark_dirty();
            }
            return Ok(true);
        }
        Ok(false)
    }

    pub(super) async fn find(
        &self,
        radius: i32,
        lookahead_limit: usize,
    ) -> io::Result<Option<BlockPos>> {
        let mut lookahead = MapStructureLookahead::new(lookahead_limit)?;
        let mut nearest = None;
        let mut scan = None;
        let mut ring_nearest = None;
        let mut ring_result = None;
        let candidates = self.plan.ring_candidates_in_placement_order();
        for (index, &candidate) in candidates.iter().enumerate() {
            if scan != Some(candidate.scan_id()) {
                if let Some(position) = ring_result.take()
                    && nearest.is_none_or(|current| {
                        squared_distance(position, self.origin)
                            < squared_distance(current, self.origin)
                    })
                {
                    nearest = Some(position);
                }
                scan = Some(candidate.scan_id());
                ring_nearest = None;
            }
            let distance = candidate.ring_distance_squared(self.origin);
            if ring_nearest.is_some_and(|best| distance >= best) {
                lookahead.discard(candidate.chunk_pos);
                continue;
            }
            let found = self
                .generated_at(candidate, &mut lookahead, &candidates[index..])
                .await?;
            lookahead.discard(candidate.chunk_pos);
            if found {
                ring_nearest = Some(distance);
                ring_result = Some(candidate.locate_pos);
            }
        }
        if let Some(position) = ring_result
            && nearest.is_none_or(|current| {
                squared_distance(position, self.origin) < squared_distance(current, self.origin)
            })
        {
            nearest = Some(position);
        }
        if !self.plan.has_random_spread() {
            return Ok(nearest);
        }
        for radius in 0..=radius {
            let mut scan = None;
            let mut found_in_scan = false;
            let mut found_in_radius = false;
            let candidates = self
                .plan
                .random_spread_candidates_at_radius(self.origin, radius);
            for (index, &candidate) in candidates.iter().enumerate() {
                if scan != Some(candidate.scan_id()) {
                    scan = Some(candidate.scan_id());
                    found_in_scan = false;
                }
                if found_in_scan {
                    lookahead.discard(candidate.chunk_pos);
                    continue;
                }
                let found = self
                    .generated_at(candidate, &mut lookahead, &candidates[index..])
                    .await?;
                lookahead.discard(candidate.chunk_pos);
                if !found {
                    continue;
                }
                found_in_scan = true;
                found_in_radius = true;
                if nearest.is_none_or(|current| {
                    squared_distance(candidate.locate_pos, self.origin)
                        < squared_distance(current, self.origin)
                }) {
                    nearest = Some(candidate.locate_pos);
                }
            }
            if found_in_radius {
                break;
            }
        }
        Ok(nearest)
    }
}

impl World {
    /// Finds the nearest requested structure, optionally claiming every start
    /// vanilla's placement scans accept along the way.
    /// Bounded generation lookahead may save extra chunks beyond vanilla's visited
    /// candidates; selection and reference claims retain vanilla's scan order.
    pub async fn find_nearest_map_structure(
        self: &Arc<Self>,
        structures: &[StructureRef],
        origin: BlockPos,
        search_radius: i32,
        skip_known_structures: bool,
    ) -> io::Result<Option<BlockPos>> {
        let Some(search) = MapStructureSearch::new(self, structures, origin, skip_known_structures)
        else {
            return Ok(None);
        };
        search
            .find(search_radius, self.chunk_map.max_running_generation_tasks())
            .await
    }

    /// Claims the first matching structure containing the loot origin before a
    /// new exploration target is searched.
    pub async fn claim_map_structure_at(
        self: &Arc<Self>,
        structures: &[StructureRef],
        origin: BlockPos,
    ) -> io::Result<()> {
        let loaded = MapChunk::load(
            self,
            ChunkPos::from_block_pos(origin),
            ChunkStatus::StructureReferences,
        )
        .await?;
        let references: Vec<(Identifier, Vec<ChunkPos>)> = {
            let Some(chunk) = loaded.holder.try_chunk(ChunkStatus::StructureReferences) else {
                return Err(io::Error::other("loot origin lost structure references"));
            };
            // Structure types are unordered, as in vanilla's identity-keyed map.
            // Positions within each type retain LongOpenHashSet iteration order.
            chunk
                .structure_references()
                .iter()
                .filter(|(key, _)| structures.iter().any(|structure| structure.key == **key))
                .map(|(key, positions)| (key.clone(), positions.iter().copied().collect()))
                .collect()
        };
        for (key, positions) in references {
            for pos in positions {
                let loaded = MapChunk::load(self, pos, ChunkStatus::StructureStarts).await?;
                let Some(chunk) = loaded.holder.try_chunk(ChunkStatus::StructureStarts) else {
                    return Err(io::Error::other("claimed structure lost readiness"));
                };
                let mut starts = chunk.structure_starts_mut();
                let Some(start) = starts.get_mut(&key) else {
                    continue;
                };
                if !start
                    .bounding_box
                    .is_some_and(|bounds| bounds.contains_blockpos(origin))
                {
                    continue;
                }
                if start.can_be_referenced() {
                    start.add_reference();
                    chunk.mark_dirty();
                }
                return Ok(());
            }
        }
        Ok(())
    }
}
