use super::*;
use crate::player::connection::NetworkConnection as _;
use crate::{
    behavior::{
        BlockStateBehaviorExt as _,
        blocks::{BedBlock, RespawnAnchorBlock},
    },
    chunk::{
        chunk_access::ChunkStatus,
        chunk_request::{ChunkRequest, ChunkTicketKind},
    },
    player::Player,
};
use steel_protocol::packets::game::{CSound, SoundSource};
use steel_registry::blocks::{
    block_state_ext::BlockStateExt as _, properties::BlockStateProperties,
};
use steel_registry::{sound_events, vanilla_blocks};
use steel_utils::wrap_degrees;

// Bed candidates reach two blocks out and collision checks read one block farther.
const PERSONAL_RESPAWN_SEARCH_BLOCK_RADIUS: i32 = 3;

/// Vanilla per-player respawn configuration.
#[derive(Debug, Clone, PartialEq)]
pub struct PlayerRespawnConfig {
    /// Dimension, bed/anchor position, yaw, and pitch used for respawn.
    pub respawn_data: RespawnData,
    /// Whether respawning is forced even if the spawn block is unavailable.
    pub forced: bool,
}

impl PlayerRespawnConfig {
    #[must_use]
    pub(crate) const fn new(respawn_data: RespawnData, forced: bool) -> Self {
        Self {
            respawn_data,
            forced,
        }
    }

    #[must_use]
    pub(crate) fn is_same_position(&self, other: Option<&Self>) -> bool {
        other.is_some_and(|other| self.respawn_data.global_pos == other.respawn_data.global_pos)
    }
}

#[derive(Clone, Copy)]
struct DeathRespawnSpawn {
    position: DVec3,
    rotation: (f32, f32),
    anchor_deplete_sound_pos: Option<BlockPos>,
    missing_respawn_block: bool,
}

struct PlayerRespawnJob {
    player: Arc<Player>,
    source_world: Arc<World>,
    fallback_respawn: Option<(Arc<World>, RespawnData)>,
    target_world: Arc<World>,
    rotation: (f32, f32),
    missing_respawn_block: bool,
    kind: RespawnRequestKind,
    phase: PlayerRespawnJobPhase,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RespawnRequestKind {
    Death,
    EndCredits,
}

enum PlayerRespawnJobPhase {
    LoadingPersonalRespawnBlock {
        config: PlayerRespawnConfig,
        request: ChunkRequestHandle,
    },
    Searching(PlayerSpawnSearch),
}

impl PlayerRespawnJob {
    fn new(
        player: Arc<Player>,
        source_world: Arc<World>,
        fallback_world: Arc<World>,
        fallback_respawn_data: RespawnData,
        personal_respawn: Option<(Arc<World>, PlayerRespawnConfig)>,
        kind: RespawnRequestKind,
    ) -> Result<Self, String> {
        let fallback_rotation = (fallback_respawn_data.yaw, fallback_respawn_data.pitch);
        let (target_world, rotation, phase, fallback_respawn) =
            if let Some((personal_world, config)) = personal_respawn {
                let pos = config.respawn_data.pos();
                let request = Self::request_personal_respawn_chunks(&personal_world, pos);
                (
                    personal_world,
                    (config.respawn_data.yaw, config.respawn_data.pitch),
                    PlayerRespawnJobPhase::LoadingPersonalRespawnBlock { config, request },
                    Some((fallback_world, fallback_respawn_data)),
                )
            } else {
                let fallback_search = PlayerSpawnSearch::new(
                    &fallback_world,
                    fallback_respawn_data.pos(),
                    fallback_world.default_gamemode,
                )?;
                (
                    fallback_world,
                    fallback_rotation,
                    PlayerRespawnJobPhase::Searching(fallback_search),
                    None,
                )
            };
        Ok(Self {
            player,
            source_world,
            fallback_respawn,
            target_world,
            rotation,
            missing_respawn_block: false,
            kind,
            phase,
        })
    }

    fn request_personal_respawn_chunks(world: &Arc<World>, pos: BlockPos) -> ChunkRequestHandle {
        world.chunk_map.request_chunks(ChunkRequest {
            status: ChunkStatus::Full,
            positions: Self::personal_respawn_chunk_positions(pos),
            ticket_kind: ChunkTicketKind::PlayerSpawn,
        })
    }

