use std::{
    env::temp_dir,
    io::Cursor,
    path::{Path, PathBuf},
    slice,
    sync::{
        Arc, Weak,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use glam::DVec3;
use steel_protocol::packet_traits::{CompressionInfo, EncodedPacket};
use steel_protocol::packets::game::CRemovePlayerInfo;
use steel_protocol::utils::ConnectionProtocol;
use steel_registry::entity_type::EntityTypeRef;
use steel_registry::item_stack::ItemStack;
use steel_registry::packets::play::{C_ADD_ENTITY, C_PLAYER_INFO_UPDATE, C_SYSTEM_CHAT};
use steel_registry::{
    vanilla_blocks, vanilla_dimension_types, vanilla_entities, vanilla_game_rules::RESPAWN_RADIUS,
    vanilla_items,
};
use steel_utils::{BlockPos, ChunkPos, types::UpdateFlags};
use steel_utils::{codec::VarInt, serial::ReadFrom, text::DisplayResolutor};
use text_components::TextComponent;
use tokio::{fs, runtime::Builder, task::JoinSet, time::sleep};
use uuid::Uuid;

use crate::behavior::init_behaviors;
use crate::command::execution::{
    CommandArgumentSource, CommandPermissionSource, CommandSource, ExecutionCommandSource,
    parse_entity_selector_text,
};
use crate::command::sender::{CommandExecutionOwner, CommandSender};
use crate::config::{ResolvedDomainConfig, RuntimeConfig, StorageSelection};
use crate::entity::{DEFAULT_MAX_AIR_SUPPLY, Entity, EntityBase, LivingEntity as _, SharedEntity};
use crate::permission::{
    OP_GROUP, PermissionEntry, PermissionExpr, PermissionGroupConfig, PermissionGroupManager,
    PermissionGroupsConfig, PermissionKey, PermissionMetadataSet, PermissionSet,
    PermissionSubjectIndex, PermissionSubjectState,
};
use crate::player::connection::NetworkConnection;
use crate::player::player_data::PersistentSlot;
use crate::player::{Player, PlayerConnection, ResetReason};
use crate::portal::WorldChangeRequest;
use crate::test_support::{
    TestPlayerBuilder, fresh_test_world, fresh_test_world_in_domain, insert_ready_full_chunk,
    test_world,
};
use crate::world::World;

use super::known_players::{
    KnownPlayerSaveStep, UncachedPlayerTarget, classify_uncached_player_target, direct_uuid_profile,
};
use super::player_admission::{PendingPlayerJoin, PlayerAdmissionState};
use super::{
    AsyncMutex, CancellationToken, ChunkSender, CommandRegistry, CommandRequest,
    CommandRequestQueue, DomainCommandStorage, DomainPlayerData, DomainPlayerState,
    DomainScoreboards, EnderPearlRestoreJob, FxHashMap, KeyStore, KnownPlayerCacheState,
    KnownPlayers, Notify, PacketProcessor, PersistentEnderPearl, PersistentEntity,
    PersistentPlayerData, PersistentRootVehicle, PlayerDataStorage, PlayerDisconnectQueue,
    PlayerJoinQueue, PlayerMap, PreparedSpawn, RegistryCache, RootVehicleRestoreJob, Server,
    ServerJobQueue, ServiceKeyStore, SyncMutex, SyncRwLock, TabListTickStats, TickRateManager,
    UnpreparedDomainPlayerData, UnpreparedDomainPlayerState, WorldMap,
    can_entity_return_from_end_to_overworld, cap_positive_thread_count,
    create_registered_dispatcher, is_allowed_to_enter_portal_target, is_end_return_transition,
    offline_uuid, portal_entity_still_valid, validate_player_permission_group_update,
};

struct TestConnection {
    sent_packets: Arc<SyncMutex<Vec<EncodedPacket>>>,
    closed: AtomicBool,
}

impl NetworkConnection for TestConnection {
    fn compression(&self) -> Option<CompressionInfo> {
        None
    }

    fn send_encoded(&self, packet: EncodedPacket) {
        self.sent_packets.lock().push(packet);
    }

    fn send_encoded_bundle(&self, packets: Vec<EncodedPacket>) {
        self.sent_packets.lock().extend(packets);
    }

    fn disconnect_with_reason(&self, _reason: TextComponent) {}

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

struct RecordingConnection {
    packets: Arc<SyncMutex<Vec<EncodedPacket>>>,
    closed: bool,
}

impl NetworkConnection for RecordingConnection {
    fn compression(&self) -> Option<CompressionInfo> {
        None
    }

    fn send_encoded(&self, packet: EncodedPacket) {
        self.packets.lock().push(packet);
    }

    fn send_encoded_bundle(&self, packets: Vec<EncodedPacket>) {
        self.packets.lock().extend(packets);
    }

    fn disconnect_with_reason(&self, _reason: TextComponent) {}

    fn tick(&self) {}

    fn latency(&self) -> i32 {
        0
    }

    fn close(&self) {}

    fn closed(&self) -> bool {
        self.closed
    }
}

fn test_runtime_config() -> Arc<RuntimeConfig> {
    Arc::new(RuntimeConfig {
        max_players: 1,
        view_distance: 2,
        simulation_distance: 2,
        max_chained_neighbor_updates: 1_000_000,
        online_mode: false,
        auth_server: None,
        profile_server: None,
        services_server: None,
        encryption: false,
        allow_flight: false,
        motd: String::new(),
        use_favicon: false,
        favicon: String::new(),
        enforce_secure_chat: false,
        chat_spam_threshold_seconds: 10,
        command_spam_threshold_seconds: 10,
        compression: None,
        server_links: None,
        packet_workers: Some(1),
        chunk_generation_threads: Some(1),
        chunk_encoding_threads: Some(1),
    })
}

fn test_storage_root(name: &str) -> PathBuf {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_nanos());
    temp_dir().join(format!("steel-server-{name}-{unique}"))
}

async fn test_server(
    world: Arc<World>,
    player_permission_states: PermissionSubjectIndex,
    storage_root: &Path,
) -> Result<Arc<Server>, String> {
    let domain = ResolvedDomainConfig {
        name: world.domain().to_owned(),
        default_world: world.key.clone(),
        worlds: vec![world.key.clone()],
    };
    test_server_with_worlds(
        domain.name.clone(),
        slice::from_ref(&domain),
        slice::from_ref(&world),
        player_permission_states,
        storage_root,
    )
    .await
}

async fn test_server_with_worlds(
    default_domain: String,
    domains: &[ResolvedDomainConfig],
    loaded_worlds: &[Arc<World>],
    player_permission_states: PermissionSubjectIndex,
    storage_root: &Path,
) -> Result<Arc<Server>, String> {
    let mut worlds = WorldMap::new(default_domain, domains, &[]);
    for world in loaded_worlds {
        worlds.insert(world.key.clone(), Arc::clone(world));
    }
    let scoreboards = DomainScoreboards::load(&worlds)
        .await
        .map_err(|error| format!("test scoreboards should load: {error}"))?;
    let command_storage = DomainCommandStorage::load(&worlds)
        .await
        .map_err(|error| format!("test command storage should load: {error}"))?;
    let player_data_storage = PlayerDataStorage::new(
        storage_root.to_owned(),
        StorageSelection::default_player_file(),
    )
    .await
    .map_err(|error| format!("test player storage should initialize: {error}"))?;
    let registered_commands = create_registered_dispatcher(CommandRegistry::new())
        .map_err(|error| format!("test commands should register: {error}"))?;
    let command_permission_keys = registered_commands
        .permissions
        .iter()
        .map(|permission| permission.as_str().to_owned())
        .collect();
    let permission_groups = PermissionGroupManager::transient(PermissionGroupsConfig::default())
        .map_err(|error| format!("test permission groups should resolve: {error}"))?;
    let config = test_runtime_config();
    let registry_cache = RegistryCache::new(config.compression);

    Ok(Arc::new(Server {
        config,
        permission_groups,
        cancel_token: CancellationToken::new(),
        key_store: KeyStore::create(),
        registry_cache,
        worlds,
        online_players: PlayerMap::new(),
        player_admissions: SyncMutex::new(FxHashMap::default()),
        tick_rate_manager: SyncRwLock::new(TickRateManager::new()),
        scoreboards,
        command_storage,
        command_dispatcher: SyncRwLock::new(registered_commands.dispatcher),
        command_permission_keys,
        command_requests: CommandRequestQueue::new(),
        packet_processor: PacketProcessor::new(),
        chunk_encoding_pool: Arc::new(
            rayon::ThreadPoolBuilder::new()
                .num_threads(1)
                .build()
                .expect("test chunk encoding pool should initialize"),
        ),
        jobs: ServerJobQueue::new(),
        player_data_storage,
        player_permission_states: SyncRwLock::new(player_permission_states),
        player_permission_updates: AsyncMutex::new(()),
        known_players: SyncMutex::new(KnownPlayerCacheState::new(KnownPlayers::new())),
        known_player_save_idle: Notify::new(),
        profile_lookup_client: reqwest::Client::new(),
        service_keys: Arc::new(
            ServiceKeyStore::new(None).expect("test services key store should initialize"),
        ),
        pending_player_joins: PlayerJoinQueue::new(),
        pending_player_disconnects: PlayerDisconnectQueue::new(),
        pending_world_changes: SyncMutex::new(Vec::new()),
        pending_domain_switches: SyncMutex::new(Vec::new()),
    }))
}

#[test]
#[expect(
    clippy::too_many_lines,
    reason = "one setup exercises all explicit and implicit saved-location planning branches"
)]
fn saved_location_planning_honors_explicit_world_selection() {
    let saved_world = fresh_test_world_in_domain("target", "saved");
    let selected_world = fresh_test_world_in_domain("target", "selected");
    let runtime = Builder::new_current_thread().enable_all().build();
    let Ok(runtime) = runtime else {
        panic!("test runtime should initialize");
    };
    runtime.block_on(async {
        let domain = ResolvedDomainConfig {
            name: "target".to_owned(),
            default_world: saved_world.key.clone(),
            worlds: vec![saved_world.key.clone(), selected_world.key.clone()],
        };
        let loaded_worlds = [Arc::clone(&saved_world), Arc::clone(&selected_world)];
        let storage_root = test_storage_root("explicit-saved-location");
        let server = test_server_with_worlds(
            domain.name.clone(),
            slice::from_ref(&domain),
            &loaded_worlds,
            PermissionSubjectIndex::new(),
            &storage_root,
        )
        .await;
        let Ok(server) = server else {
            panic!("test server should initialize");
        };

        let uuid = Uuid::from_u128(1);
        let saved_player = test_player(&server, Arc::clone(&saved_world), uuid);
        let saved_position = DVec3::new(8.25, 70.0, 8.75);
        let saved_velocity = DVec3::new(0.25, -0.5, 0.75);
        saved_player.base().set_position_local(saved_position);
        saved_player.set_velocity(saved_velocity);
        saved_player.set_health(7.0);
        let mut saved_data = PersistentPlayerData::from_player(&saved_player);
        let saved_root_uuid = [3; 16];
        saved_data.root_vehicle = Some(PersistentRootVehicle {
            attach: [4; 16],
            entity: test_persistent_entity(&vanilla_entities::MINECART, saved_root_uuid),
        });
        if let Err(error) = server
            .player_data_storage
            .save_domain_data("target", uuid, &saved_data)
            .await
        {
            panic!("saved target-domain data should persist: {error}");
        }

        let mismatch_plan = server
            .load_unprepared_domain_player_state(
                &saved_player,
                "target",
                Some(Arc::clone(&selected_world)),
            )
            .await;
        let Ok(mismatch_plan) = mismatch_plan else {
            panic!("explicit mismatch plan should load");
        };
        assert!(mismatch_plan.explicit_target);
        assert!(Arc::ptr_eq(&mismatch_plan.world, &selected_world));
        let UnpreparedDomainPlayerState {
            world: mismatch_world,
            data: mismatch_data,
            ..
        } = mismatch_plan;
        let mismatch_data = match mismatch_data {
            UnpreparedDomainPlayerData::SavedWithoutLocation { data } => {
                assert_eq!(data.health.to_bits(), 7.0_f32.to_bits());
                data
            }
            _ => panic!("explicit mismatch must use selected-world spawn"),
        };
        let mismatch_spawn = PreparedSpawn {
            position: DVec3::new(-12.5, 65.0, 4.5),
            rotation: (90.0, 0.0),
        };
        let mismatch_request = mismatch_world.request_player_spawn_chunks(mismatch_spawn.position);
        let mismatch_state = DomainPlayerState {
            world: mismatch_world,
            data: DomainPlayerData::SavedWithoutLocation {
                data: mismatch_data,
                spawn: mismatch_spawn,
            },
            spawn_chunk_request: mismatch_request,
        };
        assert!(Server::root_vehicle_to_restore(&mismatch_state).is_none());
        let mismatch_player = test_player(
            &server,
            Arc::clone(&mismatch_state.world),
            Uuid::from_u128(2),
        );
        Server::apply_domain_player_state(&mismatch_player, &mismatch_state);
        assert_eq!(mismatch_player.position(), mismatch_spawn.position);
        assert_eq!(mismatch_player.velocity(), DVec3::ZERO);
        assert_eq!(mismatch_player.get_health().to_bits(), 7.0_f32.to_bits());
        let restores = server.prepare_domain_restores(&mismatch_player, &mismatch_state);
        assert!(restores.root_vehicle.is_none());
        assert!(Server::install_domain_restores(
            &mismatch_player,
            mismatch_player.domain_residence_token(),
            &restores,
        ));
        let mismatch_snapshot = PersistentPlayerData::from_player(&mismatch_player);
        assert!(mismatch_snapshot.root_vehicle.is_none());

        let matching_plan = server
            .load_unprepared_domain_player_state(
                &saved_player,
                "target",
                Some(Arc::clone(&saved_world)),
            )
            .await;
        let Ok(matching_plan) = matching_plan else {
            panic!("matching explicit plan should load");
        };
        assert!(matching_plan.explicit_target);
        assert!(Arc::ptr_eq(&matching_plan.world, &saved_world));
        let UnpreparedDomainPlayerState {
            world: matching_world,
            data: matching_data,
            ..
        } = matching_plan;
        let UnpreparedDomainPlayerData::SavedRestored {
            data: matching_data,
        } = matching_data
        else {
            panic!("matching explicit world should restore saved location");
        };
        let matching_request = matching_world.request_player_spawn_chunks(saved_position);
        let matching_state = DomainPlayerState {
            world: matching_world,
            data: DomainPlayerData::SavedRestored {
                data: matching_data,
            },
            spawn_chunk_request: matching_request,
        };
        let matching_player = test_player(
            &server,
            Arc::clone(&matching_state.world),
            Uuid::from_u128(3),
        );
        Server::apply_domain_player_state(&matching_player, &matching_state);
        assert_eq!(matching_player.position(), saved_position);
        assert_eq!(matching_player.velocity(), saved_velocity);
        assert_eq!(matching_player.get_health().to_bits(), 7.0_f32.to_bits());
        assert_eq!(
            Server::root_vehicle_to_restore(&matching_state)
                .as_ref()
                .map(|root| root.entity.uuid),
            Some(saved_root_uuid)
        );

        let implicit_plan = server
            .load_unprepared_domain_player_state(&saved_player, "target", None)
            .await;
        let Ok(implicit_plan) = implicit_plan else {
            panic!("ordinary domain-switch plan should load");
        };
        assert!(!implicit_plan.explicit_target);
        assert!(Arc::ptr_eq(&implicit_plan.world, &saved_world));
        assert!(matches!(
            &implicit_plan.data,
            UnpreparedDomainPlayerData::SavedRestored { .. }
        ));

        saved_data.world = "target:missing".to_owned();
        if let Err(error) = server
            .player_data_storage
            .save_domain_data("target", uuid, &saved_data)
            .await
        {
            panic!("unavailable saved-world data should persist: {error}");
        }

        let missing_explicit_plan = server
            .load_unprepared_domain_player_state(
                &saved_player,
                "target",
                Some(Arc::clone(&selected_world)),
            )
            .await;
        let Ok(missing_explicit_plan) = missing_explicit_plan else {
            panic!("unavailable saved world should fall back to explicit target");
        };
        assert!(missing_explicit_plan.explicit_target);
        assert!(Arc::ptr_eq(&missing_explicit_plan.world, &selected_world));
        assert!(matches!(
            &missing_explicit_plan.data,
            UnpreparedDomainPlayerData::SavedWithoutLocation { .. }
        ));

        let missing_implicit_plan = server
            .load_unprepared_domain_player_state(&saved_player, "target", None)
            .await;
        let Ok(missing_implicit_plan) = missing_implicit_plan else {
            panic!("unavailable saved world should use domain spawn");
        };
        assert!(!missing_implicit_plan.explicit_target);
        assert!(Arc::ptr_eq(&missing_implicit_plan.world, &saved_world));
        assert!(matches!(
            &missing_implicit_plan.data,
            UnpreparedDomainPlayerData::SavedWithoutLocation { .. }
        ));

        drop(server);
        if let Err(error) = fs::remove_dir_all(&storage_root).await {
            panic!("test storage should be removed: {error}");
        }
    });
}

