//! This module contains the `Server` struct, which is the main entry point for the server.
/// The registry cache for the server.
pub mod registry_cache;
/// The tick rate manager for the server.
pub mod tick_rate_manager;
/// Domain-aware loaded world map.
pub mod worlds;

use crate::behavior::init_behaviors;
use crate::block_entity::init_block_entities;
use crate::command::CommandDispatcher;
use crate::config::{ResolvedWorldConfig, RuntimeConfig, WorldsConfig};
use crate::entity::{SharedEntity, init_entities};

use crate::chunk_saver::registry::WorldStorageRegistry;
use crate::level_data::WorldGenerationSettings;
use crate::player::chunk_sender::ChunkSender;
use crate::player::connection::NetworkConnection;
use crate::player::player_data::PersistentPlayerData;
use crate::player::player_data_storage::{GlobalPlayerData, PlayerDataStorage};
use crate::player::{Player, ResetReason};
use crate::portal::{TeleportTransition, WorldChangeRequest};
use crate::server::registry_cache::RegistryCache;
use crate::server::worlds::WorldMap;
use crate::world::{World, WorldConfig, WorldGameTickTimings};
use crate::worldgen::WorldGeneratorRegistry;
use crate::worldgen::registry::GeneratorOutput;
use glam::DVec3;
use rayon::{ThreadPool, ThreadPoolBuilder};
use std::{
    mem,
    path::Path,
    sync::Arc,
    time::{Duration, Instant},
};
use steel_crypto::key_store::KeyStore;
use steel_protocol::packet_traits::EncodedPacket;
use steel_protocol::packets::game::{
    CEntityEvent, CGameEvent, CLogin, CRemovePlayerInfo, CSystemChat, CTabList, CTickingState,
    CTickingStep, CommonPlayerSpawnInfo, GameEventType,
};
use steel_registry::game_rules::GameRuleValue;
use steel_registry::vanilla_game_rules::{IMMEDIATE_RESPAWN, LIMITED_CRAFTING, REDUCED_DEBUG_INFO};
use steel_registry::{REGISTRY, Registry, RegistryEntry};
use steel_utils::locks::SyncMutex;
use steel_utils::{ChunkPos, Identifier, entity_events::EntityStatus, locks::SyncRwLock};
use text_components::{Modifier, TextComponent, format::Color};
use tick_rate_manager::{SprintReport, TickRateManager};
use tokio::{runtime::Runtime, task::spawn_blocking, time::sleep};
use tokio_util::sync::CancellationToken;

/// Interval in ticks between tab list updates (20 ticks = 1 second).
const TAB_LIST_UPDATE_INTERVAL: u64 = 20;

/// Tick rate for the chunk sending loop.
const CHUNK_SENDING_TPS: u64 = 20;

/// Tick rate for the chunk scheduling loop.
const CHUNK_SCHEDULING_TPS: u64 = 20;

fn apply_first_visit_defaults(player: &Arc<Player>, world: &Arc<World>) {
    let spawn = world.level_data.read().data().spawn.clone();
    *player.position.lock() =
        DVec3::new(f64::from(spawn.x), f64::from(spawn.y), f64::from(spawn.z));
    player.rotation.store((spawn.angle, 0.0));
    player.game_mode.store(world.default_gamemode);
    player.prev_game_mode.store(world.default_gamemode);
    player
        .abilities
        .lock()
        .update_for_game_mode(world.default_gamemode);
}

fn world_spawn_transition(world: Arc<World>) -> TeleportTransition {
    let spawn = world.level_data.read().data().spawn.clone();
    TeleportTransition {
        target_world: world,
        position: DVec3::new(
            f64::from(spawn.x) + 0.5,
            f64::from(spawn.y),
            f64::from(spawn.z) + 0.5,
        ),
        rotation: (spawn.angle, 0.0),
        portal_cooldown: 0,
    }
}

