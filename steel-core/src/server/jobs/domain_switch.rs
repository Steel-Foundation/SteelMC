use std::{
    mem,
    sync::{Arc, Weak, mpsc},
};

use glam::DVec3;
use tokio::task::JoinHandle;

use crate::{
    chunk::chunk_request::ChunkRequestState,
    entity::{Entity, PendingWorldChangeToken},
    player::{
        DomainResidenceToken, Player, ResetReason, connection::NetworkConnection,
        player_data::PersistentPlayerData, player_data_storage::GlobalPlayerData,
    },
    server::{
        DomainPlayerData, DomainPlayerState, PlayerAdmissionState, PreparedSpawn, Server,
        UnpreparedDomainPlayerData, UnpreparedDomainPlayerState,
    },
    world::{
        World,
        player_spawn_finder::{PlayerSpawnSearch, PlayerSpawnSearchPoll},
    },
};

use super::{JobPoll, ServerJob, ServerJobContext};

const DOMAIN_SPAWN_SEARCH_READY_CANDIDATE_BUDGET: usize = 8;

pub(in crate::server) struct DomainSwitchJob {
    server: Weak<Server>,
    player: Arc<Player>,
    source_domain: String,
    source_data: Option<Arc<PersistentPlayerData>>,
    target_domain: String,
    pending_token: PendingWorldChangeToken,
    residence_token: DomainResidenceToken,
    phase: DomainSwitchJobPhase,
}

enum DomainSwitchJobPhase {
    WaitingForStorage {
        receiver: mpsc::Receiver<Result<UnpreparedDomainPlayerState, String>>,
        task: JoinHandle<()>,
    },
    SearchingSpawn {
        world: Arc<World>,
        data: UnpreparedDomainPlayerData,
        rotation: (f32, f32),
        search: PlayerSpawnSearch,
    },
    LoadingSpawn {
        state: DomainPlayerState,
    },
    SavingGlobal {
        receiver: mpsc::Receiver<Result<(), String>>,
        task: JoinHandle<()>,
    },
    Transitioning,
}

