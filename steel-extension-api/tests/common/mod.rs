//! Server fixture and peer helpers shared by the extension integration tests.
#![allow(
    dead_code,
    reason = "each integration test binary uses a different subset of these helpers"
)]
use glam::DVec3;
use std::slice::from_ref;
use std::{
    env::{
        self,
        consts::{DLL_PREFIX, DLL_SUFFIX},
    },
    fs,
    path::{Path, PathBuf},
    process,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};
use steel_core::config::CompressionInfo;
use steel_core::{
    bootstrap::init_globals,
    chunk::test_support::insert_full_chunk,
    command::{CommandRegistry, FrozenCommands, sender::CommandSender},
    config::{ResolvedDomainConfig, RuntimeConfig},
    entity::{Entity, LivingEntity, damage::DamageHistory, next_entity_id},
    extension::{ExtensionHooks, RegisteredExtensions},
    level_data::{GameTimeSource, WorldGenerationSettings},
    permission::{PermissionKey, PermissionSet, PermissionSubjectIndex, PermissionSubjectState},
    player::{
        ClientInformation, Player, PlayerConnection, PlayerSession, connection::NetworkConnection,
    },
    server::{
        Server,
        jobs::{JobPoll, ServerJob, ServerJobContext},
        test_server_with_worlds_and_config,
    },
    world::{World, WorldConfig, WorldStorageConfig},
    worldgen::{ChunkGeneratorType, EmptyChunkGenerator},
};
use steel_extension_api::host;
use steel_protocol::packet_traits::EncodedPacket;
use steel_protocol::packets::game::{PlayerAction, SPlayerAction};
use steel_registry::blocks::properties::Direction;
use steel_registry::vanilla_game_rules::NATURAL_HEALTH_REGENERATION;
use steel_registry::{game_profile::GameProfile, vanilla_blocks, vanilla_dimension_types};
use steel_utils::{
    BlockPos, ChunkPos, Identifier,
    locks::SyncMutex,
    types::{Difficulty, GameType, UpdateFlags},
};
use text_components::TextComponent;
use tokio::runtime::{Builder, Runtime};
use tokio::sync::oneshot;
use tokio::task::JoinHandle;
use tokio::time::sleep;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

#[derive(Default)]
pub struct PeerConnection {
    pub closed: AtomicBool,
    pub sent_packets: Arc<SyncMutex<Vec<EncodedPacket>>>,
}
impl NetworkConnection for PeerConnection {
    fn compression(&self) -> Option<CompressionInfo> {
        None
    }
    fn send_encoded(&self, packet: EncodedPacket) {
        self.sent_packets.lock().push(packet);
    }
    fn send_encoded_bundle(&self, packets: Vec<EncodedPacket>) {
        self.sent_packets.lock().extend(packets);
    }
    fn disconnect_with_reason(&self, _: TextComponent) {
        self.close();
    }
    fn tick(&self) {}
    fn latency(&self) -> i32 {
        0
    }
    fn close(&self) {
        self.closed.store(true, Ordering::Release);
    }
    fn closed(&self) -> bool {
        self.closed.load(Ordering::Acquire)
    }
}