fn generation_settings_for_world(
    world_entry: &ResolvedWorldConfig,
    generator_output: &GeneratorOutput,
) -> WorldGenerationSettings {
    WorldGenerationSettings::from_generator_config(
        world_entry.generator.clone(),
        &generator_output.config,
        generator_output.dimension_type.key.clone(),
        generator_output.dimension_type.min_y,
        generator_output.dimension_type.height,
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
}

enum DomainPlayerData {
    Saved {
        data: PersistentPlayerData,
        restore_location: bool,
    },
    FirstVisit,
}

struct DomainSwitchRequest {
    player: Arc<Player>,
    target_domain: String,
    target_world: Option<Arc<World>>,
    restore_saved_location: bool,
}

/// The main server struct.
pub struct Server {
    /// Runtime configuration (view distance, compression, etc.).
    pub config: Arc<RuntimeConfig>,
    /// The cancellation token for graceful shutdown.
    pub cancel_token: CancellationToken,
    /// The key store for the server.
    pub key_store: KeyStore,
    /// The registry cache for the server.
    pub registry_cache: RegistryCache,
    /// A list of all the worlds on the server.
    pub worlds: WorldMap,
    /// The tick rate manager for the server.
    pub tick_rate_manager: SyncRwLock<TickRateManager>,
    /// Saves and dispatches commands to appropriate handlers.
    pub command_dispatcher: SyncRwLock<CommandDispatcher>,
    /// Player data storage for saving/loading player state.
    pub player_data_storage: PlayerDataStorage,
    /// Queued world changes to process after the tick.
    pub pending_world_changes: SyncMutex<Vec<(SharedEntity, WorldChangeRequest)>>,
    /// Queued domain switches to process after world ticks.
    pending_domain_switches: SyncMutex<Vec<DomainSwitchRequest>>,
}

impl Server {
    /// Creates a new server.
    ///
    #[expect(
        clippy::too_many_lines,
        reason = "server initialization is a single cohesive flow"
    )]
    pub async fn new(
        chunk_runtime: Arc<Runtime>,
        cancel_token: CancellationToken,
        config: RuntimeConfig,
        worlds_config: WorldsConfig,
    ) -> Result<Self, String> {
        let config = Arc::new(config);
        let start = Instant::now();
        let mut registry = Registry::new_vanilla();
        registry.freeze();
        log::info!("Vanilla registry loaded in {:?}", start.elapsed());

        if REGISTRY.init(registry).is_err() {
            return Err("global registry has already been initialized".to_owned());
        }

        // Initialize behavior registries after the main registry is frozen
        init_behaviors();
        init_block_entities();
        init_entities();
        log::info!("Behavior registries initialized");

        let registry_cache = RegistryCache::new(config.compression);

        let (generator_registry, storage_registry) = world_config_registries()?;
        let resolved_worlds = worlds_config
            .validate_and_resolve(&generator_registry, &storage_registry)
            .map_err(|e| format!("failed to validate worlds.toml: {e}"))?;

        let generation_pool: Arc<ThreadPool> = Arc::new({
            let mut builder = ThreadPoolBuilder::new().thread_name(|i| format!("rayon-gen-{i}"));
            // Debug builds have deep call chains in density functions that overflow the default 2 MB stack
            if cfg!(debug_assertions) {
                builder = builder.stack_size(8 * 1024 * 1024);
            }
            builder
                .build()
                .map_err(|e| format!("failed to create generation thread pool: {e}"))?
        });

        let player_data_storage = PlayerDataStorage::new(
            resolved_worlds.save_path.clone(),
            resolved_worlds.player_storage.clone(),
        )
        .await
        .map_err(|e| format!("failed to create player data storage: {e}"))?;
        let mut worlds = WorldMap::new(
            resolved_worlds.default_domain.clone(),
            &resolved_worlds.domains,
        );

        for world_entry in &resolved_worlds.worlds {
            let generator_output = generator_registry
                .create(
                    &world_entry.generator,
                    &world_entry.generator_config,
                    world_entry.seed,
                )
                .map_err(|e| format!("failed to create generator for {}: {e}", world_entry.key))?;
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
            let generation_settings = generation_settings_for_world(world_entry, &generator_output);
            let world = World::new_with_config(
                chunk_runtime.clone(),
                world_entry.key.clone(),
                generator_output.dimension_type,
                world_entry.seed,
                WorldConfig {
                    storage: storage_output.storage,
                    level_data_path: storage_output
                        .level_data_path
                        .map(|path| path.to_string_lossy().into_owned()),
                    generator: Arc::new(generator_output.generator),
                    generation_settings,
                    view_distance: config.view_distance,
                    simulation_distance: config.simulation_distance,
                    compression: config.compression,
                    is_flat: generator_output.is_flat,
                    sea_level: generator_output.sea_level,
                    default_gamemode: world_entry.default_gamemode,
                    difficulty: world_entry.difficulty,
                },
                generation_pool.clone(),
            )
            .await
            .map_err(|e| format!("failed to create world {}: {e}", world_entry.key))?;
            world
                .initialize_spawn_if_needed()
                .await
                .map_err(|e| format!("failed to initialize spawn for {}: {e}", world_entry.key))?;
            worlds.insert(world_entry.key.clone(), world);
        }

        Ok(Server {
            config,
            cancel_token,
            key_store: KeyStore::create(),
            worlds,
            registry_cache,
            tick_rate_manager: SyncRwLock::new(TickRateManager::new()),
            command_dispatcher: SyncRwLock::new(CommandDispatcher::new()),
            player_data_storage,
            pending_world_changes: SyncMutex::new(vec![]),
            pending_domain_switches: SyncMutex::new(vec![]),
        })
    }

    /// Adds a player to the server.
    ///
    /// # Panics
    /// Panics if the registry is not initialized.
    pub async fn add_player(&self, player: Arc<Player>) {
        let Ok(target_domain) = self.load_join_domain(&player).await else {
            player.disconnect("Failed to load player data");
            return;
        };
        let state = match self
            .load_domain_player_state(&player, &target_domain, None, true)
            .await
        {
            Ok(state) => state,
            Err(error) => {
                log::error!(
                    "Failed to load player data for {}: {error}",
                    player.gameprofile.name
                );
                player.disconnect("Failed to load player data");
                return;
            }
        };

        Self::apply_domain_player_state(&player, &state);
        player.reset_health_if_dead();
        self.send_login_packet(&player, &state.world);

        player.reset(state.world.clone(), ResetReason::InitialJoin);
        Self::apply_domain_player_state(&player, &state);
        let pos = *player.position.lock();
        let rotation = player.rotation.load();
        player.spawn(pos, rotation, ResetReason::InitialJoin);
    }

    async fn load_join_domain(&self, player: &Player) -> Result<String, ()> {
        match self
            .player_data_storage
            .load_global(player.gameprofile.id)
            .await
        {
            Ok(Some(global)) if self.worlds.has_domain(&global.last_active_domain) => {
                Ok(global.last_active_domain)
            }
            Ok(Some(global)) => {
                log::warn!(
                    "Player {} last active domain {} no longer exists, using default domain",
                    player.gameprofile.name,
                    global.last_active_domain
                );
                Ok(self.worlds.default_domain().to_owned())
            }
            Ok(None) => Ok(self.worlds.default_domain().to_owned()),
            Err(e) => {
                log::error!(
                    "Failed to load global player data for {}: {e}",
                    player.gameprofile.name
                );
                Err(())
            }
        }
    }

    async fn load_domain_player_state(
        &self,
        player: &Player,
        target_domain: &str,
        fallback_world: Option<Arc<World>>,
        restore_saved_location: bool,
    ) -> Result<DomainPlayerState, String> {
        let mut world = self
            .worlds
            .default_world(target_domain)
            .cloned()
            .ok_or_else(|| format!("domain {target_domain} has no default world"))?;
        if let Some(fallback_world) = fallback_world {
            world = fallback_world;
        }

        match self
            .player_data_storage
            .load_domain(target_domain, player.gameprofile.id)
            .await
        {
            Ok(Some(saved_data)) => {
                let restore_location = restore_saved_location
                    && self.resolve_saved_world(
                        &saved_data.world,
                        target_domain,
                        &mut world,
                        &player.gameprofile.name,
                    );
                log::info!("Loaded saved data for player {}", player.gameprofile.name);
                Ok(DomainPlayerState {
                    world,
                    data: DomainPlayerData::Saved {
                        data: saved_data,
                        restore_location,
                    },
                })
            }
            Ok(None) => {
                log::debug!(
                    "No saved data for player {} in domain {}, using defaults",
                    player.gameprofile.name,
                    target_domain
                );
                Ok(DomainPlayerState {
                    world,
                    data: DomainPlayerData::FirstVisit,
                })
            }
            Err(e) => Err(format!(
                "failed to load domain player data for {} in domain {}: {e}",
                player.gameprofile.name, target_domain
            )),
        }
    }

    fn resolve_saved_world(
        &self,
        saved_world: &str,
        target_domain: &str,
        world: &mut Arc<World>,
        player_name: &str,
    ) -> bool {
        let Ok(saved_world_key) = saved_world.parse::<Identifier>() else {
            log::warn!(
                "Saved world {saved_world} for player {player_name} is invalid, using domain default spawn"
            );
            return false;
        };
        if saved_world_key.namespace.as_ref() != target_domain {
            log::warn!(
                "Saved world {saved_world_key} for player {player_name} is outside target domain {target_domain}, using domain default spawn"
            );
            return false;
        }
        let Some(saved_world) = self.worlds.get(&saved_world_key) else {
            log::warn!(
                "Saved world {saved_world_key} for player {player_name} is missing, using domain default spawn"
            );
            return false;
        };
        *world = saved_world.clone();
        true
    }

    fn apply_domain_player_state(player: &Arc<Player>, state: &DomainPlayerState) {
        match &state.data {
            DomainPlayerData::Saved {
                data,
                restore_location,
            } => {
                if *restore_location {
                    data.apply_to_player(player);
                } else {
                    apply_first_visit_defaults(player, &state.world);
                    data.apply_to_player_without_location(player);
                }
            }
            DomainPlayerData::FirstVisit => apply_first_visit_defaults(player, &state.world),
        }
    }

    fn send_login_packet(&self, player: &Player, world: &World) {
        let reduced_debug_info =
            world.get_game_rule(&REDUCED_DEBUG_INFO) == GameRuleValue::Bool(true);
        let immediate_respawn =
            world.get_game_rule(&IMMEDIATE_RESPAWN) == GameRuleValue::Bool(true);
        let do_limited_crafting =
            world.get_game_rule(&LIMITED_CRAFTING) == GameRuleValue::Bool(true);

        // Get world data
        let hashed_seed = world.obfuscated_seed();

        player.send_packet(CLogin {
            player_id: player.id,
            hardcore: false,
            levels: self.worlds.keys().cloned().collect(),
            max_players: self.config.max_players as i32,
            chunk_radius: player.view_distance().into(),
            simulation_distance: self.config.simulation_distance.into(),
            reduced_debug_info,
            show_death_screen: !immediate_respawn,
            do_limited_crafting,
            common_player_spawn_info: CommonPlayerSpawnInfo {
                dimension_type: world.dimension_type.id() as i32,
                dimension: world.key.clone(),
                seed: hashed_seed,
                game_type: player.game_mode.load(),
                previous_game_type: Some(player.prev_game_mode.load()),
                is_debug: false,
                is_flat: world.is_flat,
                last_death_location: None,
                portal_cooldown: 0,
                sea_level: world.sea_level,
            },
            enforces_secure_chat: self.config.enforce_secure_chat,
        });
    }

    /// Gets all the players on the server
    pub fn get_players(&self) -> Vec<Arc<Player>> {
        let mut players = vec![];
        for world in self.worlds.values() {
            world.players.iter_players(|_, p: &Arc<Player>| {
                players.push(p.clone());
                true
            });
        }
        players
    }

    /// Returns the total number of players currently online across all worlds.
    #[must_use]
    pub fn player_count(&self) -> usize {
        self.worlds.iter().map(|w| w.1.players.len()).sum()
    }

    /// Returns a sample of up to 12 online players for the server list ping.
    #[must_use]
    pub fn player_sample(&self) -> Vec<(String, String)> {
        const MAX_SAMPLE: usize = 12;

        let players = self.get_players();
        if players.is_empty() {
            return vec![];
        }

        let sample_size = players.len().min(MAX_SAMPLE);
        // Random starting offset into the player list
        let offset = if players.len() > sample_size {
            (rand::random::<u64>() as usize) % (players.len() - sample_size + 1)
        } else {
            0
        };

        let mut sample: Vec<(String, String)> = players[offset..offset + sample_size]
            .iter()
            .map(|p| {
                (
                    p.gameprofile.name.clone(),
                    p.gameprofile.id.hyphenated().to_string(),
                )
            })
            .collect();

        // Shuffle using Fisher-Yates with random indices
        for i in (1..sample.len()).rev() {
            let j = (rand::random::<u64>() as usize) % (i + 1);
            sample.swap(i, j);
        }

        sample
    }

    /// Returns the server default world or if not exists the first world.
    /// # Panics
    /// if no world exists on this server crisis is there!
    pub fn overworld(&self) -> &Arc<World> {
        self.worlds.server_default_world().unwrap_or_else(|| {
            self.worlds
                .values()
                .next()
                .expect("At least one world must exist")
        })
    }

    /// Returns the default domain's conventional nether world, if present.
    pub fn nether(&self) -> Option<&Arc<World>> {
        let key = Identifier::new(self.worlds.default_domain().to_owned(), "the_nether");
        self.worlds.get(&key)
    }

    /// Returns the default domain's conventional end world, if present.
    pub fn the_end(&self) -> Option<&Arc<World>> {
        let key = Identifier::new(self.worlds.default_domain().to_owned(), "the_end");
        self.worlds.get(&key)
    }

    /// Runs the three independent tick loops concurrently.
    pub async fn run(self: Arc<Self>, cancel_token: CancellationToken) {
        let game_handle = {
            let s = self.clone();
            let t = cancel_token.clone();
            tokio::spawn(async move { s.run_game_tick(t).await })
        };
        let chunk_send_handle = {
            let s = self.clone();
            let t = cancel_token.clone();
            tokio::spawn(async move { s.run_chunk_sending_tick(t).await })
        };
        let chunk_sched_handle = {
            let s = self.clone();
            let t = cancel_token.clone();
            tokio::spawn(async move { s.run_chunk_scheduling_tick(t).await })
        };
        let _ = tokio::join!(game_handle, chunk_send_handle, chunk_sched_handle);
    }

    /// The main game tick loop (20 TPS, governed by tick rate manager).
    async fn run_game_tick(self: Arc<Self>, cancel_token: CancellationToken) {
        let mut next_tick_time = Instant::now();

        loop {
            if cancel_token.is_cancelled() {
                break;
            }

            let (nanoseconds_per_tick, should_sprint_this_tick) = {
                let mut tick_manager = self.tick_rate_manager.write();
                let nanoseconds_per_tick = tick_manager.nanoseconds_per_tick;
                let (should_sprint, sprint_report) = tick_manager.check_should_sprint_this_tick();
                drop(tick_manager);

                if let Some(report) = sprint_report {
                    self.broadcast_sprint_report(&report);
                    self.broadcast_ticking_state();
                }

                (nanoseconds_per_tick, should_sprint)
            };

            if should_sprint_this_tick {
                next_tick_time = Instant::now();
            } else {
                let now = Instant::now();
                if now < next_tick_time {
                    tokio::select! {
                        () = cancel_token.cancelled() => break,
                        () = sleep(next_tick_time - now) => {}
                    }
                }
                next_tick_time += Duration::from_nanos(nanoseconds_per_tick);
            }

            if cancel_token.is_cancelled() {
                break;
            }

            let tick_start = Instant::now();

            let (tick_count, runs_normally) = {
                let mut tick_manager = self.tick_rate_manager.write();
                tick_manager.tick();
                let runs_normally = tick_manager.runs_normally();
                if runs_normally {
                    tick_manager.increment_tick_count();
                }
                (tick_manager.tick_count, runs_normally)
            };

            self.tick_worlds_game(tick_count, runs_normally).await;

            {
                let server = self.clone();
                let _ = spawn_blocking(move || server.process_world_changes()).await;
            }

            self.process_domain_switches().await;

            let (tps, mspt) = {
                let tick_duration_nanos = tick_start.elapsed().as_nanos() as u64;
                let mut tick_manager = self.tick_rate_manager.write();
                tick_manager.record_tick_time(tick_duration_nanos);
                (tick_manager.get_tps(), tick_manager.get_average_mspt())
            };

            if tick_count % TAB_LIST_UPDATE_INTERVAL == 0 {
                self.broadcast_tab_list(tps, mspt);
            }

            if should_sprint_this_tick {
                let mut tick_manager = self.tick_rate_manager.write();
                tick_manager.end_tick_work();
            }
        }
    }

    /// Chunk sending tick loop — encodes and sends chunks to players independently.
    async fn run_chunk_sending_tick(self: Arc<Self>, cancel_token: CancellationToken) {
        let nanos_per_tick = 1_000_000_000 / CHUNK_SENDING_TPS;
        let mut next_tick_time = Instant::now();

        loop {
            if cancel_token.is_cancelled() {
                break;
            }

            let now = Instant::now();
            if now < next_tick_time {
                tokio::select! {
                    () = cancel_token.cancelled() => break,
                    () = sleep(next_tick_time - now) => {}
                }
            }
            next_tick_time += Duration::from_nanos(nanos_per_tick);

            if cancel_token.is_cancelled() {
                break;
            }

            let server = self.clone();
            let _ = spawn_blocking(move || {
                server.tick_chunk_sending();
            })
            .await;
        }
    }

    /// Chunk scheduling tick loop — ticket updates, holder creation, generation, unloads.
    async fn run_chunk_scheduling_tick(self: Arc<Self>, cancel_token: CancellationToken) {
        let nanos_per_tick = 1_000_000_000 / CHUNK_SCHEDULING_TPS;
        let mut next_tick_time = Instant::now();

        loop {
            if cancel_token.is_cancelled() {
                break;
            }

            let now = Instant::now();
            if now < next_tick_time {
                tokio::select! {
                    () = cancel_token.cancelled() => break,
                    () = sleep(next_tick_time - now) => {}
                }
            }
            next_tick_time += Duration::from_nanos(nanos_per_tick);

            if cancel_token.is_cancelled() {
                break;
            }

            let server = self.clone();
            let _ = spawn_blocking(move || {
                server.tick_chunk_scheduling();
            })
            .await;
        }
    }

    /// Executes one chunk sending tick across all worlds and players.
    ///
    /// A per-world per-tick encode cache is used so overlapping view areas
    /// don't re-encode the same chunk within a single tick.
    fn tick_chunk_sending(&self) {
        for world in self.worlds.values() {
            let mut encode_cache = rustc_hash::FxHashMap::default();
            world.players.iter_players(|_uuid, player| {
                Self::send_chunks_for_player(player, world, &mut encode_cache);
                true
            });
        }
    }

    /// Three-phase chunk send for a single player: prepare (lock briefly),
    /// encode (no lock), commit (lock briefly + generation check).
    fn send_chunks_for_player(
        player: &Arc<Player>,
        world: &Arc<World>,
        encode_cache: &mut rustc_hash::FxHashMap<ChunkPos, EncodedPacket>,
    ) {
        let chunk_pos = *player.last_chunk_pos.lock();
        let connection = &player.connection;

        // Phase 1: prepare (brief lock)
        let prepared = {
            let mut sender = player.chunk_sender.lock();
            sender.prepare_batch(world, chunk_pos, &player.chunk_send_epoch)
        };

        let Some(batch) = prepared else {
            return;
        };

        // Phase 2: encode (no lock held — uses per-tick local cache)
        let compression = connection.compression();
        let encoded = ChunkSender::encode_batch(&batch, encode_cache, compression);

        // Phase 3: commit (brief lock + generation check)
        let mut sender = player.chunk_sender.lock();
        sender.commit_batch(&batch, encoded, connection, &player.chunk_send_epoch);
    }

    /// Executes one chunk scheduling tick across all worlds.
    fn tick_chunk_scheduling(&self) {
        for (i, world) in self.worlds.values().enumerate() {
            let timings = world.chunk_map.tick_scheduling();

            let total = timings.ticket_updates
                + timings.holder_creation
                + timings.schedule_generation
                + timings.run_generation
                + timings.process_unloads;

            if total.as_millis() >= 10 {
                tracing::warn!(
                    world = i,
                    elapsed = ?total,
                    ticket_updates = ?timings.ticket_updates,
                    holder_creation = ?timings.holder_creation,
                    schedule_generation = ?timings.schedule_generation,
                    scheduled_count = timings.scheduled_count,
                    run_generation = ?timings.run_generation,
                    process_unloads = ?timings.process_unloads,
                    "Chunk scheduling tick slow"
                );
            }
        }
    }

    fn process_world_changes(&self) {
        let changes = mem::take(&mut *self.pending_world_changes.lock());

        for (entity, request) in changes {
            if entity.is_removed() {
                continue;
            }
            match request {
                WorldChangeRequest::Computed(transition) => {
                    entity.change_world(&transition);
                }
                WorldChangeRequest::WorldSpawn { target_world } => {
                    let transition = world_spawn_transition(target_world);
                    entity.change_world(&transition);
                }
                WorldChangeRequest::Portal { .. } => {
                    // TODO: portal destination calculation + async chunk pre-warming
                }
            }
        }
    }

    /// Queues a player domain switch for processing at the server tick safe point.
    pub fn queue_domain_switch(
        &self,
        player: Arc<Player>,
        target_domain: String,
    ) -> Result<(), String> {
        if !self.worlds.has_domain(&target_domain) {
            return Err(format!("unknown domain {target_domain}"));
        }

        let current_domain = player.get_world().domain().to_owned();
        if current_domain == target_domain {
            return Err(format!("already in domain {target_domain}"));
        }
        if player.connection.closed() {
            return Err("player is disconnecting".to_owned());
        }
        if !player.begin_domain_switch() {
            return Err("domain switch already in progress".to_owned());
        }

        self.pending_domain_switches
            .lock()
            .push(DomainSwitchRequest {
                player,
                target_domain,
                target_world: None,
                restore_saved_location: true,
            });
        Ok(())
    }

    /// Queues a cross-domain teleport using saved target-domain location or target-world spawn.
    pub fn queue_domain_switch_to_world(
        &self,
        player: Arc<Player>,
        target_world: Arc<World>,
    ) -> Result<(), String> {
        let target_domain = target_world.domain().to_owned();
        if player.connection.closed() {
            return Err("player is disconnecting".to_owned());
        }
        if !player.begin_domain_switch() {
            return Err("domain switch already in progress".to_owned());
        }

        self.pending_domain_switches
            .lock()
            .push(DomainSwitchRequest {
                player,
                target_domain,
                target_world: Some(target_world),
                restore_saved_location: true,
            });
        Ok(())
    }

    async fn process_domain_switches(&self) {
        let switches = mem::take(&mut *self.pending_domain_switches.lock());

        for request in switches {
            let player = request.player.clone();
            let player_name = player.gameprofile.name.clone();
            let result = self.process_domain_switch(request).await;
            player.finish_domain_switch();

            if let Err(error) = result {
                log::error!("Failed to switch {player_name} domain: {error}");
                if !player.connection.closed() {
                    player.disconnect("Failed to switch domain");
                }
            }
        }
    }

    async fn process_domain_switch(&self, request: DomainSwitchRequest) -> Result<(), String> {
        let DomainSwitchRequest {
            player,
            target_domain,
            target_world,
            restore_saved_location,
        } = request;
        if player.connection.closed() {
            return Ok(());
        }
        if !self.worlds.has_domain(&target_domain) {
            return Err(format!("unknown domain {target_domain}"));
        }

        let current_world = player.get_world();
        let current_domain = current_world.domain().to_owned();
        if current_domain == target_domain {
            return Ok(());
        }

        let current_data = PersistentPlayerData::from_player(&player);
        current_world.remove_player_for_world_change(&player);

        if let Err(e) = self
            .player_data_storage
            .save_domain_data(&current_domain, player.gameprofile.id, &current_data)
            .await
        {
            Self::cleanup_removed_domain_switch_player(&current_world, &player);
            return Err(format!("failed to save current domain data: {e}"));
        }

        if player.connection.closed() {
            Self::cleanup_removed_domain_switch_player(&current_world, &player);
            return Ok(());
        }

        let target_state = match self
            .load_domain_player_state(
                &player,
                &target_domain,
                target_world.clone(),
                restore_saved_location,
            )
            .await
        {
            Ok(state) => state,
            Err(error) => {
                Self::cleanup_removed_domain_switch_player(&current_world, &player);
                return Err(error);
            }
        };

        if player.connection.closed() {
            Self::cleanup_removed_domain_switch_player(&current_world, &player);
            return Ok(());
        }

        Self::apply_domain_player_state(&player, &target_state);
        player.reset(target_state.world.clone(), ResetReason::WorldChange);
        Self::apply_domain_player_state(&player, &target_state);
        let pos = *player.position.lock();
        let rotation = player.rotation.load();
        player.spawn(pos, rotation, ResetReason::WorldChange);

        if let Err(e) = self
            .player_data_storage
            .save_global(
                player.gameprofile.id,
                &GlobalPlayerData {
                    last_active_domain: target_domain,
                },
            )
            .await
        {
            log::error!(
                "Failed to save global player data for {} after domain switch: {e}",
                player.gameprofile.name
            );
        }

        Ok(())
    }

    fn cleanup_removed_domain_switch_player(world: &World, player: &Player) {
        world.broadcast_to_all(CRemovePlayerInfo::single(player.gameprofile.id));
        player.cleanup();
    }

    #[tracing::instrument(level = "trace", skip(self), name = "tick_worlds")]
    async fn tick_worlds_game(&self, tick_count: u64, runs_normally: bool) {
        let mut tasks = Vec::with_capacity(self.worlds.len());
        for world in self.worlds.values() {
            let world_clone = world.clone();
            tasks.push(spawn_blocking(move || {
                world_clone.tick_game(tick_count, runs_normally)
            }));
        }
        let mut all_timings: Vec<WorldGameTickTimings> = Vec::with_capacity(tasks.len());
        for task in tasks {
            if let Ok(timings) = task.await {
                all_timings.push(timings);
            }
        }
        for (i, timings) in all_timings.iter().enumerate() {
            if timings.elapsed.as_millis() < 10 {
                continue;
            }
            let cm = &timings.chunk_map;
            tracing::warn!(
                world = i,
                elapsed = ?timings.elapsed,
                tick_count,
                player_tick = ?timings.player_tick,
                broadcast_changes = ?cm.broadcast_changes,
                collect_tickable = ?cm.collect_tickable,
                tick_chunks = ?cm.tick_chunks,
                tickable_count = cm.tickable_count,
                total_chunks = cm.total_chunks,
                "Game tick slow"
            );
        }
    }

    /// Broadcasts the tab list header/footer with current TPS and MSPT values.
    fn broadcast_tab_list(&self, tps: f32, mspt: f32) {
        // Color TPS based on value
        let tps_color = if tps >= 19.5 {
            Color::Green
        } else if tps >= 15.0 {
            Color::Yellow
        } else {
            Color::Red
        };

        // Color MSPT based on value (under 50ms is good)
        let mspt_color = if mspt <= 50.0 {
            Color::Aqua
        } else {
            Color::Red
        };

        let header = TextComponent::plain("\n").add_children(vec![
            TextComponent::plain("Steel Dev Build").color(Color::Yellow),
            TextComponent::plain("\n"),
        ]);
        let footer = TextComponent::plain("\n").add_children(vec![
            TextComponent::plain("TPS: ").color(Color::Gray),
            TextComponent::plain(format!("{tps:.1}")).color(tps_color),
            TextComponent::plain(" | ").color(Color::DarkGray),
            TextComponent::plain("MSPT: ").color(Color::Gray),
            TextComponent::plain(format!("{mspt:.2}")).color(mspt_color),
            TextComponent::plain("\n"),
        ]);

        // Broadcast to all players in all worlds
        for world in self.worlds.values() {
            world.broadcast_to_all_with(|player| CTabList::new(&header, &footer, player));
        }
    }

    /// Broadcasts a sprint completion report to all players.
    fn broadcast_sprint_report(&self, report: &SprintReport) {
        use steel_utils::translations;

        let message: TextComponent = translations::COMMANDS_TICK_SPRINT_REPORT
            .message([
                TextComponent::from(format!("{}", report.ticks_per_second)),
                TextComponent::from(format!("{:.2}", report.ms_per_tick)),
            ])
            .into();

        for world in self.worlds.values() {
            world.broadcast_to_all_with(|player| CSystemChat::new(&message, false, player));
        }
    }

    /// Broadcasts the current tick rate and frozen state to all clients.
    /// This should be called whenever the tick rate or frozen state changes.
    pub fn broadcast_ticking_state(&self) {
        let tick_manager = self.tick_rate_manager.read();
        let packet = CTickingState::new(tick_manager.tick_rate(), tick_manager.is_frozen());
        drop(tick_manager);

        for world in self.worlds.values() {
            world.broadcast_to_all(packet.clone());
        }
    }

    /// Broadcasts the current step tick count to all clients.
    /// This should be called whenever the step tick count changes.
    pub fn broadcast_ticking_step(&self) {
        let tick_manager = self.tick_rate_manager.read();
        let packet = CTickingStep::new(tick_manager.frozen_ticks_to_run());
        drop(tick_manager);

        for world in self.worlds.values() {
            world.broadcast_to_all(packet.clone());
        }
    }

    /// Sends the current ticking state and step packets to a joining player.
    /// This should be called when a player joins the server.
    pub fn send_ticking_state_to_player(&self, player: &Player) {
        let tick_manager = self.tick_rate_manager.read();
        let state_packet = CTickingState::new(tick_manager.tick_rate(), tick_manager.is_frozen());
        let step_packet = CTickingStep::new(tick_manager.frozen_ticks_to_run());
        drop(tick_manager);

        player.send_packet(state_packet);
        player.send_packet(step_packet);
    }

    /// Resends client state that is not fully covered by `CRespawn`.
    pub fn resend_player_context(&self, player: &Player) {
        player.send_difficulty();
        player.send_inventory_to_remote();

        let commands = self.command_dispatcher.read().get_commands();
        player.send_packet(commands);

        // TODO: Set permissions level to match player's level.
        player.send_packet(CEntityEvent {
            entity_id: player.id,
            event: EntityStatus::PermissionLevelOwners,
        });

        self.send_ticking_state_to_player(player);

        player.send_packet(CGameEvent {
            event: GameEventType::ChangeGameMode,
            data: player.game_mode.load().into(),
        });
    }
    /// Queues a world change to be processed after the current tick.
    pub fn queue_world_change(&self, entity: SharedEntity, request: WorldChangeRequest) {
        self.pending_world_changes.lock().push((entity, request));
    }
}