impl DomainSwitchJob {
    #[expect(
        clippy::too_many_arguments,
        reason = "the constructor makes all detached transition data and both ownership tokens explicit"
    )]
    pub(in crate::server) fn new(
        server: &Arc<Server>,
        player: Arc<Player>,
        source_domain: String,
        source_data: PersistentPlayerData,
        target_domain: String,
        target_world: Option<Arc<World>>,
        pending_token: PendingWorldChangeToken,
        residence_token: DomainResidenceToken,
    ) -> Self {
        let (sender, receiver) = mpsc::channel();
        let task_server = Arc::clone(server);
        let task_player = Arc::clone(&player);
        let task_source_domain = source_domain.clone();
        let source_data = Arc::new(source_data);
        let task_source_data = Arc::clone(&source_data);
        let task_target_domain = target_domain.clone();
        let task = tokio::spawn(async move {
            let result = async {
                task_server
                    .player_data_storage
                    .save_domain_data(
                        &task_source_domain,
                        task_player.gameprofile.id,
                        task_source_data.as_ref(),
                    )
                    .await
                    .map_err(|error| format!("failed to save current domain data: {error}"))?;
                task_server
                    .load_unprepared_domain_player_state(
                        &task_player,
                        &task_target_domain,
                        target_world,
                    )
                    .await
            }
            .await;
            let _ = sender.send(result);
        });

        Self {
            server: Arc::downgrade(server),
            player,
            source_domain,
            source_data: Some(source_data),
            target_domain,
            pending_token,
            residence_token,
            phase: DomainSwitchJobPhase::WaitingForStorage { receiver, task },
        }
    }

    fn abort_async_task(&self) {
        match &self.phase {
            DomainSwitchJobPhase::WaitingForStorage { task, .. }
            | DomainSwitchJobPhase::SavingGlobal { task, .. } => task.abort(),
            DomainSwitchJobPhase::SearchingSpawn { .. }
            | DomainSwitchJobPhase::LoadingSpawn { .. }
            | DomainSwitchJobPhase::Transitioning => {}
        }
    }

    fn finish_source_disconnect(&mut self, error: Option<&str>) -> JobPoll {
        self.abort_async_task();
        if let Some(error) = error {
            log::error!(
                "Failed to switch {} domain: {error}",
                self.player.gameprofile.name
            );
            if !self.player.connection.closed() {
                self.player.disconnect("Failed to switch domain");
            }
        }

        let Some(source_data) = self.source_data.take() else {
            self.player.finish_domain_switch(self.pending_token);
            self.player.finish_pending_world_change(self.pending_token);
            return JobPoll::Finished;
        };
        let Some(server) = self.server.upgrade() else {
            self.player.finish_domain_switch(self.pending_token);
            self.player.finish_pending_world_change(self.pending_token);
            self.player.cleanup();
            return JobPoll::Finished;
        };
        server.queue_relocating_player_disconnect(
            Arc::clone(&self.player),
            self.source_domain.clone(),
            source_data,
            self.pending_token,
        );
        JobPoll::Finished
    }

    fn phase_after_storage(
        server: &Server,
        target_domain: &str,
        state: UnpreparedDomainPlayerState,
    ) -> Result<DomainSwitchJobPhase, String> {
        let UnpreparedDomainPlayerState {
            world,
            explicit_target,
            data,
        } = state;
        if let UnpreparedDomainPlayerData::SavedRestored { data } = data {
            let spawn_position = DVec3::new(data.pos[0], data.pos[1], data.pos[2]);
            let request = world.request_player_spawn_chunks(spawn_position);
            return Ok(DomainSwitchJobPhase::LoadingSpawn {
                state: DomainPlayerState {
                    world,
                    data: DomainPlayerData::SavedRestored { data },
                    spawn_chunk_request: request,
                },
            });
        }

        let (world, spawn_suggestion, rotation) = if explicit_target {
            let (spawn, spawn_pos) = {
                let level_data = world.level_data.read();
                (
                    level_data.data().spawn.clone(),
                    level_data.data().spawn_pos(),
                )
            };
            (world, spawn_pos, (spawn.angle, 0.0))
        } else {
            let (world, respawn_data) = server.respawn_world_and_data_for_domain(target_domain)?;
            (
                world,
                respawn_data.pos(),
                (respawn_data.yaw, respawn_data.pitch),
            )
        };
        let search = PlayerSpawnSearch::new(&world, spawn_suggestion, world.default_gamemode)?;
        Ok(DomainSwitchJobPhase::SearchingSpawn {
            world,
            data,
            rotation,
            search,
        })
    }

    fn state_after_spawn_search(
        world: Arc<World>,
        data: UnpreparedDomainPlayerData,
        spawn: PreparedSpawn,
    ) -> Result<DomainPlayerState, String> {
        let data = match data {
            UnpreparedDomainPlayerData::SavedWithoutLocation { data } => {
                DomainPlayerData::SavedWithoutLocation { data, spawn }
            }
            UnpreparedDomainPlayerData::FirstVisit => DomainPlayerData::FirstVisit { spawn },
            UnpreparedDomainPlayerData::SavedRestored { .. } => {
                return Err("saved domain location unexpectedly entered spawn search".to_owned());
            }
        };
        let request = world.request_player_spawn_chunks(spawn.position);
        Ok(DomainPlayerState {
            world,
            data,
            spawn_chunk_request: request,
        })
    }

    fn commit_target_state(&mut self, server: &Arc<Server>, state: DomainPlayerState) -> JobPoll {
        if !self
            .player
            .is_domain_residence_current(self.residence_token)
        {
            return self.finish_source_disconnect(Some(
                "domain switch lost its residence before target synchronization",
            ));
        }
        if !self
            .player
            .mark_domain_switch_target_handshake(self.pending_token)
        {
            return self.finish_source_disconnect(Some(
                "domain switch lost ownership before target synchronization",
            ));
        }

        let restores = server.prepare_domain_restores(&self.player, &state);
        let restore_player = Arc::clone(&self.player);
        self.player
            .reset_after_detached_domain_restore(Arc::clone(&state.world), || {
                Server::apply_domain_player_state(&restore_player, &state);
            });
        if !Server::install_domain_restores(&self.player, self.residence_token, &restores) {
            return self.finish_source_disconnect(Some(
                "domain switch lost its residence before target restore installation",
            ));
        }
        let pos = self.player.position();
        let rotation = self.player.rotation();
        if !self.player.spawn(pos, rotation, ResetReason::WorldChange) {
            let target_data = PersistentPlayerData::from_player(&self.player);
            self.source_data = None;
            server.queue_relocating_player_disconnect(
                Arc::clone(&self.player),
                self.target_domain.clone(),
                Arc::new(target_data),
                self.pending_token,
            );
            return JobPoll::Finished;
        }
        self.source_data = None;
        if !self.player.finish_pending_world_change(self.pending_token) {
            tracing::error!(
                player = %self.player.gameprofile.name,
                "Domain switch lost its relocation lease during target admission"
            );
            self.player.finish_domain_switch(self.pending_token);
            self.player.connection.close();
            server.release_player_admission(
                self.player.gameprofile.id,
                PlayerAdmissionState::Relocating,
            );
            server.queue_player_disconnect(Arc::clone(&self.player));
            return JobPoll::Finished;
        }
        if !self.player.mark_domain_switch_live(self.pending_token) {
            tracing::error!(
                player = %self.player.gameprofile.name,
                "Domain switch lost ownership after target insertion"
            );
            self.player.finish_domain_switch(self.pending_token);
            self.player.connection.close();
            server.release_player_admission(
                self.player.gameprofile.id,
                PlayerAdmissionState::Relocating,
            );
            server.queue_player_disconnect(Arc::clone(&self.player));
            return JobPoll::Finished;
        }
        server
            .release_player_admission(self.player.gameprofile.id, PlayerAdmissionState::Relocating);
        server.schedule_domain_restores(&self.player, self.residence_token, restores);

        let (sender, receiver) = mpsc::channel();
        let task_server = Arc::clone(server);
        let task_target_domain = self.target_domain.clone();
        let uuid = self.player.gameprofile.id;
        let task = tokio::spawn(async move {
            let result = task_server
                .player_data_storage
                .save_global(
                    uuid,
                    &GlobalPlayerData {
                        last_active_domain: task_target_domain,
                    },
                )
                .await
                .map_err(|error| format!("failed to save active domain: {error}"));
            let _ = sender.send(result);
        });
        self.phase = DomainSwitchJobPhase::SavingGlobal { receiver, task };
        JobPoll::Pending
    }
}

