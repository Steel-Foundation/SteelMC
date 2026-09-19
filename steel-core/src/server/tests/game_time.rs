use std::path::Path;
use steel_utils::Identifier;

use super::*;
use crate::config::{DomainConfig, WorldEntryConfig};
use crate::level_data::LevelData;
use crate::server::world_tick_workers::WorldTickWorkers;
use crate::test_support::test_domain;
use crate::world::tick_scheduler::TickPriority;
use steel_registry::vanilla_fluids;
use steel_registry::{packets::play::C_SET_TIME, vanilla_world_clocks};

#[test]
#[expect(
    clippy::too_many_lines,
    reason = "one continuous timeline verifies transfer expiry through freeze, steps, and sprint"
)]
fn game_time_domains_freeze_steps_sprint_and_transfer_damage() {
    let first = test_domain("clock_a", &["primary", "derived", "third"]);
    let second = test_domain("clock_b", &["primary"]);
    let other_domain_start_time = 100;
    for _ in 0..other_domain_start_time {
        second.advance_domain_game_times();
    }
    let primary = Arc::clone(first.default_world("clock_a").expect("primary"));
    let derived = Arc::clone(
        first
            .get(&steel_utils::Identifier::new_static("clock_a", "derived"))
            .expect("derived"),
    );
    let domains: Vec<_> = [&first, &second]
        .iter()
        .map(|map| ResolvedDomainConfig {
            name: map.default_domain().to_owned(),
            default_world: map.server_default_world().expect("primary").key.clone(),
            worlds: map.keys().cloned().collect(),
        })
        .collect();
    let loaded: Vec<_> = first.values().chain(second.values()).cloned().collect();
    let runtime = Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");
    runtime.block_on(async {
        let root = test_storage_root("game-time-domain-ticks");
        let server = test_server_with_worlds(
            "clock_a".to_owned(),
            &domains,
            &loaded,
            PermissionSubjectIndex::new(),
            &root,
        )
        .await
        .expect("server");
        let workers = WorldTickWorkers::spawn(server.worlds.values()).expect("workers");
        let player = test_player(&server, Arc::clone(&primary));
        let source = DamageSource::environment(&vanilla_damage_types::GENERIC);
        player
            .living_base()
            .record_last_damage_source(&source, primary.game_time());
        let mut server_iteration = 0;
        let transfer_age = 39;
        for _ in 0..transfer_age {
            server_iteration += 1;
            server
                .tick_worlds_game(&workers, server_iteration, true)
                .await
                .expect("tick");
        }
        assert!(player.last_damage_source().is_some());
        player.reset(Arc::clone(&derived), ResetReason::WorldChange);
        assert!(
            player.last_damage_source().is_some(),
            "same-domain transfer preserves history"
        );
        server.tick_rate_manager.write().set_frozen(true);
        let frozen_iterations = 10;
        for _ in 0..frozen_iterations {
            server_iteration += 1;
            let runs = next_simulation_gate(&server);
            server
                .tick_worlds_game(&workers, server_iteration, runs)
                .await
                .expect("frozen tick");
        }
        assert_eq!(primary.game_time(), 39);
        assert!(player.last_damage_source().is_some());
        assert!(server.tick_rate_manager.write().step_game_if_paused(2));
        for (expected_damage_age, available) in [(40, true), (41, false)] {
            server_iteration += 1;
            let runs = next_simulation_gate(&server);
            server
                .tick_worlds_game(&workers, server_iteration, runs)
                .await
                .expect("step");
            assert_eq!(derived.game_time(), expected_damage_age);
            assert_eq!(player.last_damage_source().is_some(), available);
        }
        let sprint_ticks = 3;
        server
            .tick_rate_manager
            .write()
            .request_game_to_sprint(sprint_ticks);
        for _ in 0..sprint_ticks {
            server_iteration += 1;
            let runs = {
                let mut manager = server.tick_rate_manager.write();
                manager.tick();
                assert!(manager.check_should_sprint_this_tick().0);
                manager.runs_normally()
            };
            server
                .tick_worlds_game(&workers, server_iteration, runs)
                .await
                .expect("sprint");
            server.tick_rate_manager.write().end_tick_work();
        }
        for world in first.values() {
            assert_eq!(world.game_time(), 44);
        }
        assert_eq!(
            second
                .server_default_world()
                .expect("second primary")
                .game_time(),
            other_domain_start_time + 44
        );
        drop(workers);
        server.cancel_token.cancel();
        fs::remove_dir_all(root).await.expect("cleanup");
    });
}

