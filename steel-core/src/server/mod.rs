//! This module contains the `Server` struct, which is the main entry point for the server.
/// The registry cache for the server.
pub mod registry_cache;
/// The tick rate manager for the server.
pub mod tick_rate_manager;

use std::{
    sync::Arc,
    time::{Duration, Instant},
};

use crate::behavior::init_behaviors;
use crate::block_entity::init_block_entities;
use crate::chunk::empty_chunk_generator::EmptyChunkGenerator;
use crate::chunk::flat_chunk_generator::FlatChunkGenerator;
use crate::chunk::world_gen_context::ChunkGeneratorType;
use crate::command::CommandDispatcher;
use crate::config::{STEEL_CONFIG, WordGeneratorTypes, WorldStorageConfig};
use crate::player::Player;
use crate::server::registry_cache::RegistryCache;
use crate::world::{World, WorldConfig, WorldTickTimings};
use steel_crypto::key_store::KeyStore;
use steel_protocol::packets::game::{
    CGameEvent, CLogin, CSetHeldSlot, CSystemChat, CTabList, CTickingState, CTickingStep,
    CommonPlayerSpawnInfo, GameEventType,
};
use steel_registry::game_rules::GameRuleValue;
use steel_registry::vanilla_dimension_types::OVERWORLD;
use steel_registry::vanilla_game_rules::{IMMEDIATE_RESPAWN, LIMITED_CRAFTING, REDUCED_DEBUG_INFO};
use steel_registry::{REGISTRY, Registry, vanilla_blocks};
use steel_utils::locks::SyncRwLock;
use text_components::{Modifier, TextComponent, format::Color};
use tick_rate_manager::{SprintReport, TickRateManager};
use tokio::{runtime::Runtime, task::spawn_blocking, time::sleep};
use tokio_util::sync::CancellationToken;

use crate::entity::init_entities;
use crate::player::player_data_storage::PlayerDataStorage;

/// Interval in ticks between tab list updates (20 ticks = 1 second).
const TAB_LIST_UPDATE_INTERVAL: u64 = 20;

/// The main server struct.
pub struct Server {
    /// The cancellation token for graceful shutdown.
    pub cancel_token: CancellationToken,
    /// The key store for the server.
    pub key_store: KeyStore,
    /// The registry cache for the server.
    pub registry_cache: RegistryCache,
    /// A list of all the worlds on the server.
    pub worlds: Vec<Arc<World>>,
    /// The tick rate manager for the server.
    pub tick_rate_manager: SyncRwLock<TickRateManager>,
    /// Saves and dispatches commands to appropriate handlers.
    pub command_dispatcher: SyncRwLock<CommandDispatcher>,
    /// Player data storage for saving/loading player state.
    pub player_data_storage: PlayerDataStorage,
}

impl Server {
    /// Creates a new server.
    ///
    /// # Panics
    ///
    /// Panics if the global registry has already been initialized.
    pub async fn new(chunk_runtime: Arc<Runtime>, cancel_token: CancellationToken) -> Self {
        let start = Instant::now();
        let mut registry = Registry::new_vanilla();
        registry.freeze();
        log::info!("Vanilla registry loaded in {:?}", start.elapsed());

        REGISTRY
            .init(registry)
            .expect("We should be the ones who init the REGISTRY");

        // Initialize behavior registries after the main registry is frozen
        init_behaviors();
        init_block_entities();
        init_entities();
        log::info!("Behavior registries initialized");

        let registry_cache = RegistryCache::new();

        let seed: i64 = if STEEL_CONFIG.seed.is_empty() {
            rand::random()
        } else {
            STEEL_CONFIG.seed.parse().unwrap_or_else(|_| {
                let mut hash: i64 = 0;
                for byte in STEEL_CONFIG.seed.bytes() {
                    hash = hash.wrapping_mul(31).wrapping_add(i64::from(byte));
                }
                hash
            })
        };
        let generator = match STEEL_CONFIG.world_generator {
            WordGeneratorTypes::Flat => {
                ChunkGeneratorType::Flat(FlatChunkGenerator::new(
                    REGISTRY
                        .blocks
                        .get_default_state_id(vanilla_blocks::BEDROCK), // Bedrock
                    REGISTRY.blocks.get_default_state_id(vanilla_blocks::DIRT), // Dirt
                    REGISTRY
                        .blocks
                        .get_default_state_id(vanilla_blocks::GRASS_BLOCK), // Grass Block
                ))
            }
            WordGeneratorTypes::Empty => ChunkGeneratorType::Empty(EmptyChunkGenerator::new()),
        };
        let config = WorldConfig {
            storage: match &STEEL_CONFIG.world_storage_config {
                WorldStorageConfig::Disk { path } => WorldStorageConfig::Disk {
                    path: format!("{}/{}", path, OVERWORLD.key.path),
                },
                WorldStorageConfig::RamOnly => WorldStorageConfig::RamOnly,
            },
            generator: Arc::new(generator),
        };

        let overworld = World::new_with_config(chunk_runtime, OVERWORLD, seed, config)
            .await
            .expect("Failed to create overworld");

        let player_data_storage = PlayerDataStorage::new()
            .await
            .expect("Failed to create player data storage");

        Server {
            cancel_token,
            key_store: KeyStore::create(),
            worlds: vec![overworld],
            registry_cache,
            tick_rate_manager: SyncRwLock::new(TickRateManager::new()),
            command_dispatcher: SyncRwLock::new(CommandDispatcher::new()),
            player_data_storage,
        }
    }

