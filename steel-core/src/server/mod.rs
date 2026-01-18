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
use steel_protocol::packets::game::{CLogin, CommonPlayerSpawnInfo};
use steel_registry::game_rules::GameRuleValue;
use steel_registry::vanilla_dimension_types::OVERWORLD;
use steel_registry::vanilla_game_rules::{IMMEDIATE_RESPAWN, LIMITED_CRAFTING, REDUCED_DEBUG_INFO};
use steel_registry::{REGISTRY, Registry};
use steel_utils::locks::SyncRwLock;
use steel_utils::types::GameType;
use tick_rate_manager::TickRateManager;
use tokio::{runtime::Runtime, task::spawn_blocking, time::sleep};
use tokio_util::sync::CancellationToken;

use crate::behavior::init_behaviors;
use crate::command::CommandDispatcher;
use crate::config::STEEL_CONFIG;
use crate::player::Player;
use crate::server::registry_cache::RegistryCache;
use crate::world::World;

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

        world.add_player(player);
    }

    /// Runs the server tick loop.
    pub async fn run(self: Arc<Self>, cancel_token: CancellationToken) {
        let mut next_tick_time = Instant::now();

        loop {
            if cancel_token.is_cancelled() {
                break;
            }

            let (nanoseconds_per_tick, is_sprinting, should_sprint_this_tick) = {
                let mut tick_manager = self.tick_rate_manager.write();
                let nanoseconds_per_tick = tick_manager.nanoseconds_per_tick;

                // Handle sprinting
                let is_sprinting = tick_manager.is_sprinting();
                let should_sprint_this_tick = if is_sprinting {
                    tick_manager.check_should_sprint_this_tick()
                } else {
                    false
                };
                (nanoseconds_per_tick, is_sprinting, should_sprint_this_tick)
            };

            if is_sprinting && should_sprint_this_tick {
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

            let tick_count = {
                let mut tick_manager = self.tick_rate_manager.write();
                tick_manager.tick();
                tick_manager.tick_count
            };

            // Tick worlds
            self.tick_worlds(tick_count).await;

            if is_sprinting && should_sprint_this_tick {
                let mut tick_manager = self.tick_rate_manager.write();
                tick_manager.end_tick_work();
            }
        }
    }

    async fn tick_worlds(&self, tick_count: u64) {
        let mut tasks = Vec::with_capacity(self.worlds.len());
        for world in &self.worlds {
            let world_clone = world.clone();
            tasks.push(spawn_blocking(move || world_clone.tick_b(tick_count)));
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
}
