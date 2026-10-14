use std::sync::Arc;
use std::time::{Duration, Instant};

use glam::DVec3;
use steel_registry::vanilla_entities;
use steel_registry::vanilla_game_rules::RESPAWN_RADIUS;
use steel_utils::{BlockPos, ChunkPos, SectionPos, WorldAabb, types::GameType};
use tokio::time::sleep;

use crate::behavior::BlockCollisionContext;
use crate::chunk::chunk_request::{ChunkRequestHandle, ChunkRequestState, ChunkTicketKind};
use crate::chunk::status::ChunkStatus;
use crate::fluid::get_fluid_state;
use crate::physics::{CollisionWorld as _, WorldCollisionProvider};
use crate::world::World;

use candidates::CandidateSequence;
use chunk_load::{ChunkLoad, ChunkLoadState};
use fallback::{FallbackPass, FallbackPoll};

mod candidates;
mod chunk_load;
mod fallback;
#[cfg(test)]
mod tests;

const PLAYER_SPAWN_CHUNK_RADIUS: u8 = 3;
const BLOCKS_PER_CHUNK: i32 = 16;
const CHUNK_REQUEST_POLL_DELAY: Duration = Duration::from_millis(10);
/// time budget per spawn-search poll before yielding to the next tick
pub(crate) const POLL_TIME_BUDGET: Duration = Duration::from_millis(1);

pub(crate) enum PlayerSpawnSearchPoll {
    Pending,
    Ready(DVec3),
    Cancelled,
}

/// Batches candidates by chunk in candidate order
pub(crate) struct PlayerSpawnSearch {
    spawn_suggestion: BlockPos,
    sequence: CandidateSequence,
    next_candidate_index: i32,
    pending: Option<PendingSpawnCandidate>,
    /// cached results indexed by candidate order
    results: Vec<CandidateResult>,
    loading: Option<ChunkLoad>,
    /// tries safe standing spots before the vanilla height fixup
    safe_fallback: bool,
    fallback: FallbackPass,
    prefetch: Option<ChunkRequestHandle>,
    /// A later request committed receipt also covers the prefetch tickets
    prefetch_committed: bool,
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

/// Bounds the time one poll spends evaluating candidates.
struct PollBudget {
    started: Instant,
    limit: Duration,
    evaluated: usize,
}

impl PollBudget {
    fn new(limit: Duration) -> Self {
        Self {
            started: Instant::now(),
            limit,
            evaluated: 0,
        }
    }

    /// At least one candidate is evaluated per poll, so the search always moves forward.
    fn exhausted(&self) -> bool {
        self.evaluated > 0 && self.started.elapsed() >= self.limit
    }
}

impl PlayerSpawnSearch {
    pub(crate) fn new(
        world: &Arc<World>,
        spawn_suggestion: BlockPos,
        game_type: GameType,
    ) -> Result<Self, String> {
        let sequence = if game_type == GameType::Adventure {
            CandidateSequence::empty(spawn_suggestion)
        } else {
            CandidateSequence::new(
                spawn_suggestion,
                Self::search_radius(world, spawn_suggestion),
            )?
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
            safe_fallback: false,
            fallback: FallbackPass::default(),
            prefetch: None,
            prefetch_committed: false,
        })
    }

    /// Makes a failed search prefer a standing spot that avoids fluid and a ceiling
    /// dimension's roof over the vanilla height fixup.
    #[must_use]
    pub(crate) const fn with_safe_fallback(mut self) -> Self {
        self.safe_fallback = true;
        self
    }

    /// The radius, in blocks, of the square searched around `spawn_suggestion`.
    fn search_radius(world: &Arc<World>, spawn_suggestion: BlockPos) -> i32 {
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
        radius
    }

    /// Requests the spawn chunk square around `world`'s spawn point, for a search that
    /// has not been created yet. See [`Self::with_prefetch`].
    pub(crate) fn prefetch_world_spawn_area(world: &Arc<World>) -> Option<ChunkRequestHandle> {
        let spawn_suggestion = world.level_data.read().data().spawn_pos();
        let radius = if world.default_gamemode() == GameType::Adventure {
            0
        } else {
            Self::search_radius(world, spawn_suggestion)
        };
        Self::request_spawn_area(world, spawn_suggestion, radius)
    }

    /// Requests the chunk square around the spawn suggestion, if the search stays within one
    /// chunk of it. The final spawn is then inside the square, so the square requested after
    /// the search overlaps this one in at least 36 of its 49 chunks.
    fn request_spawn_area(
        world: &Arc<World>,
        spawn_suggestion: BlockPos,
        radius: i32,
    ) -> Option<ChunkRequestHandle> {
        (radius <= BLOCKS_PER_CHUNK).then(|| {
            world.chunk_map.request_square(
                block_to_chunk(spawn_suggestion.x(), spawn_suggestion.z()),
                PLAYER_SPAWN_CHUNK_RADIUS,
                ChunkStatus::Full,
                ChunkTicketKind::PlayerSpawn,
            )
        })
    }

    /// Adopts a spawn area request made by [`Self::prefetch_world_spawn_area`]. Chunk
    /// generation then overlaps the search instead of following it.
    #[must_use]
    pub(crate) fn with_prefetch(mut self, prefetch: Option<ChunkRequestHandle>) -> Self {
        self.prefetch = prefetch;
        self
    }