    /// Adds a player to the server.
    ///
    /// # Panics
    /// Panics if the registry is not initialized.
    pub async fn add_player(&self, player: Arc<Player>) {
        // Load saved player data if it exists
        match self.player_data_storage.load(player.gameprofile.id).await {
            Ok(Some(saved_data)) => {
                log::info!("Loaded saved data for player {}", player.gameprofile.name);
                saved_data.apply_to_player(&player);
            }
            Ok(None) => {
                log::debug!(
                    "No saved data for player {}, using defaults",
                    player.gameprofile.name
                );
            }
            Err(e) => {
                log::error!(
                    "Failed to load player data for {}: {e}",
                    player.gameprofile.name
                );
            }
        }

        let world = &self.worlds[0];

        // Get gamerule values
        let reduced_debug_info =
            world.get_game_rule(REDUCED_DEBUG_INFO) == GameRuleValue::Bool(true);
        let immediate_respawn = world.get_game_rule(IMMEDIATE_RESPAWN) == GameRuleValue::Bool(true);
        let do_limited_crafting =
            world.get_game_rule(LIMITED_CRAFTING) == GameRuleValue::Bool(true);

        // Get world data
        let hashed_seed = world.obfuscated_seed();
        let dimension_key = world.dimension.key.clone();

        player.send_packet(CLogin {
            player_id: player.id,
            hardcore: false,
            levels: vec![dimension_key.clone()],
            max_players: STEEL_CONFIG.max_players as i32,
            chunk_radius: player.view_distance().into(),
            simulation_distance: STEEL_CONFIG.simulation_distance.into(),
            reduced_debug_info,
            show_death_screen: !immediate_respawn,
            do_limited_crafting,
            common_player_spawn_info: CommonPlayerSpawnInfo {
                dimension_type: *(REGISTRY.dimension_types.get_id(
                    REGISTRY
                        .dimension_types
                        .by_key(&dimension_key)
                        .expect("Should be registered"),
                )) as i32,
                dimension: dimension_key,
                seed: hashed_seed,
                game_type: player.game_mode.load(),
                previous_game_type: None,
                is_debug: false,
                // TODO: Change once we add a normal generator
                is_flat: true,
                last_death_location: None,
                portal_cooldown: 0,
                sea_level: 63, // Standard overworld sea level
            },
            enforces_secure_chat: STEEL_CONFIG.enforce_secure_chat,
        });

        // Send player abilities (flight, invulnerability, etc.)
        player.send_abilities();

        player.send_packet(CSetHeldSlot {
            slot: i32::from(player.inventory.lock().get_selected_slot()),
        });

        if world.can_have_weather() {
            let (rain_level, thunder_level) = {
                let weather = world.weather.lock();
                (weather.rain_level, weather.thunder_level)
            };

            if world.is_raining() {
                player.send_packet(CGameEvent {
                    event: GameEventType::StartRaining,
                    data: 0.0,
                });
            }

            player.send_packet(CGameEvent {
                event: GameEventType::RainLevelChange,
                data: rain_level,
            });

            player.send_packet(CGameEvent {
                event: GameEventType::ThunderLevelChange,
                data: thunder_level,
            });
        }

        let commands = self.command_dispatcher.read().get_commands();
        player.send_packet(commands);

        // Send current ticking state to the joining player
        self.send_ticking_state_to_player(&player);

        // Get player position for teleport sync (must be done before add_player moves the Arc)
        let pos = *player.position.lock();
        let (yaw, pitch) = player.rotation.load();

        // Send position sync to client (ensures client is at the correct loaded position)
        // This must be sent after the player is added to the world
        player.teleport(pos.x, pos.y, pos.z, yaw, pitch);

        world.add_player(player.clone());
    }

    /// Gets all the players on the server
    pub fn get_players(&self) -> Vec<Arc<Player>> {
        let mut players = vec![];
        for world in &self.worlds {
            world.players.iter_players(|_, p| {
                players.push(p.clone());
                true
            });
        }
        players
    }