#[test]
fn game_time_full_partial_periodic_packets_keep_world_clocks_independent() {
    let worlds = test_domain("packets", &["primary", "derived"]);
    let primary = worlds.default_world("packets").expect("primary");
    let derived = worlds
        .get(&steel_utils::Identifier::new_static("packets", "derived"))
        .expect("derived");
    let runtime = Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");
    runtime.block_on(async {
        let root = test_storage_root("game-time-packets");
        let server = test_server(Arc::clone(primary), PermissionSubjectIndex::new(), &root)
            .await
            .expect("server");
        let (player, packets) =
            test_player_with_packets(&server, Arc::clone(derived), "ClockTest", next_entity_id());
        assert!(derived.add_player(Arc::clone(&player), ResetReason::InitialJoin));
        packets.lock().clear();
        let periodic_sync_tick = 20;
        for _ in 0..periodic_sync_tick {
            worlds.advance_domain_game_times();
        }
        derived
            .set_clock_total_ticks(&vanilla_world_clocks::OVERWORLD, 6_000)
            .expect("clock");
        derived
            .set_clock_rate(&vanilla_world_clocks::OVERWORLD, 2.0)
            .expect("rate");
        derived
            .set_clock_paused(&vanilla_world_clocks::THE_END, true)
            .expect("pause");
        derived.broadcast_time_sync();
        derived.tick_game(periodic_sync_tick, true);
        let times: Vec<_> = packets
            .lock()
            .iter()
            .filter(|packet| packet_id(packet) == C_SET_TIME)
            .map(|packet| {
                let mut cursor = Cursor::new(packet.encoded_data.as_slice());
                VarInt::read(&mut cursor).expect("length");
                VarInt::read(&mut cursor).expect("id");
                (
                    i64::read(&mut cursor).expect("game time"),
                    VarInt::read(&mut cursor).expect("clock count").0,
                )
            })
            .collect();
        assert_eq!(times, vec![(20, 1), (20, 1), (20, 1), (20, 2), (20, 0)]);
        assert_eq!(
            primary.clock_total_ticks(&vanilla_world_clocks::OVERWORLD),
            Some(0)
        );
        assert_eq!(
            derived.clock_total_ticks(&vanilla_world_clocks::OVERWORLD),
            Some(6_002)
        );
        assert_eq!(
            derived.clock_total_ticks(&vanilla_world_clocks::THE_END),
            Some(0)
        );
        server.cancel_token.cancel();
        fs::remove_dir_all(root).await.expect("cleanup");
    });
}

#[test]
fn game_time_startup_and_chunk_reload_use_the_configured_primary() {
    with_server_runtime(|runtime| {
        runtime.block_on(async {
            let root = test_storage_root("game-time-primary-last");
            let config = || {
                let mut config = RuntimeConfig::clone(&test_runtime_config());
                config.services_server = Some(UNROUTABLE_SERVICES.to_owned());
                config
            };
            let start = async || {
                Server::new(
                    Arc::clone(runtime),
                    CancellationToken::new(),
                    config(),
                    primary_last_worlds_config(&root),
                    PermissionGroupManager::transient(PermissionGroupsConfig::default())
                        .expect("permissions"),
                    BanListManager::transient(),
                    WhitelistManager::transient(),
                )
                .await
                .expect("startup")
            };
            let server = start().await;
            let initial_game_time = 73;
            let ticks_before_save = 3;
            for _ in 0..initial_game_time {
                server.worlds.advance_domain_game_times();
            }
            let derived_key = steel_utils::Identifier::new_static("custom", "derived");
            let derived = server.worlds.get(&derived_key).expect("derived");
            let tick_pos = BlockPos::new(512, 64, 512);
            let chunk_pos = ChunkPos::from_block_pos(tick_pos);
            insert_ready_full_chunk(derived, chunk_pos);
            derived.schedule_block_tick(tick_pos, &vanilla_blocks::STONE, 10, TickPriority::High);
            derived.schedule_fluid_tick(tick_pos, &vanilla_fluids::WATER, 14, TickPriority::Low);
            for _ in 0..ticks_before_save {
                server.worlds.advance_domain_game_times();
            }
            stop_game_time_test_worlds(&server).await;
            let mut saved_chunks = 0;
            for world in server.worlds.values() {
                world.cleanup(&mut saved_chunks).await;
            }
            server.cancel_token.cancel();
            drop(server);
            // The obsolete field must not anchor chunk deadlines during startup.
            write_legacy_game_time(&root.join("custom/worlds/derived/level.toml"), 900_000).await;
            let restarted = start().await;
            for world in restarted.worlds.values() {
                assert_eq!(world.game_time(), initial_game_time + ticks_before_save);
            }
            let derived = restarted.worlds.get(&derived_key).expect("derived");
            derived
                .chunk_map
                .with_full_chunks_in_radius(chunk_pos, 0, || {
                    derived
                        .unpack_scheduled_ticks(chunk_pos)
                        .expect("unpack on the world clock");
                    restarted.worlds.advance_domain_game_times();
                    let snapshot = derived
                        .chunk_map
                        .with_full_chunk(chunk_pos, |chunk| chunk.scheduled_tick_snapshot())
                        .expect("loaded chunk");
                    assert_eq!(
                        snapshot.block[0].delay, 6,
                        "seven saved ticks minus one live tick"
                    );
                    assert_eq!(
                        snapshot.fluid[0].delay, 10,
                        "eleven saved ticks minus one live tick"
                    );
                })
                .await
                .expect("load saved chunk");
            stop_game_time_test_worlds(&restarted).await;
            restarted.cancel_token.cancel();
            drop(restarted);
            fs::remove_dir_all(root).await.expect("cleanup");
        });
    });
}

