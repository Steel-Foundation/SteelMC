//! Temporary domains use real worlds and RAM persistence. Preparation is asynchronous;
//! publication and retirement run only from the server job stage between world ticks.
use super::{
    PlayerAdmissionState, Server,
    jobs::{JobPoll, ServerJob, ServerJobContext},
};
use crate::{chunk::chunk_request::ChunkRequestHandle, level_data::GameTimeSource, world::World};
use rustc_hash::FxHashMap;
use tokio::task::JoinHandle;
use tokio_util::task::TaskTracker;
mod preparation;
use preparation::PreparedWorld;
use std::sync::{Arc, Weak, mpsc};
use steel_utils::{
    BlockPos, Identifier,
    locks::SyncMutex,
    types::{Difficulty, GameType},
};

/// Identity of an asynchronous world operation, consumed when its result is read.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct WorldOperationId(pub u64);
/// A world prepared using a registered generator, with no disk-backed state.
pub struct TemporaryWorldRequest {
    /// Unique `domain:world` identity.
    pub key: Identifier,
    /// Whether this creates the domain's primary world.
    pub primary: bool,
    /// Registered world generator.
    pub generator: Identifier,
    /// Generator-specific configuration.
    pub config: toml::Value,
    /// World seed.
    pub seed: i64,
    /// Explicit spawn supplied by the map author.
    pub spawn: BlockPos,
    /// Game mode for first visits to this world.
    pub default_gamemode: GameType,
    /// Initial world difficulty.
    pub difficulty: Difficulty,
}
#[derive(Default)]
pub(super) struct TemporaryWorlds {
    state: SyncMutex<State>,
    pub(super) cleanup: TaskTracker,
}
#[derive(Default)]
struct State {
    next: u64,
    operations: FxHashMap<WorldOperationId, Option<Result<Identifier, String>>>,
    domains: FxHashMap<String, DomainState>,
}
struct DomainState {
    closing: bool,
    creating: usize,
    spawn_tickets: Vec<ChunkRequestHandle>,
}
impl TemporaryWorlds {
    fn allocate(state: &mut State) -> Result<WorldOperationId, String> {
        state.next = state
            .next
            .checked_add(1)
            .ok_or("world operation identities exhausted")?;
        let id = WorldOperationId(state.next);
        state.operations.insert(id, None);
        Ok(id)
    }
    fn finish(&self, id: WorldOperationId, result: Result<Identifier, String>) {
        if let Some(slot) = self.state.lock().operations.get_mut(&id) {
            *slot = Some(result);
        }
    }
    pub(super) fn is_closing(&self, domain: &str) -> bool {
        self.state
            .lock()
            .domains
            .get(domain)
            .is_some_and(|d| d.closing)
    }
}
impl Server {
    /// Starts preparing a RAM-only world. The operation publishes it at a tick boundary.
    pub fn create_temporary_world(
        self: &Arc<Self>,
        request: TemporaryWorldRequest,
    ) -> Result<WorldOperationId, String> {
        if !World::is_in_spawnable_bounds(request.spawn) {
            return Err("spawn is outside world bounds".into());
        }
        let domain = request.key.namespace.to_string();
        if request.primary && self.player_data_storage.is_temporary_domain(&domain) {
            return Err("temporary domain identity has already been used".into());
        }
        let source = self.overworld();
        let validated = self
            .world_generators
            .validate_config(&request.generator, &request.config)?;
        let dimension = validated.dimension_type();
        if !(dimension.min_y..dimension.min_y + dimension.height).contains(&request.spawn.y()) {
            return Err("spawn is outside the dimension build height".into());
        }
        let game_time_source = if request.primary {
            GameTimeSource::Primary
        } else {
            let primary = self
                .worlds
                .default_world(&domain)
                .ok_or("temporary domain is not loaded")?;
            GameTimeSource::Derived(Arc::clone(&primary.game_time))
        };
        let id = {
            let mut state = self.temporary_worlds.state.lock();
            if request.primary {
                if self.worlds.has_domain(&domain) || state.domains.contains_key(&domain) {
                    return Err("domain identity is already in use".into());
                }
                state.domains.insert(
                    domain.clone(),
                    DomainState {
                        closing: false,
                        creating: 1,
                        spawn_tickets: Vec::new(),
                    },
                );
            } else {
                let entry = state
                    .domains
                    .get_mut(&domain)
                    .ok_or("worlds can only be added to temporary domains")?;
                if entry.closing {
                    return Err("temporary domain is closing".into());
                }
                entry.creating += 1;
            }
            TemporaryWorlds::allocate(&mut state)?
        };
        let primary = request.primary;
        let (sender, receiver) = mpsc::channel();
        let server = Arc::clone(self);
        let task = self.temporary_worlds.cleanup.spawn(async move {
            let result =
                PreparedWorld::prepare(server, source, request, validated, game_time_source).await;
            let _ = sender.send(result);
        });
        self.jobs.spawn(CreateWorldJob {
            server: Arc::downgrade(self),
            id,
            domain,
            primary,
            receiver,
            task,
        });
        Ok(id)
    }
    /// Reads and consumes a completed operation. `None` means its job is still running.
    pub fn take_world_operation(
        &self,
        id: WorldOperationId,
    ) -> Result<Option<Result<Identifier, String>>, String> {
        let mut state = self.temporary_worlds.state.lock();
        let slot = state
            .operations
            .get_mut(&id)
            .ok_or("unknown or consumed world operation")?;
        if slot.is_none() {
            return Ok(None);
        }
        let result = slot.take();
        state.operations.remove(&id);
        Ok(result)
    }
    /// Retires a temporary domain once all players and domain transitions have left it.
    /// No new entries are accepted after this call.
    pub fn destroy_temporary_domain(
        self: &Arc<Self>,
        domain: &str,
    ) -> Result<WorldOperationId, String> {
        let id = {
            let mut state = self.temporary_worlds.state.lock();
            let entry = state
                .domains
                .get_mut(domain)
                .ok_or("domain is not temporary")?;
            if entry.closing {
                return Err("domain is already closing".into());
            }
            entry.closing = true;
            TemporaryWorlds::allocate(&mut state)?
        };
        self.jobs.spawn(DestroyDomainJob {
            server: Arc::downgrade(self),
            domain: domain.to_owned(),
            id,
        });
        Ok(id)
    }
}
struct CreateWorldJob {
    server: Weak<Server>,
    id: WorldOperationId,
    domain: String,
    primary: bool,
    receiver: mpsc::Receiver<Result<PreparedWorld, String>>,
    task: JoinHandle<()>,
}
impl ServerJob for CreateWorldJob {
    fn poll(&mut self, _: &mut ServerJobContext) -> JobPoll {
        let result = match self.receiver.try_recv() {
            Ok(result) => result,
            Err(mpsc::TryRecvError::Empty) => return JobPoll::Pending,
            Err(mpsc::TryRecvError::Disconnected) => {
                Err("temporary world preparation stopped".into())
            }
        };
        let Some(server) = self.server.upgrade() else {
            return JobPoll::Finished;
        };
        let result = result.and_then(|mut prepared| {
            let world = &prepared.world;
            if server.cancel_token.is_cancelled() {
                return Err("server is stopping".into());
            }
            if self.primary {
                server
                    .player_data_storage
                    .register_temporary_domain(&self.domain)
                    .map_err(|e| e.to_string())?;
                server.scoreboards.insert_temporary(&self.domain);
                server.command_storage.insert_temporary(&self.domain);
            }
            let key = world.key.clone();
            server
                .worlds
                .publish_temporary_world(Arc::clone(world), self.primary)?;
            if let Some(ticket) = prepared.spawn_ticket.take()
                && let Some(domain) = server
                    .temporary_worlds
                    .state
                    .lock()
                    .domains
                    .get_mut(&self.domain)
            {
                domain.spawn_tickets.push(ticket);
            }
            prepared.published = true;
            Ok(key)
        });
        {
            let mut state = server.temporary_worlds.state.lock();
            if let Some(domain) = state.domains.get_mut(&self.domain) {
                domain.creating -= 1;
            }
        }
        if result.is_err() && self.primary {
            server
                .temporary_worlds
                .state
                .lock()
                .domains
                .remove(&self.domain);
            server.scoreboards.remove_temporary(&self.domain);
            server.command_storage.remove_temporary(&self.domain);
            server
                .player_data_storage
                .retire_temporary_domain(&self.domain);
        }
        server.temporary_worlds.finish(self.id, result);
        JobPoll::Finished
    }
    fn cancel(&mut self) {
        self.task.abort();
        if let Some(server) = self.server.upgrade() {
            let mut state = server.temporary_worlds.state.lock();
            if self.primary {
                state.domains.remove(&self.domain);
            } else if let Some(domain) = state.domains.get_mut(&self.domain) {
                domain.creating -= 1;
            }
            drop(state);
            server
                .temporary_worlds
                .finish(self.id, Err("temporary world creation cancelled".into()));
        }
    }
}
struct DestroyDomainJob {
    server: Weak<Server>,
    domain: String,
    id: WorldOperationId,
}
impl ServerJob for DestroyDomainJob {
    fn poll(&mut self, _: &mut ServerJobContext) -> JobPoll {
        let Some(server) = self.server.upgrade() else {
            return JobPoll::Finished;
        };
        let creating = server
            .temporary_worlds
            .state
            .lock()
            .domains
            .get(&self.domain)
            .is_some_and(|d| d.creating != 0);
        if creating
            || server
                .get_players()
                .iter()
                .any(|p| p.get_world().domain() == self.domain)
            || server.player_admissions.lock().values().any(|state| {
                matches!(
                    state,
                    PlayerAdmissionState::Relocating | PlayerAdmissionState::Disconnecting
                )
            })
            || server
                .pending_domain_switches
                .lock()
                .iter()
                .any(|s| s.target_domain == self.domain)
        {
            return JobPoll::Pending;
        }
        let worlds = server.worlds.remove_temporary_domain(&self.domain);
        server.scoreboards.remove_temporary(&self.domain);
        server.command_storage.remove_temporary(&self.domain);
        server
            .player_data_storage
            .retire_temporary_domain(&self.domain);
        server
            .temporary_worlds
            .state
            .lock()
            .domains
            .remove(&self.domain);
        let id = self.id;
        let domain = self.domain.clone();
        server.temporary_worlds.cleanup.clone().spawn(async move {
            for world in worlds {
                world.chunk_map.stop_generation_refill_loop();
                world.chunk_map.task_tracker.close();
                world.chunk_map.task_tracker.wait().await;
                // The entire RAM world is discarded; serializing chunks would only retain it longer.
            }
            server
                .temporary_worlds
                .finish(id, Ok(Identifier::new(domain, "retired")));
        });
        JobPoll::Finished
    }
    fn cancel(&mut self) {
        if let Some(server) = self.server.upgrade() {
            if let Some(domain) = server
                .temporary_worlds
                .state
                .lock()
                .domains
                .get_mut(&self.domain)
            {
                domain.closing = false;
            }
            server
                .temporary_worlds
                .finish(self.id, Err("temporary domain retirement cancelled".into()));
        }
    }
}

#[cfg(test)]
mod tests;
