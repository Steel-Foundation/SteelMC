//! Real native Spleef extension, actual server loops, generated chunks, and a controlled test peer.
#![cfg(feature = "host")]
#![recursion_limit = "256"]
use glam::DVec3;
use std::slice::from_ref;
use std::{
    env::consts::{DLL_PREFIX, DLL_SUFFIX},
    path::PathBuf,
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
    command::{CommandRegistry, sender::CommandSender},
    config::{ResolvedDomainConfig, RuntimeConfig},
    entity::{Entity, LivingEntity, damage::DamageHistory, next_entity_id},
    extension::{ExtensionHooks, RegisteredExtensions},
    level_data::{GameTimeSource, WorldGenerationSettings},
    permission::PermissionSubjectIndex,
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
use steel_protocol::packets::game::{CPlayerInfoUpdate, PlayerAction, SPlayerAction};
use steel_protocol::utils::ConnectionProtocol;
use steel_registry::blocks::properties::Direction;
use steel_registry::vanilla_game_rules::NATURAL_HEALTH_REGENERATION;
use steel_registry::{
    game_profile::GameProfile, item_stack::ItemStack, vanilla_blocks, vanilla_dimension_types,
    vanilla_items,
};
use steel_utils::{
    BlockPos, ChunkPos, Identifier,
    locks::SyncMutex,
    types::InteractionHand,
    types::{Difficulty, GameType, UpdateFlags},
};
use text_components::TextComponent;
use tokio::runtime::{Builder, Runtime};
use tokio::sync::oneshot;
use tokio::time::sleep;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

#[derive(Default)]
struct PeerConnection {
    closed: AtomicBool,
    sent_packets: Arc<SyncMutex<Vec<EncodedPacket>>>,
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

struct Fixture {
    runtime: Arc<Runtime>,
    world: Arc<World>,
    _history: Arc<DamageHistory>,
}
impl Fixture {
    fn new() -> Result<Self, String> {
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
struct CancelOnDrop(CancellationToken);
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
async fn on_tick(
    server: &Server,
    work: impl FnOnce() -> Result<(), String> + Send + 'static,
) -> Result<(), String> {
    let (sender, receiver) = oneshot::channel();
    server.jobs.spawn(OnTick(Some(move || {
        let _ = sender.send(work());
    })));
    receiver.await.map_err(|e| e.to_string())?
}
async fn until(
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
#[test]
#[expect(
    clippy::too_many_lines,
    reason = "the independent native boundary needs one continuous room lifecycle before process-global teardown"
)]
#[ignore = "requires cargo build --manifest-path extension-example/spleef/Cargo.toml"]
fn solo_spleef_creates_lobby_and_arena_fills_bots_restores_state_and_releases_worlds()
-> Result<(), String> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../extension-example/spleef/target/debug")
        .join(format!("{DLL_PREFIX}steel_spleef_extension{DLL_SUFFIX}"));
    let mut hooks = ExtensionHooks::default();
    // SAFETY: the independently built example honors the checked v2 entrypoints;
    // the loader pins it for the lifetime of every callback.
    let extension = unsafe {
        host::load(
            &path,
            "lobby_seconds=1\nround_seconds=10\nending_seconds=1\nplayers=4\n",
            &mut hooks,
        )
    }?;
    init_globals();
    let mut extensions = RegisteredExtensions::new_with_builtins()?;
    let commands = CommandRegistry::new().freeze()?;
    host::initialize_all(vec![extension], &mut hooks)?;
    extensions.hooks = hooks;
    let fixture = Fixture::new()?;
    let runtime = Arc::clone(&fixture.runtime);
    runtime.block_on(async {
        let home = &fixture.world;
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
            PermissionSubjectIndex::new(),
            config,
            Some((extensions, commands)),
        )
        .await?;
        server.tick_rate_manager.write().set_tick_rate(200.0);
        let sent_packets = Arc::new(SyncMutex::new(Vec::new()));
        let session = Arc::new(PlayerSession::new(10, 10));
        let player = Arc::new(Player::new(
            GameProfile::new(Uuid::new_v4(), "SoloSpleef".into(), Vec::new())
                .map_err(|e| e.to_string())?,
            Arc::new(PlayerConnection::Other(Box::new(PeerConnection {
                sent_packets: Arc::clone(&sent_packets),
                ..Default::default()
            }))),
            Arc::clone(&session),
            Arc::clone(home),
            Arc::downgrade(&server),
            Arc::clone(&server.config),
            next_entity_id(),
            ClientInformation::default(),
        ));
        assert!(session.bind_initial_player(&player));
        server.queue_player_join(Arc::clone(&player));
        let token = server.cancel_token.clone();
        let _guard = CancelOnDrop(token.clone());
        let running = Arc::clone(&server);
        let task = tokio::spawn(async move {
            running.run(token).await;
        });
        until(&player, "initial admission", || {
            player.has_joined_world() && !player.is_domain_switching()
        })
        .await?;
        let actor = Arc::clone(&player);
        on_tick(&server, move || {
            actor.set_game_mode(GameType::Creative);
            actor
                .inventory
                .lock()
                .set_selected_item(ItemStack::new(&vanilla_items::DIAMOND));
            actor.set_health(9.0);
            Ok(())
        })
        .await?;
        server
            .submit_command(
                CommandSender::Player(Arc::clone(&player)),
                "spleef join".into(),
            )
            .map_err(|_| "command queue full")?;
        until(&player, "lobby with three bots", || {
            player.get_world().key.to_string() == "spleef_1:lobby"
                && server
                    .get_players()
                    .iter()
                    .filter(|p| p.is_server_controlled())
                    .count()
                    == 3
        })
        .await?;
        let lobby = server
            .worlds
            .get(&Identifier::new_static("spleef_1", "lobby"))
            .ok_or("lobby missing")?;
        let lobby_weak = Arc::downgrade(&lobby);
        drop(lobby);
        until(&player, "arena admission", || {
            player.get_world().key.to_string() == "spleef_1:arena"
                && player.game_mode() == GameType::Survival
        })
        .await?;
        assert!(
            player
                .inventory
                .lock()
                .get_selected_item()
                .is(&vanilla_items::DIAMOND_SHOVEL)
        );
        assert_eq!(server.worlds.len(), 3);
        let arena = server
            .worlds
            .get(&Identifier::new_static("spleef_1", "arena"))
            .ok_or("arena missing")?;
        let arena_weak = Arc::downgrade(&arena);
        drop(arena);
        let starts: Vec<_> = server
            .get_players()
            .into_iter()
            .filter(|p| p.is_server_controlled())
            .map(|p| (p.gameprofile.id(), p.position()))
            .collect();
        until(&player, "bot movement", || {
            server
                .get_players()
                .iter()
                .filter(|p| p.is_server_controlled())
                .any(|p| {
                    starts.iter().any(|(id, pos)| {
                        *id == p.gameprofile.id() && (p.position() - *pos).length_squared() > 0.02
                    })
                })
        })
        .await?;
        let actor = Arc::clone(&player);
        on_tick(&server, move || {
            actor
                .teleport(DVec3::new(0.5, 55.0, 0.5), 0.0, 0.0)
                .map_err(|e| e.to_string())
        })
        .await?;
        until(&player, "spectator elimination", || {
            player.game_mode() == GameType::Spectator
        })
        .await?;
        until(&player, "return and complete world cleanup", || {
            player.get_world().key == home.key
                && !player.is_domain_switching()
                && server.worlds.len() == 1
        })
        .await?;
        assert_eq!(player.game_mode(), GameType::Creative);
        let mode_packets = [
            GameType::Survival,
            GameType::Creative,
            GameType::Adventure,
            GameType::Spectator,
        ]
        .into_iter()
        .map(|mode| {
            EncodedPacket::from_bare(
                CPlayerInfoUpdate::update_game_mode(player.gameprofile.id(), mode as i32),
                None,
                ConnectionProtocol::Play,
            )
            .map(|packet| (mode, packet))
            .map_err(|e| e.to_string())
        })
        .collect::<Result<Vec<_>, _>>()?;
        let client_info_mode = sent_packets.lock().iter().rev().find_map(|packet| {
            mode_packets.iter().find_map(|(mode, expected)| {
                (packet.encoded_data.as_slice() == expected.encoded_data.as_slice())
                    .then_some(*mode)
            })
        });
        assert_eq!(
            (client_info_mode, player.is_invisible()),
            (Some(GameType::Creative), false),
            "return must clear spectator metadata and update the client's player-info mode"
        );
        let actor = Arc::clone(&player);
        on_tick(&server, move || {
            actor.handle_player_action(SPlayerAction {
                action: PlayerAction::SwapItemWithOffhand,
                pos: BlockPos::ZERO,
                direction: Direction::Down,
                sequence: 0,
            });
            let mut inventory = actor.inventory.lock();
            assert!(
                inventory
                    .get_item_in_hand(InteractionHand::OffHand)
                    .is(&vanilla_items::DIAMOND)
            );
            inventory.swap_hands();
            Ok(())
        })
        .await?;
        assert_eq!(player.get_health(), 9.0);
        assert!(
            player
                .inventory
                .lock()
                .get_selected_item()
                .is(&vanilla_items::DIAMOND)
        );
        assert!(
            !server
                .get_players()
                .iter()
                .any(|p| p.is_server_controlled())
        );
        until(&player, "world worker and chunk release", || {
            lobby_weak.upgrade().is_none() && arena_weak.upgrade().is_none()
        })
        .await?;
        // Leaving during creation must also clean up an eventual prepared lobby.
        server
            .submit_command(
                CommandSender::Player(Arc::clone(&player)),
                "spleef join".into(),
            )
            .map_err(|_| "command queue full")?;
        server
            .submit_command(
                CommandSender::Player(Arc::clone(&player)),
                "spleef leave".into(),
            )
            .map_err(|_| "command queue full")?;
        sleep(Duration::from_millis(250)).await;
        until(&player, "cancelled lobby cleanup", || {
            server.worlds.len() == 1 && player.get_world().key == home.key
        })
        .await?;
        server.cancel_token.cancel();
        task.await.map_err(|e| e.to_string())?;
        home.chunk_map.stop_generation_refill_loop();
        home.chunk_map.task_tracker.close();
        home.chunk_map.task_tracker.wait().await;
        Ok(())
    })
}
