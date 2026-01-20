//! This module contains the `Server` struct, which is the main entry point for the server.
/// The registry cache for the server.
pub mod registry_cache;
/// The tick rate manager for the server.
pub mod tick_rate_manager;

use std::{
    sync::{
        Arc,
        atomic::{AtomicI32, Ordering},
    },
    time::{Duration, Instant},
};

use steel_crypto::key_store::KeyStore;
use steel_protocol::packets::game::{
    CLogin, CSystemChat, CTabList, CTickingState, CTickingStep, CommonPlayerSpawnInfo,
};
use steel_registry::game_rules::GameRuleValue;
use steel_registry::vanilla_dimension_types::OVERWORLD;
use steel_registry::vanilla_game_rules::{IMMEDIATE_RESPAWN, LIMITED_CRAFTING, REDUCED_DEBUG_INFO};
use steel_registry::{REGISTRY, Registry};
use steel_utils::locks::SyncRwLock;
use steel_utils::text::{TextComponent, color::NamedColor};
use steel_utils::types::GameType;
use tick_rate_manager::{SprintReport, TickRateManager};
use tokio::{runtime::Runtime, task::spawn_blocking, time::sleep};
use tokio_util::sync::CancellationToken;

use crate::behavior::init_behaviors;
use crate::command::CommandDispatcher;
use crate::config::STEEL_CONFIG;
use crate::player::Player;
use crate::server::registry_cache::RegistryCache;
use crate::world::World;

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
    /// Counter for assigning unique entity IDs.
    next_entity_id: AtomicI32,
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

        let overworld = World::new(chunk_runtime, OVERWORLD, seed)
            .await
            .expect("Failed to create overworld");

        Server {
            cancel_token,
            key_store: KeyStore::create(),
            worlds: vec![overworld],
            registry_cache,
            tick_rate_manager: SyncRwLock::new(TickRateManager::new()),
            command_dispatcher: SyncRwLock::new(CommandDispatcher::new()),
            next_entity_id: AtomicI32::new(1), // Start at 1, 0 is reserved
        }
    }

    /// Allocates a new unique entity ID.
    #[must_use]
    pub fn next_entity_id(&self) -> i32 {
        self.next_entity_id.fetch_add(1, Ordering::Relaxed)
    }

    /// Adds a player to the server.
    ///
    /// # Panics
    /// Panics if the registry is not initialized.
    pub fn add_player(&self, player: Arc<Player>) {
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

        player.connection.send_packet(CLogin {
            player_id: player.entity_id,
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
                game_type: GameType::Survival,
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

        let commands = self.command_dispatcher.read().get_commands();
        player.connection.send_packet(commands);

        // Send current ticking state to the joining player
        self.send_ticking_state_to_player(&player);

        world.add_player(player);
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

    async fn tick_worlds(&self, tick_count: u64, runs_normally: bool) {
        let mut tasks = Vec::with_capacity(self.worlds.len());
        for world in &self.worlds {
            let world_clone = world.clone();
            tasks.push(spawn_blocking(move || {
                world_clone.tick_b(tick_count, runs_normally);
            }));
        }
        let start = Instant::now();
        for task in tasks {
            let _ = task.await;
        }
        if start.elapsed().as_millis() > 1 {
            log::warn!(
                "Worlds ticked in {:?}, tick count: {tick_count}",
                start.elapsed()
            );
        }
    }

    /// Broadcasts the tab list header/footer with current TPS and MSPT values.
    fn broadcast_tab_list(&self, tps: f32, mspt: f32) {
        // Color TPS based on value
        let tps_color = if tps >= 19.5 {
            NamedColor::Green
        } else if tps >= 15.0 {
            NamedColor::Yellow
        } else {
            NamedColor::Red
        };

        // Color MSPT based on value (under 50ms is good)
        let mspt_color = if mspt <= 50.0 {
            NamedColor::Aqua
        } else {
            NamedColor::Red
        };

        let packet = CTabList::new(
            // Header: Steel Dev Build
            TextComponent::new()
                .text("\n")
                .extra(
                    TextComponent::new()
                        .text("Steel Dev Build")
                        .color(NamedColor::Yellow),
                )
                .extra(TextComponent::new().text("\n")),
            // Footer: TPS and MSPT with live values
            TextComponent::new()
                .text("\n")
                .extra(TextComponent::new().text("TPS: ").color(NamedColor::Gray))
                .extra(
                    TextComponent::new()
                        .text(format!("{tps:.1}"))
                        .color(tps_color),
                )
                .extra(TextComponent::new().text(" | ").color(NamedColor::DarkGray))
                .extra(TextComponent::new().text("MSPT: ").color(NamedColor::Gray))
                .extra(
                    TextComponent::new()
                        .text(format!("{mspt:.2}"))
                        .color(mspt_color),
                )
                .extra(TextComponent::new().text("\n")),
        );

        // Broadcast to all players in all worlds
        for world in &self.worlds {
            world.players.iter_players(|_, player| {
                player.connection.send_packet(packet.clone());
                true
            });
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

        let packet = CSystemChat::new(message, false);

        for world in &self.worlds {
            world.players.iter_players(|_, player| {
                player.connection.send_packet(packet.clone());
                true
            });
        }
    }

    /// Broadcasts the current tick rate and frozen state to all clients.
    /// This should be called whenever the tick rate or frozen state changes.
    pub fn broadcast_ticking_state(&self) {
        let tick_manager = self.tick_rate_manager.read();
        let packet = CTickingState::new(tick_manager.tick_rate(), tick_manager.is_frozen());
        drop(tick_manager);

        for world in &self.worlds {
            world.players.iter_players(|_, player| {
                player.connection.send_packet(packet.clone());
                true
            });
        }
    }

    /// Broadcasts the current step tick count to all clients.
    /// This should be called whenever the step tick count changes.
    pub fn broadcast_ticking_step(&self) {
        let tick_manager = self.tick_rate_manager.read();
        let packet = CTickingStep::new(tick_manager.frozen_ticks_to_run());
        drop(tick_manager);

        for world in &self.worlds {
            world.players.iter_players(|_, player| {
                player.connection.send_packet(packet.clone());
                true
            });
        }
    }

    /// Sends the current ticking state and step packets to a joining player.
    /// This should be called when a player joins the server.
    pub fn send_ticking_state_to_player(&self, player: &Player) {
        let tick_manager = self.tick_rate_manager.read();
        let state_packet = CTickingState::new(tick_manager.tick_rate(), tick_manager.is_frozen());
        let step_packet = CTickingStep::new(tick_manager.frozen_ticks_to_run());
        drop(tick_manager);

        player.connection.send_packet(state_packet);
        player.connection.send_packet(step_packet);
    }
}