fn primary_last_worlds_config(root: &Path) -> WorldsConfig {
    game_time_worlds_config(
        root,
        [("derived", false), ("authority", true)]
            .into_iter()
            .map(|(name, default)| WorldEntryConfig {
                name: name.to_owned(),
                generator: Identifier::vanilla_static("flat"),
                default,
                seed: None,
                default_gamemode: None,
                difficulty: None,
                storage: None,
                nether_portal_target: None,
                end_portal_target: None,
                config: default.then(|| {
                    toml::Value::Table(
                        [(
                            "dimension_type".to_owned(),
                            toml::Value::String("minecraft:the_end".to_owned()),
                        )]
                        .into_iter()
                        .collect(),
                    )
                }),
            })
            .collect(),
    )
}

fn game_time_worlds_config(root: &Path, worlds: Vec<WorldEntryConfig>) -> WorldsConfig {
    WorldsConfig {
        save_path: root.to_string_lossy().into_owned(),
        seed: None,
        default_gamemode: None,
        difficulty: None,
        storage: None,
        player_storage: None,
        domains: [(
            "custom".to_owned(),
            DomainConfig {
                default: true,
                seed: None,
                default_gamemode: None,
                difficulty: None,
                storage: None,
                worlds,
            },
        )]
        .into(),
    }
}

fn next_simulation_gate(server: &Server) -> bool {
    let mut manager = server.tick_rate_manager.write();
    manager.tick();
    manager.runs_normally()
}

async fn stop_game_time_test_worlds(server: &Server) {
    for world in server.worlds.values() {
        world.chunk_map.stop_generation_refill_loop();
        world.chunk_map.task_tracker.close();
        world.chunk_map.task_tracker.wait().await;
    }
}

async fn write_legacy_game_time(path: &Path, ticks: i64) {
    let mut saved: toml::Table =
        toml::from_str(&fs::read_to_string(path).await.expect("read derived save"))
            .expect("level data");
    saved.insert("game_time".to_owned(), ticks.into());
    fs::write(
        path,
        toml::to_string(&saved).expect("serialize legacy save"),
    )
    .await
    .expect("write legacy time");
}

#[test]
fn game_time_rejects_ephemeral_primary_before_touching_derived_save() {
    with_server_runtime(|runtime| {
        runtime.block_on(async {
            init_vanilla_registry();
            let root = test_storage_root("game-time-storage-validation");
            let derived_dir = root.join("custom/worlds/derived");
            fs::create_dir_all(&derived_dir)
                .await
                .expect("fixture directory");
            let path = derived_dir.join("level.toml");
            fs::write(
                &path,
                toml::to_string(&LevelData::new_with_seed(7)).expect("level data"),
            )
            .await
            .expect("fixture save");
            write_legacy_game_time(&path, 2_000).await;
            let original = fs::read(&path).await.expect("original save");
            let worlds_config = game_time_worlds_config(
                &root,
                [("derived", "disk", false), ("lobby", "ram", true)]
                    .into_iter()
                    .map(|(name, backend, default)| WorldEntryConfig {
                        name: name.to_owned(),
                        generator: Identifier::vanilla_static("flat"),
                        default,
                        seed: None,
                        default_gamemode: None,
                        difficulty: None,
                        storage: Some(StorageSelection {
                            kind: Identifier::new_static("steel", backend),
                            config: None,
                        }),
                        nether_portal_target: None,
                        end_portal_target: None,
                        config: None,
                    })
                    .collect(),
            );
            let mut config = RuntimeConfig::clone(&test_runtime_config());
            config.services_server = Some(UNROUTABLE_SERVICES.to_owned());
            let cancel = CancellationToken::new();
            let result = Server::new(
                Arc::clone(runtime),
                cancel.clone(),
                config,
                worlds_config,
                PermissionGroupManager::transient(PermissionGroupsConfig::default())
                    .expect("permissions"),
                BanListManager::transient(),
                WhitelistManager::transient(),
            )
            .await;
            cancel.cancel();
            let Err(error) = result else {
                panic!("persistent siblings require a durable primary clock");
            };
            assert!(
                error.contains("custom:lobby must persist level data"),
                "{error}"
            );
            assert!(
                error.contains("custom:derived uses persistent storage"),
                "{error}"
            );
            assert_eq!(fs::read(&path).await.expect("unchanged save"), original);
            assert!(!root.join("custom/worlds/lobby").exists());
            fs::remove_dir_all(root).await.expect("cleanup");
        });
    });
}
