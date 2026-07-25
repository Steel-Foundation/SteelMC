use std::{
    env::temp_dir,
    io::Cursor,
    path::{Path, PathBuf},
    slice,
    sync::{Arc, Weak},
    time::{SystemTime, UNIX_EPOCH},
};

use glam::DVec3;
use steel_protocol::packet_traits::{CompressionInfo, EncodedPacket};
use steel_protocol::packets::game::CRemovePlayerInfo;
use steel_protocol::utils::ConnectionProtocol;
use steel_registry::entity_type::EntityTypeRef;
use steel_registry::packets::play::{C_ADD_ENTITY, C_PLAYER_INFO_UPDATE, C_SYSTEM_CHAT};
use steel_registry::{vanilla_dimension_types, vanilla_entities};
use steel_utils::ChunkPos;
use steel_utils::{codec::VarInt, serial::ReadFrom, text::DisplayResolutor};
use text_components::TextComponent;
use tokio::{fs, runtime::Builder};
use uuid::Uuid;

use crate::command::execution::{CommandPermissionSource, CommandSource};
use crate::command::sender::CommandSender;
use crate::config::{ResolvedDomainConfig, RuntimeConfig, StorageSelection};
use crate::entity::{Entity, EntityBase};
use crate::permission::{
    OP_GROUP, PermissionEntry, PermissionExpr, PermissionGroupConfig, PermissionGroupManager,
    PermissionGroupsConfig, PermissionKey, PermissionMetadataSet, PermissionSet,
    PermissionSubjectIndex, PermissionSubjectState,
};
use crate::player::connection::NetworkConnection;
use crate::player::{ClientInformation, GameProfile, Player, PlayerConnection, ResetReason};
use crate::test_support::{fresh_test_world, test_world};
use crate::world::World;

use super::known_players::{
    KnownPlayerSaveStep, UncachedPlayerTarget, classify_uncached_player_target, direct_uuid_profile,
};
use super::player_admission::PendingPlayerJoin;
use super::{
    AsyncMutex, CancellationToken, CommandRegistry, CommandRequestQueue, DomainCommandStorage,
    DomainPlayerData, DomainPlayerState, DomainScoreboards, FxHashMap, KeyStore,
    KnownPlayerCacheState, KnownPlayers, Notify, PacketProcessor, PlayerDataStorage,
    PlayerDisconnectQueue, PlayerJoinQueue, PlayerMap, PreparedSpawn, RegistryCache, Server,
    ServerJobQueue, SyncMutex, SyncRwLock, TabListTickStats, TickRateManager, WorldMap,
    can_entity_return_from_end_to_overworld, cap_positive_thread_count,
    create_registered_dispatcher, is_allowed_to_enter_portal_target, is_end_return_transition,
    offline_uuid, packet_workers_for_available, validate_player_permission_group_update,
};

struct TestConnection {
    sent_packets: Arc<SyncMutex<Vec<EncodedPacket>>>,
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

    fn close(&self) {}

    fn closed(&self) -> bool {
        false
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
    let mut worlds = WorldMap::new(domain.name.clone(), slice::from_ref(&domain), &[]);
    worlds.insert(world.key.clone(), world);

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
        chunk_encoding_pool: rayon::ThreadPoolBuilder::new()
            .num_threads(1)
            .build()
            .expect("test chunk encoding pool should initialize"),
        jobs: ServerJobQueue::new(),
        player_data_storage,
        player_permission_states: SyncRwLock::new(player_permission_states),
        player_permission_updates: AsyncMutex::new(()),
        known_players: SyncMutex::new(KnownPlayerCacheState::new(KnownPlayers::new())),
        known_player_save_idle: Notify::new(),
        profile_lookup_client: reqwest::Client::new(),
        pending_player_joins: PlayerJoinQueue::new(),
        pending_player_disconnects: PlayerDisconnectQueue::new(),
        pending_world_changes: SyncMutex::new(Vec::new()),
        pending_domain_switches: SyncMutex::new(Vec::new()),
    }))
}

fn test_player(server: &Arc<Server>, world: Arc<World>, uuid: Uuid) -> Arc<Player> {
    test_player_with_packets(server, world, uuid, "TestPlayer", 1).0
}

fn test_player_with_connection(
    server: &Arc<Server>,
    world: Arc<World>,
    uuid: Uuid,
    name: &str,
    entity_id: i32,
    connection: Arc<PlayerConnection>,
) -> Arc<Player> {
    Arc::new_cyclic(|weak_player| {
        Player::new(
            GameProfile {
                id: uuid,
                name: name.to_owned(),
                properties: Vec::new(),
                profile_actions: None,
            },
            Arc::clone(&connection),
            world,
            Arc::downgrade(server),
            Arc::clone(&server.config),
            entity_id,
            weak_player,
            ClientInformation::default(),
        )
    })
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
        let default_spawn = PreparedSpawn {
            position: spawn_position,
            rotation: (0.0, 0.0),
        };
        let state = DomainPlayerState {
            world: Arc::clone(&world),
            data: DomainPlayerData::FirstVisit { default_spawn },
            _spawn_chunk_request: world.request_player_spawn_chunks(spawn_position),
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

        drop(joining);
        drop(existing);
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
fn packet_worker_count_uses_the_configured_cap() {
    assert_eq!(packet_workers_for_available(Some(16), 8), 8);
    assert_eq!(packet_workers_for_available(Some(4), 8), 4);
}

#[test]
fn packet_worker_count_uses_the_automatic_default() {
    assert_eq!(packet_workers_for_available(Some(0), 8), 4);
    assert_eq!(packet_workers_for_available(None, 8), 4);
    assert_eq!(packet_workers_for_available(None, 1), 1);
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