#[test]
#[expect(
    clippy::too_many_lines,
    reason = "one integration test follows the transition across async storage and chunk scheduling"
)]
fn domain_switch_job_progresses_across_chunk_scheduling_boundaries() {
    let source_world = fresh_test_world_in_domain("source", "spawn");
    let target_world = fresh_test_world_in_domain("target", "spawn");
    let runtime = Builder::new_current_thread().enable_all().build();
    let Ok(runtime) = runtime else {
        panic!("test runtime should initialize");
    };
    runtime.block_on(async {
        let domains = [
            ResolvedDomainConfig {
                name: "source".to_owned(),
                default_world: source_world.key.clone(),
                worlds: vec![source_world.key.clone()],
            },
            ResolvedDomainConfig {
                name: "target".to_owned(),
                default_world: target_world.key.clone(),
                worlds: vec![target_world.key.clone()],
            },
        ];
        let worlds = [Arc::clone(&source_world), Arc::clone(&target_world)];
        let storage_root = test_storage_root("domain-switch-job");
        let server = test_server_with_worlds(
            "source".to_owned(),
            &domains,
            &worlds,
            PermissionSubjectIndex::new(),
            &storage_root,
        )
        .await;
        let Ok(server) = server else {
            panic!("test server should initialize");
        };

        let uuid = Uuid::from_u128(1);
        let player = test_player(&server, Arc::clone(&source_world), uuid);
        assert!(server.online_players.insert(Arc::clone(&player)));
        assert!(source_world.add_player(Arc::clone(&player), ResetReason::InitialJoin));
        let _ = player.mark_joined_world();
        let source_residence = player.domain_residence_token();

        let target_position = DVec3::new(8.5, 70.0, 8.5);
        let mut target_data = PersistentPlayerData::from_player(&player);
        target_data.world = target_world.key.to_string();
        target_data.pos = target_position.to_array();
        let saved = server
            .player_data_storage
            .save_domain_data("target", uuid, &target_data)
            .await;
        if let Err(error) = saved {
            panic!("target-domain data should save: {error}");
        }

        let queued = server.queue_domain_switch(Arc::clone(&player), "target".to_owned());
        assert!(queued.is_ok());
        assert!(player.is_world_change_pending());
        assert!(
            server
                .queue_domain_switch(Arc::clone(&player), "target".to_owned())
                .is_err(),
            "the first admitted relocation must retain exclusive ownership"
        );
        server.process_domain_switches();

        assert_eq!(server.jobs.len(), 1);
        assert!(player.is_domain_switching());
        let target_residence = player.domain_residence_token();
        assert_ne!(source_residence, target_residence);
        assert!(!player.is_domain_residence_current(source_residence));
        assert!(source_world.players.get_by_uuid(&uuid).is_none());
        assert!(target_world.players.get_by_uuid(&uuid).is_none());

        let mut saw_target_admission = false;
        for tick in 1..=10_000 {
            source_world.chunk_map.advance_scheduling();
            target_world.chunk_map.advance_scheduling();
            server.tick_jobs(tick, true);
            if target_world.contains_player(&player) {
                saw_target_admission = true;
                assert!(
                    !player.is_world_change_pending(),
                    "target admission must release the relocation lease before gameplay resumes"
                );
            }
            if server.jobs.is_empty() {
                break;
            }
            sleep(Duration::from_millis(1)).await;
        }

        assert!(server.jobs.is_empty(), "domain switch job should finish");
        assert!(saw_target_admission);
        assert!(player.is_domain_residence_current(target_residence));
        assert!(!player.is_domain_switching());
        assert!(!player.is_world_change_pending());
        assert!(source_world.players.get_by_uuid(&uuid).is_none());
        assert!(
            target_world
                .players
                .get_by_uuid(&uuid)
                .is_some_and(|current| Arc::ptr_eq(&current, &player))
        );
        assert_eq!(player.position(), target_position);

        drop(player);
        drop(server);
        if let Err(error) = fs::remove_dir_all(&storage_root).await {
            panic!("test storage should be removed: {error}");
        }
    });
}

#[test]
fn ender_pearl_restore_waits_while_its_owner_has_no_live_membership() {
    let world = fresh_test_world_in_domain("survival", "spawn");
    let runtime = Builder::new_current_thread().enable_all().build();
    let Ok(runtime) = runtime else {
        panic!("test runtime should initialize");
    };
    runtime.block_on(async {
        let storage_root = test_storage_root("detached-pearl-restore");
        let server = test_server(
            Arc::clone(&world),
            PermissionSubjectIndex::new(),
            &storage_root,
        )
        .await;
        let Ok(server) = server else {
            panic!("test server should initialize");
        };
        let player = test_player(&server, Arc::clone(&world), Uuid::from_u128(1));
        assert!(server.online_players.insert(Arc::clone(&player)));

        let residence_token = player.domain_residence_token();
        let pearl = PersistentEnderPearl {
            world: world.key.to_string(),
            entity: test_persistent_entity(&vanilla_entities::ENDER_PEARL, [7; 16]),
        };
        assert!(player.install_pending_domain_restores(
            residence_token,
            &world,
            None,
            vec![pearl.clone()],
        ));
        let job = EnderPearlRestoreJob::new(
            Arc::clone(&player),
            Arc::clone(&world),
            pearl.entity,
            residence_token,
        );
        let Some(job) = job else {
            panic!("valid pearl data should create a restore job");
        };
        server.jobs.spawn(job);

        server.tick_jobs(1, true);

        assert_eq!(
            server.jobs.len(),
            1,
            "temporary detachment must retain the restore job"
        );
        assert_eq!(player.pending_ender_pearls().len(), 1);
        server.jobs.cancel_all();

        drop(player);
        drop(server);
        if let Err(error) = fs::remove_dir_all(&storage_root).await {
            panic!("test storage should be removed: {error}");
        }
    });
}

