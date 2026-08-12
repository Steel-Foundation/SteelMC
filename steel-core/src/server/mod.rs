//! This module contains the `Server` struct, which is the main entry point for the server.
mod broadcasting;
/// Tick-polled server jobs.
pub mod jobs;
mod packet_processor;
#[cfg(not(feature = "benchmark-support"))]
mod pools;
#[cfg(feature = "benchmark-support")]
pub mod pools;
#[cfg(not(feature = "benchmark-support"))]
mod pregen;
#[cfg(feature = "benchmark-support")]
pub mod pregen;
/// The registry cache for the server.
pub mod registry_cache;
mod run_loop;
/// The tick rate manager for the server.
pub mod tick_rate_manager;
mod world_tick_workers;
/// Domain-aware loaded world map.
pub mod worlds;

use crate::bootstrap::init_globals;
use crate::chunk::{
    chunk_request::{ChunkRequest, ChunkRequestHandle, ChunkRequestState, ChunkTicketKind},
    status::ChunkStatus,
};
use crate::command::brigadier::{StringReader, SuggestionError, Suggestions};
use crate::command::execution::{
    CommandExecutionContext, CommandResultCallback, CommandSource, ExecutionCommandSource,
    ExecutionStop,
};
use crate::command::sender::{CommandExecutionOwner, CommandSender};
use crate::command::storage::DomainCommandStorage;
use crate::command::{
    COMMAND_REQUESTS_PER_TICK, COMMAND_RESUMPTIONS_PER_TICK, CommandCompletion, CommandDispatcher,
    CommandQueueFull, CommandRegistry, CommandRequest, CommandRequestQueue,
    PendingCommandExecutionQueue, client_permission_event, command_suggestions_packet,
    command_tree_packet, create_registered_dispatcher,
};
use crate::config::{ResolvedWorldConfig, RuntimeConfig, WorldsConfig, validate_login_security};
use crate::entity::{
    Entity, EntityBase, PendingWorldChangeToken, RemovalReason, SharedEntity, change_entity_world,
};
use crate::server::pools::{build_chunk_encoding_pool, build_generation_pool};

use crate::chunk_saver::{ChunkStorage, PersistentEntity, registry::WorldStorageRegistry};
use crate::level_data::{LevelDataManager, RespawnData, WorldGenerationSettings};
use crate::permission::{
    OP_GROUP, PermissionGroupManager, PermissionGroupManagerError, PermissionGroupUpdateError,
    PermissionGroupsConfig, PermissionMetadataExpression, PermissionRuleExpression, PermissionSet,
    PermissionSubjectIndex, PermissionSubjectState,
};
use crate::player::chunk_sender::{ChunkSender, EncodedChunk};
use crate::player::connection::NetworkConnection;
use crate::player::connection::ScheduledPlayPacket;
use crate::player::player_data::{
    PersistentEnderPearl, PersistentPlayerData, PersistentRootVehicle,
};
use crate::player::player_data_storage::{GlobalPlayerData, PlayerDataStorage};
use crate::player::player_inventory::MenuRemovalStatus;
use crate::player::{
    DomainResidenceToken, GameProfile, KnownPlayer, KnownPlayerNameLookup, KnownPlayers, Player,
    ProfileLookupError, ResetReason, is_valid_player_name, lookup_online_profile, offline_uuid,
};
use crate::portal::{
    PortalKind, TeleportPostTransition, TeleportTransition, WorldChangeRequest, end_gateway,
    end_portal, nether_portal,
};
use crate::scoreboard::DomainScoreboards;
use crate::server::jobs::{FnServerJob, ServerJobContext, ServerJobQueue};
use crate::server::packet_processor::PacketProcessor;
use crate::server::registry_cache::RegistryCache;
use crate::server::worlds::WorldMap;
use crate::world::player_spawn_finder::{PlayerSpawnSearch, PlayerSpawnSearchPoll};
use crate::world::{PlayerMap, World, WorldConfig};
use crate::worldgen::WorldGeneratorRegistry;
use crate::worldgen::registry::GeneratorOutput;
use crossbeam::queue::SegQueue;
use glam::DVec3;
use rayon::ThreadPool;
use rustc_hash::FxHashMap;
use std::{
    collections::BTreeSet,
    io, mem,
    num::NonZero,
    path::Path,
    sync::{Arc, mpsc},
    thread,
    time::{Duration, Instant},
};
use steel_crypto::key_store::KeyStore;
use steel_protocol::packet_traits::{ClientPacket, EncodedPacket};
use steel_protocol::packets::game::{
    CCommandSuggestions, CEntityEvent, CLogin, CPlayerInfoUpdate, CRemovePlayerInfo,
    CSetDefaultSpawnPosition, CSystemChat, CTabList, CTickingState, CTickingStep,
    CommonPlayerSpawnInfo, RelativeMovement,
};
use steel_protocol::utils::ConnectionProtocol;
use steel_registry::vanilla_game_rules::{
    ALLOW_ENTERING_NETHER_USING_PORTALS, IMMEDIATE_RESPAWN, LIMITED_CRAFTING, REDUCED_DEBUG_INFO,
};
use steel_registry::{
    RegistryEntry, dimension_type::DimensionTypeRef, vanilla_dimension_types, vanilla_entities,
};
use steel_utils::{
    BlockPos, ChunkPos, Identifier,
    locks::{AsyncMutex, SyncMutex, SyncRwLock},
    text::DisplayResolutor,
    translations,
};
use text_components::{Modifier, TextComponent, format::Color};
use tick_rate_manager::{SprintReport, TickRateManager};
use tokio::{
    runtime::Runtime,
    sync::Notify,
    task::{JoinSet, spawn_blocking},
    time::sleep,
};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