    fn personal_respawn_chunk_positions(pos: BlockPos) -> Vec<ChunkPos> {
        let min = ChunkPos::from_block_pos(pos.offset(
            -PERSONAL_RESPAWN_SEARCH_BLOCK_RADIUS,
            0,
            -PERSONAL_RESPAWN_SEARCH_BLOCK_RADIUS,
        ));
        let max = ChunkPos::from_block_pos(pos.offset(
            PERSONAL_RESPAWN_SEARCH_BLOCK_RADIUS,
            0,
            PERSONAL_RESPAWN_SEARCH_BLOCK_RADIUS,
        ));
        let mut positions = Vec::new();
        for x in min.0.x..=max.0.x {
            for z in min.0.y..=max.0.y {
                positions.push(ChunkPos::new(x, z));
            }
        }
        positions
    }

    fn still_valid(&self) -> bool {
        !self.player.connection.closed()
            && Arc::ptr_eq(&self.player.get_world(), &self.source_world)
            && match self.kind {
                RespawnRequestKind::Death => {
                    Player::should_process_respawn(self.player.get_health())
                }
                RespawnRequestKind::EndCredits => self.player.has_won_game(),
            }
    }
}

impl ServerJob for PlayerRespawnJob {
    fn poll(&mut self, _context: &mut ServerJobContext) -> JobPoll {
        if !self.still_valid() {
            self.player.finish_respawn_request();
            return JobPoll::Finished;
        }

        loop {
            match &mut self.phase {
                PlayerRespawnJobPhase::LoadingPersonalRespawnBlock { config, request } => {
                    match request.poll() {
                        ChunkRequestState::Pending { .. } => return JobPoll::Pending,
                        ChunkRequestState::Cancelled => {
                            self.player.finish_respawn_request();
                            return JobPoll::Finished;
                        }
                        ChunkRequestState::Ready if request.ready_chunks().is_none() => {
                            return JobPoll::Pending;
                        }
                        ChunkRequestState::Ready => {}
                    }
                    if let Some(spawn) = Self::resolve_personal_respawn(
                        &self.target_world,
                        &self.player,
                        config,
                        matches!(self.kind, RespawnRequestKind::Death),
                    ) {
                        self.finish(spawn);
                        return JobPoll::Finished;
                    }

                    let Some((fallback_world, fallback_respawn_data)) =
                        self.fallback_respawn.take()
                    else {
                        self.player.finish_respawn_request();
                        return JobPoll::Finished;
                    };
                    let fallback_search = match PlayerSpawnSearch::new(
                        &fallback_world,
                        fallback_respawn_data.pos(),
                        fallback_world.default_gamemode,
                    ) {
                        Ok(search) => search,
                        Err(error) => {
                            log::error!(
                                "Failed to prepare fallback respawn for player {}: {error}",
                                self.player.gameprofile.name
                            );
                            self.player.finish_respawn_request();
                            return JobPoll::Finished;
                        }
                    };
                    self.target_world = fallback_world;
                    self.rotation = (fallback_respawn_data.yaw, fallback_respawn_data.pitch);
                    self.missing_respawn_block = true;
                    self.phase = PlayerRespawnJobPhase::Searching(fallback_search);
                }
                PlayerRespawnJobPhase::Searching(search) => {
                    match search.poll_with_ready_candidate_budget(
                        &self.target_world,
                        RESPAWN_SEARCH_READY_CANDIDATE_BUDGET,
                    ) {
                        PlayerSpawnSearchPoll::Pending => return JobPoll::Pending,
                        PlayerSpawnSearchPoll::Cancelled => {
                            self.player.finish_respawn_request();
                            return JobPoll::Finished;
                        }
                        PlayerSpawnSearchPoll::Ready(position) => {
                            let spawn = DeathRespawnSpawn {
                                position,
                                rotation: self.rotation,
                                anchor_deplete_sound_pos: None,
                                missing_respawn_block: self.missing_respawn_block,
                            };
                            self.finish(spawn);
                            return JobPoll::Finished;
                        }
                    }
                }
            }
        }
    }

    fn cancel(&mut self) {
        self.player.finish_respawn_request();
    }
}

impl PlayerRespawnJob {
    fn finish(&self, spawn: DeathRespawnSpawn) {
        match self.kind {
            RespawnRequestKind::Death => {
                self.player
                    .finish_death_respawn(&self.source_world, &self.target_world, spawn);
            }
            RespawnRequestKind::EndCredits => {
                self.player.finish_end_credits_respawn(
                    &self.source_world,
                    &self.target_world,
                    spawn,
                );
            }
        }
    }

