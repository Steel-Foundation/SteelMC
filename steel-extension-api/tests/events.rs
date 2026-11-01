//! Iteration-2 events reaching a native extension through real handlers of a test peer.
#![cfg(feature = "host")]
#![recursion_limit = "256"]
mod common;

use common::library;
use glam::DVec3;
use rustc_hash::FxHashMap;
use std::slice::from_ref;
use std::{
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
    world::explosion::{ExplosionInteraction, ExplosionOptions},
    world::{World, WorldConfig, WorldStorageConfig},
    worldgen::{ChunkGeneratorType, EmptyChunkGenerator},
};
use steel_extension_api::host;
use steel_protocol::packet_traits::EncodedPacket;
use steel_protocol::packets::game::{
    ClickType, HashedStack, PlayerAction, PlayerCommandAction, SContainerClick, SPlayerAction,
    SPlayerCommand, SSetCarriedItem,
};
use steel_registry::blocks::properties::Direction;
use steel_registry::vanilla_game_rules::NATURAL_HEALTH_REGENERATION;
use steel_registry::{
    game_profile::GameProfile, item_stack::ItemStack, vanilla_blocks, vanilla_dimension_types,
    vanilla_items, vanilla_mob_effects,
};
use steel_utils::{
    BlockPos, ChunkPos, Identifier,
    locks::SyncMutex,
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
#[ignore = "requires the extension-example and event-probe debug builds (see README)"]
#[expect(
    clippy::too_many_lines,
    reason = "one linear server session drives every probed binding in order"
)]
fn iteration_two_events_reach_plugins_and_cancellation_keeps_state() -> Result<(), String> {
    let data_root =
        std::env::temp_dir().join(format!("steel-extension-events-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&data_root);
    let sources = [host::ExtensionSource {
        path: library("event_probe"),
        config: None,
    }];
    let mut hooks = ExtensionHooks::default();
    // SAFETY: the probe is built from this repository against the same v3 contract.
    let report = unsafe { host::load_all(&sources, &data_root, &mut hooks) }?;
    assert!(report.skipped.is_empty(), "{:?}", report.skipped);
    init_globals();
    let mut extensions = RegisteredExtensions::new_with_builtins()?;
    let commands = CommandRegistry::new().freeze()?;
    let initialized = host::initialize_all(report.extensions, &data_root, &mut hooks)?;
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
        let session = Arc::new(PlayerSession::new(10, 10));
        let player = Arc::new(Player::new(
            GameProfile::new(Uuid::new_v4(), "EventTester".into(), Vec::new())
                .map_err(|e| e.to_string())?,
            Arc::new(PlayerConnection::Other(Box::new(PeerConnection::default()))),
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
        let world = Arc::clone(home);
        on_tick(&server, move || {
            // Cancelled by the probe: nothing changes.
            if actor.set_game_mode(GameType::Creative) {
                return Err("game mode change should be cancelled".into());
            }
            actor.handle_set_carried_item(SSetCarriedItem { slot: 3 });
            actor
                .inventory
                .lock()
                .set_selected_item(ItemStack::with_count(&vanilla_items::STONE, 5));
            actor.handle_player_action(SPlayerAction {
                action: PlayerAction::DropItem,
                pos: BlockPos::ZERO,
                direction: Direction::Down,
                sequence: 0,
            });
            actor.handle_player_action(SPlayerAction {
                action: PlayerAction::SwapItemWithOffhand,
                pos: BlockPos::ZERO,
                direction: Direction::Down,
                sequence: 0,
            });
            actor.handle_player_command(SPlayerCommand {
                entity_id: actor.id(),
                action: PlayerCommandAction::StartSprinting,
                data: 0,
            });
            world.explode(ExplosionOptions::new(
                DVec3::new(8.5, 65.0, 8.5),
                3.0,
                ExplosionInteraction::Tnt,
            ));
            Ok(())
        })
        .await?;
        assert_eq!(player.game_mode(), GameType::Survival);
        assert_eq!(player.inventory.lock().get_selected_slot(), 0);
        let held = player.inventory.lock().get_selected_item().clone();
        assert!(
            held.is(&vanilla_items::STONE) && held.count() == 5,
            "drop and swap are cancelled"
        );
        assert_eq!(
            home.get_block_state(BlockPos::new(8, 64, 8)),
            vanilla_blocks::STONE.default_state(),
            "the cancelled explosion keeps blocks"
        );

        server
            .submit_command(CommandSender::Console, "xp add EventTester 5 points".into())
            .map_err(|_| "command queue full")?;
        until(&player, "doubled experience", || {
            player.experience.lock().total_points() == 10
        })
        .await?;
        let (motd, _, _) = server.fire_server_list_ping("A Steel Server".into(), 20, 1);
        assert_eq!(motd, "probe motd");

        // Game API round trip: HUD, effects, world visit and a extension menu.
        server
            .submit_command(CommandSender::Console, "probe hud".into())
            .map_err(|_| "command queue full")?;
        until(&player, "probe menu open", || {
            player.open_container_id().is_some()
        })
        .await?;
        assert!(player.has_mob_effect(vanilla_mob_effects::HASTE));
        assert!((player.get_walking_speed() - 0.2).abs() < f32::EPSILON);
        assert!(player.has_tab_list_override());
        let clock = home
            .dimension_type
            .default_clock
            .ok_or("overworld has a clock")?;
        let day_time = home.clock_total_ticks(clock).ok_or("clock missing")?;
        assert!(
            (1000..1200).contains(&day_time),
            "day time {day_time} was set to 1000"
        );
        let container_id = i32::from(player.open_container_id().ok_or("menu closed")?);
        let clicker = Arc::clone(&player);
        on_tick(&server, move || {
            clicker.handle_container_click(SContainerClick {
                container_id,
                state_id: 0,
                slot_num: 0,
                button_num: 0,
                click_type: ClickType::Pickup,
                changed_slots: FxHashMap::default(),
                carried_item: HashedStack::Empty,
            });
            Ok(())
        })
        .await?;
        assert!(
            !player
                .inventory
                .lock()
                .get_items()
                .iter()
                .any(|stack| stack.is(&vanilla_items::DIAMOND)),
            "menu icons never reach the player"
        );

        let log = std::fs::read_to_string(data_root.join("event-probe/events.log"))
            .map_err(|e| e.to_string())?;
        for expected in [
            "world_load:home:overworld",
            "chunk_load",
            "game_mode_change",
            "item_held",
            "drop_item:minecraft:stone",
            "swap_hands",
            "toggle:Sprint:true",
            "server_command:xp add EventTester 5 points",
            "exp_change:5",
            "probe_hud:true",
            "fill:81",
            "inventory_open",
            "menu_click:0:Pickup",
        ] {
            assert!(
                log.lines().any(|line| line == expected),
                "missing {expected:?} in\n{log}"
            );
        }
        assert!(
            log.lines().any(|line| line.starts_with("explosion:")),
            "{log}"
        );

        initialized.shutdown(&server);
        server.cancel_token.cancel();
        task.await.map_err(|e| e.to_string())?;
        home.chunk_map.stop_generation_refill_loop();
        home.chunk_map.task_tracker.close();
        home.chunk_map.task_tracker.wait().await;
        Ok(())
    })
}