/// Interval in ticks between tab list updates (20 ticks = 1 second).
const TAB_LIST_UPDATE_INTERVAL: u64 = 20;
/// Interval in ticks between player info broadcasts (600 ticks = 30 seconds).
/// Matches vanilla `PlayerList.SEND_PLAYER_INFO_INTERVAL`.
const SEND_PLAYER_INFO_INTERVAL: u64 = 600;
/// Wall-clock interval between saves of command-owned persistent server data.
/// Matches vanilla's intended five-minute autosave cadence.
const COMMAND_DATA_AUTOSAVE_INTERVAL: Duration = Duration::from_secs(300);

#[derive(Clone, Copy)]
struct TabListTickStats {
    tps: f32,
    recent_mspt: f32,
    average_mspt: f32,
    p95_mspt: f32,
}

impl TabListTickStats {
    fn capture(tick_manager: &TickRateManager) -> Self {
        Self {
            tps: tick_manager.get_tps(),
            recent_mspt: tick_manager.get_smoothed_mspt(),
            average_mspt: tick_manager.get_average_mspt(),
            p95_mspt: tick_manager.get_p95(),
        }
    }
}

/// Results from saving every command-owned persistent data set.
pub struct CommandDataSaveResults {
    /// Number of dirty domain scoreboards written, or the save error.
    pub scoreboards: io::Result<usize>,
    /// Number of dirty domain command-storage values written, or the save error.
    pub storage: io::Result<usize>,
}

mod known_players;

use known_players::KnownPlayerCacheState;

/// Tick rate for the chunk sending loop.
const CHUNK_SENDING_TPS: u64 = 20;

/// Work duration at which background chunk work is considered slow.
const SLOW_CHUNK_TICK_THRESHOLD: Duration = Duration::from_millis(50);

fn configured_packet_workers(configured_workers: Option<usize>) -> usize {
    packet_workers_for_available(configured_workers, available_worker_threads())
}

pub(crate) fn available_worker_threads() -> usize {
    thread::available_parallelism().map_or(4, NonZero::get)
}

pub(crate) fn cap_positive_thread_count(
    configured_threads: Option<usize>,
    available_threads: usize,
) -> Option<usize> {
    let configured_threads = configured_threads.filter(|&threads| threads > 0)?;
    Some(configured_threads.min(available_threads.max(1)))
}