#[test]
fn domain_detach_snapshots_pending_restores_before_stale_jobs_finish() {
    let world = fresh_test_world_in_domain("source", "spawn");
    let runtime = Builder::new_current_thread().enable_all().build();
    let Ok(runtime) = runtime else {
        panic!("test runtime should initialize");
    };
    runtime.block_on(async {
        let storage_root = test_storage_root("detached-stale-restores");
        let server = test_server(
            Arc::clone(&world),
            PermissionSubjectIndex::new(),
            &storage_root,
        )
        .await;
        let Ok(server) = server else {
            panic!("test server should initialize");
        };
        let player = test_player(&server, Arc::clone(&world), Uuid::from_u128(1));
        assert!(server.online_players.insert(Arc::clone(&player)));
        assert!(world.add_player(Arc::clone(&player), ResetReason::InitialJoin));
        let _ = player.mark_joined_world();

        let source_token = player.domain_residence_token();
        let root = PersistentRootVehicle {
            attach: [6; 16],
            entity: test_persistent_entity(&vanilla_entities::MINECART, [7; 16]),
        };
        let pearl = PersistentEnderPearl {
            world: world.key.to_string(),
            entity: test_persistent_entity(&vanilla_entities::ENDER_PEARL, [8; 16]),
        };
        assert!(player.install_pending_domain_restores(
            source_token,
            &world,
            Some(root.clone()),
            vec![pearl.clone()],
        ));
        let root_job = RootVehicleRestoreJob::new(
            Arc::clone(&player),
            Arc::clone(&world),
            &root,
            source_token,
        );
        let Some(root_job) = root_job else {
            panic!("valid root vehicle data should create a restore job");
        };
        let pearl_job = EnderPearlRestoreJob::new(
            Arc::clone(&player),
            Arc::clone(&world),
            pearl.entity,
            source_token,
        );
        let Some(pearl_job) = pearl_job else {
            panic!("valid pearl data should create a restore job");
        };
        server.jobs.spawn(root_job);
        server.jobs.spawn(pearl_job);

        let detached = world.detach_player_for_domain_switch(&player);
        let Some((snapshot, target_token)) = detached else {
            panic!("live player should detach");
        };
        assert_ne!(source_token, target_token);
        assert_eq!(
            snapshot.root_vehicle.as_ref().map(|root| root.entity.uuid),
            Some([7; 16])
        );
        assert_eq!(snapshot.ender_pearls.len(), 1);
        assert_eq!(snapshot.ender_pearls[0].entity.uuid, [8; 16]);

        server.tick_jobs(1, true);

        assert!(
            server.jobs.is_empty(),
            "source restore jobs must finish after residence advancement"
        );
        assert!(player.pending_root_vehicle_for_current_world().is_none());
        assert!(player.pending_ender_pearls().is_empty());
        assert!(server.online_players.remove_player_sync(&player).is_some());

        drop(player);
        drop(server);
        if let Err(error) = fs::remove_dir_all(&storage_root).await {
            panic!("test storage should be removed: {error}");
        }
    });
}

#[test]
fn domain_detach_invalidates_an_encoded_source_chunk_batch() {
    let world = fresh_test_world_in_domain("source", "chunk_epoch");
    let center = ChunkPos::new(0, 0);
    insert_ready_full_chunk(&world, center);
    let runtime = Builder::new_current_thread().enable_all().build();
    let Ok(runtime) = runtime else {
        panic!("test runtime should initialize");
    };
    runtime.block_on(async {
        let storage_root = test_storage_root("detached-chunk-epoch");
        let server = test_server(
            Arc::clone(&world),
            PermissionSubjectIndex::new(),
            &storage_root,
        )
        .await;
        let Ok(server) = server else {
            panic!("test server should initialize");
        };
        let (player, sent_packets) = test_player_with_packets(
            &server,
            Arc::clone(&world),
            Uuid::from_u128(1),
            "ChunkTester",
            1,
        );
        assert!(server.online_players.insert(Arc::clone(&player)));
        assert!(world.add_player(Arc::clone(&player), ResetReason::InitialJoin));
        let _ = player.mark_joined_world();

        let old_epoch = *player.chunk_send_epoch.lock();
        let batch = {
            let mut sender = player.chunk_sender.lock();
            sender.prepare_batch(&world, center, &player.chunk_send_epoch)
        };
        let Some(batch) = batch else {
            panic!("ready source chunk should produce a batch");
        };
        let mut encode_cache = FxHashMap::default();
        let encoded = ChunkSender::encode_batch(
            &batch,
            &mut encode_cache,
            None,
            server.chunk_encoding_pool.as_ref(),
        );
        assert!(!encoded.is_empty());
        sent_packets.lock().clear();

        let detached = world.detach_player_for_domain_switch(&player);
        assert!(detached.is_some());
        assert_eq!(*player.chunk_send_epoch.lock(), old_epoch.wrapping_add(1));
        assert!(player.last_tracking_view.lock().is_none());
        assert!(player.chunk_sender.lock().pending_chunks.is_empty());

        let committed = player.chunk_sender.lock().commit_batch(
            &batch,
            encoded,
            &player.connection,
            &player.chunk_send_epoch,
        );
        assert!(committed.is_empty());
        assert!(sent_packets.lock().is_empty());
        assert!(server.online_players.remove_player_sync(&player).is_some());

        drop(player);
        drop(server);
        if let Err(error) = fs::remove_dir_all(&storage_root).await {
            panic!("test storage should be removed: {error}");
        }
    });
}

#[test]
fn detached_domain_switch_owns_disconnect_snapshot_exclusively() {
    let source_world = fresh_test_world_in_domain("source", "spawn");
    let target_world = fresh_test_world_in_domain("target", "spawn");
    let runtime = Builder::new_current_thread().enable_all().build();
    let Ok(runtime) = runtime else {
        panic!("test runtime should initialize");
    };
    runtime.block_on(async {
        let domains = [
            ResolvedDomainConfig {
                name: "source".to_owned(),
                default_world: source_world.key.clone(),
                worlds: vec![source_world.key.clone()],
            },
            ResolvedDomainConfig {
                name: "target".to_owned(),
                default_world: target_world.key.clone(),
                worlds: vec![target_world.key.clone()],
            },
        ];
        let worlds = [Arc::clone(&source_world), Arc::clone(&target_world)];
        let storage_root = test_storage_root("detached-domain-disconnect-owner");
        let server = test_server_with_worlds(
            "source".to_owned(),
            &domains,
            &worlds,
            PermissionSubjectIndex::new(),
            &storage_root,
        )
        .await;
        let Ok(server) = server else {
            panic!("test server should initialize");
        };

        let uuid = Uuid::from_u128(1);
        let player = test_player(&server, Arc::clone(&source_world), uuid);
        player.set_health(7.0);
        player
            .base()
            .set_position_local(DVec3::new(12.5, 70.0, -3.5));
        assert!(server.online_players.insert(Arc::clone(&player)));
        assert!(source_world.add_player(Arc::clone(&player), ResetReason::InitialJoin));
        let _ = player.mark_joined_world();

        assert!(
            server
                .queue_domain_switch(Arc::clone(&player), "target".to_owned())
                .is_ok()
        );
        server.process_domain_switches();
        assert!(!source_world.contains_player(&player));
        assert_eq!(
            server.player_admissions.lock().get(&uuid),
            Some(&PlayerAdmissionState::Relocating)
        );

        player.connection.close();
        server.queue_player_disconnect(Arc::clone(&player));
        let mut disconnect_saves = JoinSet::new();
        server.start_player_disconnect_saves(&mut disconnect_saves);
        assert!(
            disconnect_saves.is_empty(),
            "ordinary disconnect saving must not race the detached domain snapshot"
        );

        server.tick_jobs(1, true);
        assert!(server.jobs.is_empty());
        assert_eq!(
            server.player_admissions.lock().get(&uuid),
            Some(&PlayerAdmissionState::Disconnecting)
        );
        for _ in 0..1_000 {
            server.start_player_disconnect_saves(&mut disconnect_saves);
            if disconnect_saves.is_empty() && server.player_admissions.lock().get(&uuid).is_none() {
                break;
            }
            sleep(Duration::from_millis(1)).await;
        }
        assert!(
            disconnect_saves.is_empty(),
            "the detached snapshot should finish saving"
        );

        let saved = server.player_data_storage.load_domain("source", uuid).await;
        let Ok(Some(saved)) = saved else {
            panic!("the source-domain snapshot should be the active save");
        };
        assert_eq!(saved.health.to_bits(), 7.0_f32.to_bits());
        assert_eq!(
            saved.pos.map(f64::to_bits),
            [12.5_f64.to_bits(), 70.0_f64.to_bits(), (-3.5_f64).to_bits()]
        );

        drop(player);
        drop(server);
        if let Err(error) = fs::remove_dir_all(&storage_root).await {
            panic!("test storage should be removed: {error}");
        }
    });
}

#[test]
#[expect(
    clippy::too_many_lines,
    reason = "the failure-path test follows async transition and disconnect persistence boundaries"
)]
fn failed_target_admission_preserves_only_valid_target_restores() {
    let source_world = fresh_test_world_in_domain("source", "spawn");
    let target_world = fresh_test_world_in_domain("target", "spawn");
    let runtime = Builder::new_current_thread().enable_all().build();
    let Ok(runtime) = runtime else {
        panic!("test runtime should initialize");
    };
    runtime.block_on(async {
        let domains = [
            ResolvedDomainConfig {
                name: "source".to_owned(),
                default_world: source_world.key.clone(),
                worlds: vec![source_world.key.clone()],
            },
            ResolvedDomainConfig {
                name: "target".to_owned(),
                default_world: target_world.key.clone(),
                worlds: vec![target_world.key.clone()],
            },
        ];
        let worlds = [Arc::clone(&source_world), Arc::clone(&target_world)];
        let storage_root = test_storage_root("failed-target-restore-persistence");
        let server = test_server_with_worlds(
            "source".to_owned(),
            &domains,
            &worlds,
            PermissionSubjectIndex::new(),
            &storage_root,
        )
        .await;
        let Ok(server) = server else {
            panic!("test server should initialize");
        };

        let uuid = Uuid::from_u128(1);
        let player = test_player(&server, Arc::clone(&source_world), uuid);
        assert!(server.online_players.insert(Arc::clone(&player)));
        assert!(source_world.add_player(Arc::clone(&player), ResetReason::InitialJoin));
        let _ = player.mark_joined_world();

        let target_root_uuid = [8; 16];
        let target_pearl_uuid = [9; 16];
        let foreign_pearl_uuid = [10; 16];
        let mut target_data = PersistentPlayerData::from_player(&player);
        target_data.world = target_world.key.to_string();
        target_data.pos = [8.5, 70.0, 8.5];
        target_data.root_vehicle = Some(PersistentRootVehicle {
            attach: [11; 16],
            entity: test_persistent_entity(&vanilla_entities::MINECART, target_root_uuid),
        });
        target_data.ender_pearls = vec![
            PersistentEnderPearl {
                world: target_world.key.to_string(),
                entity: test_persistent_entity(&vanilla_entities::ENDER_PEARL, target_pearl_uuid),
            },
            PersistentEnderPearl {
                world: source_world.key.to_string(),
                entity: test_persistent_entity(&vanilla_entities::ENDER_PEARL, foreign_pearl_uuid),
            },
        ];
        if let Err(error) = server
            .player_data_storage
            .save_domain_data("target", uuid, &target_data)
            .await
        {
            panic!("target-domain data should save: {error}");
        }

        let source_pearl_uuid = [12; 16];
        let source_residence = player.domain_residence_token();
        assert!(player.install_pending_domain_restores(
            source_residence,
            &source_world,
            None,
            vec![PersistentEnderPearl {
                world: source_world.key.to_string(),
                entity: test_persistent_entity(&vanilla_entities::ENDER_PEARL, source_pearl_uuid,),
            }],
        ));

        assert!(
            server
                .queue_domain_switch(Arc::clone(&player), "target".to_owned())
                .is_ok()
        );
        server.process_domain_switches();
        assert!(
            target_world.players.insert(Arc::clone(&player)),
            "an injected duplicate target membership should force admission failure"
        );

        for tick in 1..=10_000 {
            source_world.chunk_map.advance_scheduling();
            target_world.chunk_map.advance_scheduling();
            server.tick_jobs(tick, true);
            if server.jobs.is_empty() {
                break;
            }
            sleep(Duration::from_millis(1)).await;
        }
        assert!(server.jobs.is_empty(), "domain switch job should finish");
        assert!(!server.owns_online_player(&player));

        let mut disconnect_saves = JoinSet::new();
        for _ in 0..1_000 {
            server.start_player_disconnect_saves(&mut disconnect_saves);
            if disconnect_saves.is_empty() {
                break;
            }
            sleep(Duration::from_millis(1)).await;
        }
        assert!(
            disconnect_saves.is_empty(),
            "failed target admission should persist its prepared disconnect"
        );

        let saved_target = server.player_data_storage.load_domain("target", uuid).await;
        let Ok(Some(saved_target)) = saved_target else {
            panic!("failed target state should be persisted");
        };
        assert_eq!(
            saved_target
                .root_vehicle
                .as_ref()
                .map(|root| root.entity.uuid),
            Some(target_root_uuid)
        );
        assert_eq!(saved_target.ender_pearls.len(), 1);
        assert_eq!(saved_target.ender_pearls[0].entity.uuid, target_pearl_uuid);
        assert_eq!(
            saved_target.ender_pearls[0].world,
            target_world.key.to_string()
        );

        drop(player);
        drop(server);
        if let Err(error) = fs::remove_dir_all(&storage_root).await {
            panic!("test storage should be removed: {error}");
        }
    });
}