    fn resolve_personal_respawn(
        world: &Arc<World>,
        player: &Player,
        config: &PlayerRespawnConfig,
        consume_spawn_block: bool,
    ) -> Option<DeathRespawnSpawn> {
        let pos = config.respawn_data.pos();
        let state = world.get_block_state(pos);

        if RespawnAnchorBlock::can_use_for_respawn(world, pos, state, config.forced) {
            let position = RespawnAnchorBlock::find_standup_position(world, player, pos)?;
            if RespawnAnchorBlock::should_consume_charge_after_respawn(
                config.forced,
                consume_spawn_block,
                true,
            ) {
                let _ = RespawnAnchorBlock::consume_charge(world, pos, state);
            }
            return Some(DeathRespawnSpawn {
                position,
                rotation: (calculate_respawn_look_at_yaw(position, pos), 0.0),
                anchor_deplete_sound_pos: Some(pos),
                missing_respawn_block: false,
            });
        }

        if state.is_bed()
            && Player::bed_rule_value_allows_in_world(
                world,
                world.dimension_type.bed_rule.can_set_spawn,
            )
        {
            let facing = state.get_value(&BlockStateProperties::HORIZONTAL_FACING);
            let position = BedBlock::find_standup_position_with_yaw(
                world,
                player,
                facing,
                pos,
                config.respawn_data.yaw,
            )?;
            return Some(DeathRespawnSpawn {
                position,
                rotation: (calculate_respawn_look_at_yaw(position, pos), 0.0),
                anchor_deplete_sound_pos: None,
                missing_respawn_block: false,
            });
        }

        if !config.forced {
            return None;
        }
        let top_state = world.get_block_state(pos.above());
        Self::resolve_forced_respawn_fallback(
            pos,
            state,
            top_state,
            (config.respawn_data.yaw, config.respawn_data.pitch),
        )
    }

    fn resolve_forced_respawn_fallback(
        pos: BlockPos,
        state: BlockStateId,
        top_state: BlockStateId,
        rotation: (f32, f32),
    ) -> Option<DeathRespawnSpawn> {
        if !state.is_possible_to_respawn_in_this() || !top_state.is_possible_to_respawn_in_this() {
            return None;
        }
        Some(DeathRespawnSpawn {
            position: DVec3::new(
                f64::from(pos.x()) + 0.5,
                f64::from(pos.y()) + 0.1,
                f64::from(pos.z()) + 0.5,
            ),
            rotation,
            anchor_deplete_sound_pos: None,
            missing_respawn_block: false,
        })
    }
}

fn calculate_respawn_look_at_yaw(position: DVec3, look_at_block_pos: BlockPos) -> f32 {
    let bed_center = DVec3::new(
        f64::from(look_at_block_pos.x()) + 0.5,
        f64::from(look_at_block_pos.y()),
        f64::from(look_at_block_pos.z()) + 0.5,
    );
    let look_direction = (bed_center - position).normalize_or_zero();
    wrap_degrees((look_direction.z.atan2(look_direction.x).to_degrees() - 90.0) as f32)
}

impl Player {
    /// Schedules a vanilla death respawn for this player.
    pub fn respawn(&self) {
        let health = self.get_health();
        if !Self::should_process_respawn(health) {
            return;
        }

        let source_world = self.get_world();
        let Some(player_arc) = source_world.players.get_by_entity_id(self.id()) else {
            return;
        };
        if !self.begin_respawn_request() {
            return;
        }

        let Some(server) = self.server.upgrade() else {
            self.finish_respawn_request();
            log::error!(
                "Failed to schedule respawn for player {}: server is gone",
                self.gameprofile.name
            );
            return;
        };
        let (fallback_world, fallback_respawn_data) =
            match server.respawn_world_and_data_for_domain(source_world.domain()) {
                Ok(resolved) => resolved,
                Err(error) => {
                    self.finish_respawn_request();
                    log::error!(
                        "Failed to schedule respawn for player {}: {error}",
                        self.gameprofile.name
                    );
                    return;
                }
            };
        let personal_respawn = self.personal_respawn(&server, &source_world);

        match PlayerRespawnJob::new(
            player_arc,
            source_world,
            fallback_world,
            fallback_respawn_data,
            personal_respawn,
            RespawnRequestKind::Death,
        ) {
            Ok(job) => server.jobs.spawn(job),
            Err(error) => {
                self.finish_respawn_request();
                log::error!(
                    "Failed to schedule respawn for player {}: {error}",
                    self.gameprofile.name
                );
            }
        }
    }