fn packet_workers_for_available(
    configured_workers: Option<usize>,
    available_threads: usize,
) -> usize {
    let available_threads = available_threads.max(1);
    if let Some(configured_workers) = configured_workers.filter(|&workers| workers > 0) {
        return configured_workers.min(available_threads);
    }

    ((available_threads / 2).max(2)).min(available_threads)
}

#[cfg(test)]
mod tests;

#[derive(Clone, Copy)]
struct PreparedSpawn {
    position: DVec3,
    rotation: (f32, f32),
}

fn apply_default_spawn(player: &Arc<Player>, world: &Arc<World>, spawn: PreparedSpawn) {
    player.base().set_position_local(spawn.position);
    player.set_rotation(spawn.rotation);
    player.restore_game_modes(world.default_gamemode, None);
    player
        .abilities
        .lock()
        .update_for_game_mode(world.default_gamemode);
}

fn is_allowed_to_enter_portal(source_world: &World, target_world: &World) -> bool {
    is_allowed_to_enter_portal_target(
        is_nether_dimension_type(target_world),
        source_world.get_game_rule(&ALLOW_ENTERING_NETHER_USING_PORTALS),
    )
}

const fn is_allowed_to_enter_portal_target(
    target_is_nether: bool,
    allow_entering_nether_using_portals: bool,
) -> bool {
    if !target_is_nether {
        return true;
    }

    allow_entering_nether_using_portals
}

fn can_teleport_between_worlds(
    entity: &dyn Entity,
    source_world: &World,
    target_world: &World,
    projectile_owner_seen_credits: impl Fn(&uuid::Uuid) -> Option<bool>,
) -> bool {
    if is_end_return_transition(source_world.dimension_type, target_world.dimension_type) {
        return can_entity_return_from_end_to_overworld(entity, projectile_owner_seen_credits);
    }

    true
}

fn is_end_return_transition(
    source_dimension_type: DimensionTypeRef,
    target_dimension_type: DimensionTypeRef,
) -> bool {
    source_dimension_type == &vanilla_dimension_types::THE_END
        && target_dimension_type == &vanilla_dimension_types::OVERWORLD
}

fn is_nether_dimension_type(world: &World) -> bool {
    world.dimension_type == &vanilla_dimension_types::THE_NETHER
}

fn is_end_dimension_type(world: &World) -> bool {
    world.dimension_type == &vanilla_dimension_types::THE_END
}

fn can_entity_return_from_end_to_overworld(
    entity: &dyn Entity,
    projectile_owner_seen_credits: impl Fn(&uuid::Uuid) -> Option<bool>,
) -> bool {
    if entity.entity_type() == &vanilla_entities::ENDER_PEARL
        && entity
            .projectile_owner_uuid()
            .and_then(|uuid| projectile_owner_seen_credits(&uuid))
            == Some(false)
    {
        return false;
    }

    direct_passengers_allow_end_return(entity)
}

fn direct_passengers_allow_end_return(entity: &dyn Entity) -> bool {
    for passenger in entity.passengers() {
        if passenger
            .as_player()
            .is_some_and(|player| !player.has_seen_credits())
        {
            return false;
        }
    }

    true
}

fn local_respawn_data_for_world(world: &World) -> RespawnData {
    let level_data = world.level_data.read();
    let data = level_data.data();
    RespawnData::of(world.key.clone(), data.spawn_pos(), data.spawn.angle, 0.0)
}

fn generation_settings_for_world(
    world_entry: &ResolvedWorldConfig,
    generator_output: &GeneratorOutput,
) -> WorldGenerationSettings {
    WorldGenerationSettings::from_generator_output(
        world_entry.generator_config.generator().clone(),
        generator_output,
    )
}

fn world_config_registries() -> Result<(WorldGeneratorRegistry, WorldStorageRegistry), String> {
    let generator_registry = WorldGeneratorRegistry::new_with_builtins()
        .map_err(|e| format!("failed to initialize world generator registry: {e}"))?;
    let storage_registry = WorldStorageRegistry::new_with_builtins()
        .map_err(|e| format!("failed to initialize world storage registry: {e}"))?;
    Ok((generator_registry, storage_registry))
}