#[test]
fn rejected_queued_domain_switch_retries_deferred_respawn_request() {
    let source_world = fresh_test_world_in_domain("source", "spawn");
    let target_world = fresh_test_world_in_domain("target", "spawn");
    let runtime = Builder::new_current_thread().enable_all().build();
    let Ok(runtime) = runtime else {
        panic!("test runtime should initialize");
    };
    runtime.block_on(async {
        let domains = [
            ResolvedDomainConfig {
                name: "source".to_owned(),
                default_world: source_world.key.clone(),
                worlds: vec![source_world.key.clone()],
            },
            ResolvedDomainConfig {
                name: "target".to_owned(),
                default_world: target_world.key.clone(),
                worlds: vec![target_world.key.clone()],
            },
        ];
        let worlds = [Arc::clone(&source_world), Arc::clone(&target_world)];
        let storage_root = test_storage_root("domain-switch-deferred-respawn");
        let server = test_server_with_worlds(
            "source".to_owned(),
            &domains,
            &worlds,
            PermissionSubjectIndex::new(),
            &storage_root,
        )
        .await;
        let Ok(server) = server else {
            panic!("test server should initialize");
        };

        let player = test_player(&server, Arc::clone(&source_world), Uuid::from_u128(1));
        assert!(server.online_players.insert(Arc::clone(&player)));
        assert!(source_world.add_player(Arc::clone(&player), ResetReason::InitialJoin));
        let _ = player.mark_joined_world();

        assert!(
            server
                .queue_domain_switch(Arc::clone(&player), "target".to_owned())
                .is_ok()
        );
        player.set_health(0.0);
        player.respawn();
        assert_eq!(server.jobs.len(), 0);

        server.process_domain_switches();

        assert!(!player.is_domain_switching());
        assert!(source_world.contains_player(&player));
        assert!(!target_world.contains_player(&player));
        assert_eq!(
            server.jobs.len(),
            1,
            "releasing a dead queued switch must replay the one-shot client respawn request"
        );
        assert!(player.is_world_change_pending());

        server.jobs.cancel_all();
        assert!(!player.is_world_change_pending());

        drop(player);
        drop(server);
        if let Err(error) = fs::remove_dir_all(&storage_root).await {
            panic!("test storage should be removed: {error}");
        }
    });
}

#[test]
fn portal_job_validity_rechecks_vanilla_portal_eligibility() {
    let world = fresh_test_world("portal_revalidation");
    let runtime = Builder::new_current_thread().enable_all().build();
    let Ok(runtime) = runtime else {
        panic!("test runtime should initialize");
    };
    runtime.block_on(async {
        let storage_root = test_storage_root("portal-revalidation");
        let server = test_server(
            Arc::clone(&world),
            PermissionSubjectIndex::new(),
            &storage_root,
        )
        .await;
        let Ok(server) = server else {
            panic!("test server should initialize");
        };
        let player = test_player(&server, Arc::clone(&world), Uuid::from_u128(1));
        assert!(server.online_players.insert(Arc::clone(&player)));
        assert!(world.add_player(Arc::clone(&player), ResetReason::InitialJoin));
        let _ = player.mark_joined_world();
        let entity: SharedEntity = player.clone();
        let Some(pending_token) = entity.begin_pending_world_change() else {
            panic!("live player should acquire a portal relocation token");
        };

        assert!(portal_entity_still_valid(&entity, &world, pending_token));
        player.set_sleeping_pos(steel_utils::BlockPos::new(0, 64, 0));
        assert!(!portal_entity_still_valid(&entity, &world, pending_token));
        player.clear_sleeping_pos();
        player.set_health(0.0);
        assert!(!portal_entity_still_valid(&entity, &world, pending_token));

        assert!(entity.finish_pending_world_change(pending_token));
        world.remove_player_for_world_change(&player);
        assert!(server.online_players.remove_player_sync(&player).is_some());
        drop(player);
        drop(server);
        if let Err(error) = fs::remove_dir_all(&storage_root).await {
            panic!("test storage should be removed: {error}");
        }
    });
}

fn apply_non_default_domain_data(player: &Player) {
    let mut source_data = PersistentPlayerData::from_player(player);
    source_data.remaining_fire_ticks = 40;
    source_data.ticks_frozen = 20;
    source_data.is_in_powder_snow = true;
    source_data.was_in_powder_snow = true;
    source_data.has_visual_fire = true;
    source_data.health = 7.0;
    source_data.abilities.flying_speed = 0.2;
    source_data.abilities.walking_speed = 0.3;
    source_data.inventory = vec![PersistentSlot {
        slot: 0,
        item: ItemStack::new(&vanilla_items::STICK),
    }];
    source_data.selected_slot = 4;
    source_data.food_level = 6;
    source_data.food_saturation_level = 1.0;
    source_data.food_exhaustion_level = 12.0;
    source_data.food_tick_timer = 7;
    source_data.experience_level = 12;
    source_data.experience_progress = 0.5;
    source_data.experience_total = 300;
    source_data.score = 42;
    source_data.seen_credits = true;
    source_data.apply_to_player_without_location(player);
}

fn assert_default_domain_data(player: &Player) {
    let target_data = PersistentPlayerData::from_player(player);
    assert_eq!(target_data.remaining_fire_ticks, 0);
    assert_eq!(target_data.ticks_frozen, 0);
    assert!(!target_data.is_in_powder_snow);
    assert!(!target_data.was_in_powder_snow);
    assert!(!target_data.has_visual_fire);
    assert_eq!(
        target_data.health.to_bits(),
        player.get_max_health().to_bits()
    );
    assert_eq!(
        target_data.abilities.flying_speed.to_bits(),
        0.05_f32.to_bits()
    );
    assert_eq!(
        target_data.abilities.walking_speed.to_bits(),
        0.1_f32.to_bits()
    );
    assert!(target_data.inventory.is_empty());
    assert_eq!(target_data.selected_slot, 0);
    assert_eq!(target_data.food_level, 20);
    assert_eq!(
        target_data.food_saturation_level.to_bits(),
        5.0_f32.to_bits()
    );
    assert_eq!(
        target_data.food_exhaustion_level.to_bits(),
        0.0_f32.to_bits()
    );
    assert_eq!(target_data.food_tick_timer, 0);
    assert_eq!(target_data.experience_level, 0);
    assert_eq!(target_data.experience_progress.to_bits(), 0.0_f32.to_bits());
    assert_eq!(target_data.experience_total, 0);
    assert_eq!(target_data.score, 0);
    assert!(!target_data.seen_credits);
}

#[test]
fn first_domain_visit_resets_domain_scoped_player_data() {
    let source_world = fresh_test_world_in_domain("source", "spawn");
    let target_world = fresh_test_world_in_domain("target", "spawn");
    let runtime = Builder::new_current_thread().enable_all().build();
    let Ok(runtime) = runtime else {
        panic!("test runtime should initialize");
    };
    runtime.block_on(async {
        let domains = [
            ResolvedDomainConfig {
                name: "source".to_owned(),
                default_world: source_world.key.clone(),
                worlds: vec![source_world.key.clone()],
            },
            ResolvedDomainConfig {
                name: "target".to_owned(),
                default_world: target_world.key.clone(),
                worlds: vec![target_world.key.clone()],
            },
        ];
        let worlds = [Arc::clone(&source_world), Arc::clone(&target_world)];
        let storage_root = test_storage_root("first-domain-visit");
        let server = test_server_with_worlds(
            "source".to_owned(),
            &domains,
            &worlds,
            PermissionSubjectIndex::new(),
            &storage_root,
        )
        .await;
        let Ok(server) = server else {
            panic!("test server should initialize");
        };

        let uuid = Uuid::from_u128(1);
        let player = test_player(&server, Arc::clone(&source_world), uuid);
        assert!(server.online_players.insert(Arc::clone(&player)));
        assert!(source_world.add_player(Arc::clone(&player), ResetReason::InitialJoin));
        let _ = player.mark_joined_world();

        apply_non_default_domain_data(&player);

        let target_before_switch = server.player_data_storage.load_domain("target", uuid).await;
        assert!(matches!(target_before_switch, Ok(None)));

        let queued = server.queue_domain_switch(Arc::clone(&player), "target".to_owned());
        assert!(queued.is_ok());
        server.process_domain_switches();

        for tick in 1..=10_000 {
            source_world.chunk_map.advance_scheduling();
            target_world.chunk_map.advance_scheduling();
            server.tick_jobs(tick, true);
            if server.jobs.is_empty() {
                break;
            }
            sleep(Duration::from_millis(1)).await;
        }

        assert!(server.jobs.is_empty(), "domain switch job should finish");
        assert!(Arc::ptr_eq(&player.get_world(), &target_world));

        assert_default_domain_data(&player);

        drop(player);
        drop(server);
        if let Err(error) = fs::remove_dir_all(&storage_root).await {
            panic!("test storage should be removed: {error}");
        }
    });
}

#[test]
fn command_world_scope_survives_entity_transforms() {
    let alpha = fresh_test_world_in_domain("alpha", "spawn");
    let beta = fresh_test_world_in_domain("beta", "spawn");
    let domains = [
        ResolvedDomainConfig {
            name: "alpha".to_owned(),
            default_world: alpha.key.clone(),
            worlds: vec![alpha.key.clone()],
        },
        ResolvedDomainConfig {
            name: "beta".to_owned(),
            default_world: beta.key.clone(),
            worlds: vec![beta.key.clone()],
        },
    ];
    let loaded_worlds = [Arc::clone(&alpha), Arc::clone(&beta)];
    let storage_root = test_storage_root("command-world-scope");
    let runtime = Builder::new_current_thread().enable_all().build();
    let Ok(runtime) = runtime else {
        panic!("test runtime should initialize");
    };
    runtime.block_on(async {
        let server = test_server_with_worlds(
            "alpha".to_owned(),
            &domains,
            &loaded_worlds,
            PermissionSubjectIndex::new(),
            &storage_root,
        )
        .await;
        let Ok(server) = server else {
            panic!("test server should initialize");
        };
        let player = test_player(&server, Arc::clone(&alpha), Uuid::from_u128(30));
        let player_source = CommandSource::new(
            CommandSender::Player(Arc::clone(&player)),
            Arc::clone(&server),
        );

        assert!(
            player_source.with_world(Arc::clone(&alpha)).is_ok(),
            "players may project within their initial domain"
        );
        assert!(
            player_source.with_world(Arc::clone(&beta)).is_err(),
            "players may not project outside their initial domain"
        );

        player.set_world(Arc::clone(&beta));
        let transformed = player_source.with_entity(Arc::clone(&player) as SharedEntity);
        assert!(
            transformed.with_world(Arc::clone(&beta)).is_err(),
            "changing the execution entity must not change the initiating domain"
        );

        let console_source = CommandSource::new(CommandSender::Console, Arc::clone(&server));
        assert!(console_source.with_world(Arc::clone(&beta)).is_ok());
        let rcon_source = CommandSource::new(CommandSender::Rcon, Arc::clone(&server));
        assert!(rcon_source.with_world(Arc::clone(&beta)).is_ok());

        drop((
            transformed,
            player_source,
            player,
            console_source,
            rcon_source,
        ));
        drop(server);
        if let Err(error) = fs::remove_dir_all(&storage_root).await {
            panic!("test storage should be removed: {error}");
        }
    });
}

