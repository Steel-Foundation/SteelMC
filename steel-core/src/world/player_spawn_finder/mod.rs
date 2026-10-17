use std::sync::Arc;
use std::time::Duration;

use glam::DVec3;
use steel_registry::vanilla_entities;
use steel_registry::vanilla_game_rules::RESPAWN_RADIUS;
use steel_utils::{BlockPos, ChunkPos, WorldAabb, types::GameType};
use tokio::time::sleep;

use crate::behavior::BlockCollisionContext;
use crate::chunk::chunk_request::{ChunkRequestHandle, ChunkRequestState, ChunkTicketKind};
use crate::chunk::status::ChunkStatus;
use crate::fluid::get_fluid_state;
use crate::physics::{CollisionWorld as _, WorldCollisionProvider};
use crate::world::World;

use candidates::CandidateSequence;
use chunk_load::{ChunkLoad, ChunkLoadState, PollBudget, ScanEnd, Visit};
use fallback::{FallbackPass, FallbackPoll};

mod candidates;
mod chunk_load;
mod fallback;
#[cfg(test)]
mod tests;

const PLAYER_SPAWN_CHUNK_RADIUS: u8 = 3;
const BLOCKS_PER_CHUNK: i32 = 16;
const CHUNK_REQUEST_POLL_DELAY: Duration = Duration::from_millis(10);
/// Time budget per spawn search poll before yielding to the next tick.
pub(crate) const POLL_TIME_BUDGET: Duration = Duration::from_millis(1);

pub(crate) enum PlayerSpawnSearchPoll {
    Pending,
    Ready(DVec3),
    Cancelled,
}

/// Batches candidates by chunk in candidate order.
pub(crate) struct PlayerSpawnSearch {
    spawn_suggestion: BlockPos,
    sequence: CandidateSequence,
    next_candidate_index: i32,
    pending: Option<PendingSpawnCandidate>,
    /// Cached results, indexed by candidate order.
    results: Vec<CandidateResult>,
    loading: Option<ChunkLoad>,
    /// Tries safe standing spots before the vanilla height fixup.
    fallback: Option<FallbackPass>,
    prefetch: Option<ChunkRequestHandle>,
}

/// Result of polling the chunk being read; a ready load is handed over.
enum LoadPoll {
    Ready(ChunkLoad),
    Waiting,
    Cancelled,
}

#[derive(Clone, Copy)]
struct PendingSpawnCandidate {
    index: i32,
    kind: SpawnCandidateKind,
    chunk: ChunkPos,
}

#[derive(Clone, Copy)]
enum SpawnCandidateKind {
    Candidate,
    Fixup,
}

#[derive(Clone, Copy)]
enum CandidateResult {
    Unchecked,
    Invalid,
    Valid(BlockPos),
}

impl PlayerSpawnSearch {
    pub(crate) fn new(
        world: &Arc<World>,
        spawn_suggestion: BlockPos,
        game_type: GameType,
    ) -> Result<Self, String> {
        let sequence = match Self::search_radius(world, spawn_suggestion, game_type) {
            Some(radius) => CandidateSequence::new(spawn_suggestion, radius)?,
            None => CandidateSequence::empty(spawn_suggestion),
        };
        let slots = usize::try_from(sequence.count())
            .map_err(|e| format!("invalid spawn candidate count: {e}"))?;

        Ok(Self {
            spawn_suggestion,
            sequence,
            next_candidate_index: 0,
            pending: None,
            results: vec![CandidateResult::Unchecked; slots],
            loading: None,
            fallback: None,
            prefetch: None,
        })
    }

    /// Tries a standing spot that avoids fluid and a ceiling dimension's roof before the
    /// vanilla height fixup.
    #[must_use]
    pub(crate) fn with_safe_fallback(mut self) -> Self {
        self.fallback = Some(FallbackPass::new(self.results.len()));
        self
    }

    /// The radius, in blocks, of the square searched around `spawn_suggestion`, or `None` when
    /// the game mode skips the search.
    fn search_radius(
        world: &Arc<World>,
        spawn_suggestion: BlockPos,
        game_type: GameType,
    ) -> Option<i32> {
        if game_type == GameType::Adventure {
            return None;
        }
        let mut radius = world.get_game_rule(&RESPAWN_RADIUS).max(0);
        let border_distance = world
            .world_border_snapshot()
            .distance_to_border(
                f64::from(spawn_suggestion.x()),
                f64::from(spawn_suggestion.z()),
            )
            .floor() as i32;
        if border_distance < radius {
            radius = border_distance;
        }
        if border_distance <= 1 {
            radius = 1;
        }
        Some(radius)
    }