struct DomainPlayerState {
    world: Arc<World>,
    data: DomainPlayerData,
    spawn_chunk_request: ChunkRequestHandle,
}

struct UnpreparedDomainPlayerState {
    world: Arc<World>,
    explicit_target: bool,
    data: UnpreparedDomainPlayerData,
}

enum UnpreparedDomainPlayerData {
    SavedRestored { data: Box<PersistentPlayerData> },
    SavedWithoutLocation { data: Box<PersistentPlayerData> },
    FirstVisit,
}

enum DomainPlayerData {
    SavedRestored {
        data: Box<PersistentPlayerData>,
    },
    SavedWithoutLocation {
        data: Box<PersistentPlayerData>,
        spawn: PreparedSpawn,
    },
    FirstVisit {
        spawn: PreparedSpawn,
    },
}

struct DomainSwitchRequest {
    player: Arc<Player>,
    target_domain: String,
    target_world: Option<Arc<World>>,
    pending_token: PendingWorldChangeToken,
}

/// Failure while atomically editing one player's persisted permission state.
#[derive(Debug, thiserror::Error)]
pub enum PlayerPermissionUpdateError<E> {
    /// The caller rejected the proposed edit.
    #[error("{0}")]
    Edit(E),
    /// The edit assigns a group that is not configured.
    #[error("unknown permission group '{0}'")]
    UnknownGroup(String),
    /// The permission snapshot could not be persisted.
    #[error("failed to update player permissions: {0}")]
    Storage(io::Error),
}

impl<E> From<io::Error> for PlayerPermissionUpdateError<E> {
    fn from(value: io::Error) -> Self {
        Self::Storage(value)
    }
}

mod permissions;

#[cfg(test)]
use permissions::validate_player_permission_group_update;

mod player_admission;
mod player_lifecycle;

use player_admission::{PlayerAdmissionState, PlayerDisconnectQueue, PlayerJoinQueue};

mod world_changes;

use jobs::domain_switch::DomainSwitchJob;
use jobs::teleport::{
    EndGatewayTeleportJob, EndPortalTeleportJob, EnderPearlRestoreJob, NetherPortalTeleportJob,
    RootVehicleRestoreJob, WorldSpawnTeleportJob, clear_pending_world_change,
    portal_entity_still_valid,
};

/// The main server struct.
pub struct Server {
    /// Runtime configuration (view distance, compression, etc.).
    pub config: Arc<RuntimeConfig>,
    /// Runtime permission groups and their persistence boundary.
    pub permission_groups: PermissionGroupManager,
    /// The cancellation token for graceful shutdown.
    pub cancel_token: CancellationToken,
    /// The key store for the server.
    pub key_store: KeyStore,
    /// The registry cache for the server.
    pub registry_cache: RegistryCache,
    /// A list of all the worlds on the server.
    pub worlds: WorldMap,
    /// Players currently connected to the server, independent of world membership.
    online_players: PlayerMap,
    /// UUIDs reserved by a join or disconnect/save lifecycle transition.
    player_admissions: SyncMutex<FxHashMap<Uuid, PlayerAdmissionState>>,
    /// The tick rate manager for the server.
    pub tick_rate_manager: SyncRwLock<TickRateManager>,
    /// Command scoreboards isolated by Steel domain.
    pub scoreboards: DomainScoreboards,
    /// Command NBT storage isolated by Steel domain.
    pub(crate) command_storage: DomainCommandStorage,
    /// Saves and dispatches commands to appropriate handlers.
    command_dispatcher: SyncRwLock<CommandDispatcher>,
    /// Steel-owned permission keys exposed for command autocomplete.
    command_permission_keys: Vec<String>,
    /// Command work submitted from connection and console tasks.
    command_requests: CommandRequestQueue,
    /// Decoded serverbound play packets handled during the inter-tick phase.
    packet_processor: PacketProcessor,
    /// Dedicated worker pool for CPU-heavy chunk persistence and packet encoding.
    chunk_encoding_pool: Arc<ThreadPool>,
    /// Jobs resumed from a known point in the server game tick.
    pub jobs: ServerJobQueue,
    /// Player data storage for saving/loading player state.
    pub player_data_storage: PlayerDataStorage,
    /// Persisted permission state indexed by player UUID.
    player_permission_states: SyncRwLock<PermissionSubjectIndex>,
    /// Serializes persistence and cache publication for player permission edits.
    player_permission_updates: AsyncMutex<()>,
    /// Player identities and coalesced persistence state.
    known_players: SyncMutex<KnownPlayerCacheState>,
    /// Wakes shutdown when the single known-player save worker becomes idle.
    known_player_save_idle: Notify,
    /// HTTP client used by online-mode name-to-profile lookups.
    profile_lookup_client: reqwest::Client,
    /// Player joins prepared by async I/O and finalized at the game tick safe point.
    pending_player_joins: PlayerJoinQueue,
    /// Disconnected players waiting to be detached at the next game tick safe point.
    pending_player_disconnects: PlayerDisconnectQueue,
    /// Queued world changes to process after the tick.
    pub pending_world_changes: SyncMutex<Vec<(SharedEntity, WorldChangeRequest)>>,
    /// Queued domain switches to process after world ticks.
    pending_domain_switches: SyncMutex<Vec<DomainSwitchRequest>>,
}

