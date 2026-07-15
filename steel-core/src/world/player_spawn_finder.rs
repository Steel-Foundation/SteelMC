use std::sync::Arc;
use std::time::Duration;

use glam::DVec3;
use steel_registry::vanilla_entities;
use steel_registry::vanilla_game_rules::RESPAWN_RADIUS;
use steel_utils::{BlockPos, ChunkPos, SectionPos, WorldAabb, types::GameType};
use tokio::time::sleep;

use crate::behavior::BlockCollisionContext;
use crate::chunk::chunk_access::ChunkStatus;
use crate::chunk::chunk_request::{ChunkRequestHandle, ChunkRequestState, ChunkTicketKind};
use crate::fluid::get_fluid_state;
use crate::physics::{CollisionWorld as _, WorldCollisionProvider};
use crate::world::World;

const ABSOLUTE_MAX_ATTEMPTS: i32 = 1024;
const PLAYER_SPAWN_CHUNK_RADIUS: u8 = 3;
const CHUNK_REQUEST_POLL_DELAY: Duration = Duration::from_millis(10);

pub(crate) enum PlayerSpawnSearchPoll {
    Pending,
    Ready(DVec3),
    Cancelled,
}

pub(crate) struct PlayerSpawnSearch {
    spawn_suggestion: BlockPos,
    radius: i32,
    candidate_count: i32,
    coprime: i32,
    offset: i32,
    next_candidate_index: i32,
    pending: Option<PendingSpawnCandidate>,
}

struct PendingSpawnCandidate {
    x: i32,
    z: i32,
    kind: SpawnCandidateKind,
    request: ChunkRequestHandle,
}

#[derive(Clone, Copy)]
enum SpawnCandidateKind {
    Candidate,
    Fixup,
}

impl PlayerSpawnSearch {
    pub(crate) fn new(
        world: &Arc<World>,
        spawn_suggestion: BlockPos,
        game_type: GameType,
    ) -> Result<Self, String> {
        if game_type == GameType::Adventure {
            return Ok(Self {
                spawn_suggestion,
                radius: 0,
                candidate_count: 0,
                coprime: 0,
                offset: 0,
                next_candidate_index: 0,
                pending: None,
            });
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

        let square_side = i64::from(radius) * 2 + 1;
        let candidate_count =
            i32::try_from(i64::from(ABSOLUTE_MAX_ATTEMPTS).min(square_side * square_side))
                .map_err(|e| format!("invalid spawn candidate count: {e}"))?;
        let coprime = get_coprime(candidate_count);
        let offset = rand::random_range(0..candidate_count);

        Ok(Self {
            spawn_suggestion,
            radius,
            candidate_count,
            coprime,
            offset,
            next_candidate_index: 0,
            pending: None,
        })
    }

    #[must_use]
    pub(crate) fn poll(&mut self, world: &Arc<World>) -> PlayerSpawnSearchPoll {
        self.poll_with_ready_candidate_budget(world, usize::MAX)
    }

    #[must_use]
    pub(crate) fn poll_with_ready_candidate_budget(
        &mut self,
        world: &Arc<World>,
        ready_candidate_budget: usize,
    ) -> PlayerSpawnSearchPoll {
        let ready_candidate_budget = ready_candidate_budget.max(1);
        let mut ready_candidates_checked = 0;

        loop {
            if let Some(pending) = &self.pending {
                match pending.request.poll() {
                    ChunkRequestState::Pending { .. } => return PlayerSpawnSearchPoll::Pending,
                    ChunkRequestState::Cancelled => return PlayerSpawnSearchPoll::Cancelled,
                    ChunkRequestState::Ready => {
                        if pending.request.ready_chunks().is_none() {
                            return PlayerSpawnSearchPoll::Pending;
                        }
                    }
                }
            }

            if let Some(pending) = self.pending.take() {
                ready_candidates_checked += 1;
                match pending.kind {
                    SpawnCandidateKind::Candidate => {
                        let Some(spawn_pos) = world.level_respawn_pos(pending.x, pending.z) else {
                            if ready_candidates_checked >= ready_candidate_budget {
                                self.pending = Some(self.next_candidate(world));
                                return PlayerSpawnSearchPoll::Pending;
                            }
                            continue;
                        };
                        if world.no_collision_no_liquid(spawn_pos) {
                            return PlayerSpawnSearchPoll::Ready(block_bottom_center(spawn_pos));
                        }
                        if ready_candidates_checked >= ready_candidate_budget {
                            self.pending = Some(self.next_candidate(world));
                            return PlayerSpawnSearchPoll::Pending;
                        }
                    }
                    SpawnCandidateKind::Fixup => {
                        return PlayerSpawnSearchPoll::Ready(
                            world.fixup_spawn_height(self.spawn_suggestion),
                        );
                    }
                }
            }

            self.pending = Some(self.next_candidate(world));
        }
    }

    fn next_candidate(&mut self, world: &Arc<World>) -> PendingSpawnCandidate {
        if self.next_candidate_index < self.candidate_count {
            let candidate_index = self.next_candidate_index;
            self.next_candidate_index += 1;

            let value = (self.offset + self.coprime * candidate_index) % self.candidate_count;
            let delta_x = value % (self.radius * 2 + 1);
            let delta_z = value / (self.radius * 2 + 1);
            let target_x = self.spawn_suggestion.x() + delta_x - self.radius;
            let target_z = self.spawn_suggestion.z() + delta_z - self.radius;

            return PendingSpawnCandidate {
                x: target_x,
                z: target_z,
                kind: SpawnCandidateKind::Candidate,
                request: world.request_spawn_candidate_chunk(target_x, target_z),
            };
        }

        PendingSpawnCandidate {
            x: self.spawn_suggestion.x(),
            z: self.spawn_suggestion.z(),
            kind: SpawnCandidateKind::Fixup,
            request: world.request_spawn_candidate_chunk(
                self.spawn_suggestion.x(),
                self.spawn_suggestion.z(),
            ),
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

    fn request_spawn_candidate_chunk(self: &Arc<Self>, x: i32, z: i32) -> ChunkRequestHandle {
        let chunk = ChunkPos::new(
            SectionPos::block_to_section_coord(x),
            SectionPos::block_to_section_coord(z),
        );
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

fn aabb_contains_any_liquid(world: &Arc<World>, aabb: WorldAabb) -> bool {
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

const fn get_coprime(possible_origins: i32) -> i32 {
    if possible_origins <= 16 {
        possible_origins - 1
    } else {
        17
    }
}

fn block_bottom_center(pos: BlockPos) -> DVec3 {
    let (x, y, z) = pos.get_bottom_center();
    DVec3::new(x, y, z)
}