    /// Runs the server tick loop.
    pub async fn run(self: Arc<Self>, cancel_token: CancellationToken) {
        let mut next_tick_time = Instant::now();

        loop {
            if cancel_token.is_cancelled() {
                break;
            }

            let (nanoseconds_per_tick, should_sprint_this_tick) = {
                let mut tick_manager = self.tick_rate_manager.write();
                let nanoseconds_per_tick = tick_manager.nanoseconds_per_tick;

                // Handle sprinting - returns (should_sprint, Option<sprint_report>)
                let (should_sprint, sprint_report) = tick_manager.check_should_sprint_this_tick();
                drop(tick_manager);

                // If sprint finished, broadcast the report and state change to all players
                if let Some(report) = sprint_report {
                    self.broadcast_sprint_report(&report);
                    self.broadcast_ticking_state();
                }

                (nanoseconds_per_tick, should_sprint)
            };

            if should_sprint_this_tick {
                // If sprinting, we don't wait
                next_tick_time = Instant::now();
            } else {
                // Normal wait logic
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

            // Record tick start time for MSPT tracking
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

            // Always tick worlds (for chunk loading/gen), but pass runs_normally
            // so game elements like random ticks only run when not frozen
            self.tick_worlds(tick_count, runs_normally).await;

            // Record tick duration for TPS/MSPT tracking
            let (tps, mspt) = {
                let tick_duration_nanos = tick_start.elapsed().as_nanos() as u64;
                let mut tick_manager = self.tick_rate_manager.write();
                tick_manager.record_tick_time(tick_duration_nanos);
                (tick_manager.get_tps(), tick_manager.get_average_mspt())
            };

            // Update tab list with TPS/MSPT periodically
            if tick_count % TAB_LIST_UPDATE_INTERVAL == 0 {
                self.broadcast_tab_list(tps, mspt);
            }

            if should_sprint_this_tick {
                let mut tick_manager = self.tick_rate_manager.write();
                tick_manager.end_tick_work();
            }
        }
    }

    #[tracing::instrument(level = "trace", skip(self), name = "tick_worlds")]
    async fn tick_worlds(&self, tick_count: u64, runs_normally: bool) {
        let mut tasks = Vec::with_capacity(self.worlds.len());
        for world in &self.worlds {
            let world_clone = world.clone();
            tasks.push(spawn_blocking(move || {
                world_clone.tick_b(tick_count, runs_normally)
            }));
        }
        let start = Instant::now();
        let mut all_timings: Vec<WorldTickTimings> = Vec::with_capacity(tasks.len());
        for task in tasks {
            if let Ok(timings) = task.await {
                all_timings.push(timings);
            }
        }
        let elapsed = start.elapsed();
        if elapsed.as_millis() >= 30 {
            // Log detailed breakdown when tick is slow
            for (i, timings) in all_timings.iter().enumerate() {
                let cm = &timings.chunk_map;
                tracing::warn!(
                    world = i,
                    ?elapsed,
                    tick_count,
                    player_tick = ?timings.player_tick,
                    ticket_updates = ?cm.ticket_updates,
                    holder_creation = ?cm.holder_creation,
                    schedule_generation = ?cm.schedule_generation,
                    scheduled_count = cm.scheduled_count,
                    run_generation = ?cm.run_generation,
                    broadcast_changes = ?cm.broadcast_changes,
                    process_unloads = ?cm.process_unloads,
                    collect_tickable = ?cm.collect_tickable,
                    tick_chunks = ?cm.tick_chunks,
                    tickable_count = cm.tickable_count,
                    total_chunks = cm.total_chunks,
                    "Worlds tick slow"
                );
            }
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
        for world in &self.worlds {
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

        for world in &self.worlds {
            world.broadcast_to_all_with(|player| CSystemChat::new(&message, false, player));
        }
    }

    /// Broadcasts the current tick rate and frozen state to all clients.
    /// This should be called whenever the tick rate or frozen state changes.
    pub fn broadcast_ticking_state(&self) {
        let tick_manager = self.tick_rate_manager.read();
        let packet = CTickingState::new(tick_manager.tick_rate(), tick_manager.is_frozen());
        drop(tick_manager);

        for world in &self.worlds {
            world.broadcast_to_all(packet.clone());
        }
    }

    /// Broadcasts the current step tick count to all clients.
    /// This should be called whenever the step tick count changes.
    pub fn broadcast_ticking_step(&self) {
        let tick_manager = self.tick_rate_manager.read();
        let packet = CTickingStep::new(tick_manager.frozen_ticks_to_run());
        drop(tick_manager);

        for world in &self.worlds {
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
}