    /// Starts loading the spawn chunk square around the spawn suggestion now.
    #[must_use]
    pub(crate) fn with_spawn_area_prefetch(self, world: &Arc<World>) -> Self {
        let prefetch =
            Self::request_spawn_area(world, self.spawn_suggestion, self.sequence.radius());
        self.with_prefetch(prefetch)
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

            if matches!(self.result(pending.index), CandidateResult::Unchecked)
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
                // The batch always evaluates the pending candidate before it is done.
                CandidateResult::Unchecked => return PlayerSpawnSearchPoll::Pending,
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
        if self.safe_fallback && !self.fallback.exhausted {
            match self.poll_fallback_pass(world, budget) {
                FallbackPoll::Found(pos) => {
                    return PlayerSpawnSearchPoll::Ready(block_bottom_center(pos));
                }
                FallbackPoll::Waiting => return PlayerSpawnSearchPoll::Pending,
                FallbackPoll::Cancelled => return PlayerSpawnSearchPoll::Cancelled,
                FallbackPoll::Exhausted => self.fallback.exhausted = true,
            }
        }
        match self.poll_loading(world, pending.chunk, 0) {
            ChunkLoadState::Ready => {}
            ChunkLoadState::Waiting => return PlayerSpawnSearchPoll::Pending,
            ChunkLoadState::Cancelled => return PlayerSpawnSearchPoll::Cancelled,
        }
        let safe_pos = self
            .safe_fallback
            .then(|| {
                world.safe_spawn_in_column(self.spawn_suggestion.x(), self.spawn_suggestion.z())
            })
            .flatten();
        self.loading = None;
        PlayerSpawnSearchPoll::Ready(safe_pos.map_or_else(
            || world.fixup_spawn_height(self.spawn_suggestion),
            block_bottom_center,
        ))
    }

    /// Whether `chunk` is inside the prefetch square and the prefetch's tickets are committed.
    fn chunk_is_held(&self, chunk: ChunkPos) -> bool {
        self.prefetch_committed
            && self
                .prefetch
                .as_ref()
                .is_some_and(|prefetch| prefetch.positions().contains(&chunk))
    }

/// Reuses prefetched chunks otherwise waits for a new request ticket commit
    fn poll_loading(
        &mut self,
        world: &Arc<World>,
        chunk: ChunkPos,
        next_scan: i32,
    ) -> ChunkLoadState {
        if self.loading.is_none() {
            let held = self.chunk_is_held(chunk);
            self.loading = Some(ChunkLoad::open(world, chunk, next_scan, held));
        }
        let Some(load) = &self.loading else {
            return ChunkLoadState::Waiting;
        };
        let state = load.poll(world);
        if matches!(state, ChunkLoadState::Ready) && load.made_request() && self.prefetch.is_some()
        {
            self.prefetch_committed = true;
        }
        state
    }

    fn result(&self, index: i32) -> CandidateResult {
        self.results
            .get(index as usize)
            .copied()
            .unwrap_or(CandidateResult::Invalid)
    }

    /// Evaluates candidates in one chunk and caches their results by sequence index
    fn poll_chunk_batch(
        &mut self,
        world: &Arc<World>,
        pending: PendingSpawnCandidate,
        budget: &mut PollBudget,
    ) -> Option<PlayerSpawnSearchPoll> {
        match self.poll_loading(world, pending.chunk, pending.index) {
            ChunkLoadState::Ready => {}
            ChunkLoadState::Waiting => return Some(PlayerSpawnSearchPoll::Pending),
            ChunkLoadState::Cancelled => return Some(PlayerSpawnSearchPoll::Cancelled),
        }
        let sequence = self.sequence;
        let Some(load) = self.loading.as_mut() else {
            return Some(PlayerSpawnSearchPoll::Pending);
        };

        while load.next_scan < sequence.count() {
            if budget.exhausted() {
                return Some(PlayerSpawnSearchPoll::Pending);
            }
            let index = load.next_scan;
            load.next_scan += 1;
            let (x, z) = sequence.column(index);
            let Some(slot) = self.results.get_mut(index as usize) else {
                continue;
            };
            if !matches!(slot, CandidateResult::Unchecked) || block_to_chunk(x, z) != load.chunk {
                continue;
            }
            let result = Self::evaluate_candidate(world, x, z);
            budget.evaluated += 1;
            *slot = result;
            if matches!(result, CandidateResult::Valid(_)) && index == pending.index {
                break;
            }
        }
        self.loading = None;
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
            let (x, z) = self.sequence.column(index);

            return PendingSpawnCandidate {
                index,
                kind: SpawnCandidateKind::Candidate,
                chunk: block_to_chunk(x, z),
            };
        }

        PendingSpawnCandidate {
            index: self.sequence.count(),
            kind: SpawnCandidateKind::Fixup,
            chunk: block_to_chunk(self.spawn_suggestion.x(), self.spawn_suggestion.z()),
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
        let spawn_pos = BlockPos::containing(spawn_position.x, spawn_position.y, spawn_position.z);
        let center = ChunkPos::new(
            SectionPos::block_to_section_coord(spawn_pos.x()),
            SectionPos::block_to_section_coord(spawn_pos.z()),
        );
        self.chunk_map.request_square(
            center,
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

const fn block_to_chunk(x: i32, z: i32) -> ChunkPos {
    ChunkPos::new(
        SectionPos::block_to_section_coord(x),
        SectionPos::block_to_section_coord(z),
    )
}

fn block_bottom_center(pos: BlockPos) -> DVec3 {
    let (x, y, z) = pos.get_bottom_center();
    DVec3::new(x, y, z)
}