    fn personal_respawn(
        &self,
        server: &Server,
        source_world: &World,
    ) -> Option<(Arc<World>, PlayerRespawnConfig)> {
        self.respawn_config().and_then(|config| {
            server
                .worlds
                .get(config.respawn_data.dimension())
                .filter(|world| world.domain() == source_world.domain())
                .cloned()
                .map(|world| (world, config))
        })
    }

    fn finish_death_respawn(
        self: &Arc<Self>,
        source_world: &Arc<World>,
        target_world: &Arc<World>,
        spawn: DeathRespawnSpawn,
    ) {
        self.finish_respawn_request();

        if self.connection.closed()
            || !Arc::ptr_eq(&self.get_world(), source_world)
            || !Self::should_process_respawn(self.get_health())
        {
            return;
        }

        self.reset_state_for_death_respawn();
        let was_removed = self.base.clear_removed();

        if !was_removed && Arc::ptr_eq(source_world, target_world) {
            source_world.unregister_player_entity(self);
        }

        let keep_inventory =
            target_world.get_game_rule(&KEEP_INVENTORY) || self.game_mode() == GameType::Spectator;
        if keep_inventory {
            self.inventory
                .lock()
                .refresh_all_player_equipment_attribute_modifiers();
        } else {
            self.inventory.lock().clear_content();
            self.experience.lock().clear();
            self.set_score(0);
        }

        self.handle_missing_respawn_block(spawn.missing_respawn_block);

        // Shared reset (clears transient state, sends CRespawn)
        self.reset(target_world.clone(), ResetReason::Respawn);

        self.send_difficulty();

        // Handle XP and score loss on death.
        {
            let mut experience = self.experience.lock();
            // Re-send XP to client after respawn regardless of keepInventory
            experience.dirty = true;
        }

        // TODO: send mob effect packets once effects are implemented

        // Shared spawn (teleport, abilities, weather, time, chunk tracking reset)
        let _ = self.spawn(spawn.position, spawn.rotation, ResetReason::Respawn);

        if let Some(pos) = spawn.anchor_deplete_sound_pos
            && target_world.get_block_state(pos).get_block() == &vanilla_blocks::RESPAWN_ANCHOR
        {
            self.send_packet(CSound::new(
                &sound_events::BLOCK_RESPAWN_ANCHOR_DEPLETE,
                SoundSource::Blocks,
                pos.0.as_dvec3(),
                1.0,
                1.0,
                rand::random(),
            ));
        }
    }

    fn finish_end_credits_respawn(
        self: &Arc<Self>,
        source_world: &Arc<World>,
        target_world: &Arc<World>,
        spawn: DeathRespawnSpawn,
    ) {
        self.finish_respawn_request();

        if self.connection.closed()
            || !Arc::ptr_eq(&self.get_world(), source_world)
            || !self.has_won_game()
        {
            return;
        }

        self.set_won_game(false);
        self.handle_missing_respawn_block(spawn.missing_respawn_block);
        self.reset(target_world.clone(), ResetReason::EndCredits);
        self.send_difficulty();
        self.experience.lock().dirty = true;
        let _ = self.spawn(spawn.position, spawn.rotation, ResetReason::EndCredits);
    }

    fn handle_missing_respawn_block(&self, missing_respawn_block: bool) {
        if !missing_respawn_block {
            return;
        }
        self.set_respawn_position(None, false);
        self.send_packet(CGameEvent {
            event: GameEventType::NoRespawnBlockAvailable,
            data: 0.0,
        });
    }

    fn reset_state_for_death_respawn(&self) {
        self.close_container();
        self.detach_relationships_for_respawn();

        self.attributes().lock().remove_all_transient();
        self.reset_abilities_for_death_respawn();
        self.living_base.reset_for_player_respawn();
        self.base
            .reset_for_player_respawn(Self::dimensions_for_pose(EntityPose::Standing));

        self.set_health(self.get_max_health());
        self.set_pose(EntityPose::Standing);
        self.reset_entity_state();
        self.sync_base_entity_data();
        self.update_dirty_mob_effect_entity_data();

        *self.food_data.lock() = FoodData::new();
        *self.block_breaking.lock() = BlockBreakingManager::new();
        *self.teleport_state.lock() = TeleportState::new();
        *self.tick_state.lock() = PlayerTickState::new();
        *self.last_item_in_main_hand.lock() = ItemStack::empty();
        self.health_sync.lock().reset_for_respawn();
        self.clear_pending_root_vehicle();
        self.movement.lock().reset_last_known_client_movement();
    }