    /// Requests the radius-3 chunk square around `spawn_suggestion` before the search starts. A
    /// search wider than [`BLOCKS_PER_CHUNK`] blocks gets no prefetch. See [`Self::with_prefetch`].
    pub(crate) fn prefetch_spawn_area(
        world: &Arc<World>,
        spawn_suggestion: BlockPos,
        game_type: GameType,
    ) -> Option<ChunkRequestHandle> {
        let radius = Self::search_radius(world, spawn_suggestion, game_type);
        if radius.is_some_and(|radius| radius > BLOCKS_PER_CHUNK) {
            return None;
        }
        Some(world.request_player_spawn_chunks(block_bottom_center(spawn_suggestion)))
    }

    /// Adopts a request made by [`Self::prefetch_spawn_area`].
    #[must_use]
    pub(crate) fn with_prefetch(mut self, prefetch: Option<ChunkRequestHandle>) -> Self {
        self.prefetch = prefetch;
        self
    }

    #[must_use]
    pub(crate) fn poll(&mut self, world: &Arc<World>) -> PlayerSpawnSearchPoll {
        self.poll_with_time_budget(world, Duration::MAX)
    }

    /// Polls the search, evaluating candidates for about `time_budget`.
    #[must_use]
    pub(crate) fn poll_with_time_budget(
        &mut self,
        world: &Arc<World>,
        time_budget: Duration,
    ) -> PlayerSpawnSearchPoll {
        let mut budget = PollBudget::new(time_budget);

        loop {
            let pending = if let Some(pending) = self.pending {
                pending
            } else {
                let pending = self.next_candidate();
                self.pending = Some(pending);
                pending
            };
            if matches!(pending.kind, SpawnCandidateKind::Fixup) {
                return self.poll_fixup(world, pending, &mut budget);
            }

            // A result cached by an earlier batch is evaluated again at its turn, as in vanilla.
            if self.loading.is_none()
                && !matches!(self.result(pending.index), CandidateResult::Unchecked)
            {
                if budget.exhausted() {
                    return PlayerSpawnSearchPoll::Pending;
                }
                self.recheck_cached_result(world, pending);
                budget.record_evaluation();
            }
            // A batch that ran out of budget is still open and continues before the next candidate.
            if (self.loading.is_some()
                || matches!(self.result(pending.index), CandidateResult::Unchecked))
                && let Some(poll) = self.poll_chunk_batch(world, pending, &mut budget)
            {
                return poll;
            }
            self.pending = None;
            match self.result(pending.index) {
                CandidateResult::Valid(pos) => {
                    return PlayerSpawnSearchPoll::Ready(block_bottom_center(pos));
                }
                CandidateResult::Invalid => {}
                CandidateResult::Unchecked => tracing::error!(
                    "Spawn search finished chunk {:?} without evaluating candidate {}",
                    pending.chunk,
                    pending.index
                ),
            }
        }
    }

    /// Ends a search in which every candidate failed.
    fn poll_fixup(
        &mut self,
        world: &Arc<World>,
        pending: PendingSpawnCandidate,
        budget: &mut PollBudget,
    ) -> PlayerSpawnSearchPoll {
        if self
            .fallback
            .as_ref()
            .is_some_and(|fallback| !fallback.exhausted)
        {
            match self.poll_fallback_pass(world, budget) {
                FallbackPoll::Found(pos) => {
                    return PlayerSpawnSearchPoll::Ready(block_bottom_center(pos));
                }
                FallbackPoll::Waiting => return PlayerSpawnSearchPoll::Pending,
                FallbackPoll::Cancelled => return PlayerSpawnSearchPoll::Cancelled,
                FallbackPoll::Exhausted => {}
            }
        }
        let _load = match self.poll_loading(world, pending.chunk, 0) {
            LoadPoll::Ready(load) => load,
            LoadPoll::Waiting => return PlayerSpawnSearchPoll::Pending,
            LoadPoll::Cancelled => return PlayerSpawnSearchPoll::Cancelled,
        };
        let safe_pos = self
            .fallback
            .is_some()
            .then(|| {
                world.safe_spawn_in_column(self.spawn_suggestion.x(), self.spawn_suggestion.z())
            })
            .flatten();
        PlayerSpawnSearchPoll::Ready(safe_pos.map_or_else(
            || world.fixup_spawn_height(self.spawn_suggestion),
            block_bottom_center,
        ))
    }