pub struct Fixture {
    pub runtime: Arc<Runtime>,
    pub world: Arc<World>,
    _history: Arc<DamageHistory>,
}
impl Fixture {
    pub fn new() -> Result<Self, String> {
        let runtime = Arc::new(
            Builder::new_multi_thread()
                .worker_threads(2)
                .enable_all()
                .build()
                .map_err(|e| e.to_string())?,
        );
        let pool = Arc::new(
            rayon::ThreadPoolBuilder::new()
                .num_threads(1)
                .build()
                .map_err(|e| e.to_string())?,
        );
        let dimension = &vanilla_dimension_types::OVERWORLD;
        let history = Arc::new(DamageHistory::default());
        let settings = WorldGenerationSettings::from_generator_config(
            Identifier::from_steel("empty"),
            &toml::from_str("dimension_type='minecraft:overworld'").map_err(|e| e.to_string())?,
            dimension.key.clone(),
            dimension.min_y,
            dimension.height,
        );
        let world = runtime
            .block_on(World::new_with_config(
                Arc::clone(&runtime),
                Identifier::new_static("home", "overworld"),
                dimension,
                0,
                WorldConfig {
                    damage_history: Arc::clone(&history),
                    game_time_source: GameTimeSource::Primary,
                    storage: WorldStorageConfig::RamOnly,
                    level_data_path: None,
                    generator: Arc::new(ChunkGeneratorType::Empty(EmptyChunkGenerator::new())),
                    generation_settings: settings,
                    view_distance: 2,
                    simulation_distance: 2,
                    max_chained_neighbor_updates: 1_000_000,
                    compression: None,
                    is_flat: false,
                    sea_level: 0,
                    default_gamemode: GameType::Survival,
                    difficulty: Difficulty::Normal,
                },
                pool,
            ))
            .map_err(|e| e.to_string())?;
        let _ = insert_full_chunk(&world, ChunkPos::new(0, 0), true);
        world.set_game_rule(&NATURAL_HEALTH_REGENERATION, false);
        for x in 1..15 {
            for z in 1..15 {
                world.set_block(
                    BlockPos::new(x, 64, z),
                    vanilla_blocks::STONE.default_state(),
                    UpdateFlags::UPDATE_ALL,
                );
            }
        }
        world
            .level_data
            .write()
            .data_mut()
            .set_spawn_pos(BlockPos::new(8, 65, 8));
        Ok(Self {
            runtime,
            world,
            _history: history,
        })
    }
}
pub struct CancelOnDrop(pub CancellationToken);
impl Drop for CancelOnDrop {
    fn drop(&mut self) {
        self.0.cancel();
    }
}
struct OnTick<F>(Option<F>);
impl<F: FnOnce() + Send> ServerJob for OnTick<F> {
    fn poll(&mut self, _: &mut ServerJobContext) -> JobPoll {
        if let Some(work) = self.0.take() {
            work();
        }
        JobPoll::Finished
    }
}
pub async fn on_tick(
    server: &Server,
    work: impl FnOnce() -> Result<(), String> + Send + 'static,
) -> Result<(), String> {
    let (sender, receiver) = oneshot::channel();
    server.jobs.spawn(OnTick(Some(move || {
        let _ = sender.send(work());
    })));
    receiver.await.map_err(|e| e.to_string())?
}
pub async fn until(
    player: &Arc<Player>,
    label: &str,
    condition: impl Fn() -> bool,
) -> Result<(), String> {
    let deadline = Instant::now() + Duration::from_secs(25);
    while Instant::now() < deadline {
        // A real peer sends this signal after each respawn/world load.
        player.set_client_loaded(true);
        if condition() {
            return Ok(());
        }
        if player.connection.closed() {
            return Err(format!("peer disconnected while waiting for {label}"));
        }
        sleep(Duration::from_millis(2)).await;
    }
    Err(format!(
        "timed out waiting for {label}: world={}, mode={:?}, health={}, changing={}",
        player.get_world().key,
        player.game_mode(),
        player.get_health(),
        player.is_domain_switching()
    ))
}

pub fn library(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../extension-projects/target/debug")
        .join(format!("{DLL_PREFIX}{name}{DLL_SUFFIX}"))
}

pub fn extension_data_root() -> PathBuf {
    let root = env::temp_dir().join(format!("steel-extension-ports-{}", process::id()));
    let _ = fs::remove_dir_all(&root);
    root
}

pub async fn command(server: &Arc<Server>, player: &Arc<Player>, line: &str) -> Result<(), String> {
    server
        .submit_command(CommandSender::Player(Arc::clone(player)), line.into())
        .map_err(|_| "command queue full".to_owned())?;
    // Commands drain on the next ticks; give the queue a few at 200 TPS.
    sleep(Duration::from_millis(60)).await;
    Ok(())
}

pub async fn teleport(
    server: &Arc<Server>,
    player: &Arc<Player>,
    pos: DVec3,
) -> Result<(), String> {
    let actor = Arc::clone(player);
    on_tick(server, move || {
        actor.teleport(pos, 0.0, 0.0).map_err(|e| e.to_string())
    })
    .await?;
    until(player, "teleport", || {
        (player.position() - pos).length_squared() < 0.01
    })
    .await
}

pub async fn break_block(
    server: &Arc<Server>,
    player: &Arc<Player>,
    pos: BlockPos,
) -> Result<(), String> {
    let actor = Arc::clone(player);
    on_tick(server, move || {
        actor.handle_player_action(SPlayerAction {
            action: PlayerAction::StartDestroyBlock,
            pos,
            direction: Direction::Up,
            sequence: 0,
        });
        Ok(())
    })
    .await
}

/// Loaded and initialized extensions plus the extensions that carry their hooks.
pub struct Loaded {
    pub extensions: RegisteredExtensions,
    /// Frozen command registry; freezing runs the extensions' command registration.
    pub commands: FrozenCommands,
    pub initialized: host::InitializedExtensions,
    /// Extension names in initialization order.
    pub order: Vec<&'static str>,
}