struct GameTickTaskGuard {
    server: Arc<Server>,
    cancel_token: CancellationToken,
}

impl GameTickTaskGuard {
    const fn new(server: Arc<Server>, cancel_token: CancellationToken) -> Self {
        Self {
            server,
            cancel_token,
        }
    }
}

impl Drop for GameTickTaskGuard {
    fn drop(&mut self) {
        self.server.packet_processor.stop();
        self.cancel_token.cancel();
    }
}

impl Server {
    pub(crate) fn permission_rule_suggestions(&self) -> Vec<String> {
        let mut suggestions = self
            .command_permission_keys
            .iter()
            .cloned()
            .collect::<BTreeSet<_>>();
        let config = self.permission_groups.config_snapshot();
        for group in config.groups.values() {
            suggestions.extend(group.allow.iter().cloned());
            suggestions.extend(group.deny.iter().cloned());
        }
        for (_, state) in self.player_permission_states.read().entries() {
            suggestions.extend(state.overrides().entries().iter().map(|entry| {
                PermissionRuleExpression::new(entry.key().clone(), entry.context().clone())
                    .to_string()
            }));
        }
        suggestions.into_iter().collect()
    }

    pub(crate) fn permission_metadata_suggestions(&self) -> Vec<String> {
        let mut suggestions = BTreeSet::new();
        let config = self.permission_groups.config_snapshot();
        for group in config.groups.values() {
            suggestions.extend(group.metadata.iter().map(|rule| rule.key.clone()));
        }
        for (_, state) in self.player_permission_states.read().entries() {
            suggestions.extend(state.metadata_overrides().entries().iter().map(|entry| {
                PermissionMetadataExpression::new(entry.key().clone(), entry.context().clone())
                    .to_string()
            }));
        }
        suggestions.into_iter().collect()
    }

    /// Creates a new server with only Steel's built-in commands.
    pub async fn new(
        chunk_runtime: Arc<Runtime>,
        cancel_token: CancellationToken,
        config: RuntimeConfig,
        worlds_config: WorldsConfig,
        permission_groups: PermissionGroupManager,
    ) -> Result<Self, String> {
        Self::new_with_commands(
            chunk_runtime,
            cancel_token,
            config,
            worlds_config,
            permission_groups,
            CommandRegistry::new(),
        )
        .await
    }