    /// Whether the prefetch holds `chunk`: it is in the square and the tickets are committed.
    fn chunk_is_held(&self, chunk: ChunkPos) -> bool {
        self.prefetch.as_ref().is_some_and(|prefetch| {
            prefetch.tickets_committed() && prefetch.positions().contains(&chunk)
        })
    }

    /// Reuses a prefetched chunk; otherwise waits for a new request's tickets to commit.
    fn poll_loading(&mut self, world: &Arc<World>, chunk: ChunkPos, next_scan: i32) -> LoadPoll {
        let load = self
            .loading
            .take()
            .unwrap_or_else(|| ChunkLoad::open(world, chunk, next_scan, self.chunk_is_held(chunk)));
        match load.poll(world) {
            ChunkLoadState::Ready => LoadPoll::Ready(load),
            ChunkLoadState::Waiting => {
                self.loading = Some(load);
                LoadPoll::Waiting
            }
            ChunkLoadState::Cancelled => LoadPoll::Cancelled,
        }
    }

    /// Evaluates a cached result again, since blocks or entities may have changed while the search
    /// waited. If its chunk has unloaded, the result is dropped and the batch reads it again.
    fn recheck_cached_result(&mut self, world: &Arc<World>, pending: PendingSpawnCandidate) {
        let (x, z) = self.sequence.column(pending.index);
        let Some(slot) = self.results.get_mut(pending.index as usize) else {
            return;
        };
        let result = Self::evaluate_candidate(world, x, z);
        // A chunk that unloaded during the evaluation reads as invalid, so the result only counts
        // if the chunk is still loaded.
        *slot = if world.has_full_chunk(pending.chunk) {
            result
        } else {
            CandidateResult::Unchecked
        };
    }

    fn result(&self, index: i32) -> CandidateResult {
        self.results
            .get(index as usize)
            .copied()
            .unwrap_or(CandidateResult::Invalid)
    }

    /// Evaluates candidates in one chunk and caches their results by sequence index.
    fn poll_chunk_batch(
        &mut self,
        world: &Arc<World>,
        pending: PendingSpawnCandidate,
        budget: &mut PollBudget,
    ) -> Option<PlayerSpawnSearchPoll> {
        let mut load = match self.poll_loading(world, pending.chunk, pending.index) {
            LoadPoll::Ready(load) => load,
            LoadPoll::Waiting => return Some(PlayerSpawnSearchPoll::Pending),
            LoadPoll::Cancelled => return Some(PlayerSpawnSearchPoll::Cancelled),
        };
        let results = &mut self.results;
        let end = load.scan(&self.sequence, budget, |index, x, z| {
            let Some(slot) = results.get_mut(index as usize) else {
                return Visit::Skip;
            };
            if !matches!(slot, CandidateResult::Unchecked) {
                return Visit::Skip;
            }
            let result = Self::evaluate_candidate(world, x, z);
            *slot = result;
            if matches!(result, CandidateResult::Valid(_)) && index == pending.index {
                Visit::Stop(())
            } else {
                Visit::Evaluated
            }
        });
        if matches!(end, ScanEnd::OutOfBudget) {
            self.loading = Some(load);
            return Some(PlayerSpawnSearchPoll::Pending);
        }
        None
    }

    fn evaluate_candidate(world: &Arc<World>, x: i32, z: i32) -> CandidateResult {
        match world.level_respawn_pos(x, z) {
            Some(pos) if world.no_collision_no_liquid(pos) => CandidateResult::Valid(pos),
            _ => CandidateResult::Invalid,
        }
    }

    const fn next_candidate(&mut self) -> PendingSpawnCandidate {
        if self.next_candidate_index < self.sequence.count() {
            let index = self.next_candidate_index;
            self.next_candidate_index += 1;

            return PendingSpawnCandidate {
                index,
                kind: SpawnCandidateKind::Candidate,
                chunk: self.sequence.chunk(index),
            };
        }

        PendingSpawnCandidate {
            index: self.sequence.count(),
            kind: SpawnCandidateKind::Fixup,
            chunk: ChunkPos::from_block_pos(self.spawn_suggestion),
        }
    }
}