    fn begin_respawn_request(&self) -> bool {
        self.lifecycle.lock().begin_respawn()
    }

    fn finish_respawn_request(&self) {
        self.lifecycle.lock().finish_respawn();
    }

    fn detach_relationships_for_respawn(&self) {
        for passenger in self.passengers() {
            passenger.stop_riding();
        }
        self.stop_riding();
        self.base.set_boarding_cooldown(0);
    }

    /// Handles client commands, requestStats and `RequestGameRuleValues` are still todo
    pub fn handle_client_command(self: &Arc<Self>, action: ClientCommandAction) {
        match action {
            ClientCommandAction::PerformRespawn => {
                if self.has_won_game() {
                    self.respawn_after_end_credits();
                } else {
                    self.respawn();
                }
            }
            ClientCommandAction::RequestStats | ClientCommandAction::RequestGameRuleValues => {
                // TODO: implement stats
            }
        }
    }

    /// Vanilla accepts a client respawn request only when player health is dead-or-dying.
    /// Steel's death-processed guard is not respawn authority.
    #[must_use]
    pub(in crate::player) const fn should_process_respawn(health: f32) -> bool {
        health <= 0.0
    }

    /// Returns vanilla `ServerPlayer.seenCredits`.
    #[must_use]
    pub fn has_seen_credits(&self) -> bool {
        *self.seen_credits.lock()
    }

    /// Sets vanilla `ServerPlayer.seenCredits`.
    pub fn set_seen_credits(&self, seen_credits: bool) {
        *self.seen_credits.lock() = seen_credits;
    }

    /// Returns vanilla `ServerPlayer.wonGame`.
    #[must_use]
    pub(crate) fn has_won_game(&self) -> bool {
        *self.won_game.lock()
    }

    fn set_won_game(&self, won_game: bool) {
        *self.won_game.lock() = won_game;
    }

    /// Starts the vanilla End credits flow.
    pub(crate) fn show_end_credits(&self) {
        let world = self.get_world();
        let Some(player) = world.players.get_by_entity_id(self.id()) else {
            return;
        };

        world.remove_player_for_world_change(&player);
        if player.has_won_game() {
            return;
        }

        player.set_won_game(true);
        player.send_packet(CGameEvent {
            event: GameEventType::WinGame,
            data: 0.0,
        });
        player.set_seen_credits(true);
    }

    fn respawn_after_end_credits(self: &Arc<Self>) {
        if !self.has_won_game() {
            return;
        }

        let source_world = self.get_world();
        if !self.begin_respawn_request() {
            return;
        }

        let Some(server) = self.server.upgrade() else {
            self.finish_respawn_request();
            log::error!(
                "Failed to schedule End credits respawn for player {}: server is gone",
                self.gameprofile.name
            );
            return;
        };
        let (target_world, respawn_data) =
            match server.respawn_world_and_data_for_domain(source_world.domain()) {
                Ok(resolved) => resolved,
                Err(error) => {
                    self.finish_respawn_request();
                    log::error!(
                        "Failed to schedule End credits respawn for player {}: {error}",
                        self.gameprofile.name
                    );
                    return;
                }
            };
        let personal_respawn = self.personal_respawn(&server, &source_world);

        match PlayerRespawnJob::new(
            Arc::clone(self),
            source_world,
            target_world,
            respawn_data,
            personal_respawn,
            RespawnRequestKind::EndCredits,
        ) {
            Ok(job) => server.jobs.spawn(job),
            Err(error) => {
                self.finish_respawn_request();
                log::error!(
                    "Failed to schedule End credits respawn for player {}: {error}",
                    self.gameprofile.name
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use steel_utils::{BlockPos, ChunkPos};

    use super::PlayerRespawnJob;

    #[test]
    fn personal_respawn_loads_only_chunks_touched_by_standup_search() {
        assert_eq!(
            PlayerRespawnJob::personal_respawn_chunk_positions(BlockPos::new(8, 64, 8)),
            [ChunkPos::new(0, 0)]
        );

        assert_eq!(
            PlayerRespawnJob::personal_respawn_chunk_positions(BlockPos::new(13, 64, 13)),
            [
                ChunkPos::new(0, 0),
                ChunkPos::new(0, 1),
                ChunkPos::new(1, 0),
                ChunkPos::new(1, 1),
            ]
        );
    }
}