#[test]
fn execute_as_entity_transform_uses_receiver_with_initiator_permissions() {
    let world = Arc::clone(test_world());
    let runtime = Builder::new_current_thread().enable_all().build();
    let Ok(runtime) = runtime else {
        panic!("test runtime should initialize");
    };
    runtime.block_on(async {
        let initiator_uuid = Uuid::from_u128(31);
        let receiver_uuid = Uuid::from_u128(32);
        let invsee = PermissionExpr::key(permission_key("steel.command.invsee"));
        let modify_key = permission_key("steel.command.invsee.modify");
        let modify = PermissionExpr::key(modify_key.clone());
        let access = PermissionExpr::Any(vec![invsee, modify.clone()]);
        let mut published_states = PermissionSubjectIndex::new();
        published_states.set(
            initiator_uuid,
            PermissionSubjectState::new(
                Vec::new(),
                PermissionSet::from_entries([PermissionEntry::allow(modify_key)]),
            ),
        );
        let storage_root = test_storage_root("command-entity-transform-authorization");
        let server = test_server(Arc::clone(&world), published_states, &storage_root).await;
        let Ok(server) = server else {
            panic!("test server should initialize");
        };
        let (initiator, _) =
            test_player_with_packets(&server, Arc::clone(&world), initiator_uuid, "Initiator", 31);
        let (receiver, _) = test_player_with_packets(&server, world, receiver_uuid, "Receiver", 32);
        assert!(server.online_players.insert(Arc::clone(&initiator)));
        assert!(server.online_players.insert(Arc::clone(&receiver)));

        let initiating_source = CommandSource::new(
            CommandSender::Player(Arc::clone(&initiator)),
            Arc::clone(&server),
        );
        server
            .player_permission_states
            .write()
            .set(initiator_uuid, PermissionSubjectState::default());
        let transformed = initiating_source.with_entity(Arc::clone(&receiver) as SharedEntity);

        let Some(effective_player) = transformed.player() else {
            panic!("a player entity transform should retain an effective player");
        };
        assert!(Arc::ptr_eq(effective_player, &receiver));
        assert!(CommandPermissionSource::has_permission(
            &transformed,
            &access
        ));
        assert!(CommandPermissionSource::has_permission(
            &transformed,
            &modify
        ));

        let receiver_source = CommandSource::new(
            CommandSender::Player(Arc::clone(&receiver)),
            Arc::clone(&server),
        );
        assert!(!CommandPermissionSource::has_permission(
            &receiver_source,
            &access
        ));
        assert!(!CommandPermissionSource::has_permission(
            &receiver_source,
            &modify
        ));

        drop((
            transformed,
            initiating_source,
            receiver_source,
            initiator,
            receiver,
        ));
        drop(server);
        if let Err(error) = fs::remove_dir_all(&storage_root).await {
            panic!("test storage should be removed: {error}");
        }
    });
}

#[test]
#[expect(
    clippy::too_many_lines,
    reason = "one lifecycle test compares gameplay and administrative command ownership through every domain phase"
)]
fn command_gameplay_availability_tracks_exact_domain_residence() {
    let world = fresh_test_world_in_domain("alpha", "spawn");
    let remote_world = fresh_test_world_in_domain("beta", "spawn");
    let domains = [
        ResolvedDomainConfig {
            name: "alpha".to_owned(),
            default_world: world.key.clone(),
            worlds: vec![world.key.clone()],
        },
        ResolvedDomainConfig {
            name: "beta".to_owned(),
            default_world: remote_world.key.clone(),
            worlds: vec![remote_world.key.clone()],
        },
    ];
    let loaded_worlds = [Arc::clone(&world), Arc::clone(&remote_world)];
    let storage_root = test_storage_root("command-domain-residence");
    let runtime = Builder::new_current_thread().enable_all().build();
    let Ok(runtime) = runtime else {
        panic!("test runtime should initialize");
    };
    runtime.block_on(async {
        let server = test_server_with_worlds(
            "alpha".to_owned(),
            &domains,
            &loaded_worlds,
            PermissionSubjectIndex::new(),
            &storage_root,
        )
        .await;
        let Ok(server) = server else {
            panic!("test server should initialize");
        };
        let uuid = Uuid::from_u128(40);
        let player = test_player(&server, Arc::clone(&world), uuid);
        assert!(world.add_player(Arc::clone(&player), ResetReason::InitialJoin));
        let pre_admission_owner =
            CommandExecutionOwner::capture(CommandSender::Player(Arc::clone(&player)), &server);
        assert!(
            !pre_admission_owner.is_current(&server),
            "world membership alone must not admit command work"
        );
        assert!(server.online_players.insert(Arc::clone(&player)));
        let _ = player.mark_joined_world();
        assert!(
            !pre_admission_owner.is_current(&server),
            "an owner rejected at capture must not become valid after admission"
        );
        let (remote, _) = test_player_with_packets(
            &server,
            Arc::clone(&remote_world),
            Uuid::from_u128(42),
            "Remote",
            42,
        );
        assert!(server.online_players.insert(Arc::clone(&remote)));
        assert!(remote_world.add_player(Arc::clone(&remote), ResetReason::InitialJoin));
        let _ = remote.mark_joined_world();

        let selector = parse_entity_selector_text("@a");
        let Ok(selector) = selector else {
            panic!("all-player selector should parse");
        };
        let spatial_selector = parse_entity_selector_text("@a[x=0]");
        let Ok(spatial_selector) = spatial_selector else {
            panic!("spatial all-player selector should parse");
        };
        let source = CommandSource::new(CommandSender::Console, Arc::clone(&server));
        let transformed = source.with_entity(Arc::clone(&player) as SharedEntity);
        let owner =
            CommandExecutionOwner::capture(CommandSender::Player(Arc::clone(&player)), &server);

        assert!(owner.is_current(&server));
        assert!(transformed.execution_is_current());
        assert_eq!(
            selector.find_players(&source).map(|players| players.len()),
            Ok(1)
        );
        assert_eq!(
            selector
                .find_online_profile_players(&source)
                .map(|players| players.len()),
            Ok(2),
            "administrative profile selectors span Steel domains"
        );
        assert_eq!(
            spatial_selector
                .find_online_profile_players(&source)
                .map(|players| players.len()),
            Ok(1),
            "spatial profile selectors use the exact source world"
        );
        assert_eq!(
            source.selector_player_names(),
            vec![player.gameprofile.name.clone()]
        );
        assert!(
            server
                .submit_command(
                    CommandSender::Player(Arc::clone(&player)),
                    "list".to_owned()
                )
                .is_ok()
        );
        assert!(
            server
                .submit_command_suggestions(Arc::clone(&player), 1, "/".to_owned())
                .is_ok()
        );

        let Some(pending_token) = player.begin_pending_world_change() else {
            panic!("test player should acquire the relocation lease");
        };
        assert!(player.begin_domain_switch(pending_token));
        assert!(server.command_world_for_player(&player).is_none());
        assert!(!owner.is_current(&server));
        assert!(!transformed.execution_is_current());
        assert_eq!(
            selector.find_players(&source).map(|players| players.len()),
            Ok(0)
        );
        assert_eq!(
            selector
                .find_online_profile_players(&source)
                .map(|players| players.len()),
            Ok(2),
            "administrative profile selectors retain globally online players"
        );
        assert!(source.selector_player_names().is_empty());
        assert_eq!(server.get_players().len(), 2);
        for _ in 0..2 {
            let Some(request) = server.command_requests.pop_front_runnable(|_| true) else {
                panic!("both captured command requests should remain queued");
            };
            let owner = match request {
                CommandRequest::Execute { owner, .. }
                | CommandRequest::Suggestions { owner, .. } => owner,
            };
            assert!(
                !owner.is_current(&server),
                "requests captured before the transition must be rejected"
            );
        }

        assert!(player.mark_domain_switch_detached(pending_token));
        let Some((_data, _residence)) = world.detach_player_for_domain_switch(&player) else {
            panic!("test player should detach from its source world");
        };
        let detached_owner =
            CommandExecutionOwner::capture(CommandSender::Player(Arc::clone(&player)), &server);
        assert!(player.mark_domain_switch_target_handshake(pending_token));
        assert!(server.command_world_for_player(&player).is_none());
        assert_eq!(
            selector
                .find_online_profile_players(&source)
                .map(|players| players.len()),
            Ok(2),
            "non-spatial profile selectors retain detached online players"
        );
        assert_eq!(
            spatial_selector
                .find_online_profile_players(&source)
                .map(|players| players.len()),
            Ok(0),
            "spatial profile selectors require live source-world membership"
        );
        assert!(
            server
                .submit_command(
                    CommandSender::Player(Arc::clone(&player)),
                    "list".to_owned()
                )
                .is_ok()
        );
        assert!(world.add_player(Arc::clone(&player), ResetReason::WorldChange));
        assert!(player.mark_domain_switch_live(pending_token));

        assert!(server.command_world_for_player(&player).is_some());
        assert!(
            !detached_owner.is_current(&server),
            "work admitted while detached must not become valid after target admission"
        );
        let Some(CommandRequest::Execute {
            owner: handshake_owner,
            ..
        }) = server.command_requests.pop_front_runnable(|_| true)
        else {
            panic!("the handshake request should remain queued for rejection");
        };
        assert!(
            !handshake_owner.is_current(&server),
            "public submissions during the target handshake must stay rejected"
        );
        assert!(
            !owner.is_current(&server),
            "work captured before detachment must stay stale after target admission"
        );
        assert!(!transformed.execution_is_current());
        assert_eq!(
            selector.find_players(&source).map(|players| players.len()),
            Ok(1)
        );
        let target_owner =
            CommandExecutionOwner::capture(CommandSender::Player(Arc::clone(&player)), &server);
        assert!(target_owner.is_current(&server));
        assert!(
            server
                .submit_command_suggestions(Arc::clone(&player), 2, "/old".to_owned())
                .is_ok()
        );

        assert!(player.finish_domain_switch(pending_token));
        assert!(player.finish_pending_world_change(pending_token));
        world.remove_player_for_world_change(&player);
        assert!(server.online_players.remove_player_sync(&player).is_some());
        let replacement = test_player(&server, Arc::clone(&world), uuid);
        assert!(server.online_players.insert(Arc::clone(&replacement)));
        assert!(world.add_player(Arc::clone(&replacement), ResetReason::InitialJoin));
        assert!(
            !target_owner.is_current(&server),
            "a replacement login must not inherit queued work from the old Arc"
        );
        assert!(
            CommandExecutionOwner::capture(
                CommandSender::Player(Arc::clone(&replacement)),
                &server
            )
            .is_current(&server)
        );
        assert!(
            server
                .submit_command_suggestions(Arc::clone(&replacement), 3, "/new".to_owned())
                .is_ok()
        );
        let mut suggestion_owners_are_current = Vec::new();
        for _ in 0..2 {
            let Some(CommandRequest::Suggestions { owner, .. }) =
                server.command_requests.pop_front_runnable(|_| true)
            else {
                panic!("replacement suggestions must not coalesce with the old exact session");
            };
            suggestion_owners_are_current.push(owner.is_current(&server));
        }
        assert_eq!(suggestion_owners_are_current, [false, true]);

        drop((
            replacement,
            player,
            source,
            transformed,
            pre_admission_owner,
            owner,
            detached_owner,
            target_owner,
            remote,
            server,
        ));
        if let Err(error) = fs::remove_dir_all(&storage_root).await {
            panic!("test storage should be removed: {error}");
        }
    });
}