    /// Creates a new server and atomically merges startup command extensions after built-ins.
    #[expect(
        clippy::too_many_lines,
        reason = "server initialization is a single cohesive flow"
    )]
    pub async fn new_with_commands(
        chunk_runtime: Arc<Runtime>,
        cancel_token: CancellationToken,
        config: RuntimeConfig,
        worlds_config: WorldsConfig,
        permission_groups: PermissionGroupManager,
        command_registry: CommandRegistry,
    ) -> Result<Self, String> {
        validate_login_security(config.online_mode, config.encryption).map_err(str::to_owned)?;
        let config = Arc::new(config);
        init_globals()?;
        log::info!(
            "SteelMC is not affiliated with Mojang or Microsoft. Use is subject to the Minecraft EULA: https://aka.ms/MinecraftEULA"
        );

        let registry_cache = RegistryCache::new(config.compression);

        let (generator_registry, storage_registry) = world_config_registries()?;
        let resolved_worlds = worlds_config
            .validate_and_resolve(&generator_registry, &storage_registry)
            .map_err(|e| format!("failed to validate worlds.toml: {e}"))?;

        let generation_pool = build_generation_pool(config.chunk_generation_threads)?;
        let chunk_encoding_pool = build_chunk_encoding_pool(config.chunk_encoding_threads)?;

        let player_data_storage = PlayerDataStorage::new(
            resolved_worlds.save_path.clone(),
            resolved_worlds.player_storage.clone(),
        )
        .await
        .map_err(|e| format!("failed to create player data storage: {e}"))?;
        let player_permission_states = player_data_storage
            .load_permission_subjects()
            .await
            .map_err(|error| format!("failed to load player permissions: {error}"))?;
        let known_players = player_data_storage
            .load_known_players()
            .await
            .map_err(|error| format!("failed to load known players: {error}"))?;
        let mut worlds = WorldMap::new(
            resolved_worlds.default_domain.clone(),
            &resolved_worlds.domains,
            &resolved_worlds.worlds,
        );

        for world_entry in &resolved_worlds.worlds {
            let default_world_path = resolved_worlds
                .save_path
                .join(&world_entry.domain)
                .join("worlds")
                .join(&world_entry.name);
            let storage_output = storage_registry
                .create(
                    &world_entry.storage,
                    &resolved_worlds.save_path,
                    Path::new(&default_world_path),
                )
                .map_err(|e| format!("failed to create storage for {}: {e}", world_entry.key))?;
            let world_seed = LevelDataManager::load_seed_or_default(
                storage_output.level_data_path.as_deref(),
                world_entry.seed,
            )
            .await
            .map_err(|e| {
                format!(
                    "failed to load level data seed for {}: {e}",
                    world_entry.key
                )
            })?;
            let generator_output = generator_registry
                .create(
                    storage_output.level_data_path.as_deref(),
                    &world_entry.generator_config,
                    world_seed,
                    generation_pool.clone(),
                )
                .map_err(|e| format!("failed to create generator for {}: {e}", world_entry.key))?;
            let generation_settings = generation_settings_for_world(world_entry, &generator_output);
            let world = World::new_with_config_and_encoding_pool(
                chunk_runtime.clone(),
                world_entry.key.clone(),
                generator_output.dimension_type,
                world_seed,
                WorldConfig {
                    storage: storage_output.storage,
                    level_data_path: storage_output
                        .level_data_path
                        .map(|path| path.to_string_lossy().into_owned()),
                    generator: Arc::new(generator_output.generator),
                    generation_settings,
                    view_distance: config.view_distance,
                    simulation_distance: config.simulation_distance,
                    max_chained_neighbor_updates: config.max_chained_neighbor_updates,
                    compression: config.compression,
                    is_flat: generator_output.is_flat,
                    sea_level: generator_output.sea_level,
                    default_gamemode: world_entry.default_gamemode,
                    difficulty: world_entry.difficulty,
                },
                generation_pool.clone(),
                Arc::clone(&chunk_encoding_pool),
            )
            .await
            .map_err(|e| format!("failed to create world {}: {e}", world_entry.key))?;
            world
                .initialize_spawn_if_needed()
                .await
                .map_err(|e| format!("failed to initialize spawn for {}: {e}", world_entry.key))?;
            worlds.insert(world_entry.key.clone(), world);
        }

        let scoreboards = DomainScoreboards::load(&worlds)
            .await
            .map_err(|error| format!("failed to load domain scoreboards: {error}"))?;
        let command_storage = DomainCommandStorage::load(&worlds)
            .await
            .map_err(|error| format!("failed to load domain command storage: {error}"))?;
        let registered_commands = create_registered_dispatcher(command_registry)
            .map_err(|error| format!("failed to register commands: {error}"))?;
        let command_permission_keys = registered_commands
            .permissions
            .into_iter()
            .map(|permission| permission.as_str().to_owned())
            .collect();

        Ok(Server {
            config,
            permission_groups,
            cancel_token,
            key_store: KeyStore::create(),
            worlds,
            online_players: PlayerMap::new(),
            player_admissions: SyncMutex::new(FxHashMap::default()),
            registry_cache,
            tick_rate_manager: SyncRwLock::new(TickRateManager::new()),
            scoreboards,
            command_storage,
            command_dispatcher: SyncRwLock::new(registered_commands.dispatcher),
            command_permission_keys,
            command_requests: CommandRequestQueue::new(),
            packet_processor: PacketProcessor::new(),
            chunk_encoding_pool,
            jobs: ServerJobQueue::new(),
            player_data_storage,
            player_permission_states: SyncRwLock::new(player_permission_states),
            player_permission_updates: AsyncMutex::new(()),
            known_players: SyncMutex::new(KnownPlayerCacheState::new(known_players)),
            known_player_save_idle: Notify::new(),
            profile_lookup_client: reqwest::Client::new(),
            pending_player_joins: PlayerJoinQueue::new(),
            pending_player_disconnects: PlayerDisconnectQueue::new(),
            pending_world_changes: SyncMutex::new(vec![]),
            pending_domain_switches: SyncMutex::new(vec![]),
        })
    }

    /// Saves all dirty domain command storage through domain default worlds.
    pub async fn save_command_storage(&self) -> io::Result<usize> {
        self.command_storage.save(&self.worlds).await
    }

    /// Saves all command-owned persistent data while allowing each data set to fail independently.
    pub async fn save_command_data(&self) -> CommandDataSaveResults {
        CommandDataSaveResults {
            scoreboards: self.scoreboards.save(&self.worlds).await,
            storage: self.save_command_storage().await,
        }
    }

    /// Queues a command for execution at the start of the next game tick.
    pub fn submit_command(
        &self,
        sender: CommandSender,
        command: String,
    ) -> Result<(), CommandQueueFull> {
        self.command_requests.submit(CommandRequest::Execute {
            owner: CommandExecutionOwner::capture(sender, self),
            command,
        })
    }

    pub(crate) fn submit_command_suggestions(
        &self,
        player: Arc<Player>,
        transaction_id: i32,
        input: String,
    ) -> Result<(), CommandQueueFull> {
        self.command_requests.submit(CommandRequest::Suggestions {
            owner: CommandExecutionOwner::capture(CommandSender::Player(player), self),
            transaction_id,
            input,
        })
    }

    /// Schedules a decoded play packet for the inter-tick packet phase.
    pub(crate) fn schedule_play_packet(
        &self,
        player: Arc<Player>,
        packet: ScheduledPlayPacket,
        payload_bytes: usize,
    ) {
        self.packet_processor
            .schedule(player, packet, payload_bytes);
    }

    /// Returns Brigadier completions visible to a command sender.
    pub fn command_completions(
        self: &Arc<Self>,
        sender: CommandSender,
        input: &str,
    ) -> Vec<CommandCompletion> {
        if !CommandExecutionOwner::capture(sender.clone(), self).is_current(self) {
            return Vec::new();
        }
        match self.build_command_suggestions(sender, input) {
            Ok(suggestions) => {
                let range = suggestions.range();
                suggestions
                    .list()
                    .iter()
                    .map(|suggestion| {
                        CommandCompletion::new(
                            range.start(),
                            range.len(),
                            suggestion.text().to_owned(),
                        )
                    })
                    .collect()
            }
            Err(error) => {
                tracing::warn!(%error, "failed to build command suggestions");
                Vec::new()
            }
        }
    }
}
