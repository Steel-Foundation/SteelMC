use super::*;
use crate::player::connection::NetworkConnection as _;

#[derive(Clone, Copy)]
struct DeathRespawnSpawn {
    position: DVec3,
    rotation: (f32, f32),
}

struct PlayerRespawnJob {
    player: Arc<Player>,
    source_world: Arc<World>,
    target_world: Arc<World>,
    rotation: (f32, f32),
    kind: RespawnRequestKind,
    phase: PlayerRespawnJobPhase,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RespawnRequestKind {
    Death,
    EndCredits,
}

enum PlayerRespawnJobPhase {
    Searching(PlayerSpawnSearch),
    LoadingSpawnChunks {
        spawn: DeathRespawnSpawn,
        request: ChunkRequestHandle,
    },
}

impl PlayerRespawnJob {
    fn new(
        player: Arc<Player>,
        source_world: Arc<World>,
        target_world: Arc<World>,
        respawn_data: RespawnData,
        kind: RespawnRequestKind,
    ) -> Result<Self, String> {
        let search = PlayerSpawnSearch::new(
            &target_world,
            respawn_data.pos(),
            target_world.default_gamemode,
        )?;
        Ok(Self {
            player,
            source_world,
            target_world,
            rotation: (respawn_data.yaw, respawn_data.pitch),
            kind,
            phase: PlayerRespawnJobPhase::Searching(search),
        })
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
                            };
                            let request = self.target_world.request_player_spawn_chunks(position);
                            self.phase =
                                PlayerRespawnJobPhase::LoadingSpawnChunks { spawn, request };
                        }
                    }
                }
                PlayerRespawnJobPhase::LoadingSpawnChunks { spawn, request } => {
                    match request.poll() {
                        ChunkRequestState::Pending { .. } => return JobPoll::Pending,
                        ChunkRequestState::Cancelled => {
                            self.player.finish_respawn_request();
                            return JobPoll::Finished;
                        }
                        ChunkRequestState::Ready => {
                            if request.ready_chunks().is_none() {
                                return JobPoll::Pending;
                            }

                            match self.kind {
                                RespawnRequestKind::Death => self.player.finish_death_respawn(
                                    &self.source_world,
                                    &self.target_world,
                                    *spawn,
                                ),
                                RespawnRequestKind::EndCredits => {
                                    self.player.finish_end_credits_respawn(
                                        &self.source_world,
                                        &self.target_world,
                                        *spawn,
                                    );
                                }
                            }
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

impl Player {
    /// TODO: personal respawn blocks/anchors and noRespawnBlockAvailable.
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
        let (target_world, respawn_data) =
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

        match PlayerRespawnJob::new(
            player_arc,
            source_world,
            target_world,
            respawn_data,
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

        // TODO: personal respawn blocks/anchors and NO_RESPAWN_BLOCK_AVAILABLE.

        if !was_removed && Arc::ptr_eq(source_world, target_world) {
            source_world.unregister_player_entity(self);
        }

        // Shared reset (clears transient state, sends CRespawn)
        self.reset(target_world.clone(), ResetReason::Respawn);

        self.send_difficulty();

        // Handle XP and score loss on death.
        let loses_inventory =
            !target_world.get_game_rule(&KEEP_INVENTORY) && self.game_mode() != GameType::Spectator;
        {
            let mut experience = self.experience.lock();
            if loses_inventory {
                // TODO: drop XP orbs (min(level * 7, 100))
                experience.clear();
            }
            // Re-send XP to client after respawn regardless of keepInventory
            experience.dirty = true;
        }
        if loses_inventory {
            self.set_score(0);
        }

        // TODO: send mob effect packets once effects are implemented

        // Shared spawn (teleport, abilities, weather, time, chunk tracking reset)
        let _ = self.spawn(spawn.position, spawn.rotation, ResetReason::Respawn);
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
        self.reset(target_world.clone(), ResetReason::EndCredits);
        self.send_difficulty();
        self.experience.lock().dirty = true;
        let _ = self.spawn(spawn.position, spawn.rotation, ResetReason::EndCredits);
    }

    fn reset_state_for_death_respawn(&self) {
        self.close_container();
        self.detach_relationships_for_respawn();

        self.attributes().lock().remove_all_transient();
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

        match PlayerRespawnJob::new(
            Arc::clone(self),
            source_world,
            target_world,
            respawn_data,
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