/// Loads `(library, config)` pairs from `extension-projects/target/debug` and initializes them.
pub fn load_extensions(extensions: &[(&str, &str)], data_root: &Path) -> Result<Loaded, String> {
    let sources: Vec<host::ExtensionSource> = extensions
        .iter()
        .map(|(name, config)| host::ExtensionSource {
            path: library(name),
            config: Some((*config).to_owned()),
        })
        .collect();
    let mut hooks = ExtensionHooks::default();
    // SAFETY: the extensions are built from this repository against the same descriptor
    // contract; the loader pins every library for the lifetime of its callbacks.
    let report = unsafe { host::load_all(&sources, data_root, &mut hooks) }?;
    if !report.skipped.is_empty() {
        return Err(format!("unexpected skips: {:?}", report.skipped));
    }
    let order = report
        .extensions
        .iter()
        .map(|extension| extension.name)
        .collect();
    init_globals();
    let mut extensions = RegisteredExtensions::new_with_builtins()?;
    let commands = CommandRegistry::new().freeze()?;
    let initialized = host::initialize_all(report.extensions, data_root, &mut hooks)?;
    extensions.hooks = hooks;
    Ok(Loaded {
        extensions,
        commands,
        initialized,
        order,
    })
}

/// Grants exactly `keys` to `uuid`.
pub fn grants(uuid: Uuid, keys: &[&str]) -> Result<PermissionSubjectIndex, String> {
    let mut set = PermissionSet::default();
    for key in keys {
        set.allow(PermissionKey::parse(*key).map_err(|e| e.to_string())?);
    }
    let mut index = PermissionSubjectIndex::new();
    index.set(uuid, PermissionSubjectState::new(Vec::new(), set));
    Ok(index)
}

/// A running server at 200 TPS with one joined creative-capable player.
pub struct Running {
    pub server: Arc<Server>,
    pub player: Arc<Player>,
    task: JoinHandle<()>,
    _guard: CancelOnDrop,
}

/// Starts a server over `home` and admits one player named `name`.
pub async fn start(
    home: &Arc<World>,
    (extensions, commands): (RegisteredExtensions, FrozenCommands),
    permissions: PermissionSubjectIndex,
    uuid: Uuid,
    name: &str,
) -> Result<Running, String> {
    let domain = ResolvedDomainConfig {
        name: "home".into(),
        default_world: home.key.clone(),
        worlds: vec![home.key.clone()],
    };
    let config = Arc::new(RuntimeConfig {
        online_mode: false,
        encryption: false,
        view_distance: 2,
        simulation_distance: 2,
        max_players: 1,
        packet_workers: Some(1),
        compression: None,
        ..Default::default()
    });
    let server = test_server_with_worlds_and_config(
        "home".into(),
        from_ref(&domain),
        from_ref(home),
        permissions,
        config,
        Some((extensions, commands)),
    )
    .await?;
    server.tick_rate_manager.write().set_tick_rate(200.0);
    let session = Arc::new(PlayerSession::new(10, 10));
    let player = Arc::new(Player::new(
        GameProfile::new(uuid, name.into(), Vec::new()).map_err(|e| e.to_string())?,
        Arc::new(PlayerConnection::Other(Box::new(PeerConnection::default()))),
        Arc::clone(&session),
        Arc::clone(home),
        Arc::downgrade(&server),
        Arc::clone(&server.config),
        next_entity_id(),
        ClientInformation::default(),
    ));
    if !session.bind_initial_player(&player) {
        return Err("session refused the player".into());
    }
    server.queue_player_join(Arc::clone(&player));
    let token = server.cancel_token.clone();
    let guard = CancelOnDrop(token.clone());
    let running = Arc::clone(&server);
    let task = tokio::spawn(async move {
        running.run(token).await;
    });
    until(&player, "initial admission", || {
        player.has_joined_world() && !player.is_domain_switching()
    })
    .await?;
    Ok(Running {
        server,
        player,
        task,
        _guard: guard,
    })
}

/// Shuts extensions down and stops the server and the world's chunk workers.
pub async fn stop(
    running: Running,
    extensions: &host::InitializedExtensions,
    home: &Arc<World>,
) -> Result<(), String> {
    extensions.shutdown(&running.server);
    running.server.cancel_token.cancel();
    running.task.await.map_err(|e| e.to_string())?;
    home.chunk_map.stop_generation_refill_loop();
    home.chunk_map.task_tracker.close();
    home.chunk_map.task_tracker.wait().await;
    Ok(())
}