#[test]
#[expect(
    clippy::too_many_lines,
    reason = "one routing test compares stale, repeated, same-domain, and cross-domain selections"
)]
fn player_world_selection_uses_one_token_owned_route() {
    let source_world = fresh_test_world_in_domain("alpha", "source");
    let sibling_world = fresh_test_world_in_domain("alpha", "sibling");
    let stale_sibling_world = fresh_test_world_in_domain("alpha", "sibling");
    let target_world = fresh_test_world_in_domain("beta", "target");
    let domains = [
        ResolvedDomainConfig {
            name: "alpha".to_owned(),
            default_world: source_world.key.clone(),
            worlds: vec![source_world.key.clone(), sibling_world.key.clone()],
        },
        ResolvedDomainConfig {
            name: "beta".to_owned(),
            default_world: target_world.key.clone(),
            worlds: vec![target_world.key.clone()],
        },
    ];
    let loaded_worlds = [
        Arc::clone(&source_world),
        Arc::clone(&sibling_world),
        Arc::clone(&target_world),
    ];
    let storage_root = test_storage_root("player-world-selection");
    let runtime = Builder::new_current_thread().enable_all().build();
    let Ok(runtime) = runtime else {
        panic!("test runtime should initialize");
    };
    runtime.block_on(async {
        let server = test_server_with_worlds(
            "alpha".to_owned(),
            &domains,
            &loaded_worlds,
            PermissionSubjectIndex::new(),
            &storage_root,
        )
        .await;
        let Ok(server) = server else {
            panic!("test server should initialize");
        };
        let player = test_player(&server, Arc::clone(&source_world), Uuid::from_u128(41));
        assert!(server.online_players.insert(Arc::clone(&player)));
        assert!(source_world.add_player(Arc::clone(&player), ResetReason::InitialJoin));
        let _ = player.mark_joined_world();

        assert!(
            server
                .queue_player_world_selection(Arc::clone(&player), Arc::clone(&stale_sibling_world))
                .is_err()
        );
        assert!(!player.is_world_change_pending());

        assert!(
            server
                .queue_player_world_selection(Arc::clone(&player), Arc::clone(&sibling_world))
                .is_ok(),
            "world selection is authorization-neutral after command admission"
        );
        assert!(player.is_world_change_pending());
        assert!(
            server
                .queue_player_world_selection(Arc::clone(&player), Arc::clone(&target_world))
                .is_err(),
            "the relocation lease must reject a repeated selection"
        );
        let sibling_token = {
            let mut pending = server.pending_world_changes.lock();
            let Some((
                _,
                WorldChangeRequest::WorldSpawn {
                    target_world,
                    pending_token,
                },
            )) = pending.pop()
            else {
                panic!("same-domain selection should queue a world-spawn transition");
            };
            assert!(Arc::ptr_eq(&target_world, &sibling_world));
            pending_token
        };
        assert!(player.finish_pending_world_change(sibling_token));

        assert!(
            server
                .queue_player_world_selection(Arc::clone(&player), Arc::clone(&source_world))
                .is_ok()
        );
        server.process_world_changes(0, true);
        assert!(player.is_world_change_pending());
        assert!(source_world.contains_player(&player));
        assert_eq!(
            server.jobs.len(),
            1,
            "a cold world spawn should remain a tick-polled job"
        );
        server.jobs.cancel_all();
        assert!(!player.is_world_change_pending());

        assert!(
            server
                .queue_player_world_selection(Arc::clone(&player), Arc::clone(&target_world))
                .is_ok()
        );
        let request = server.pending_domain_switches.lock().pop();
        let Some(request) = request else {
            panic!("cross-domain selection should queue a domain switch");
        };
        assert_eq!(request.target_domain, "beta");
        assert!(
            request
                .target_world
                .as_ref()
                .is_some_and(|world| Arc::ptr_eq(world, &target_world))
        );
        assert!(player.finish_domain_switch(request.pending_token));
        assert!(player.finish_pending_world_change(request.pending_token));

        drop((player, server));
        if let Err(error) = fs::remove_dir_all(&storage_root).await {
            panic!("test storage should be removed: {error}");
        }
    });
}

#[test]
fn same_domain_world_selection_waits_for_safe_spawn_and_full_chunk_square() {
    let source_world = fresh_test_world_in_domain("alpha", "safe_source");
    let target_world = fresh_test_world_in_domain("alpha", "safe_target");
    init_behaviors();
    {
        let mut level_data = target_world.level_data.write();
        level_data.data_mut().set_spawn_pos(BlockPos::new(0, 64, 0));
        level_data.data_mut().spawn.angle = 37.0;
    }
    assert!(target_world.set_game_rule(&RESPAWN_RADIUS, 0));

    let domain = ResolvedDomainConfig {
        name: "alpha".to_owned(),
        default_world: source_world.key.clone(),
        worlds: vec![source_world.key.clone(), target_world.key.clone()],
    };
    let loaded_worlds = [Arc::clone(&source_world), Arc::clone(&target_world)];
    let storage_root = test_storage_root("safe-same-domain-selection");
    let runtime = Builder::new_current_thread().enable_all().build();
    let Ok(runtime) = runtime else {
        panic!("test runtime should initialize");
    };
    runtime.block_on(async {
        let server = test_server_with_worlds(
            domain.name.clone(),
            slice::from_ref(&domain),
            &loaded_worlds,
            PermissionSubjectIndex::new(),
            &storage_root,
        )
        .await;
        let Ok(server) = server else {
            panic!("test server should initialize");
        };
        let player = test_player(&server, Arc::clone(&source_world), Uuid::from_u128(51));
        assert!(server.online_players.insert(Arc::clone(&player)));
        assert!(source_world.players.insert(Arc::clone(&player)));
        let _ = player.mark_joined_world();

        assert!(
            server
                .queue_player_world_selection(Arc::clone(&player), Arc::clone(&target_world))
                .is_ok()
        );
        server.process_world_changes(0, true);
        assert!(player.is_world_change_pending());
        assert!(source_world.contains_player(&player));
        assert!(!target_world.contains_player(&player));
        assert_eq!(server.jobs.len(), 1);

        for z in -3..=3 {
            for x in -3..=3 {
                insert_ready_full_chunk(&target_world, ChunkPos::new(x, z));
            }
        }
        assert!(target_world.set_block(
            BlockPos::new(0, 64, 0),
            vanilla_blocks::STONE.default_state(),
            UpdateFlags::UPDATE_ALL,
        ));
        assert!(target_world.set_block(
            BlockPos::new(0, 65, 0),
            vanilla_blocks::STONE.default_state(),
            UpdateFlags::UPDATE_ALL,
        ));

        for tick in 1..=1_000 {
            target_world.chunk_map.advance_scheduling();
            server.tick_jobs(tick, true);
            if server.jobs.is_empty() {
                break;
            }
            sleep(Duration::from_millis(1)).await;
        }

        assert!(server.jobs.is_empty());
        assert!(!player.is_world_change_pending());
        assert!(!source_world.contains_player(&player));
        assert!(target_world.contains_player(&player));
        assert_eq!(player.position(), DVec3::new(0.5, 66.0, 0.5));
        assert_eq!(player.rotation(), (37.0, 0.0));

        target_world.remove_player_for_world_change(&player);
        assert!(server.online_players.remove_player_sync(&player).is_some());
        source_world.chunk_map.stop_generation_refill_loop();
        target_world.chunk_map.stop_generation_refill_loop();
        source_world.chunk_map.task_tracker.close();
        target_world.chunk_map.task_tracker.close();
        tokio::join!(
            source_world.chunk_map.task_tracker.wait(),
            target_world.chunk_map.task_tracker.wait(),
        );

        drop((player, server));
        if let Err(error) = fs::remove_dir_all(&storage_root).await {
            panic!("test storage should be removed: {error}");
        }
    });
}

fn test_player(server: &Arc<Server>, world: Arc<World>, uuid: Uuid) -> Arc<Player> {
    test_player_with_packets(server, world, uuid, "TestPlayer", 1).0
}

fn test_persistent_entity(entity_type: EntityTypeRef, uuid: [u8; 16]) -> PersistentEntity {
    PersistentEntity {
        entity_type: entity_type.key.clone(),
        uuid,
        pos: [4.0, 65.0, 6.0],
        motion: [0.0, 0.0, 0.0],
        rotation: [0.0, 0.0],
        fall_distance: 0.0,
        remaining_fire_ticks: 0,
        ticks_frozen: 0,
        is_in_powder_snow: false,
        was_in_powder_snow: false,
        has_visual_fire: false,
        on_ground: true,
        no_gravity: false,
        invulnerable: false,
        air_supply: DEFAULT_MAX_AIR_SUPPLY,
        portal_cooldown: 0,
        custom_name_nbt: Vec::new(),
        custom_name_visible: false,
        silent: false,
        glowing: false,
        tags: Vec::new(),
        custom_data_nbt: Vec::new(),
        nbt_data: Vec::new(),
        passengers: Vec::new(),
    }
}

fn test_player_with_connection(
    server: &Arc<Server>,
    world: Arc<World>,
    uuid: Uuid,
    name: &str,
    entity_id: i32,
    connection: Arc<PlayerConnection>,
) -> Arc<Player> {
    TestPlayerBuilder::new(world, uuid, name, entity_id)
        .connection(connection)
        .server(server)
        .build()
}

fn test_player_with_packets(
    server: &Arc<Server>,
    world: Arc<World>,
    uuid: Uuid,
    name: &str,
    entity_id: i32,
) -> (Arc<Player>, Arc<SyncMutex<Vec<EncodedPacket>>>) {
    let sent_packets = Arc::new(SyncMutex::new(Vec::new()));
    let connection = Arc::new(PlayerConnection::Other(Box::new(TestConnection {
        sent_packets: Arc::clone(&sent_packets),
        closed: AtomicBool::new(false),
    })));
    let player = test_player_with_connection(server, world, uuid, name, entity_id, connection);
    (player, sent_packets)
}

fn decode_system_chat(packet: &EncodedPacket) -> TextComponent {
    let mut cursor = Cursor::new(packet.encoded_data.as_slice());
    let packet_length = VarInt::read(&mut cursor);
    assert!(packet_length.is_ok(), "packet length should decode");
    let packet_id = VarInt::read(&mut cursor);
    let Ok(packet_id) = packet_id else {
        panic!("packet id should decode");
    };
    assert_eq!(packet_id.0, C_SYSTEM_CHAT, "packet should be system chat");
    let component = TextComponent::read(&mut cursor);
    let Ok(component) = component else {
        panic!("system chat component should decode");
    };
    component
}

fn packet_id(packet: &EncodedPacket) -> i32 {
    let mut cursor = Cursor::new(packet.encoded_data.as_slice());
    assert!(
        VarInt::read(&mut cursor).is_ok(),
        "packet length should decode"
    );
    match VarInt::read(&mut cursor) {
        Ok(packet_id) => packet_id.0,
        Err(error) => panic!("packet id should decode: {error}"),
    }
}

#[test]
fn initial_player_info_precedes_entity_spawn_for_existing_players() {
    let world = fresh_test_world("join_player_info_before_spawn");
    let runtime = Builder::new_current_thread().enable_all().build();
    let Ok(runtime) = runtime else {
        panic!("test runtime should initialize");
    };

    runtime.block_on(async {
        let storage_root = test_storage_root("join-player-info-before-spawn");
        let server = test_server(
            Arc::clone(&world),
            PermissionSubjectIndex::new(),
            &storage_root,
        )
        .await;
        let Ok(server) = server else {
            panic!("test server should initialize");
        };
        let (existing, existing_packets) = test_player_with_packets(
            &server,
            Arc::clone(&world),
            Uuid::from_u128(1),
            "ExistingPlayer",
            1,
        );
        assert!(server.online_players.insert(Arc::clone(&existing)));
        assert!(world.add_player(Arc::clone(&existing), ResetReason::InitialJoin));
        let _ = existing.mark_joined_world();

        let spawn_position = existing.position();
        let spawn_chunk = ChunkPos::from_entity_pos(spawn_position);
        existing
            .chunk_sender
            .lock()
            .mark_chunk_sent_for_test(spawn_chunk);
        existing_packets.lock().clear();

        let joining = test_player_with_packets(
            &server,
            Arc::clone(&world),
            Uuid::from_u128(2),
            "JoiningPlayer",
            2,
        )
        .0;
        assert!(server.reserve_player_join(&joining));
        let spawn = PreparedSpawn {
            position: spawn_position,
            rotation: (0.0, 0.0),
        };
        let state = DomainPlayerState {
            world: Arc::clone(&world),
            data: DomainPlayerData::FirstVisit { spawn },
            spawn_chunk_request: world.request_player_spawn_chunks(spawn_position),
        };

        server.finish_prepared_player_join(PendingPlayerJoin {
            player: Arc::clone(&joining),
            state: Ok(state),
        });

        assert!(
            world.players.get_by_uuid(&joining.gameprofile.id).is_some(),
            "joining player should enter the world"
        );
        let packet_ids = existing_packets
            .lock()
            .iter()
            .map(packet_id)
            .collect::<Vec<_>>();
        let Some(player_info_index) = packet_ids
            .iter()
            .position(|packet_id| *packet_id == C_PLAYER_INFO_UPDATE)
        else {
            panic!("existing player should receive joining player info");
        };
        let Some(entity_spawn_index) = packet_ids
            .iter()
            .position(|packet_id| *packet_id == C_ADD_ENTITY)
        else {
            panic!("existing player should receive joining player entity spawn");
        };
        assert!(
            player_info_index < entity_spawn_index,
            "player info must precede the entity spawn; packet ids: {packet_ids:?}"
        );

        if let Err(error) = server.flush_known_players().await {
            panic!("known player cache should flush before test teardown: {error}");
        }
        drop(joining);
        drop(existing);
        drop(server);
        if let Err(error) = fs::remove_dir_all(&storage_root).await {
            panic!("test storage should be removed: {error}");
        }
    });
}