impl ServerJob for DomainSwitchJob {
    #[expect(
        clippy::too_many_lines,
        reason = "the domain transition phases stay together so every state transfer remains explicit"
    )]
    fn poll(&mut self, context: &mut ServerJobContext) -> JobPoll {
        if self.player.connection.closed() {
            return self.finish_source_disconnect(None);
        }
        if self.source_data.is_some()
            && (!self.player.is_domain_switch_detached(self.pending_token)
                || !self
                    .player
                    .is_world_change_token_pending(self.pending_token)
                || !self
                    .player
                    .is_domain_residence_current(self.residence_token))
        {
            return self.finish_source_disconnect(Some(
                "domain switch no longer owns the detached player",
            ));
        }
        let Some(server) = context.server() else {
            return self.finish_source_disconnect(None);
        };

        loop {
            let phase = mem::replace(&mut self.phase, DomainSwitchJobPhase::Transitioning);
            match phase {
                DomainSwitchJobPhase::WaitingForStorage { receiver, task } => {
                    let result = match receiver.try_recv() {
                        Ok(result) => result,
                        Err(mpsc::TryRecvError::Empty) => {
                            self.phase = DomainSwitchJobPhase::WaitingForStorage { receiver, task };
                            return JobPoll::Pending;
                        }
                        Err(mpsc::TryRecvError::Disconnected) => {
                            return self.finish_source_disconnect(Some(
                                "domain storage task ended without a result",
                            ));
                        }
                    };
                    drop(task);
                    let state = match result {
                        Ok(state) => state,
                        Err(error) => return self.finish_source_disconnect(Some(&error)),
                    };
                    self.phase =
                        match Self::phase_after_storage(&server, &self.target_domain, state) {
                            Ok(phase) => phase,
                            Err(error) => return self.finish_source_disconnect(Some(&error)),
                        };
                }
                DomainSwitchJobPhase::SearchingSpawn {
                    world,
                    data,
                    rotation,
                    mut search,
                } => match search.poll_with_ready_candidate_budget(
                    &world,
                    DOMAIN_SPAWN_SEARCH_READY_CANDIDATE_BUDGET,
                ) {
                    PlayerSpawnSearchPoll::Pending => {
                        self.phase = DomainSwitchJobPhase::SearchingSpawn {
                            world,
                            data,
                            rotation,
                            search,
                        };
                        return JobPoll::Pending;
                    }
                    PlayerSpawnSearchPoll::Cancelled => {
                        return self.finish_source_disconnect(Some(
                            "spawn search chunk request was cancelled",
                        ));
                    }
                    PlayerSpawnSearchPoll::Ready(position) => {
                        let state = match Self::state_after_spawn_search(
                            world,
                            data,
                            PreparedSpawn { position, rotation },
                        ) {
                            Ok(state) => state,
                            Err(error) => return self.finish_source_disconnect(Some(&error)),
                        };
                        self.phase = DomainSwitchJobPhase::LoadingSpawn { state };
                    }
                },
                DomainSwitchJobPhase::LoadingSpawn { state } => {
                    match state.spawn_chunk_request.poll() {
                        ChunkRequestState::Pending { .. } => {
                            self.phase = DomainSwitchJobPhase::LoadingSpawn { state };
                            return JobPoll::Pending;
                        }
                        ChunkRequestState::Cancelled => {
                            return self.finish_source_disconnect(Some(
                                "player spawn chunk request was cancelled",
                            ));
                        }
                        ChunkRequestState::Ready => {
                            if state.spawn_chunk_request.ready_chunks().is_none() {
                                self.phase = DomainSwitchJobPhase::LoadingSpawn { state };
                                return JobPoll::Pending;
                            }
                        }
                    }
                    return self.commit_target_state(&server, state);
                }
                DomainSwitchJobPhase::SavingGlobal { receiver, task } => {
                    let result = match receiver.try_recv() {
                        Ok(result) => result,
                        Err(mpsc::TryRecvError::Empty) => {
                            self.phase = DomainSwitchJobPhase::SavingGlobal { receiver, task };
                            return JobPoll::Pending;
                        }
                        Err(mpsc::TryRecvError::Disconnected) => {
                            log::error!(
                                "Active-domain save task for {} ended without a result",
                                self.player.gameprofile.name
                            );
                            self.player.finish_domain_switch(self.pending_token);
                            return JobPoll::Finished;
                        }
                    };
                    drop(task);
                    if let Err(error) = result {
                        log::error!(
                            "Failed to save global player data for {} after domain switch: {error}",
                            self.player.gameprofile.name
                        );
                    }
                    self.player.finish_domain_switch(self.pending_token);
                    return JobPoll::Finished;
                }
                DomainSwitchJobPhase::Transitioning => {
                    return self.finish_source_disconnect(Some(
                        "domain switch entered an invalid transition state",
                    ));
                }
            }
        }
    }

    fn cancel(&mut self) {
        self.abort_async_task();
        if self.source_data.is_some() {
            if !self.player.connection.closed() {
                self.player.connection.close();
            }
            let _ = self.finish_source_disconnect(None);
        } else {
            self.player.finish_domain_switch(self.pending_token);
            self.player.finish_pending_world_change(self.pending_token);
            if let Some(server) = self.server.upgrade() {
                server.release_player_admission(
                    self.player.gameprofile.id,
                    PlayerAdmissionState::Relocating,
                );
            }
        }
    }
}