impl World {
    /// Finds the adjusted shared spawn position used for players entering this world's default spawn.
    pub async fn find_adjusted_shared_spawn_pos(
        self: &Arc<Self>,
        spawn_suggestion: BlockPos,
        game_type: GameType,
    ) -> Result<DVec3, String> {
        let mut search = PlayerSpawnSearch::new(self, spawn_suggestion, game_type)?;
        loop {
            match search.poll(self) {
                PlayerSpawnSearchPoll::Pending => sleep(CHUNK_REQUEST_POLL_DELAY).await,
                PlayerSpawnSearchPoll::Ready(position) => return Ok(position),
                PlayerSpawnSearchPoll::Cancelled => {
                    return Err("spawn search chunk request was cancelled".to_owned());
                }
            }
        }
    }

    /// Loads the vanilla radius-3 full chunk square around a prepared player spawn.
    pub async fn prepare_player_spawn_chunks(
        self: &Arc<Self>,
        spawn_position: DVec3,
    ) -> Result<ChunkRequestHandle, String> {
        let request = self.request_player_spawn_chunks(spawn_position);
        Self::wait_for_chunk_request(&request).await?;
        Ok(request)
    }

    pub(crate) fn request_player_spawn_chunks(
        self: &Arc<Self>,
        spawn_position: DVec3,
    ) -> ChunkRequestHandle {
        self.chunk_map.request_square(
            ChunkPos::from_entity_pos(spawn_position),
            PLAYER_SPAWN_CHUNK_RADIUS,
            ChunkStatus::Full,
            ChunkTicketKind::PlayerSpawn,
        )
    }

    fn request_spawn_candidate_chunk(self: &Arc<Self>, chunk: ChunkPos) -> ChunkRequestHandle {
        self.chunk_map
            .request_chunk(chunk, ChunkStatus::Full, ChunkTicketKind::SpawnSearch)
    }

    async fn wait_for_chunk_request(request: &ChunkRequestHandle) -> Result<(), String> {
        loop {
            match request.poll() {
                ChunkRequestState::Ready => return Ok(()),
                ChunkRequestState::Cancelled => {
                    return Err("chunk request was cancelled".to_owned());
                }
                ChunkRequestState::Pending { .. } => {
                    sleep(CHUNK_REQUEST_POLL_DELAY).await;
                }
            }
        }
    }

    fn fixup_spawn_height(self: &Arc<Self>, spawn_pos: BlockPos) -> DVec3 {
        let mut pos = spawn_pos;

        while !self.no_collision_no_liquid(pos) && pos.y() < self.get_max_y() {
            pos = pos.above();
        }

        pos = pos.below();

        while self.no_collision_no_liquid(pos) && pos.y() > self.get_min_y() {
            pos = pos.below();
        }

        block_bottom_center(pos.above())
    }

    fn no_collision_no_liquid(self: &Arc<Self>, pos: BlockPos) -> bool {
        let dimensions = vanilla_entities::PLAYER.dimensions;
        let aabb = WorldAabb::entity_box(
            f64::from(pos.x()) + 0.5,
            f64::from(pos.y()),
            f64::from(pos.z()) + 0.5,
            f64::from(dimensions.half_width()),
            f64::from(dimensions.height),
        );
        let collision_world = WorldCollisionProvider::new(self);

        !collision_world.has_entity_collision(&aabb)
            && !collision_world
                .has_block_collision_with_context(&aabb, BlockCollisionContext::empty())
            && !aabb_contains_any_liquid(self, aabb)
    }
}

pub(crate) fn aabb_contains_any_liquid(world: &Arc<World>, aabb: WorldAabb) -> bool {
    let min_x = aabb.min_x().floor() as i32;
    let max_x = aabb.max_x().ceil() as i32;
    let min_y = aabb.min_y().floor() as i32;
    let max_y = aabb.max_y().ceil() as i32;
    let min_z = aabb.min_z().floor() as i32;
    let max_z = aabb.max_z().ceil() as i32;

    for x in min_x..max_x {
        for y in min_y..max_y {
            for z in min_z..max_z {
                if !get_fluid_state(world, BlockPos::new(x, y, z)).is_empty() {
                    return true;
                }
            }
        }
    }

    false
}

fn block_bottom_center(pos: BlockPos) -> DVec3 {
    let (x, y, z) = pos.get_bottom_center();
    DVec3::new(x, y, z)
}