#[test]
fn initial_admission_installs_restores_before_scheduling_jobs() {
    let world = fresh_test_world_in_domain("survival", "spawn");
    let runtime = Builder::new_current_thread().enable_all().build();
    let Ok(runtime) = runtime else {
        panic!("test runtime should initialize");
    };
    runtime.block_on(async {
        let storage_root = test_storage_root("initial-admission-restores");
        let server = test_server(
            Arc::clone(&world),
            PermissionSubjectIndex::new(),
            &storage_root,
        )
        .await;
        let Ok(server) = server else {
            panic!("test server should initialize");
        };
        let joining = test_player(&server, Arc::clone(&world), Uuid::from_u128(1));
        assert!(server.reserve_player_join(&joining));

        let root_uuid = [13; 16];
        let pearl_uuid = [14; 16];
        let mut data = PersistentPlayerData::from_player(&joining);
        data.world = world.key.to_string();
        data.root_vehicle = Some(PersistentRootVehicle {
            attach: [15; 16],
            entity: test_persistent_entity(&vanilla_entities::MINECART, root_uuid),
        });
        data.ender_pearls = vec![PersistentEnderPearl {
            world: world.key.to_string(),
            entity: test_persistent_entity(&vanilla_entities::ENDER_PEARL, pearl_uuid),
        }];
        let spawn_position = DVec3::new(data.pos[0], data.pos[1], data.pos[2]);
        let state = DomainPlayerState {
            world: Arc::clone(&world),
            data: DomainPlayerData::SavedRestored {
                data: Box::new(data),
            },
            spawn_chunk_request: world.request_player_spawn_chunks(spawn_position),
        };

        server.finish_prepared_player_join(PendingPlayerJoin {
            player: Arc::clone(&joining),
            state: Ok(state),
        });

        assert!(world.contains_player(&joining));
        assert_eq!(
            joining
                .pending_root_vehicle_for_current_world()
                .as_ref()
                .map(|root| root.entity.uuid),
            Some(root_uuid)
        );
        assert_eq!(joining.pending_ender_pearls().len(), 1);
        assert_eq!(joining.pending_ender_pearls()[0].entity.uuid, pearl_uuid);
        assert_eq!(server.jobs.len(), 2);
        server.jobs.cancel_all();

        if let Err(error) = server.flush_known_players().await {
            panic!("known player cache should flush before test teardown: {error}");
        }
        drop(joining);
        drop(server);
        if let Err(error) = fs::remove_dir_all(&storage_root).await {
            panic!("test storage should be removed: {error}");
        }
    });
}

#[test]
fn player_disconnect_detaches_before_async_persistence() {
    let world = fresh_test_world("disconnect_safe_point");
    let runtime = Builder::new_current_thread().enable_all().build();
    let Ok(runtime) = runtime else {
        panic!("test runtime should initialize");
    };

    runtime.block_on(async {
        let storage_root = test_storage_root("disconnect-safe-point");
        let server = test_server(
            Arc::clone(&world),
            PermissionSubjectIndex::new(),
            &storage_root,
        )
        .await;
        let Ok(server) = server else {
            panic!("test server should initialize");
        };
        let player = test_player(&server, Arc::clone(&world), Uuid::from_u128(1));

        assert!(server.online_players.insert(Arc::clone(&player)));
        assert!(world.add_player(Arc::clone(&player), ResetReason::InitialJoin));
        let _ = player.mark_joined_world();

        let pending = server.process_player_disconnect(Arc::clone(&player));

        assert!(pending.is_some());
        assert!(
            server
                .online_players
                .get_by_uuid(&player.gameprofile.id)
                .is_none()
        );
        assert!(world.players.get_by_uuid(&player.gameprofile.id).is_none());
        assert!(world.get_entity_by_id(player.id()).is_none());

        drop(pending);
        drop(player);
        drop(server);
        if let Err(error) = fs::remove_dir_all(&storage_root).await {
            panic!("test storage should be removed: {error}");
        }
    });
}

#[test]
fn simultaneous_disconnects_batch_tab_list_removal() {
    let world = fresh_test_world("batched_disconnects");
    let runtime = Builder::new_current_thread().enable_all().build();
    let Ok(runtime) = runtime else {
        panic!("test runtime should initialize");
    };

    runtime.block_on(async {
        let storage_root = test_storage_root("batched-disconnects");
        let server = test_server(
            Arc::clone(&world),
            PermissionSubjectIndex::new(),
            &storage_root,
        )
        .await;
        let Ok(server) = server else {
            panic!("test server should initialize");
        };

        let survivor_packets = Arc::new(SyncMutex::new(Vec::new()));
        let survivor = test_player_with_connection(
            &server,
            Arc::clone(&world),
            Uuid::from_u128(1),
            "TestPlayer",
            1,
            Arc::new(PlayerConnection::Other(Box::new(RecordingConnection {
                packets: Arc::clone(&survivor_packets),
                closed: false,
            }))),
        );
        let first_uuid = Uuid::from_u128(2);
        let first = test_player_with_connection(
            &server,
            Arc::clone(&world),
            first_uuid,
            "TestPlayer",
            2,
            Arc::new(PlayerConnection::Other(Box::new(RecordingConnection {
                packets: Arc::new(SyncMutex::new(Vec::new())),
                closed: true,
            }))),
        );
        let second_uuid = Uuid::from_u128(3);
        let second = test_player_with_connection(
            &server,
            Arc::clone(&world),
            second_uuid,
            "TestPlayer",
            3,
            Arc::new(PlayerConnection::Other(Box::new(RecordingConnection {
                packets: Arc::new(SyncMutex::new(Vec::new())),
                closed: true,
            }))),
        );

        for player in [&survivor, &first, &second] {
            assert!(server.online_players.insert(Arc::clone(player)));
            assert!(world.add_player(Arc::clone(player), ResetReason::InitialJoin));
            let _ = player.mark_joined_world();
        }
        survivor_packets.lock().clear();

        server.queue_player_disconnect(Arc::clone(&first));
        server.queue_player_disconnect(Arc::clone(&second));
        let pending = server.process_player_disconnects();

        assert_eq!(pending.len(), 2);
        {
            let packets = survivor_packets.lock();
            assert_eq!(packets.len(), 3);
            for packet in &packets[..2] {
                assert_eq!(
                    decode_system_chat(packet).to_plain(&DisplayResolutor),
                    "TestPlayer left the game"
                );
            }
            let expected = EncodedPacket::from_bare(
                CRemovePlayerInfo {
                    uuids: vec![first_uuid, second_uuid],
                },
                None,
                ConnectionProtocol::Play,
            );
            let Ok(expected) = expected else {
                panic!("expected player removal packet should encode");
            };
            assert_eq!(
                packets[2].encoded_data.as_slice(),
                expected.encoded_data.as_slice()
            );
        }

        drop(pending);
        drop(first);
        drop(second);
        drop(survivor);
        drop(server);
        if let Err(error) = fs::remove_dir_all(&storage_root).await {
            panic!("test storage should be removed: {error}");
        }
    });
}

#[test]
fn online_player_snapshot_includes_player_detached_for_end_credits() {
    let world = fresh_test_world("end_credits_shutdown_snapshot");
    let runtime = Builder::new_current_thread().enable_all().build();
    let Ok(runtime) = runtime else {
        panic!("test runtime should initialize");
    };

    runtime.block_on(async {
        let storage_root = test_storage_root("end-credits-shutdown-snapshot");
        let server = test_server(
            Arc::clone(&world),
            PermissionSubjectIndex::new(),
            &storage_root,
        )
        .await;
        let Ok(server) = server else {
            panic!("test server should initialize");
        };
        let player = test_player(&server, Arc::clone(&world), Uuid::from_u128(1));

        assert!(server.online_players.insert(Arc::clone(&player)));
        assert!(world.add_player(Arc::clone(&player), ResetReason::InitialJoin));
        let _ = player.mark_joined_world();

        player.show_end_credits();

        assert!(world.players.get_by_uuid(&player.gameprofile.id).is_none());
        assert!(
            server
                .get_players()
                .iter()
                .any(|online| Arc::ptr_eq(online, &player))
        );
        let pending = server.process_player_disconnect(Arc::clone(&player));
        assert!(pending.is_some());
        assert!(
            server
                .online_players
                .get_by_uuid(&player.gameprofile.id)
                .is_none()
        );

        drop(pending);
        drop(player);
        drop(server);
        if let Err(error) = fs::remove_dir_all(&storage_root).await {
            panic!("test storage should be removed: {error}");
        }
    });
}

struct TestEntity {
    base: EntityBase,
    entity_type: EntityTypeRef,
    projectile_owner_uuid: Option<Uuid>,
}

impl TestEntity {
    fn new(entity_type: EntityTypeRef, projectile_owner_uuid: Option<Uuid>) -> Self {
        Self {
            base: EntityBase::new(1, DVec3::ZERO, entity_type.dimensions, Weak::new()),
            entity_type,
            projectile_owner_uuid,
        }
    }
}

fn permission_key(value: &str) -> PermissionKey {
    match PermissionKey::parse(value) {
        Ok(key) => key,
        Err(error) => panic!("test permission key should parse: {error}"),
    }
}

crate::entity::impl_test_downcast_type!(TestEntity);

impl Entity for TestEntity {
    fn base(&self) -> &EntityBase {
        &self.base
    }

    fn entity_type(&self) -> EntityTypeRef {
        self.entity_type
    }

    fn projectile_owner_uuid(&self) -> Option<Uuid> {
        self.projectile_owner_uuid
    }
}

#[test]
fn positive_thread_count_is_capped_to_available_threads() {
    assert_eq!(cap_positive_thread_count(Some(16), 8), Some(8));
    assert_eq!(cap_positive_thread_count(Some(4), 8), Some(4));
}

#[test]
fn zero_thread_count_keeps_pool_default() {
    assert_eq!(cap_positive_thread_count(Some(0), 8), None);
    assert_eq!(cap_positive_thread_count(None, 8), None);
}

#[test]
fn tab_list_distinguishes_recent_and_five_second_tick_times() {
    let (_, footer) = Server::tab_list_components(TabListTickStats {
        tps: 20.0,
        recent_mspt: 1.02,
        average_mspt: 7.84,
        p95_mspt: 12.31,
    });

    assert_eq!(
        footer.to_plain(&DisplayResolutor),
        "\nTPS: 20.0 | MSPT: 1.02 recent | 7.84 avg (5s) | 12.31 p95\n"
    );
}

#[test]
fn uncached_uuid_target_is_preserved_in_online_mode() {
    let uuid = Uuid::from_u128(0x1234_5678_90ab_cdef_1234_5678_90ab_cdef);
    let target = "1234567890ABCDEF1234567890ABCDEF";

    assert_eq!(
        classify_uncached_player_target(target, true),
        UncachedPlayerTarget::DirectUuid(uuid)
    );
}

#[test]
fn uncached_uuid_target_is_preserved_in_offline_mode() {
    let uuid = Uuid::from_u128(0x1234_5678_90ab_cdef_1234_5678_90ab_cdef);
    let target = "1234567890ABCDEF1234567890ABCDEF";

    assert_eq!(
        classify_uncached_player_target(target, false),
        UncachedPlayerTarget::DirectUuid(uuid)
    );
    assert_ne!(offline_uuid(target), uuid);
}

#[test]
fn uncached_uuid_profile_uses_a_canonical_display_label() {
    let uuid = Uuid::from_u128(0x1234_5678_90ab_cdef_1234_5678_90ab_cdef);
    let profile = direct_uuid_profile(uuid);

    assert_eq!(profile.uuid(), uuid);
    assert_eq!(
        profile.last_known_name(),
        "12345678-90ab-cdef-1234-567890abcdef"
    );
}

#[test]
fn known_player_changes_are_coalesced_while_a_save_is_running() {
    let mut cache = KnownPlayerCacheState::new(KnownPlayers::new());
    assert!(cache.record(Uuid::from_u128(1), "Player1".to_owned()));
    let (_, first_generation) = cache.snapshot();

    for value in 2..=1_000 {
        assert!(!cache.record(Uuid::from_u128(value), format!("Player{value}")));
    }
    assert_eq!(
        cache.finish_save(first_generation),
        KnownPlayerSaveStep::SaveAgain
    );

    let (latest, latest_generation) = cache.snapshot();
    assert_eq!(latest.entries().len(), 1_000);
    assert_eq!(
        cache.finish_save(latest_generation),
        KnownPlayerSaveStep::Finished
    );
}

#[test]
fn known_player_change_cannot_be_lost_when_a_worker_becomes_idle() {
    let mut cache = KnownPlayerCacheState::new(KnownPlayers::new());
    assert!(cache.record(Uuid::from_u128(1), "Player1".to_owned()));
    let (_, generation) = cache.snapshot();
    assert_eq!(cache.finish_save(generation), KnownPlayerSaveStep::Finished);

    assert!(cache.record(Uuid::from_u128(2), "Player2".to_owned()));
}

#[test]
fn known_player_change_during_a_failed_save_gets_a_follow_up() {
    let mut cache = KnownPlayerCacheState::new(KnownPlayers::new());
    assert!(cache.record(Uuid::from_u128(1), "Player1".to_owned()));
    let (_, generation) = cache.snapshot();
    assert!(!cache.record(Uuid::from_u128(2), "Player2".to_owned()));
    assert_eq!(
        cache.finish_save(generation),
        KnownPlayerSaveStep::SaveAgain
    );

    let (_, latest_generation) = cache.snapshot();
    assert_eq!(
        cache.finish_save(latest_generation),
        KnownPlayerSaveStep::Finished
    );
    assert!(cache.record(Uuid::from_u128(3), "Player3".to_owned()));
}

#[test]
fn known_player_cache_closes_only_after_the_worker_is_idle() {
    let mut cache = KnownPlayerCacheState::new(KnownPlayers::new());
    assert!(cache.record(Uuid::from_u128(1), "Player1".to_owned()));
    assert!(cache.close_if_idle().is_none());

    let (_, generation) = cache.snapshot();
    assert_eq!(cache.finish_save(generation), KnownPlayerSaveStep::Finished);
    let final_snapshot = cache
        .close_if_idle()
        .unwrap_or_else(|| panic!("idle cache should close"));
    assert_eq!(final_snapshot.entries().len(), 1);
    assert!(!cache.record(Uuid::from_u128(2), "Player2".to_owned()));
}

#[test]
fn permission_updates_reject_only_new_unknown_group_assignments() {
    let manager = PermissionGroupManager::transient(PermissionGroupsConfig::default());
    let Ok(manager) = manager else {
        panic!("default permission groups should resolve");
    };

    assert!(
        validate_player_permission_group_update::<()>(&manager, &[], &["op".to_owned()]).is_ok()
    );
    assert!(
        validate_player_permission_group_update::<()>(
            &manager,
            &["retired".to_owned()],
            &["retired".to_owned()],
        )
        .is_ok()
    );
    assert!(
        validate_player_permission_group_update::<()>(&manager, &[], &["missing".to_owned()],)
            .is_err()
    );
}

#[test]
fn command_source_and_operator_checks_use_published_subject_state() {
    let world = Arc::clone(test_world());
    let runtime = Builder::new_current_thread().enable_all().build();
    let Ok(runtime) = runtime else {
        panic!("test runtime should initialize");
    };
    runtime.block_on(async {
        let uuid = Uuid::from_u128(1);
        let storage_root = test_storage_root("published-permissions");
        let mut published_states = PermissionSubjectIndex::new();
        published_states.set(uuid, PermissionSubjectState::default());
        let server = test_server(Arc::clone(&world), published_states, &storage_root).await;
        let Ok(server) = server else {
            panic!("test server should initialize");
        };
        let player = test_player(&server, world, uuid);
        let permission = permission_key("minecraft.command.stop");
        let stale_player_permissions =
            PermissionSet::from_entries([PermissionEntry::allow(permission.clone())]);
        player.set_permission_state(
            vec![OP_GROUP.to_owned()],
            PermissionSet::new(),
            PermissionMetadataSet::new(),
            stale_player_permissions,
            PermissionMetadataSet::new(),
        );

        assert!(!player.is_operator());
        let revoked_source = CommandSource::new(
            CommandSender::Player(Arc::clone(&player)),
            Arc::clone(&server),
        );
        assert!(!CommandPermissionSource::has_permission(
            &revoked_source,
            &PermissionExpr::key(permission.clone()),
        ));

        server.player_permission_states.write().set(
            uuid,
            PermissionSubjectState::new(vec![OP_GROUP.to_owned()], PermissionSet::new()),
        );
        player.set_permission_state(
            Vec::new(),
            PermissionSet::new(),
            PermissionMetadataSet::new(),
            PermissionSet::new(),
            PermissionMetadataSet::new(),
        );

        assert!(player.is_operator());
        let granted_source = CommandSource::new(
            CommandSender::Player(Arc::clone(&player)),
            Arc::clone(&server),
        );
        assert!(CommandPermissionSource::has_permission(
            &granted_source,
            &PermissionExpr::key(permission),
        ));

        drop(revoked_source);
        drop(granted_source);
        drop(player);
        drop(server);
        if let Err(error) = fs::remove_dir_all(&storage_root).await {
            panic!("test storage should be removed: {error}");
        }
    });
}

#[test]
fn renamed_join_message_only_reaches_existing_players() {
    let world = Arc::clone(test_world());
    let runtime = Builder::new_current_thread().enable_all().build();
    let Ok(runtime) = runtime else {
        panic!("test runtime should initialize");
    };
    runtime.block_on(async {
        let storage_root = test_storage_root("join-message-recipients");
        let server = test_server(
            Arc::clone(&world),
            PermissionSubjectIndex::new(),
            &storage_root,
        )
        .await;
        let Ok(server) = server else {
            panic!("test server should initialize");
        };
        let (existing_player, existing_packets) = test_player_with_packets(
            &server,
            Arc::clone(&world),
            Uuid::from_u128(1),
            "ExistingPlayer",
            1,
        );
        let (joining_player, joining_packets) =
            test_player_with_packets(&server, world, Uuid::from_u128(2), "NewName", 2);
        assert!(server.online_players.insert(existing_player));
        assert!(server.online_players.insert(Arc::clone(&joining_player)));

        server.broadcast_player_join_message(&joining_player, Some("OldName"));

        {
            let existing_packets = existing_packets.lock();
            assert_eq!(existing_packets.len(), 1);
            let message = decode_system_chat(&existing_packets[0]);
            assert_eq!(
                message.to_plain(&DisplayResolutor),
                "NewName (formerly known as OldName) joined the game"
            );
        }
        assert!(joining_packets.lock().is_empty());

        drop(joining_player);
        drop(server);
        if let Err(error) = fs::remove_dir_all(&storage_root).await {
            panic!("test storage should be removed: {error}");
        }
    });
}

#[tokio::test]
async fn effective_permissions_reflect_published_group_revocation() {
    let mut config = PermissionGroupsConfig::default();
    config.groups.insert(
        "staff".to_owned(),
        PermissionGroupConfig {
            allow: vec!["minecraft.command.stop".to_owned()],
            ..PermissionGroupConfig::default()
        },
    );
    let manager = PermissionGroupManager::transient(config);
    let Ok(manager) = manager else {
        panic!("test permission groups should resolve");
    };
    let subject = PermissionSubjectState::new(vec!["staff".to_owned()], PermissionSet::new());
    let permission = permission_key("minecraft.command.stop");
    let stale_player_snapshot =
        manager.effective_permissions(subject.groups(), subject.overrides());
    assert!(stale_player_snapshot.allows_key(&permission));

    let mut revoked = manager.config_snapshot();
    let Some(staff) = revoked.groups.get_mut("staff") else {
        panic!("test staff group should exist");
    };
    staff.allow.clear();
    assert_eq!(manager.replace_config(revoked).await, Ok(()));

    let command_snapshot = manager.effective_permissions(subject.groups(), subject.overrides());
    assert!(!command_snapshot.allows_key(&permission));
}

#[test]
fn nether_portal_entry_obeys_allow_entering_nether_gamerule() {
    assert!(is_allowed_to_enter_portal_target(false, false));
    assert!(is_allowed_to_enter_portal_target(true, true));
    assert!(!is_allowed_to_enter_portal_target(true, false));
}

#[test]
fn can_teleport_passenger_gate_only_applies_to_end_return() {
    assert!(is_end_return_transition(
        &vanilla_dimension_types::THE_END,
        &vanilla_dimension_types::OVERWORLD
    ));
    assert!(!is_end_return_transition(
        &vanilla_dimension_types::THE_END,
        &vanilla_dimension_types::THE_NETHER
    ));
    assert!(!is_end_return_transition(
        &vanilla_dimension_types::OVERWORLD,
        &vanilla_dimension_types::OVERWORLD
    ));
    assert!(!is_end_return_transition(
        &vanilla_dimension_types::OVERWORLD,
        &vanilla_dimension_types::THE_END
    ));
}

#[test]
fn ender_pearl_end_return_requires_owner_seen_credits_when_owner_is_player() {
    let blocked_owner = Uuid::from_u128(1);
    let allowed_owner = Uuid::from_u128(2);
    let unknown_owner = Uuid::from_u128(3);
    let blocked_pearl = TestEntity::new(&vanilla_entities::ENDER_PEARL, Some(blocked_owner));
    let allowed_pearl = TestEntity::new(&vanilla_entities::ENDER_PEARL, Some(allowed_owner));
    let unknown_owner_pearl = TestEntity::new(&vanilla_entities::ENDER_PEARL, Some(unknown_owner));
    let no_player_owner_pearl = TestEntity::new(&vanilla_entities::ENDER_PEARL, None);
    let item = TestEntity::new(&vanilla_entities::ITEM, Some(blocked_owner));
    let owner_seen_credits = |uuid: &Uuid| match *uuid {
        uuid if uuid == blocked_owner => Some(false),
        uuid if uuid == allowed_owner => Some(true),
        _ => None,
    };

    assert!(!can_entity_return_from_end_to_overworld(
        &blocked_pearl,
        owner_seen_credits
    ));
    assert!(can_entity_return_from_end_to_overworld(
        &allowed_pearl,
        owner_seen_credits
    ));
    assert!(can_entity_return_from_end_to_overworld(
        &unknown_owner_pearl,
        owner_seen_credits
    ));
    assert!(can_entity_return_from_end_to_overworld(
        &no_player_owner_pearl,
        owner_seen_credits
    ));
    assert!(can_entity_return_from_end_to_overworld(
        &item,
        owner_seen_credits
    ));
}
