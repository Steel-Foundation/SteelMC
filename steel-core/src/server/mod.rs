//! This module contains the `Server` struct, which is the main entry point for the server.
/// Tick-polled server jobs.
pub mod jobs;
mod pregen;
/// The registry cache for the server.
pub mod registry_cache;
/// The tick rate manager for the server.
pub mod tick_rate_manager;
/// Domain-aware loaded world map.
pub mod worlds;

use crate::behavior::init_behaviors;
use crate::block_entity::init_block_entities;
use crate::chunk::{
    chunk_access::ChunkStatus,
    chunk_request::{ChunkRequest, ChunkRequestHandle, ChunkRequestState, ChunkTicketKind},
};
use crate::command::brigadier::{StringReader, SuggestionError, Suggestions};
use crate::command::execution::{
    CommandExecutionContext, CommandResultCallback, CommandSource, ExecutionCommandSource,
    ExecutionStop,
};
use crate::command::sender::CommandSender;
use crate::command::storage::DomainCommandStorage;
use crate::command::{
    COMMAND_REQUESTS_PER_TICK, COMMAND_RESUMPTIONS_PER_TICK, CommandCompletion, CommandDispatcher,
    CommandQueueFull, CommandRegistry, CommandRequest, CommandRequestQueue,
    PendingCommandExecutionQueue, client_permission_event, command_suggestions_packet,
    command_tree_packet, create_registered_dispatcher,
};
use crate::config::{ResolvedWorldConfig, RuntimeConfig, WorldsConfig, validate_login_security};
use crate::entity::{
    Entity, EntityBase, PendingWorldChangeToken, RemovalReason, SharedEntity, change_entity_world,
    init_entities,
};

use crate::chunk_saver::{ChunkStorage, PersistentEntity, registry::WorldStorageRegistry};
use crate::level_data::{LevelDataManager, RespawnData, WorldGenerationSettings};
use crate::permission::{
    OP_GROUP, PermissionGroupManager, PermissionGroupManagerError, PermissionGroupUpdateError,
    PermissionGroupsConfig, PermissionMetadataExpression, PermissionRuleExpression, PermissionSet,
    PermissionSubjectIndex, PermissionSubjectState,
};
use crate::player::chunk_sender::{ChunkSender, EncodedChunk};
use crate::player::connection::NetworkConnection;
use crate::player::player_data::{
    PersistentEnderPearl, PersistentPlayerData, PersistentRootVehicle,
};
use crate::player::player_data_storage::{GlobalPlayerData, PlayerDataStorage};
use crate::player::{
    GameProfile, KnownPlayer, KnownPlayerNameLookup, KnownPlayers, Player, ProfileLookupError,
    ResetReason, is_valid_player_name, lookup_online_profile, offline_uuid,
};
use crate::portal::{
    PortalKind, TeleportPostTransition, TeleportTransition, WorldChangeRequest, end_gateway,
    end_portal, nether_portal,
};
use crate::scoreboard::DomainScoreboards;
use crate::server::jobs::{FnServerJob, JobPoll, ServerJob, ServerJobContext, ServerJobQueue};
use crate::server::registry_cache::RegistryCache;
use crate::server::worlds::WorldMap;
use crate::world::player_spawn_finder::{PlayerSpawnSearch, PlayerSpawnSearchPoll};
use crate::world::{PlayerMap, World, WorldConfig, WorldGameTickTimings};
use crate::worldgen::WorldGeneratorRegistry;
use crate::worldgen::registry::GeneratorOutput;
use glam::DVec3;
use rayon::{ThreadPool, ThreadPoolBuilder};
use rustc_hash::FxHashMap;
use std::{
    collections::BTreeSet,
    io, mem,
    num::NonZero,
    path::Path,
    sync::{Arc, mpsc},
    thread,
    time::{Duration, Instant},
};
use steel_crypto::key_store::KeyStore;
use steel_protocol::packet_traits::{ClientPacket, EncodedPacket};
use steel_protocol::packets::game::{
    CCommandSuggestions, CEntityEvent, CGameEvent, CLogin, CPlayerInfoUpdate, CRemovePlayerInfo,
    CSetDefaultSpawnPosition, CSystemChat, CTabList, CTickingState, CTickingStep,
    CommonPlayerSpawnInfo, GameEventType, RelativeMovement,
};
use steel_protocol::utils::ConnectionProtocol;
use steel_registry::vanilla_game_rules::{
    ALLOW_ENTERING_NETHER_USING_PORTALS, IMMEDIATE_RESPAWN, LIMITED_CRAFTING, REDUCED_DEBUG_INFO,
};
use steel_registry::{
    REGISTRY, Registry, RegistryEntry, dimension_type::DimensionTypeRef, vanilla_dimension_types,
    vanilla_entities,
};
use steel_utils::{
    BlockPos, ChunkPos, Identifier,
    locks::{AsyncMutex, SyncMutex, SyncRwLock},
    text::DisplayResolutor,
    translations,
};
use text_components::{Modifier, TextComponent, format::Color};
use tick_rate_manager::{SprintReport, TickRateManager};
use tokio::{runtime::Runtime, sync::Notify, task::spawn_blocking, time::sleep};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

/// Interval in ticks between tab list updates (20 ticks = 1 second).
const TAB_LIST_UPDATE_INTERVAL: u64 = 20;
/// Interval in ticks between player info broadcasts (600 ticks = 30 seconds).
/// Matches vanilla `PlayerList.SEND_PLAYER_INFO_INTERVAL`.
const SEND_PLAYER_INFO_INTERVAL: u64 = 600;
/// Wall-clock interval between saves of command-owned persistent server data.
/// Matches vanilla's intended five-minute autosave cadence.
const COMMAND_DATA_AUTOSAVE_INTERVAL: Duration = Duration::from_secs(300);

/// Results from saving every command-owned persistent data set.
pub struct CommandDataSaveResults {
    /// Number of dirty domain scoreboards written, or the save error.
    pub scoreboards: io::Result<usize>,
    /// Number of dirty domain command-storage values written, or the save error.
    pub storage: io::Result<usize>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum UncachedPlayerTarget {
    DirectUuid(Uuid),
    OfflineName,
    OnlineName,
}

fn classify_uncached_player_target(target: &str, online_mode: bool) -> UncachedPlayerTarget {
    if let Ok(uuid) = Uuid::parse_str(target) {
        return UncachedPlayerTarget::DirectUuid(uuid);
    }
    if online_mode {
        UncachedPlayerTarget::OnlineName
    } else {
        UncachedPlayerTarget::OfflineName
    }
}

fn direct_uuid_profile(uuid: Uuid) -> KnownPlayer {
    KnownPlayer::new(uuid, uuid.to_string())
}

struct KnownPlayerCacheState {
    players: KnownPlayers,
    generation: u64,
    worker_running: bool,
    closed: bool,
}

impl KnownPlayerCacheState {
    const fn new(players: KnownPlayers) -> Self {
        Self {
            players,
            generation: 0,
            worker_running: false,
            closed: false,
        }
    }

    fn record(&mut self, uuid: Uuid, name: String) -> bool {
        if self.closed || !self.players.record(uuid, name) {
            return false;
        }
        self.mark_changed()
    }

    const fn mark_changed(&mut self) -> bool {
        if self.closed {
            return false;
        }
        self.generation = self.generation.wrapping_add(1);
        if self.worker_running {
            false
        } else {
            self.worker_running = true;
            true
        }
    }

    fn snapshot(&self) -> (KnownPlayers, u64) {
        (self.players.clone(), self.generation)
    }

    const fn is_current(&self, generation: u64) -> bool {
        !self.closed && self.generation == generation
    }

    const fn finish_save(&mut self, generation: u64) -> KnownPlayerSaveStep {
        if !self.closed && self.generation != generation {
            KnownPlayerSaveStep::SaveAgain
        } else {
            self.worker_running = false;
            KnownPlayerSaveStep::Finished
        }
    }

    fn close_if_idle(&mut self) -> Option<KnownPlayers> {
        if self.worker_running {
            return None;
        }
        self.closed = true;
        Some(self.players.clone())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum KnownPlayerSaveStep {
    SaveAgain,
    Finished,
}

/// Tick rate for the chunk sending loop.
const CHUNK_SENDING_TPS: u64 = 20;

/// Tick rate for the chunk scheduling loop.
const CHUNK_SCHEDULING_TPS: u64 = 20;

fn configured_chunk_generation_threads(configured_threads: Option<usize>) -> Option<usize> {
    cap_positive_thread_count(configured_threads, available_worker_threads())
}

fn available_worker_threads() -> usize {
    thread::available_parallelism().map_or(4, NonZero::get)
}

fn cap_positive_thread_count(
    configured_threads: Option<usize>,
    available_threads: usize,
) -> Option<usize> {
    let configured_threads = configured_threads.filter(|&threads| threads > 0)?;
    Some(configured_threads.min(available_threads.max(1)))
}

#[cfg(test)]
mod tests {
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
    use steel_registry::entity_type::EntityTypeRef;
    use steel_registry::packets::play::C_SYSTEM_CHAT;
    use steel_registry::{vanilla_dimension_types, vanilla_entities};
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
    use crate::player::{ClientInformation, GameProfile, Player, PlayerConnection};
    use crate::test_support::test_world;
    use crate::world::World;

    use super::{
        AsyncMutex, CancellationToken, CommandRegistry, CommandRequestQueue, DomainCommandStorage,
        DomainScoreboards, FxHashMap, KeyStore, KnownPlayerCacheState, KnownPlayerSaveStep,
        KnownPlayers, Notify, PlayerDataStorage, PlayerJoinQueue, PlayerMap, RegistryCache, Server,
        ServerJobQueue, SyncMutex, SyncRwLock, TickRateManager, UncachedPlayerTarget, WorldMap,
        can_entity_return_from_end_to_overworld, cap_positive_thread_count,
        classify_uncached_player_target, create_registered_dispatcher, direct_uuid_profile,
        is_allowed_to_enter_portal_target, is_end_return_transition, offline_uuid,
        validate_player_permission_group_update,
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

    fn test_runtime_config() -> Arc<RuntimeConfig> {
        Arc::new(RuntimeConfig {
            max_players: 1,
            view_distance: 2,
            simulation_distance: 2,
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
            chunk_generation_threads: Some(1),
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
        let permission_groups =
            PermissionGroupManager::transient(PermissionGroupsConfig::default())
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
            jobs: ServerJobQueue::new(),
            player_data_storage,
            player_permission_states: SyncRwLock::new(player_permission_states),
            player_permission_updates: AsyncMutex::new(()),
            known_players: SyncMutex::new(KnownPlayerCacheState::new(KnownPlayers::new())),
            known_player_save_idle: Notify::new(),
            profile_lookup_client: reqwest::Client::new(),
            pending_player_joins: PlayerJoinQueue::new(),
            pending_world_changes: SyncMutex::new(Vec::new()),
            pending_domain_switches: SyncMutex::new(Vec::new()),
        }))
    }

    fn test_player(server: &Arc<Server>, world: Arc<World>, uuid: Uuid) -> Arc<Player> {
        test_player_with_packets(server, world, uuid, "TestPlayer", 1).0
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
        let player = Arc::new_cyclic(|weak_player| {
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
        });
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
            validate_player_permission_group_update::<()>(&manager, &[], &["op".to_owned()])
                .is_ok()
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
        let unknown_owner_pearl =
            TestEntity::new(&vanilla_entities::ENDER_PEARL, Some(unknown_owner));
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
}

#[derive(Clone, Copy)]
struct PreparedSpawn {
    position: DVec3,
    rotation: (f32, f32),
}

fn apply_default_spawn(player: &Arc<Player>, world: &Arc<World>, spawn: PreparedSpawn) {
    player.base().set_position_local(spawn.position);
    player.set_rotation(spawn.rotation);
    player.restore_game_modes(world.default_gamemode, None);
    player
        .abilities
        .lock()
        .update_for_game_mode(world.default_gamemode);
}

fn world_spawn_transition(world: Arc<World>) -> TeleportTransition {
    let spawn = local_respawn_data_for_world(&world);
    TeleportTransition {
        target_world: world,
        position: respawn_position(&spawn),
        rotation: (spawn.yaw, spawn.pitch),
        velocity: DVec3::ZERO,
        relatives: RelativeMovement::NONE,
        portal_cooldown: 0,
        as_passenger: false,
        post_transition: TeleportPostTransition::do_nothing(),
    }
}

fn is_allowed_to_enter_portal(source_world: &World, target_world: &World) -> bool {
    is_allowed_to_enter_portal_target(
        is_nether_dimension_type(target_world),
        source_world.get_game_rule(&ALLOW_ENTERING_NETHER_USING_PORTALS),
    )
}

const fn is_allowed_to_enter_portal_target(
    target_is_nether: bool,
    allow_entering_nether_using_portals: bool,
) -> bool {
    if !target_is_nether {
        return true;
    }

    allow_entering_nether_using_portals
}

fn can_teleport_between_worlds(
    entity: &dyn Entity,
    source_world: &World,
    target_world: &World,
    projectile_owner_seen_credits: impl Fn(&uuid::Uuid) -> Option<bool>,
) -> bool {
    if is_end_return_transition(source_world.dimension_type, target_world.dimension_type) {
        return can_entity_return_from_end_to_overworld(entity, projectile_owner_seen_credits);
    }

    true
}

fn is_end_return_transition(
    source_dimension_type: DimensionTypeRef,
    target_dimension_type: DimensionTypeRef,
) -> bool {
    source_dimension_type == &vanilla_dimension_types::THE_END
        && target_dimension_type == &vanilla_dimension_types::OVERWORLD
}

fn is_nether_dimension_type(world: &World) -> bool {
    world.dimension_type == &vanilla_dimension_types::THE_NETHER
}

fn is_end_dimension_type(world: &World) -> bool {
    world.dimension_type == &vanilla_dimension_types::THE_END
}

fn can_entity_return_from_end_to_overworld(
    entity: &dyn Entity,
    projectile_owner_seen_credits: impl Fn(&uuid::Uuid) -> Option<bool>,
) -> bool {
    if entity.entity_type() == &vanilla_entities::ENDER_PEARL
        && entity
            .projectile_owner_uuid()
            .and_then(|uuid| projectile_owner_seen_credits(&uuid))
            == Some(false)
    {
        return false;
    }

    direct_passengers_allow_end_return(entity)
}

fn direct_passengers_allow_end_return(entity: &dyn Entity) -> bool {
    for passenger in entity.passengers() {
        if passenger
            .as_player()
            .is_some_and(|player| !player.has_seen_credits())
        {
            return false;
        }
    }

    true
}

fn local_respawn_data_for_world(world: &World) -> RespawnData {
    let level_data = world.level_data.read();
    let data = level_data.data();
    RespawnData::of(world.key.clone(), data.spawn_pos(), data.spawn.angle, 0.0)
}

fn respawn_position(respawn_data: &RespawnData) -> DVec3 {
    let pos = respawn_data.pos();
    DVec3::new(
        f64::from(pos.x()) + 0.5,
        f64::from(pos.y()),
        f64::from(pos.z()) + 0.5,
    )
}

fn generation_settings_for_world(
    world_entry: &ResolvedWorldConfig,
    generator_output: &GeneratorOutput,
) -> WorldGenerationSettings {
    WorldGenerationSettings::from_generator_config(
        world_entry.generator_config.generator().clone(),
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
    _spawn_chunk_request: ChunkRequestHandle,
}

enum DomainPlayerData {
    SavedRestored {
        data: Box<PersistentPlayerData>,
    },
    SavedWithoutLocation {
        data: Box<PersistentPlayerData>,
        default_spawn: PreparedSpawn,
    },
    FirstVisit {
        default_spawn: PreparedSpawn,
    },
}

struct DomainSwitchRequest {
    player: Arc<Player>,
    target_domain: String,
    target_world: Option<Arc<World>>,
    restore_saved_location: bool,
}

/// Failure while atomically editing one player's persisted permission state.
#[derive(Debug, thiserror::Error)]
pub enum PlayerPermissionUpdateError<E> {
    /// The caller rejected the proposed edit.
    #[error("{0}")]
    Edit(E),
    /// The edit assigns a group that is not configured.
    #[error("unknown permission group '{0}'")]
    UnknownGroup(String),
    /// The permission snapshot could not be persisted.
    #[error("failed to update player permissions: {0}")]
    Storage(io::Error),
}

impl<E> From<io::Error> for PlayerPermissionUpdateError<E> {
    fn from(value: io::Error) -> Self {
        Self::Storage(value)
    }
}

fn validate_player_permission_group_update<E>(
    manager: &PermissionGroupManager,
    previous_groups: &[String],
    updated_groups: &[String],
) -> Result<(), PlayerPermissionUpdateError<E>> {
    for group in updated_groups {
        let already_assigned = previous_groups.iter().any(|current| current == group);
        if !already_assigned && !manager.contains_group(group) {
            return Err(PlayerPermissionUpdateError::UnknownGroup(group.clone()));
        }
    }
    Ok(())
}

struct PendingPlayerJoin {
    player: Arc<Player>,
    state: Result<DomainPlayerState, String>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum PlayerAdmissionState {
    Joining,
    Disconnecting,
}

struct PlayerJoinQueue {
    sender: mpsc::Sender<PendingPlayerJoin>,
    receiver: SyncMutex<mpsc::Receiver<PendingPlayerJoin>>,
}

impl PlayerJoinQueue {
    fn new() -> Self {
        let (sender, receiver) = mpsc::channel();
        Self {
            sender,
            receiver: SyncMutex::new(receiver),
        }
    }

    fn send(&self, join: PendingPlayerJoin) {
        let _ = self.sender.send(join);
    }

    fn drain(&self) -> Vec<PendingPlayerJoin> {
        let receiver = self.receiver.lock();
        let mut joins = Vec::new();
        while let Ok(join) = receiver.try_recv() {
            joins.push(join);
        }
        joins
    }
}

struct RootVehicleRestoreJob {
    player: Arc<Player>,
    world: Arc<World>,
    request: ChunkRequestHandle,
    attach: [u8; 16],
    root_uuid: [u8; 16],
}

impl RootVehicleRestoreJob {
    fn new(
        player: Arc<Player>,
        world: Arc<World>,
        root_vehicle: &PersistentRootVehicle,
    ) -> Option<Self> {
        let root_chunk = persistent_entity_chunk(&root_vehicle.entity)?;
        let request = world.chunk_map.request_chunk(
            root_chunk,
            ChunkStatus::StructureStarts,
            ChunkTicketKind::PlayerSpawn,
        );
        Some(Self {
            player,
            world,
            request,
            attach: root_vehicle.attach,
            root_uuid: root_vehicle.entity.uuid,
        })
    }
}

impl ServerJob for RootVehicleRestoreJob {
    fn poll(&mut self, _context: &mut ServerJobContext) -> JobPoll {
        if self.player.connection.closed()
            || !self.player.has_joined_world()
            || !Arc::ptr_eq(&self.player.get_world(), &self.world)
        {
            return JobPoll::Finished;
        }

        match self.request.poll() {
            ChunkRequestState::Pending { .. } => JobPoll::Pending,
            ChunkRequestState::Cancelled => JobPoll::Finished,
            ChunkRequestState::Ready => {
                let Some(_ready) = self.request.ready_chunks() else {
                    return JobPoll::Pending;
                };
                if let Some(root_vehicle) = self.player.take_matching_pending_root_vehicle(
                    &self.world,
                    self.attach,
                    self.root_uuid,
                ) {
                    restore_root_vehicle_for_player(&self.player, &self.world, root_vehicle);
                }
                JobPoll::Finished
            }
        }
    }

    fn cancel(&mut self) {
        self.request.cancel();
    }
}

fn clear_pending_world_change(entity: &SharedEntity, pending_token: PendingWorldChangeToken) {
    entity.finish_pending_world_change(pending_token);
}

fn finish_pending_world_change_after_transition(
    entity: &SharedEntity,
    pending_token: PendingWorldChangeToken,
    changed_entity: Option<SharedEntity>,
) {
    match changed_entity {
        Some(changed_entity) if Arc::ptr_eq(entity, &changed_entity) => {
            changed_entity.finish_pending_world_change(pending_token);
        }
        Some(_) => {}
        None => {
            entity.finish_pending_world_change(pending_token);
        }
    }
}

fn finish_portal_world_change(
    entity: &SharedEntity,
    pending_token: PendingWorldChangeToken,
    changed_entity: Option<SharedEntity>,
) -> JobPoll {
    finish_pending_world_change_after_transition(entity, pending_token, changed_entity);
    JobPoll::Finished
}

fn portal_entity_still_valid(
    entity: &SharedEntity,
    source_world: &Arc<World>,
    pending_token: PendingWorldChangeToken,
) -> bool {
    !entity.is_removed()
        && entity.is_world_change_token_pending(pending_token)
        && entity
            .level()
            .is_some_and(|world| Arc::ptr_eq(&world, source_world))
        && source_world.contains_live_or_unloading_entity(entity)
        && !entity
            .as_player()
            .is_some_and(|player| player.connection.closed())
}

fn poll_portal_chunks_until_ready(
    request: &mut ChunkRequestHandle,
    entity: &SharedEntity,
    pending_token: PendingWorldChangeToken,
) -> Option<JobPoll> {
    match request.poll() {
        ChunkRequestState::Pending { .. } => Some(JobPoll::Pending),
        ChunkRequestState::Cancelled => {
            clear_pending_world_change(entity, pending_token);
            Some(JobPoll::Finished)
        }
        ChunkRequestState::Ready => {
            if request.ready_chunks().is_some() {
                None
            } else {
                Some(JobPoll::Pending)
            }
        }
    }
}

struct NetherPortalTeleportJob {
    entity: SharedEntity,
    source_world: Arc<World>,
    target_world: Arc<World>,
    portal_pos: BlockPos,
    approximate_exit_pos: BlockPos,
    to_nether: bool,
    pending_token: PendingWorldChangeToken,
    request: ChunkRequestHandle,
}

impl NetherPortalTeleportJob {
    fn new(
        entity: SharedEntity,
        source_world: Arc<World>,
        target_world: Arc<World>,
        portal_pos: BlockPos,
        approximate_exit_pos: BlockPos,
        to_nether: bool,
        pending_token: PendingWorldChangeToken,
    ) -> Self {
        let request = target_world.chunk_map.request_square(
            nether_portal::prewarm_center(approximate_exit_pos),
            nether_portal::prewarm_chunk_radius(to_nether),
            ChunkStatus::Full,
            ChunkTicketKind::Portal,
        );
        Self {
            entity,
            source_world,
            target_world,
            portal_pos,
            approximate_exit_pos,
            to_nether,
            pending_token,
            request,
        }
    }

    fn still_valid(&self) -> bool {
        portal_entity_still_valid(&self.entity, &self.source_world, self.pending_token)
    }

    fn clear_pending(&self) {
        clear_pending_world_change(&self.entity, self.pending_token);
    }

    fn finish_transition(&self, changed_entity: Option<SharedEntity>) {
        finish_pending_world_change_after_transition(
            &self.entity,
            self.pending_token,
            changed_entity,
        );
    }
}

impl ServerJob for NetherPortalTeleportJob {
    fn poll(&mut self, context: &mut ServerJobContext) -> JobPoll {
        if !self.still_valid() {
            self.clear_pending();
            return JobPoll::Finished;
        }

        if let Some(job_poll) =
            poll_portal_chunks_until_ready(&mut self.request, &self.entity, self.pending_token)
        {
            return job_poll;
        }

        let Some(server) = context.server() else {
            self.clear_pending();
            return JobPoll::Finished;
        };
        if !is_allowed_to_enter_portal(&self.source_world, &self.target_world)
            || !server.can_teleport_between_worlds(
                self.entity.as_ref(),
                &self.source_world,
                &self.target_world,
            )
        {
            self.clear_pending();
            return JobPoll::Finished;
        }
        let Some(transition) = nether_portal::calculate_transition(
            &self.source_world,
            &self.target_world,
            self.entity.as_ref(),
            self.portal_pos,
            self.approximate_exit_pos,
            self.to_nether,
        ) else {
            self.clear_pending();
            return JobPoll::Finished;
        };
        let changed_entity = change_entity_world(Arc::clone(&self.entity), &transition);
        self.finish_transition(changed_entity);
        JobPoll::Finished
    }

    fn cancel(&mut self) {
        self.clear_pending();
        self.request.cancel();
    }
}

const END_PORTAL_RESPAWN_SEARCH_READY_CANDIDATE_BUDGET: usize = 8;

struct EndPortalRespawnSpawn {
    position: DVec3,
    rotation: (f32, f32),
}

struct EndPortalTeleportJob {
    entity: SharedEntity,
    source_world: Arc<World>,
    pending_token: PendingWorldChangeToken,
    phase: EndPortalTeleportPhase,
}

enum EndPortalTeleportPhase {
    EntryToEnd {
        target_world: Arc<World>,
        request: ChunkRequestHandle,
    },
    ReturningEntity {
        target_world: Arc<World>,
        respawn_data: RespawnData,
        request: ChunkRequestHandle,
    },
    SearchingPlayerRespawn {
        target_world: Arc<World>,
        respawn_data: RespawnData,
        search: PlayerSpawnSearch,
    },
    LoadingPlayerRespawn {
        target_world: Arc<World>,
        spawn: EndPortalRespawnSpawn,
        request: ChunkRequestHandle,
    },
}

impl EndPortalTeleportJob {
    fn entry_to_end(
        entity: SharedEntity,
        source_world: Arc<World>,
        target_world: Arc<World>,
        pending_token: PendingWorldChangeToken,
    ) -> Self {
        let request = target_world.chunk_map.request_square(
            end_portal::end_platform_prewarm_center(),
            end_portal::end_platform_prewarm_chunk_radius(),
            ChunkStatus::Full,
            ChunkTicketKind::Portal,
        );
        Self {
            entity,
            source_world,
            pending_token,
            phase: EndPortalTeleportPhase::EntryToEnd {
                target_world,
                request,
            },
        }
    }

    fn returning_entity(
        entity: SharedEntity,
        source_world: Arc<World>,
        target_world: Arc<World>,
        respawn_data: RespawnData,
        pending_token: PendingWorldChangeToken,
    ) -> Self {
        let request = target_world.chunk_map.request_chunk(
            end_portal::prewarm_center(respawn_data.pos()),
            ChunkStatus::Full,
            ChunkTicketKind::Portal,
        );
        Self {
            entity,
            source_world,
            pending_token,
            phase: EndPortalTeleportPhase::ReturningEntity {
                target_world,
                respawn_data,
                request,
            },
        }
    }

    fn returning_player(
        entity: SharedEntity,
        source_world: Arc<World>,
        target_world: Arc<World>,
        respawn_data: RespawnData,
        pending_token: PendingWorldChangeToken,
    ) -> Result<Self, String> {
        let search = PlayerSpawnSearch::new(
            &target_world,
            respawn_data.pos(),
            target_world.default_gamemode,
        )?;
        Ok(Self {
            entity,
            source_world,
            pending_token,
            phase: EndPortalTeleportPhase::SearchingPlayerRespawn {
                target_world,
                respawn_data,
                search,
            },
        })
    }

    fn still_valid(&self) -> bool {
        portal_entity_still_valid(&self.entity, &self.source_world, self.pending_token)
    }

    fn clear_pending(&self) {
        clear_pending_world_change(&self.entity, self.pending_token);
    }
}

impl ServerJob for EndPortalTeleportJob {
    fn poll(&mut self, _context: &mut ServerJobContext) -> JobPoll {
        if !self.still_valid() {
            self.clear_pending();
            return JobPoll::Finished;
        }

        let entity = Arc::clone(&self.entity);
        let pending_token = self.pending_token;
        loop {
            match &mut self.phase {
                EndPortalTeleportPhase::EntryToEnd {
                    target_world,
                    request,
                } => {
                    if let Some(job_poll) =
                        poll_portal_chunks_until_ready(request, &entity, pending_token)
                    {
                        return job_poll;
                    }
                    let Some(transition) =
                        end_portal::calculate_entry_transition(target_world, entity.as_ref())
                    else {
                        clear_pending_world_change(&entity, pending_token);
                        return JobPoll::Finished;
                    };
                    let changed_entity = change_entity_world(Arc::clone(&entity), &transition);
                    return finish_portal_world_change(&entity, pending_token, changed_entity);
                }
                EndPortalTeleportPhase::ReturningEntity {
                    target_world,
                    respawn_data,
                    request,
                } => {
                    if let Some(job_poll) =
                        poll_portal_chunks_until_ready(request, &entity, pending_token)
                    {
                        return job_poll;
                    }
                    let transition = end_portal::calculate_entity_return_transition(
                        target_world,
                        entity.as_ref(),
                        respawn_data,
                    );
                    let changed_entity = change_entity_world(Arc::clone(&entity), &transition);
                    return finish_portal_world_change(&entity, pending_token, changed_entity);
                }
                EndPortalTeleportPhase::SearchingPlayerRespawn {
                    target_world,
                    respawn_data,
                    search,
                } => match search.poll_with_ready_candidate_budget(
                    target_world,
                    END_PORTAL_RESPAWN_SEARCH_READY_CANDIDATE_BUDGET,
                ) {
                    PlayerSpawnSearchPoll::Pending => return JobPoll::Pending,
                    PlayerSpawnSearchPoll::Cancelled => {
                        clear_pending_world_change(&entity, pending_token);
                        return JobPoll::Finished;
                    }
                    PlayerSpawnSearchPoll::Ready(position) => {
                        let spawn = EndPortalRespawnSpawn {
                            position,
                            rotation: (respawn_data.yaw, respawn_data.pitch),
                        };
                        let request = target_world.request_player_spawn_chunks(position);
                        self.phase = EndPortalTeleportPhase::LoadingPlayerRespawn {
                            target_world: target_world.clone(),
                            spawn,
                            request,
                        };
                    }
                },
                EndPortalTeleportPhase::LoadingPlayerRespawn {
                    target_world,
                    spawn,
                    request,
                } => {
                    if let Some(job_poll) =
                        poll_portal_chunks_until_ready(request, &entity, pending_token)
                    {
                        return job_poll;
                    }
                    let transition = end_portal::calculate_player_return_transition(
                        target_world,
                        entity.as_ref(),
                        spawn.position,
                        spawn.rotation,
                    );
                    let changed_entity = change_entity_world(Arc::clone(&entity), &transition);
                    return finish_portal_world_change(&entity, pending_token, changed_entity);
                }
            }
        }
    }

    fn cancel(&mut self) {
        self.clear_pending();
        match &mut self.phase {
            EndPortalTeleportPhase::EntryToEnd { request, .. }
            | EndPortalTeleportPhase::ReturningEntity { request, .. }
            | EndPortalTeleportPhase::LoadingPlayerRespawn { request, .. } => request.cancel(),
            EndPortalTeleportPhase::SearchingPlayerRespawn { .. } => {}
        }
    }
}

struct EndGatewayTeleportJob {
    entity: SharedEntity,
    source_world: Arc<World>,
    portal_pos: BlockPos,
    source_is_end: bool,
    pending_token: PendingWorldChangeToken,
    phase: EndGatewayTeleportPhase,
}

enum EndGatewayTeleportPhase {
    LoadingReady { request: ChunkRequestHandle },
    LoadingSearchPath { request: ChunkRequestHandle },
}

impl EndGatewayTeleportJob {
    fn new(
        entity: SharedEntity,
        source_world: Arc<World>,
        portal_pos: BlockPos,
        source_is_end: bool,
        pending_token: PendingWorldChangeToken,
    ) -> Option<Self> {
        let preparation = end_gateway::initial_chunks(&source_world, portal_pos, source_is_end)?;
        let phase = match preparation {
            end_gateway::EndGatewayChunkPreparation::Ready(chunks) => {
                EndGatewayTeleportPhase::LoadingReady {
                    request: request_end_gateway_chunks(&source_world, chunks),
                }
            }
            end_gateway::EndGatewayChunkPreparation::SearchPath(chunks) => {
                EndGatewayTeleportPhase::LoadingSearchPath {
                    request: request_end_gateway_chunks(&source_world, chunks),
                }
            }
        };
        Some(Self {
            entity,
            source_world,
            portal_pos,
            source_is_end,
            pending_token,
            phase,
        })
    }

    fn still_valid(&self) -> bool {
        portal_entity_still_valid(&self.entity, &self.source_world, self.pending_token)
    }

    fn clear_pending(&self) {
        clear_pending_world_change(&self.entity, self.pending_token);
    }
}

impl ServerJob for EndGatewayTeleportJob {
    fn poll(&mut self, _context: &mut ServerJobContext) -> JobPoll {
        if !self.still_valid() {
            self.clear_pending();
            return JobPoll::Finished;
        }

        let entity = Arc::clone(&self.entity);
        let pending_token = self.pending_token;
        let source_world = Arc::clone(&self.source_world);
        let portal_pos = self.portal_pos;
        let source_is_end = self.source_is_end;
        loop {
            match &mut self.phase {
                EndGatewayTeleportPhase::LoadingReady { request } => match request.poll() {
                    ChunkRequestState::Pending { .. } => return JobPoll::Pending,
                    ChunkRequestState::Cancelled => {
                        clear_pending_world_change(&entity, pending_token);
                        return JobPoll::Finished;
                    }
                    ChunkRequestState::Ready => {
                        let Some(_ready) = request.ready_chunks() else {
                            return JobPoll::Pending;
                        };
                        let Some(transition) = end_gateway::calculate_transition(
                            &source_world,
                            entity.as_ref(),
                            portal_pos,
                            source_is_end,
                        ) else {
                            clear_pending_world_change(&entity, pending_token);
                            return JobPoll::Finished;
                        };
                        let changed_entity = change_entity_world(Arc::clone(&entity), &transition);
                        finish_pending_world_change_after_transition(
                            &entity,
                            pending_token,
                            changed_entity,
                        );
                        return JobPoll::Finished;
                    }
                },
                EndGatewayTeleportPhase::LoadingSearchPath { request } => match request.poll() {
                    ChunkRequestState::Pending { .. } => return JobPoll::Pending,
                    ChunkRequestState::Cancelled => {
                        clear_pending_world_change(&entity, pending_token);
                        return JobPoll::Finished;
                    }
                    ChunkRequestState::Ready => {
                        let Some(_ready) = request.ready_chunks() else {
                            return JobPoll::Pending;
                        };
                        let Some(chunks) = end_gateway::final_chunks_after_search(
                            &source_world,
                            portal_pos,
                            source_is_end,
                        ) else {
                            clear_pending_world_change(&entity, pending_token);
                            return JobPoll::Finished;
                        };
                        self.phase = EndGatewayTeleportPhase::LoadingReady {
                            request: request_end_gateway_chunks(&source_world, chunks),
                        };
                    }
                },
            }
        }
    }

    fn cancel(&mut self) {
        self.clear_pending();
        match &mut self.phase {
            EndGatewayTeleportPhase::LoadingReady { request }
            | EndGatewayTeleportPhase::LoadingSearchPath { request } => request.cancel(),
        }
    }
}

fn request_end_gateway_chunks(world: &Arc<World>, chunks: Vec<ChunkPos>) -> ChunkRequestHandle {
    world.chunk_map.request_chunks(ChunkRequest {
        status: ChunkStatus::Full,
        positions: chunks,
        ticket_kind: ChunkTicketKind::Portal,
    })
}

fn persistent_entity_chunk(entity: &PersistentEntity) -> Option<ChunkPos> {
    let pos = DVec3::new(entity.pos[0], entity.pos[1], entity.pos[2]);
    if !pos.x.is_finite() || !pos.y.is_finite() || !pos.z.is_finite() {
        tracing::warn!(
            uuid = ?Uuid::from_bytes(entity.uuid),
            "Skipping persisted entity with non-finite position {pos:?}",
        );
        return None;
    }
    Some(ChunkPos::from_entity_pos(pos))
}

fn restore_root_vehicle_for_player(
    player: &Arc<Player>,
    world: &Arc<World>,
    root_vehicle: PersistentRootVehicle,
) {
    let Some(root_chunk) = persistent_entity_chunk(&root_vehicle.entity) else {
        return;
    };
    let level = Arc::downgrade(world);
    let entities =
        ChunkStorage::persistent_to_entity_tree_at_level(&root_vehicle.entity, root_chunk, &level);
    if entities.is_empty() {
        tracing::warn!(
            player = %player.gameprofile.name,
            "Persisted RootVehicle did not recreate any runtime entities",
        );
        return;
    }

    let attach_uuid = Uuid::from_bytes(root_vehicle.attach);
    let Some(attach_entity) = entities
        .iter()
        .find(|entity| entity.uuid() == attach_uuid)
        .cloned()
    else {
        tracing::warn!(
            player = %player.gameprofile.name,
            attach = ?attach_uuid,
            "Discarding persisted RootVehicle because the attach entity is missing",
        );
        discard_restored_entities(&entities);
        return;
    };

    if let Err(error) = world.register_loaded_entity_tree(&entities) {
        tracing::warn!(
            player = %player.gameprofile.name,
            attach = ?attach_uuid,
            root = ?Uuid::from_bytes(root_vehicle.entity.uuid),
            "Discarding persisted RootVehicle because its entity tree could not be registered: {error}",
        );
        discard_restored_entities(&entities);
        return;
    }

    let player_entity: SharedEntity = player.clone();
    EntityBase::restore_passenger_relationship(&attach_entity, &player_entity);
    attach_entity.position_rider(player.as_ref());
    player.send_restored_vehicle_mount_sync(attach_entity.as_ref());

    world.mark_chunk_dirty(root_chunk);
    for entity in &entities {
        world.mark_chunk_dirty(ChunkPos::from_entity_pos(entity.position()));
    }
}

fn discard_restored_entities(entities: &[SharedEntity]) {
    for entity in entities {
        entity.set_removed(RemovalReason::Discarded);
    }
}

/// Re-spawns a single persisted ender pearl in its own world once the target
/// chunk is loaded (vanilla `ServerPlayer.loadAndSpawnEnderPearl`).
struct EnderPearlRestoreJob {
    player: Arc<Player>,
    world: Arc<World>,
    request: ChunkRequestHandle,
    uuid: Uuid,
    entity: PersistentEntity,
}

impl EnderPearlRestoreJob {
    fn new(player: Arc<Player>, world: Arc<World>, entity: PersistentEntity) -> Option<Self> {
        let chunk = persistent_entity_chunk(&entity)?;
        let uuid = Uuid::from_bytes(entity.uuid);
        let request = world.chunk_map.request_chunk(
            chunk,
            ChunkStatus::StructureStarts,
            ChunkTicketKind::PlayerSpawn,
        );
        Some(Self {
            player,
            world,
            request,
            uuid,
            entity,
        })
    }
}

impl ServerJob for EnderPearlRestoreJob {
    fn poll(&mut self, _context: &mut ServerJobContext) -> JobPoll {
        // The pearl lives in its own world, which may differ from the player's, so
        // only the connection (not the player's current world) gates the restore.
        if self.player.connection.closed() {
            return JobPoll::Finished;
        }

        match self.request.poll() {
            ChunkRequestState::Pending { .. } => JobPoll::Pending,
            ChunkRequestState::Cancelled => JobPoll::Finished,
            ChunkRequestState::Ready => {
                if self.request.ready_chunks().is_none() {
                    return JobPoll::Pending;
                }
                if !restore_ender_pearl_for_player(&self.player, &self.world, &self.entity) {
                    self.player.remove_pending_ender_pearl(self.uuid);
                }
                JobPoll::Finished
            }
        }
    }

    fn cancel(&mut self) {
        self.request.cancel();
    }
}

fn restore_ender_pearl_for_player(
    player: &Arc<Player>,
    world: &Arc<World>,
    entity: &PersistentEntity,
) -> bool {
    let Some(chunk) = persistent_entity_chunk(entity) else {
        return false;
    };
    let level = Arc::downgrade(world);
    let entities = ChunkStorage::persistent_to_entity_tree_at_level(entity, chunk, &level);
    let Some(pearl) = entities.first().cloned() else {
        tracing::warn!(
            player = %player.gameprofile.name,
            "Persisted ender pearl did not recreate a runtime entity",
        );
        return false;
    };
    if pearl.entity_type() != &vanilla_entities::ENDER_PEARL {
        tracing::warn!(
            player = %player.gameprofile.name,
            entity_type = ?pearl.entity_type().key,
            "Persisted ender pearl recreated a non-pearl root entity",
        );
        return false;
    }

    let owner: SharedEntity = player.clone();
    for entity in &entities {
        entity.restore_owner_reference(&owner);
    }

    if let Err(error) = world.register_loaded_entity_tree(&entities) {
        tracing::warn!(
            player = %player.gameprofile.name,
            "Discarding persisted ender pearl because it could not be registered: {error}",
        );
        discard_restored_entities(&entities);
        return false;
    }

    player.register_ender_pearl(&pearl);
    world.chunk_map.place_ender_pearl_ticket(chunk);
    world.mark_chunk_dirty(chunk);
    true
}

/// The main server struct.
pub struct Server {
    /// Runtime configuration (view distance, compression, etc.).
    pub config: Arc<RuntimeConfig>,
    /// Runtime permission groups and their persistence boundary.
    pub permission_groups: PermissionGroupManager,
    /// The cancellation token for graceful shutdown.
    pub cancel_token: CancellationToken,
    /// The key store for the server.
    pub key_store: KeyStore,
    /// The registry cache for the server.
    pub registry_cache: RegistryCache,
    /// A list of all the worlds on the server.
    pub worlds: WorldMap,
    /// Players currently connected to the server, independent of world membership.
    online_players: PlayerMap,
    /// UUIDs reserved by a join or disconnect/save lifecycle transition.
    player_admissions: SyncMutex<FxHashMap<Uuid, PlayerAdmissionState>>,
    /// The tick rate manager for the server.
    pub tick_rate_manager: SyncRwLock<TickRateManager>,
    /// Command scoreboards isolated by Steel domain.
    pub scoreboards: DomainScoreboards,
    /// Command NBT storage isolated by Steel domain.
    pub(crate) command_storage: DomainCommandStorage,
    /// Saves and dispatches commands to appropriate handlers.
    command_dispatcher: SyncRwLock<CommandDispatcher>,
    /// Steel-owned permission keys exposed for command autocomplete.
    command_permission_keys: Vec<String>,
    /// Command work submitted from connection and console tasks.
    command_requests: CommandRequestQueue,
    /// Jobs resumed from a known point in the server game tick.
    pub jobs: ServerJobQueue,
    /// Player data storage for saving/loading player state.
    pub player_data_storage: PlayerDataStorage,
    /// Persisted permission state indexed by player UUID.
    player_permission_states: SyncRwLock<PermissionSubjectIndex>,
    /// Serializes persistence and cache publication for player permission edits.
    player_permission_updates: AsyncMutex<()>,
    /// Player identities and coalesced persistence state.
    known_players: SyncMutex<KnownPlayerCacheState>,
    /// Wakes shutdown when the single known-player save worker becomes idle.
    known_player_save_idle: Notify,
    /// HTTP client used by online-mode name-to-profile lookups.
    profile_lookup_client: reqwest::Client,
    /// Player joins prepared by async I/O and finalized at the game tick safe point.
    pending_player_joins: PlayerJoinQueue,
    /// Queued world changes to process after the tick.
    pub pending_world_changes: SyncMutex<Vec<(SharedEntity, WorldChangeRequest)>>,
    /// Queued domain switches to process after world ticks.
    pending_domain_switches: SyncMutex<Vec<DomainSwitchRequest>>,
}

impl Server {
    pub(crate) fn permission_rule_suggestions(&self) -> Vec<String> {
        let mut suggestions = self
            .command_permission_keys
            .iter()
            .cloned()
            .collect::<BTreeSet<_>>();
        let config = self.permission_groups.config_snapshot();
        for group in config.groups.values() {
            suggestions.extend(group.allow.iter().cloned());
            suggestions.extend(group.deny.iter().cloned());
        }
        for (_, state) in self.player_permission_states.read().entries() {
            suggestions.extend(state.overrides().entries().iter().map(|entry| {
                PermissionRuleExpression::new(entry.key().clone(), entry.context().clone())
                    .to_string()
            }));
        }
        suggestions.into_iter().collect()
    }

    pub(crate) fn permission_metadata_suggestions(&self) -> Vec<String> {
        let mut suggestions = BTreeSet::new();
        let config = self.permission_groups.config_snapshot();
        for group in config.groups.values() {
            suggestions.extend(group.metadata.iter().map(|rule| rule.key.clone()));
        }
        for (_, state) in self.player_permission_states.read().entries() {
            suggestions.extend(state.metadata_overrides().entries().iter().map(|entry| {
                PermissionMetadataExpression::new(entry.key().clone(), entry.context().clone())
                    .to_string()
            }));
        }
        suggestions.into_iter().collect()
    }

    /// Creates a new server with only Steel's built-in commands.
    pub async fn new(
        chunk_runtime: Arc<Runtime>,
        cancel_token: CancellationToken,
        config: RuntimeConfig,
        worlds_config: WorldsConfig,
        permission_groups: PermissionGroupManager,
    ) -> Result<Self, String> {
        Self::new_with_commands(
            chunk_runtime,
            cancel_token,
            config,
            worlds_config,
            permission_groups,
            CommandRegistry::new(),
        )
        .await
    }

    /// Creates a new server and atomically merges startup command extensions after built-ins.
    #[expect(
        clippy::too_many_lines,
        reason = "server initialization is a single cohesive flow"
    )]
    pub async fn new_with_commands(
        chunk_runtime: Arc<Runtime>,
        cancel_token: CancellationToken,
        config: RuntimeConfig,
        worlds_config: WorldsConfig,
        permission_groups: PermissionGroupManager,
        command_registry: CommandRegistry,
    ) -> Result<Self, String> {
        validate_login_security(config.online_mode, config.encryption).map_err(str::to_owned)?;
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
        log::info!(
            "SteelMC is not affiliated with Mojang or Microsoft. Use is subject to the Minecraft EULA: https://aka.ms/MinecraftEULA"
        );

        let registry_cache = RegistryCache::new(config.compression);

        let (generator_registry, storage_registry) = world_config_registries()?;
        let resolved_worlds = worlds_config
            .validate_and_resolve(&generator_registry, &storage_registry)
            .map_err(|e| format!("failed to validate worlds.toml: {e}"))?;

        let generation_pool: Arc<ThreadPool> = Arc::new({
            let mut builder = ThreadPoolBuilder::new().thread_name(|i| format!("rayon-gen-{i}"));
            if let Some(chunk_generation_threads) =
                configured_chunk_generation_threads(config.chunk_generation_threads)
            {
                builder = builder.num_threads(chunk_generation_threads);
            }
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
        let player_permission_states = player_data_storage
            .load_permission_subjects()
            .await
            .map_err(|error| format!("failed to load player permissions: {error}"))?;
        let known_players = player_data_storage
            .load_known_players()
            .await
            .map_err(|error| format!("failed to load known players: {error}"))?;
        let mut worlds = WorldMap::new(
            resolved_worlds.default_domain.clone(),
            &resolved_worlds.domains,
            &resolved_worlds.worlds,
        );

        for world_entry in &resolved_worlds.worlds {
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
            let world_seed = LevelDataManager::load_seed_or_default(
                storage_output.level_data_path.as_deref(),
                world_entry.seed,
            )
            .await
            .map_err(|e| {
                format!(
                    "failed to load level data seed for {}: {e}",
                    world_entry.key
                )
            })?;
            let generator_output = generator_registry
                .create(
                    storage_output.level_data_path.as_deref(),
                    &world_entry.generator_config,
                    world_seed,
                    generation_pool.clone(),
                )
                .map_err(|e| format!("failed to create generator for {}: {e}", world_entry.key))?;
            let generation_settings = generation_settings_for_world(world_entry, &generator_output);
            let world = World::new_with_config(
                chunk_runtime.clone(),
                world_entry.key.clone(),
                generator_output.dimension_type,
                world_seed,
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

        let scoreboards = DomainScoreboards::load(&worlds)
            .await
            .map_err(|error| format!("failed to load domain scoreboards: {error}"))?;
        let command_storage = DomainCommandStorage::load(&worlds)
            .await
            .map_err(|error| format!("failed to load domain command storage: {error}"))?;
        let registered_commands = create_registered_dispatcher(command_registry)
            .map_err(|error| format!("failed to register commands: {error}"))?;
        let command_permission_keys = registered_commands
            .permissions
            .into_iter()
            .map(|permission| permission.as_str().to_owned())
            .collect();

        Ok(Server {
            config,
            permission_groups,
            cancel_token,
            key_store: KeyStore::create(),
            worlds,
            online_players: PlayerMap::new(),
            player_admissions: SyncMutex::new(FxHashMap::default()),
            registry_cache,
            tick_rate_manager: SyncRwLock::new(TickRateManager::new()),
            scoreboards,
            command_storage,
            command_dispatcher: SyncRwLock::new(registered_commands.dispatcher),
            command_permission_keys,
            command_requests: CommandRequestQueue::new(),
            jobs: ServerJobQueue::new(),
            player_data_storage,
            player_permission_states: SyncRwLock::new(player_permission_states),
            player_permission_updates: AsyncMutex::new(()),
            known_players: SyncMutex::new(KnownPlayerCacheState::new(known_players)),
            known_player_save_idle: Notify::new(),
            profile_lookup_client: reqwest::Client::new(),
            pending_player_joins: PlayerJoinQueue::new(),
            pending_world_changes: SyncMutex::new(vec![]),
            pending_domain_switches: SyncMutex::new(vec![]),
        })
    }

    /// Saves all dirty domain command storage through domain default worlds.
    pub async fn save_command_storage(&self) -> io::Result<usize> {
        self.command_storage.save(&self.worlds).await
    }

    /// Saves all command-owned persistent data while allowing each data set to fail independently.
    pub async fn save_command_data(&self) -> CommandDataSaveResults {
        CommandDataSaveResults {
            scoreboards: self.scoreboards.save(&self.worlds).await,
            storage: self.save_command_storage().await,
        }
    }

    /// Queues a command for execution at the start of the next game tick.
    pub fn submit_command(
        &self,
        sender: CommandSender,
        command: String,
    ) -> Result<(), CommandQueueFull> {
        self.command_requests
            .submit(CommandRequest::Execute { sender, command })
    }

    pub(crate) fn submit_command_suggestions(
        &self,
        player: Arc<Player>,
        transaction_id: i32,
        input: String,
    ) -> Result<(), CommandQueueFull> {
        self.command_requests.submit(CommandRequest::Suggestions {
            player,
            transaction_id,
            input,
        })
    }

    /// Returns Brigadier completions visible to a command sender.
    pub fn command_completions(
        self: &Arc<Self>,
        sender: CommandSender,
        input: &str,
    ) -> Vec<CommandCompletion> {
        match self.build_command_suggestions(sender, input) {
            Ok(suggestions) => {
                let range = suggestions.range();
                suggestions
                    .list()
                    .iter()
                    .map(|suggestion| {
                        CommandCompletion::new(
                            range.start(),
                            range.len(),
                            suggestion.text().to_owned(),
                        )
                    })
                    .collect()
            }
            Err(error) => {
                tracing::warn!(%error, "failed to build command suggestions");
                Vec::new()
            }
        }
    }

    /// Queues initial player join work.
    ///
    /// Persistent data is loaded asynchronously, then world insertion is finalized at the
    /// game tick safe point so the socket reader can enter play immediately.
    pub fn queue_player_join(self: &Arc<Self>, player: Arc<Player>) {
        if player.connection.closed() {
            return;
        }
        if !self.reserve_player_join(&player) {
            player.disconnect("You are already connected to this server");
            return;
        }

        let server = Arc::clone(self);
        tokio::spawn(async move {
            let state = server.prepare_player_join(&player).await;
            server
                .pending_player_joins
                .send(PendingPlayerJoin { player, state });
        });
    }

    async fn prepare_player_join(&self, player: &Player) -> Result<DomainPlayerState, String> {
        let target_domain = self.load_join_domain(player).await?;
        self.load_domain_player_state(player, &target_domain, None, true)
            .await
    }

    fn process_player_joins(self: &Arc<Self>) {
        for join in self.pending_player_joins.drain() {
            self.finish_prepared_player_join(join);
        }
    }

    fn finish_prepared_player_join(self: &Arc<Self>, join: PendingPlayerJoin) {
        let PendingPlayerJoin { player, state } = join;
        let uuid = player.gameprofile.id;
        if player.connection.closed() {
            self.release_player_admission(uuid, PlayerAdmissionState::Joining);
            return;
        }

        let state = match state {
            Ok(state) => state,
            Err(error) => {
                self.release_player_admission(uuid, PlayerAdmissionState::Joining);
                log::error!(
                    "Failed to load player data for {}: {error}",
                    player.gameprofile.name
                );
                player.disconnect("Failed to load player data");
                return;
            }
        };

        if !self.admit_reserved_player(Arc::clone(&player)) {
            player.disconnect("You are already connected to this server");
            return;
        }

        self.apply_cached_or_default_permission_state(&player);
        Self::apply_domain_player_state(&player, &state);
        self.send_login_packet(&player, &state.world);

        player.reset(Arc::clone(&state.world), ResetReason::InitialJoin);
        Self::apply_domain_player_state(&player, &state);
        let pos = player.position();
        let rotation = player.rotation();
        let admitted = player.spawn(pos, rotation, ResetReason::InitialJoin);
        if !admitted {
            self.remove_online_player_sync(&player);
            return;
        }
        let previous_name = self.record_known_player(&player.gameprofile);
        self.broadcast_player_join_message(&player, previous_name.as_deref());
        self.sync_tab_list(&player);
        if player.mark_joined_world() {
            player.send_inventory_to_remote();
        }
        self.schedule_root_vehicle_restore(&player, &state);
        self.schedule_ender_pearl_restores(&player, &state);
        if player.connection.closed() {
            tokio::spawn(async move {
                state.world.remove_player(player).await;
            });
        }
    }

    fn reserve_player_join(&self, player: &Player) -> bool {
        let uuid = player.gameprofile.id;
        let mut admissions = self.player_admissions.lock();
        if admissions.contains_key(&uuid) {
            return false;
        }
        if self.online_players.get_by_uuid(&uuid).is_some() {
            return false;
        }
        admissions
            .insert(uuid, PlayerAdmissionState::Joining)
            .is_none()
    }

    fn admit_reserved_player(&self, player: Arc<Player>) -> bool {
        let uuid = player.gameprofile.id;
        let mut admissions = self.player_admissions.lock();
        if admissions.get(&uuid) != Some(&PlayerAdmissionState::Joining) {
            return false;
        }

        let admitted = self.online_players.insert(player);
        let _ = admissions.remove(&uuid);
        admitted
    }

    fn reserve_player_disconnect(&self, player: &Arc<Player>) -> bool {
        let uuid = player.gameprofile.id;
        let mut admissions = self.player_admissions.lock();
        if admissions.contains_key(&uuid) {
            return false;
        }
        if !self
            .online_players
            .get_by_uuid(&uuid)
            .is_some_and(|current| Arc::ptr_eq(&current, player))
        {
            return false;
        }
        admissions
            .insert(uuid, PlayerAdmissionState::Disconnecting)
            .is_none()
    }

    fn release_player_admission(&self, uuid: Uuid, state: PlayerAdmissionState) {
        let mut admissions = self.player_admissions.lock();
        if admissions.get(&uuid) == Some(&state) {
            let _ = admissions.remove(&uuid);
        }
    }

    fn remove_online_player_sync(&self, player: &Arc<Player>) {
        let _ = self.online_players.remove_player_sync(player);
    }

    pub(crate) async fn remove_online_player_after_disconnect(
        &self,
        player: Arc<Player>,
        domain: String,
        player_data: PersistentPlayerData,
    ) {
        let uuid = player.gameprofile.id;
        if !self.reserve_player_disconnect(&player) {
            return;
        }

        // Vanilla broadcasts before removing the player from its global player list.
        self.broadcast_player_leave_message(&player);
        self.broadcast_to_online(CRemovePlayerInfo::single(uuid));
        let player = self.online_players.remove_player_sync(&player);

        let Some(player) = player else {
            self.release_player_admission(uuid, PlayerAdmissionState::Disconnecting);
            return;
        };

        if let Err(e) = self
            .player_data_storage
            .save_domain_data(&domain, uuid, &player_data)
            .await
        {
            log::error!("Failed to save player domain data for {uuid}: {e}");
        }
        if let Err(e) = self
            .player_data_storage
            .save_global(
                uuid,
                &GlobalPlayerData {
                    last_active_domain: domain,
                },
            )
            .await
        {
            log::error!("Failed to save global player data for {uuid}: {e}");
        }

        player.cleanup();
        self.release_player_admission(uuid, PlayerAdmissionState::Disconnecting);
    }

    /// Broadcasts a packet to every online player, regardless of world membership.
    pub fn broadcast_to_online<P: ClientPacket>(&self, packet: P) {
        let Ok(encoded) =
            EncodedPacket::from_bare(packet, self.config.compression, ConnectionProtocol::Play)
        else {
            return;
        };
        self.online_players.iter_players(|_, player| {
            player.connection.send_encoded(encoded.clone());
            true
        });
    }

    fn broadcast_to_online_with<P: ClientPacket, F: Fn(&Player) -> P>(&self, packet: F) {
        self.online_players.iter_players(|_, player| {
            player.send_packet(packet(player));
            true
        });
    }

    /// Sends full tab list synchronization for a newly joined player.
    ///
    /// Server membership mirrors vanilla `PlayerList`; world entity spawning remains
    /// owned by the per-world entity tracker.
    fn sync_tab_list(&self, player: &Arc<Player>) {
        self.online_players.iter_players(|_, existing_player| {
            if existing_player.gameprofile.id == player.gameprofile.id {
                return true;
            }

            let add_existing = CPlayerInfoUpdate::create_player_initializing(
                existing_player.gameprofile.id,
                existing_player.gameprofile.name.clone(),
                existing_player.gameprofile.properties.clone(),
                existing_player.game_mode().into(),
                existing_player.connection.latency(),
                None,
                true,
            );
            player.send_packet(add_existing);

            if let Some(session) = existing_player.chat_session()
                && let Ok(protocol_data) = session.as_data().to_protocol_data()
            {
                player.send_packet(CPlayerInfoUpdate::update_chat_session(
                    existing_player.gameprofile.id,
                    protocol_data,
                ));
            }

            true
        });

        let player_info_packet = CPlayerInfoUpdate::create_player_initializing(
            player.gameprofile.id,
            player.gameprofile.name.clone(),
            player.gameprofile.properties.clone(),
            player.game_mode().into(),
            player.connection.latency(),
            None,
            true,
        );
        self.broadcast_to_online(player_info_packet);
    }

    fn broadcast_player_latency_updates(&self) {
        let mut latency_entries = Vec::new();
        self.online_players.iter_players(|uuid, player| {
            latency_entries.push((*uuid, player.connection.latency()));
            true
        });

        if !latency_entries.is_empty() {
            self.broadcast_to_online(CPlayerInfoUpdate::update_latency(latency_entries));
        }
    }

    async fn load_join_domain(&self, player: &Player) -> Result<String, String> {
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
            Err(e) => Err(format!("failed to load global player data: {e}")),
        }
    }

    fn apply_cached_or_default_permission_state(&self, player: &Player) -> u64 {
        let state = self
            .player_permission_states
            .read()
            .get(player.gameprofile.id)
            .cloned()
            .unwrap_or_default();
        self.apply_player_permission_state(player, state)
    }

    fn apply_player_permission_state(&self, player: &Player, state: PermissionSubjectState) -> u64 {
        let (groups, overrides, metadata_overrides) = state.into_parts();
        for group in &groups {
            if !self.permission_groups.contains_group(group) {
                log::warn!(
                    "Player {} has unknown permission group {group}",
                    player.gameprofile.name
                );
            }
        }
        let effective = self
            .permission_groups
            .effective_permissions(&groups, &overrides);
        let effective_metadata = self
            .permission_groups
            .effective_metadata(&groups, &metadata_overrides);
        player.set_permission_state(
            groups,
            overrides,
            metadata_overrides,
            effective,
            effective_metadata,
        )
    }

    /// Returns one player's cached persisted permission state.
    #[must_use]
    pub fn player_permission_state(&self, uuid: Uuid) -> Option<PermissionSubjectState> {
        self.player_permission_states.read().get(uuid).cloned()
    }

    /// Returns whether the latest published subject state assigns the operator group.
    #[must_use]
    pub(crate) fn is_operator(&self, uuid: Uuid) -> bool {
        self.player_permission_states
            .read()
            .get(uuid)
            .is_some_and(|state| state.groups().iter().any(|group| group == OP_GROUP))
    }

    /// Captures effective command permissions from the latest published subject and group state.
    #[must_use]
    pub(crate) fn command_permission_snapshot(&self, uuid: Uuid) -> PermissionSet {
        let subject = self.player_permission_state(uuid).unwrap_or_default();
        self.permission_groups
            .effective_permissions(subject.groups(), subject.overrides())
    }

    /// Atomically edits one player's persisted permission state.
    ///
    /// Persistence completes before the cache is published. An online player is
    /// refreshed from the latest cached snapshot at the server job tick stage.
    ///
    /// # Errors
    ///
    /// Returns an edit error, an unknown newly assigned group, or a storage error.
    pub async fn try_update_player_permissions<T, E>(
        self: &Arc<Self>,
        uuid: Uuid,
        update: impl FnOnce(PermissionSubjectState) -> Result<(PermissionSubjectState, T), E> + Send,
    ) -> Result<(PermissionSubjectState, T), PlayerPermissionUpdateError<E>>
    where
        T: Send,
        E: Send,
    {
        let _guard = self.player_permission_updates.lock().await;
        let mut states = self.player_permission_states.read().clone();
        let current = states.get(uuid).cloned().unwrap_or_default();
        let previous_groups = current.groups().to_vec();
        let (updated, result) = update(current).map_err(PlayerPermissionUpdateError::Edit)?;
        validate_player_permission_group_update(
            &self.permission_groups,
            &previous_groups,
            updated.groups(),
        )?;

        if updated.is_empty() {
            states.remove(uuid);
        } else {
            states.set(uuid, updated.clone());
        }
        self.player_data_storage
            .save_permission_subjects(&states)
            .await?;

        *self.player_permission_states.write() = states;
        self.queue_player_permission_refresh(uuid);
        Ok((updated, result))
    }

    /// Replaces the complete permission group config and refreshes online players.
    ///
    /// # Errors
    ///
    /// Returns an error when validation or persistence fails.
    pub async fn replace_permission_groups(
        self: &Arc<Self>,
        config: PermissionGroupsConfig,
    ) -> Result<(), PermissionGroupManagerError> {
        self.permission_groups.replace_config(config).await?;
        self.queue_online_permission_group_refresh();
        Ok(())
    }

    /// Edits the latest permission group config and refreshes online players.
    ///
    /// # Errors
    ///
    /// Returns an error when validation or persistence fails.
    pub async fn update_permission_groups(
        self: &Arc<Self>,
        update: impl FnOnce(&mut PermissionGroupsConfig) + Send,
    ) -> Result<(), PermissionGroupManagerError> {
        self.permission_groups.update_config(update).await?;
        self.queue_online_permission_group_refresh();
        Ok(())
    }

    /// Applies a fallible permission group edit and refreshes online players.
    ///
    /// # Errors
    ///
    /// Returns the caller edit error or a validation/persistence error.
    pub async fn try_update_permission_groups<T, E>(
        self: &Arc<Self>,
        update: impl FnOnce(&mut PermissionGroupsConfig) -> Result<T, E> + Send,
    ) -> Result<T, PermissionGroupUpdateError<E>>
    where
        T: Send,
        E: Send,
    {
        let result = self.permission_groups.try_update_config(update).await?;
        self.queue_online_permission_group_refresh();
        Ok(result)
    }

    /// Returns a snapshot of player identities known to this server.
    #[must_use]
    pub fn known_players(&self) -> KnownPlayers {
        self.known_players.lock().players.clone()
    }

    /// Records a connected player identity in the persistent profile cache.
    /// Returns the previous cached name for this UUID, if any.
    pub fn record_known_player(self: &Arc<Self>, profile: &GameProfile) -> Option<String> {
        let mut known = self.known_players.lock();
        let previous = known
            .players
            .by_uuid(profile.id)
            .map(|entry| entry.last_known_name().to_owned());
        let start_worker = known.record(profile.id, profile.name.clone());
        drop(known);
        if start_worker {
            self.start_known_player_save_worker();
        }
        previous
    }

    /// Records a UUID and last-known name in the persistent profile cache.
    pub fn record_known_profile(self: &Arc<Self>, uuid: Uuid, last_known_name: impl Into<String>) {
        let start_worker = self
            .known_players
            .lock()
            .record(uuid, last_known_name.into());
        if start_worker {
            self.start_known_player_save_worker();
        }
    }

    /// Resolves a vanilla game-profile command target by name or UUID.
    ///
    /// Online players and cached profiles are checked first. Uncached UUIDs remain
    /// direct UUID targets in either server mode. Offline-mode names use vanilla's
    /// deterministic UUID, while online mode queries the configured profile service.
    ///
    /// # Errors
    ///
    /// Returns an error when the profile is unknown or the profile service fails.
    pub async fn resolve_player_profile(
        self: &Arc<Self>,
        name: &str,
    ) -> Result<KnownPlayer, ProfileLookupError> {
        if let Some(profile) = self.cached_player_profile(name) {
            return Ok(profile);
        }

        match classify_uncached_player_target(name, self.config.online_mode) {
            UncachedPlayerTarget::DirectUuid(uuid) => {
                // No verified name is available, so use the canonical UUID for
                // feedback without adding a synthetic identity-cache entry.
                return Ok(direct_uuid_profile(uuid));
            }
            UncachedPlayerTarget::OfflineName => {
                let profile = KnownPlayer::new(offline_uuid(name), name.to_owned());
                self.record_known_profile(profile.uuid(), profile.last_known_name().to_owned());
                return Ok(profile);
            }
            UncachedPlayerTarget::OnlineName => {}
        }
        if !is_valid_player_name(name) {
            return Err(ProfileLookupError::UnknownPlayer(name.to_owned()));
        }

        let profile = lookup_online_profile(
            &self.profile_lookup_client,
            self.config.profile_server.as_deref(),
            name,
        )
        .await?;
        self.record_known_profile(profile.uuid(), profile.last_known_name().to_owned());
        Ok(profile)
    }

    fn cached_player_profile(self: &Arc<Self>, name: &str) -> Option<KnownPlayer> {
        let uuid = Uuid::parse_str(name).ok();
        if let Some(player) = self.get_players().into_iter().find(|player| {
            player.gameprofile.name.eq_ignore_ascii_case(name)
                || uuid.is_some_and(|uuid| player.gameprofile.id == uuid)
        }) {
            return Some(KnownPlayer::new(
                player.gameprofile.id,
                player.gameprofile.name.clone(),
            ));
        }

        let mut known = self.known_players.lock();
        if let Some(uuid) = uuid {
            return known.players.resolve_uuid(uuid);
        }
        let (profile, start_worker) = match known
            .players
            .resolve_name(name, chrono::Utc::now().timestamp_millis())
        {
            KnownPlayerNameLookup::Found(profile) => (Some(profile), false),
            KnownPlayerNameLookup::Missing => (None, false),
            KnownPlayerNameLookup::Expired => {
                let start_worker = known.mark_changed();
                (None, start_worker)
            }
        };
        drop(known);
        if start_worker {
            self.start_known_player_save_worker();
        }
        profile
    }

    fn start_known_player_save_worker(self: &Arc<Self>) {
        let server = Arc::clone(self);
        tokio::spawn(async move {
            server.run_known_player_save_worker().await;
        });
    }

    async fn run_known_player_save_worker(self: &Arc<Self>) {
        loop {
            let (players, generation) = self.known_players.lock().snapshot();
            let result = self
                .player_data_storage
                .save_known_players_if_current(&players, || {
                    self.known_players.lock().is_current(generation)
                })
                .await;
            match result {
                Ok(true | false) => {}
                Err(error) => {
                    tracing::error!(%error, "failed to save known player cache");
                }
            }
            let step = self.known_players.lock().finish_save(generation);
            if step == KnownPlayerSaveStep::SaveAgain {
                continue;
            }
            self.known_player_save_idle.notify_one();
            return;
        }
    }

    /// Waits for the coalesced identity-cache writer and persists the final snapshot.
    ///
    /// Later identity observations are ignored because the server is shutting down.
    ///
    /// # Errors
    ///
    /// Returns an error when the final rebuildable cache snapshot cannot be persisted.
    pub async fn flush_known_players(&self) -> io::Result<()> {
        let players = loop {
            let idle = self.known_player_save_idle.notified();
            let snapshot = self.known_players.lock().close_if_idle();
            if let Some(players) = snapshot {
                break players;
            }
            idle.await;
        };
        self.player_data_storage
            .save_known_players_if_current(&players, || true)
            .await
            .map(|_| ())
    }

    fn queue_player_permission_refresh(self: &Arc<Self>, uuid: Uuid) {
        self.jobs
            .spawn(FnServerJob::new(move |context: &mut ServerJobContext| {
                if let Some(server) = context.server() {
                    server.refresh_player_permission_state(uuid);
                }
            }));
    }

    pub(crate) fn refresh_player_permission_state(self: &Arc<Self>, uuid: Uuid) {
        let Some(player) = self.online_players.get_by_uuid(&uuid) else {
            return;
        };
        let state = self.player_permission_state(uuid).unwrap_or_default();
        self.apply_player_permission_state(&player, state);
        self.resend_player_permission_context(&player);
    }

    fn queue_online_permission_group_refresh(self: &Arc<Self>) {
        self.jobs
            .spawn(FnServerJob::new(|context: &mut ServerJobContext| {
                if let Some(server) = context.server() {
                    server.refresh_online_permission_groups();
                }
            }));
    }

    fn refresh_online_permission_groups(self: &Arc<Self>) {
        for player in self.get_players() {
            let state = self
                .player_permission_state(player.gameprofile.id)
                .unwrap_or_default();
            self.apply_player_permission_state(&player, state);
            self.resend_player_permission_context(&player);
        }
    }

    async fn load_domain_player_state(
        &self,
        player: &Player,
        target_domain: &str,
        fallback_world: Option<Arc<World>>,
        restore_saved_location: bool,
    ) -> Result<DomainPlayerState, String> {
        let explicit_target_world = fallback_world.is_some();
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
                let (data, spawn_position) = if restore_location {
                    let spawn_position =
                        DVec3::new(saved_data.pos[0], saved_data.pos[1], saved_data.pos[2]);
                    (
                        DomainPlayerData::SavedRestored {
                            data: Box::new(saved_data),
                        },
                        spawn_position,
                    )
                } else {
                    let (default_world, default_spawn) = self
                        .prepare_domain_default_spawn(target_domain, explicit_target_world, &world)
                        .await?;
                    world = default_world;
                    (
                        DomainPlayerData::SavedWithoutLocation {
                            data: Box::new(saved_data),
                            default_spawn,
                        },
                        default_spawn.position,
                    )
                };
                let spawn_chunk_request = world.prepare_player_spawn_chunks(spawn_position).await?;
                log::info!("Loaded saved data for player {}", player.gameprofile.name);
                Ok(DomainPlayerState {
                    world,
                    data,
                    _spawn_chunk_request: spawn_chunk_request,
                })
            }
            Ok(None) => {
                log::debug!(
                    "No saved data for player {} in domain {}, using defaults",
                    player.gameprofile.name,
                    target_domain
                );
                let (default_world, default_spawn) = self
                    .prepare_domain_default_spawn(target_domain, explicit_target_world, &world)
                    .await?;
                world = default_world;
                let spawn_chunk_request = world
                    .prepare_player_spawn_chunks(default_spawn.position)
                    .await?;
                Ok(DomainPlayerState {
                    world,
                    data: DomainPlayerData::FirstVisit { default_spawn },
                    _spawn_chunk_request: spawn_chunk_request,
                })
            }
            Err(e) => Err(format!(
                "failed to load domain player data for {} in domain {}: {e}",
                player.gameprofile.name, target_domain
            )),
        }
    }

    async fn prepare_domain_default_spawn(
        &self,
        target_domain: &str,
        explicit_target_world: bool,
        world: &Arc<World>,
    ) -> Result<(Arc<World>, PreparedSpawn), String> {
        if explicit_target_world {
            return Ok((world.clone(), Self::prepare_default_spawn(world).await?));
        }

        let (world, respawn_data) = self.respawn_world_and_data_for_domain(target_domain)?;
        let spawn = Self::prepare_respawn_spawn(&world, &respawn_data).await?;
        Ok((world, spawn))
    }

    async fn prepare_default_spawn(world: &Arc<World>) -> Result<PreparedSpawn, String> {
        let (spawn, spawn_pos) = {
            let level_data = world.level_data.read();
            (
                level_data.data().spawn.clone(),
                level_data.data().spawn_pos(),
            )
        };
        let position = world
            .find_adjusted_shared_spawn_pos(spawn_pos, world.default_gamemode)
            .await?;
        Ok(PreparedSpawn {
            position,
            rotation: (spawn.angle, 0.0),
        })
    }

    async fn prepare_respawn_spawn(
        world: &Arc<World>,
        respawn_data: &RespawnData,
    ) -> Result<PreparedSpawn, String> {
        let position = world
            .find_adjusted_shared_spawn_pos(respawn_data.pos(), world.default_gamemode)
            .await?;
        Ok(PreparedSpawn {
            position,
            rotation: (respawn_data.yaw, respawn_data.pitch),
        })
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
            DomainPlayerData::SavedRestored { data } => {
                data.apply_to_player(player);
            }
            DomainPlayerData::SavedWithoutLocation {
                data,
                default_spawn,
            } => {
                apply_default_spawn(player, &state.world, *default_spawn);
                data.apply_to_player_without_location(player);
            }
            DomainPlayerData::FirstVisit { default_spawn } => {
                apply_default_spawn(player, &state.world, *default_spawn);
            }
        }
    }

    fn schedule_root_vehicle_restore(&self, player: &Arc<Player>, state: &DomainPlayerState) {
        let Some(root_vehicle) = Self::root_vehicle_to_restore(state) else {
            player.clear_pending_root_vehicle();
            return;
        };
        player.set_pending_root_vehicle(&state.world, root_vehicle.clone());
        let Some(job) =
            RootVehicleRestoreJob::new(Arc::clone(player), Arc::clone(&state.world), &root_vehicle)
        else {
            player.clear_pending_root_vehicle();
            return;
        };
        self.jobs.spawn(job);
    }

    fn root_vehicle_to_restore(state: &DomainPlayerState) -> Option<PersistentRootVehicle> {
        match &state.data {
            DomainPlayerData::SavedRestored { data } => data.root_vehicle.clone(),
            DomainPlayerData::SavedWithoutLocation { .. } | DomainPlayerData::FirstVisit { .. } => {
                None
            }
        }
    }

    /// Spawns a restore job per persisted ender pearl, each in its own world
    /// (vanilla `ServerPlayer.loadAndSpawnEnderPearls`).
    fn schedule_ender_pearl_restores(&self, player: &Arc<Player>, state: &DomainPlayerState) {
        let pearls = Self::ender_pearls_to_restore(state);
        if pearls.is_empty() {
            player.clear_pending_ender_pearls();
            return;
        }
        player.set_pending_ender_pearls(pearls.clone());
        for pearl in pearls {
            let pearl_uuid = Uuid::from_bytes(pearl.entity.uuid);
            let Some(world) = self.resolve_pearl_world(&pearl.world, player) else {
                player.remove_pending_ender_pearl(pearl_uuid);
                continue;
            };
            if let Some(job) = EnderPearlRestoreJob::new(Arc::clone(player), world, pearl.entity) {
                self.jobs.spawn(job);
            } else {
                player.remove_pending_ender_pearl(pearl_uuid);
            }
        }
    }

    fn ender_pearls_to_restore(state: &DomainPlayerState) -> Vec<PersistentEnderPearl> {
        match &state.data {
            DomainPlayerData::SavedRestored { data }
            | DomainPlayerData::SavedWithoutLocation { data, .. } => data.ender_pearls.clone(),
            DomainPlayerData::FirstVisit { .. } => Vec::new(),
        }
    }

    fn resolve_pearl_world(&self, world_key: &str, player: &Player) -> Option<Arc<World>> {
        let Ok(key) = world_key.parse::<Identifier>() else {
            log::warn!(
                "Saved ender pearl world {world_key} for player {} is invalid, skipping",
                player.gameprofile.name
            );
            return None;
        };
        let Some(world) = self.worlds.get(&key) else {
            log::warn!(
                "Saved ender pearl world {key} for player {} is missing, skipping",
                player.gameprofile.name
            );
            return None;
        };
        Some(world.clone())
    }

    fn send_login_packet(&self, player: &Player, world: &World) {
        let reduced_debug_info = world.get_game_rule(&REDUCED_DEBUG_INFO);
        let immediate_respawn = world.get_game_rule(&IMMEDIATE_RESPAWN);
        let do_limited_crafting = world.get_game_rule(&LIMITED_CRAFTING);

        // Get world data
        let hashed_seed = world.obfuscated_seed();

        player.send_packet(CLogin {
            player_id: player.id(),
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
                game_type: player.game_mode(),
                previous_game_type: player.previous_game_mode(),
                is_debug: false,
                is_flat: world.is_flat,
                last_death_location: None,
                portal_cooldown: 0,
                sea_level: world.sea_level,
            },
            online_mode: self.config.online_mode,
            enforces_secure_chat: self.config.enforce_secure_chat,
        });
    }

    /// Gets all the players on the server
    pub fn get_players(&self) -> Vec<Arc<Player>> {
        let mut players = vec![];
        self.online_players.iter_players(|_, p: &Arc<Player>| {
            players.push(p.clone());
            true
        });
        players
    }

    /// Returns the total number of players currently online across all worlds.
    #[must_use]
    pub fn player_count(&self) -> usize {
        self.online_players.len()
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

    /// Resolves the default respawn world and data for a domain.
    pub fn respawn_world_and_data_for_domain(
        &self,
        domain: &str,
    ) -> Result<(Arc<World>, RespawnData), String> {
        let default_world = self
            .worlds
            .default_world(domain)
            .cloned()
            .ok_or_else(|| format!("domain {domain} has no default world"))?;
        let respawn_data = {
            let level_data = default_world.level_data.read();
            level_data.data().respawn_data_or_local(&default_world.key)
        };

        let Some(target_world) = self
            .worlds
            .get(respawn_data.dimension())
            .filter(|world| world.domain() == domain)
            .cloned()
        else {
            let respawn_data = default_world
                .world_border_adjusted_respawn_data(local_respawn_data_for_world(&default_world));
            return Ok((default_world.clone(), respawn_data));
        };

        let respawn_data = target_world.world_border_adjusted_respawn_data(respawn_data);
        Ok((target_world, respawn_data))
    }

    /// Returns the default respawn data sent to clients in the given domain.
    pub fn respawn_data_for_domain(&self, domain: &str) -> Result<RespawnData, String> {
        self.respawn_world_and_data_for_domain(domain)
            .map(|(_, respawn_data)| respawn_data)
    }

    /// Sets the default respawn data for the respawn data's domain and broadcasts it.
    pub fn set_respawn_data(&self, respawn_data: RespawnData) -> Result<(), String> {
        let domain = respawn_data.dimension().namespace.as_ref();
        let default_world = self
            .worlds
            .default_world(domain)
            .cloned()
            .ok_or_else(|| format!("domain {domain} has no default world"))?;
        let target_world = self
            .worlds
            .get(respawn_data.dimension())
            .filter(|world| world.domain() == domain)
            .cloned()
            .ok_or_else(|| {
                format!(
                    "respawn dimension {} is not loaded in domain {domain}",
                    respawn_data.dimension()
                )
            })?;

        if Arc::ptr_eq(&default_world, &target_world) {
            let mut level_data = default_world.level_data.write();
            let data = level_data.data_mut();
            data.set_spawn_pos(respawn_data.pos());
            data.spawn.angle = respawn_data.yaw;
            data.set_respawn_data(respawn_data.clone());
        } else {
            default_world
                .level_data
                .write()
                .data_mut()
                .set_respawn_data(respawn_data.clone());

            let mut level_data = target_world.level_data.write();
            let data = level_data.data_mut();
            data.set_spawn_pos(respawn_data.pos());
            data.spawn.angle = respawn_data.yaw;
        }

        let packet = CSetDefaultSpawnPosition {
            global_pos: respawn_data.global_pos.clone(),
            yaw: respawn_data.yaw,
            pitch: respawn_data.pitch,
        };
        for world in self
            .worlds
            .values()
            .filter(|world| world.domain() == domain)
        {
            world.broadcast_to_all(packet.clone());
        }

        Ok(())
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
        let mut next_command_data_autosave = Instant::now() + COMMAND_DATA_AUTOSAVE_INTERVAL;
        let mut player_info_ticks = 0_u64;
        let mut pending_command_executions = PendingCommandExecutionQueue::<CommandSource>::new();

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

            Self::tick_pending_command_executions(&mut pending_command_executions);
            self.tick_command_requests(&mut pending_command_executions);
            self.tick_worlds_game(tick_count, runs_normally).await;
            player_info_ticks += 1;
            if player_info_ticks > SEND_PLAYER_INFO_INTERVAL {
                let _span = tracing::trace_span!("broadcast_latency").entered();
                self.broadcast_player_latency_updates();
                player_info_ticks = 0;
            }
            self.tick_jobs(tick_count, runs_normally);
            self.process_player_joins();

            {
                let server = self.clone();
                let _ =
                    spawn_blocking(move || server.process_world_changes(tick_count, runs_normally))
                        .await;
            }

            self.process_domain_switches().await;

            if Instant::now() >= next_command_data_autosave {
                self.autosave_command_data().await;
                next_command_data_autosave = Instant::now() + COMMAND_DATA_AUTOSAVE_INTERVAL;
            }

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

        self.jobs.cancel_all();
        pending_command_executions.cancel_all();
        self.command_requests.clear();
    }

    async fn autosave_command_data(&self) {
        tracing::debug!("Command data autosave started");
        let results = self.save_command_data().await;
        match results.scoreboards {
            Ok(saved) => tracing::debug!(saved, "Domain scoreboard autosave completed"),
            Err(error) => tracing::error!(%error, "Domain scoreboard autosave failed"),
        }
        match results.storage {
            Ok(saved) => tracing::debug!(saved, "Domain command-storage autosave completed"),
            Err(error) => tracing::error!(%error, "Domain command-storage autosave failed"),
        }
    }

    fn tick_pending_command_executions(pending: &mut PendingCommandExecutionQueue<CommandSource>) {
        let stats = pending.tick(COMMAND_RESUMPTIONS_PER_TICK);
        if stats.polled == COMMAND_RESUMPTIONS_PER_TICK && stats.pending > 0 {
            tracing::debug!(
                polled = stats.polled,
                finished = stats.finished,
                pending = stats.pending,
                "Command resumption tick reached per-tick processing limit"
            );
        }
    }

    fn tick_command_requests(
        self: &Arc<Self>,
        pending: &mut PendingCommandExecutionQueue<CommandSource>,
    ) {
        let mut handled = 0;
        for _ in 0..COMMAND_REQUESTS_PER_TICK {
            let Some(request) = self
                .command_requests
                .pop_front_runnable(|sender| !pending.blocks(sender.key()))
            else {
                break;
            };
            handled += 1;

            match request {
                CommandRequest::Execute { sender, command } => {
                    if sender
                        .get_player()
                        .is_some_and(|player| player.connection.closed())
                    {
                        continue;
                    }
                    self.execute_command_request(pending, sender, &command);
                }
                CommandRequest::Suggestions {
                    player,
                    transaction_id,
                    input,
                } => {
                    if player.connection.closed() {
                        continue;
                    }
                    self.send_command_suggestions(&player, transaction_id, &input);
                }
            }
        }

        if handled == COMMAND_REQUESTS_PER_TICK {
            tracing::debug!(handled, "Command request tick reached its processing limit");
        }
    }

    fn execute_command_request(
        self: &Arc<Self>,
        pending: &mut PendingCommandExecutionQueue<CommandSource>,
        sender: CommandSender,
        command: &str,
    ) {
        let sender_key = sender.key();
        let source = CommandSource::new(sender, Arc::clone(self));
        let command = command.strip_prefix('/').unwrap_or(command);
        let chain = {
            let dispatcher = self.command_dispatcher.read();
            let parse = dispatcher.parse(command, source.clone());
            dispatcher.context_chain(parse)
        };
        let chain = match chain {
            Ok(chain) => chain,
            Err(error) => {
                source.handle_error(&error, false);
                return;
            }
        };

        let mut execution = CommandExecutionContext::for_source(&source);
        execution.queue_initial_command(chain, source, CommandResultCallback::empty());
        if execution.run() == ExecutionStop::Suspended
            && !pending.push_suspended(sender_key, execution)
        {
            tracing::error!("suspended command execution could not be retained");
        }
    }

    fn send_command_suggestions(
        self: &Arc<Self>,
        player: &Arc<Player>,
        transaction_id: i32,
        input: &str,
    ) {
        let suggestions =
            self.build_command_suggestions(CommandSender::Player(Arc::clone(player)), input);
        match suggestions {
            Ok(suggestions) => {
                player.send_packet(command_suggestions_packet(transaction_id, &suggestions));
            }
            Err(error) => {
                tracing::warn!(%error, "failed to build command suggestions");
                player.send_packet(CCommandSuggestions::new(transaction_id, 0, 0, Vec::new()));
            }
        }
    }

    fn build_command_suggestions(
        self: &Arc<Self>,
        sender: CommandSender,
        input: &str,
    ) -> Result<Suggestions, SuggestionError> {
        let source = CommandSource::new(sender, Arc::clone(self));
        let mut reader = StringReader::new(input);
        if reader.peek() == Some('/') {
            reader.skip();
        }
        let dispatcher = self.command_dispatcher.read();
        let parse = dispatcher.parse_reader(reader, source);
        dispatcher.completion_suggestions(&parse)
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
        encode_cache: &mut rustc_hash::FxHashMap<ChunkPos, EncodedChunk>,
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
        let sent_chunks = {
            let mut sender = player.chunk_sender.lock();
            sender.commit_batch(&batch, encoded, connection, &player.chunk_send_epoch)
        };

        if sent_chunks.is_empty() {
            return;
        }

        let Some(view) = *player.last_tracking_view.lock() else {
            return;
        };
        let sent_chunks = player.chunk_sender.lock().sent_chunks_snapshot();
        world
            .entity_tracker()
            .update_player(player, &view, |chunk| sent_chunks.contains(&chunk));
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

            if total.as_millis() >= 50 {
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

    fn process_world_changes(self: &Arc<Self>, tick_count: u64, runs_normally: bool) {
        let mut changes = mem::take(&mut *self.pending_world_changes.lock());
        for world in self.worlds.values() {
            changes.extend(world.drain_world_changes());
        }

        for (entity, request) in changes {
            if entity.is_removed() {
                continue;
            }
            match request {
                WorldChangeRequest::Computed(transition) => {
                    change_entity_world(entity, &transition);
                }
                WorldChangeRequest::WorldSpawn { target_world } => {
                    let transition = world_spawn_transition(target_world);
                    change_entity_world(entity, &transition);
                }
                WorldChangeRequest::Portal {
                    portal: PortalKind::Nether,
                    source_world,
                    portal_pos,
                    pending_token,
                } => {
                    self.queue_nether_portal_change(
                        entity,
                        source_world,
                        portal_pos,
                        pending_token,
                        tick_count,
                        runs_normally,
                    );
                }
                WorldChangeRequest::Portal {
                    portal: PortalKind::End,
                    source_world,
                    portal_pos: _,
                    pending_token,
                } => {
                    self.queue_end_portal_change(
                        entity,
                        source_world,
                        pending_token,
                        tick_count,
                        runs_normally,
                    );
                }
                WorldChangeRequest::Portal {
                    portal: PortalKind::EndGateway,
                    source_world,
                    portal_pos,
                    pending_token,
                } => {
                    self.queue_end_gateway_change(
                        entity,
                        source_world,
                        portal_pos,
                        pending_token,
                        tick_count,
                        runs_normally,
                    );
                }
            }
        }
    }

    fn queue_nether_portal_change(
        self: &Arc<Self>,
        entity: SharedEntity,
        source_world: Arc<World>,
        portal_pos: BlockPos,
        pending_token: PendingWorldChangeToken,
        tick_count: u64,
        runs_normally: bool,
    ) {
        if !portal_entity_still_valid(&entity, &source_world, pending_token) {
            clear_pending_world_change(&entity, pending_token);
            return;
        }
        let Some(target_world) = self.worlds.resolve_nether_portal_target(&source_world) else {
            log::warn!(
                "No Nether portal target world loaded for source world {}",
                source_world.key
            );
            clear_pending_world_change(&entity, pending_token);
            return;
        };
        if !is_allowed_to_enter_portal(&source_world, &target_world)
            || !self.can_teleport_between_worlds(entity.as_ref(), &source_world, &target_world)
        {
            clear_pending_world_change(&entity, pending_token);
            return;
        }
        let to_nether = is_nether_dimension_type(&target_world);
        let approximate_exit_pos = nether_portal::approximate_exit_position(
            &source_world,
            &target_world,
            entity.position(),
        );
        self.jobs.poll_now_or_spawn(
            Arc::downgrade(self),
            tick_count,
            runs_normally,
            NetherPortalTeleportJob::new(
                entity,
                source_world,
                target_world,
                portal_pos,
                approximate_exit_pos,
                to_nether,
                pending_token,
            ),
        );
    }

    fn queue_end_portal_change(
        self: &Arc<Self>,
        entity: SharedEntity,
        source_world: Arc<World>,
        pending_token: PendingWorldChangeToken,
        tick_count: u64,
        runs_normally: bool,
    ) {
        if !portal_entity_still_valid(&entity, &source_world, pending_token) {
            clear_pending_world_change(&entity, pending_token);
            return;
        }
        if !is_end_dimension_type(&source_world) {
            self.queue_end_entry_portal_change(
                entity,
                source_world,
                pending_token,
                tick_count,
                runs_normally,
            );
            return;
        }

        if entity.as_player().is_some() {
            self.queue_end_portal_player_return_change(
                entity,
                source_world,
                pending_token,
                tick_count,
                runs_normally,
            );
            return;
        }

        self.queue_end_portal_entity_return_change(
            entity,
            source_world,
            pending_token,
            tick_count,
            runs_normally,
        );
    }

    fn queue_end_entry_portal_change(
        self: &Arc<Self>,
        entity: SharedEntity,
        source_world: Arc<World>,
        pending_token: PendingWorldChangeToken,
        tick_count: u64,
        runs_normally: bool,
    ) {
        let Some(target_world) = self.worlds.resolve_end_entry_portal_target(&source_world) else {
            log::warn!(
                "No End portal target world loaded for source world {}",
                source_world.key
            );
            clear_pending_world_change(&entity, pending_token);
            return;
        };
        if !is_allowed_to_enter_portal(&source_world, &target_world)
            || !self.can_teleport_between_worlds(entity.as_ref(), &source_world, &target_world)
        {
            clear_pending_world_change(&entity, pending_token);
            return;
        }
        self.jobs.poll_now_or_spawn(
            Arc::downgrade(self),
            tick_count,
            runs_normally,
            EndPortalTeleportJob::entry_to_end(entity, source_world, target_world, pending_token),
        );
    }

    fn queue_end_portal_player_return_change(
        self: &Arc<Self>,
        entity: SharedEntity,
        source_world: Arc<World>,
        pending_token: PendingWorldChangeToken,
        tick_count: u64,
        runs_normally: bool,
    ) {
        let (target_world, respawn_data) =
            match self.strict_respawn_world_and_data_for_domain(source_world.domain()) {
                Ok(resolved) => resolved,
                Err(error) => {
                    log::warn!(
                        "No End portal return target world loaded for source world {}: {error}",
                        source_world.key
                    );
                    clear_pending_world_change(&entity, pending_token);
                    return;
                }
            };
        if !is_allowed_to_enter_portal(&source_world, &target_world)
            || !self.can_teleport_between_worlds(entity.as_ref(), &source_world, &target_world)
        {
            clear_pending_world_change(&entity, pending_token);
            return;
        }
        match EndPortalTeleportJob::returning_player(
            Arc::clone(&entity),
            source_world,
            target_world,
            respawn_data,
            pending_token,
        ) {
            Ok(job) => {
                self.jobs
                    .poll_now_or_spawn(Arc::downgrade(self), tick_count, runs_normally, job);
            }
            Err(error) => {
                clear_pending_world_change(&entity, pending_token);
                log::error!("Failed to schedule End portal player return: {error}");
            }
        }
    }

    fn queue_end_portal_entity_return_change(
        self: &Arc<Self>,
        entity: SharedEntity,
        source_world: Arc<World>,
        pending_token: PendingWorldChangeToken,
        tick_count: u64,
        runs_normally: bool,
    ) {
        let (target_world, respawn_data) =
            match self.strict_respawn_world_and_data_for_domain(source_world.domain()) {
                Ok(resolved) => resolved,
                Err(error) => {
                    log::warn!(
                        "No End portal return target world loaded for source world {}: {error}",
                        source_world.key
                    );
                    clear_pending_world_change(&entity, pending_token);
                    return;
                }
            };
        if !is_allowed_to_enter_portal(&source_world, &target_world)
            || !self.can_teleport_between_worlds(entity.as_ref(), &source_world, &target_world)
        {
            clear_pending_world_change(&entity, pending_token);
            return;
        }
        self.jobs.poll_now_or_spawn(
            Arc::downgrade(self),
            tick_count,
            runs_normally,
            EndPortalTeleportJob::returning_entity(
                entity,
                source_world,
                target_world,
                respawn_data,
                pending_token,
            ),
        );
    }

    fn queue_end_gateway_change(
        self: &Arc<Self>,
        entity: SharedEntity,
        source_world: Arc<World>,
        portal_pos: BlockPos,
        pending_token: PendingWorldChangeToken,
        tick_count: u64,
        runs_normally: bool,
    ) {
        if !portal_entity_still_valid(&entity, &source_world, pending_token) {
            clear_pending_world_change(&entity, pending_token);
            return;
        }
        let source_is_end = is_end_dimension_type(&source_world);
        let Some(job) = EndGatewayTeleportJob::new(
            Arc::clone(&entity),
            source_world,
            portal_pos,
            source_is_end,
            pending_token,
        ) else {
            tracing::debug!("End gateway world change ignored because no destination is available");
            clear_pending_world_change(&entity, pending_token);
            return;
        };
        self.jobs
            .poll_now_or_spawn(Arc::downgrade(self), tick_count, runs_normally, job);
    }

    fn can_teleport_between_worlds(
        &self,
        entity: &dyn Entity,
        source_world: &World,
        target_world: &World,
    ) -> bool {
        can_teleport_between_worlds(entity, source_world, target_world, |uuid| {
            self.projectile_owner_seen_credits_in_domain(source_world.domain(), uuid)
        })
    }

    fn projectile_owner_seen_credits_in_domain(
        &self,
        domain: &str,
        uuid: &uuid::Uuid,
    ) -> Option<bool> {
        self.worlds
            .values()
            .filter(|world| world.domain() == domain)
            .find_map(|world| {
                world.get_entity_by_uuid(uuid).and_then(|entity| {
                    entity
                        .as_player()
                        .map(super::player::Player::has_seen_credits)
                })
            })
    }

    fn strict_respawn_world_and_data_for_domain(
        &self,
        domain: &str,
    ) -> Result<(Arc<World>, RespawnData), String> {
        let default_world = self
            .worlds
            .default_world(domain)
            .cloned()
            .ok_or_else(|| format!("domain {domain} has no default world"))?;
        let respawn_data = {
            let level_data = default_world.level_data.read();
            level_data.data().respawn_data_or_local(&default_world.key)
        };
        let target_world = self
            .worlds
            .get(respawn_data.dimension())
            .filter(|world| world.domain() == domain)
            .cloned()
            .ok_or_else(|| {
                format!(
                    "respawn dimension {} is not loaded in domain {domain}",
                    respawn_data.dimension()
                )
            })?;
        Ok((target_world, respawn_data))
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

        let current_domain = player.get_world().domain().to_owned();
        if current_domain == target_domain {
            return Ok(());
        }

        let current_data = PersistentPlayerData::from_player(&player);
        if let Err(e) = self
            .player_data_storage
            .save_domain_data(&current_domain, player.gameprofile.id, &current_data)
            .await
        {
            return Err(format!("failed to save current domain data: {e}"));
        }

        if player.connection.closed() {
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
                return Err(error);
            }
        };

        if player.connection.closed() {
            return Ok(());
        }

        let restore_player = Arc::clone(&player);
        player.reset_after_domain_save_and_restore(target_state.world.clone(), || {
            Self::apply_domain_player_state(&restore_player, &target_state);
        });
        let pos = player.position();
        let rotation = player.rotation();
        if !player.spawn(pos, rotation, ResetReason::WorldChange) {
            return Err("failed to add player to target world".to_owned());
        }
        self.schedule_root_vehicle_restore(&player, &target_state);
        self.schedule_ender_pearl_restores(&player, &target_state);

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

    #[tracing::instrument(level = "trace", skip(self), name = "tick_worlds")]
    async fn tick_worlds_game(&self, tick_count: u64, runs_normally: bool) {
        let mut tasks = Vec::with_capacity(self.worlds.len());
        for world in self.worlds.values() {
            let world_clone = world.clone();
            tasks.push(spawn_blocking(move || {
                if runs_normally {
                    world_clone.chunk_map.tick_timed_tickets();
                }
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
            if timings.elapsed.as_millis() < 50 {
                continue;
            }
            let cm = &timings.chunk_map;
            tracing::warn!(
                world = i,
                elapsed = ?timings.elapsed,
                tick_count,
                entity_tick = ?timings.entity_tick,
                broadcast_changes = ?cm.broadcast_changes,
                collect_tickable = ?cm.collect_tickable,
                tick_chunks = ?cm.tick_chunks,
                tick_block_entities = ?cm.tick_block_entities,
                tickable_count = cm.tickable_count,
                total_chunks = cm.total_chunks,
                "Game tick slow"
            );
        }
    }

    fn tick_jobs(self: &Arc<Self>, tick_count: u64, runs_normally: bool) {
        let stats = self
            .jobs
            .tick(Arc::downgrade(self), tick_count, runs_normally);
        if stats.polled > 0 && stats.pending > 0 && tick_count.is_multiple_of(100) {
            tracing::debug!(
                polled = stats.polled,
                finished = stats.finished,
                pending = stats.pending,
                "Server jobs pending"
            );
        }
    }

    /// Logs and broadcasts a system chat message to online players.
    fn broadcast_system_chat(&self, message: &TextComponent, excluded_player: Option<Uuid>) {
        log::info!("{}", message.to_plain(&DisplayResolutor));
        self.online_players.iter_players(|uuid, player| {
            if Some(*uuid) != excluded_player {
                player.send_packet(CSystemChat::new(message, false, player));
            }
            true
        });
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

        self.broadcast_to_online_with(|player| CTabList::new(&header, &footer, player));
    }

    /// Broadcasts a sprint completion report to all players.
    pub(crate) fn broadcast_sprint_report(&self, report: &SprintReport) {
        let message: TextComponent = translations::COMMANDS_TICK_SPRINT_REPORT
            .message([
                TextComponent::from(format!("{}", report.ticks_per_second)),
                TextComponent::from(format!("{:.2}", report.ms_per_tick)),
            ])
            .into();

        self.broadcast_system_chat(&message, None);
    }

    fn broadcast_player_join_message(&self, player: &Player, previous_name: Option<&str>) {
        let display_name = player.display_name();
        // Fallback to the current name when the cache has no prior entry.
        let old_name = previous_name.unwrap_or(player.gameprofile.name.as_str());
        let message: TextComponent = if player.gameprofile.name.eq_ignore_ascii_case(old_name) {
            translations::MULTIPLAYER_PLAYER_JOINED
                .message([display_name])
                .into()
        } else {
            translations::MULTIPLAYER_PLAYER_JOINED_RENAMED
                .message([display_name, TextComponent::plain(old_name.to_owned())])
                .into()
        };
        let message = message.color(Color::Yellow);
        self.broadcast_system_chat(&message, Some(player.gameprofile.id));
    }

    fn broadcast_player_leave_message(&self, player: &Player) {
        let message: TextComponent = translations::MULTIPLAYER_PLAYER_LEFT
            .message([player.display_name()])
            .into();
        let message = message.color(Color::Yellow);
        self.broadcast_system_chat(&message, None);
    }

    /// Broadcasts the current tick rate and frozen state to all clients.
    /// This should be called whenever the tick rate or frozen state changes.
    pub fn broadcast_ticking_state(&self) {
        let tick_manager = self.tick_rate_manager.read();
        let packet = CTickingState::new(tick_manager.tick_rate(), tick_manager.is_frozen());
        drop(tick_manager);

        self.broadcast_to_online(packet);
    }

    /// Broadcasts the current step tick count to all clients.
    /// This should be called whenever the step tick count changes.
    pub fn broadcast_ticking_step(&self) {
        let tick_manager = self.tick_rate_manager.read();
        let packet = CTickingStep::new(tick_manager.frozen_ticks_to_run());
        drop(tick_manager);

        self.broadcast_to_online(packet);
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
    pub fn resend_player_context(self: &Arc<Self>, player: &Arc<Player>) {
        player.send_difficulty();
        player.send_inventory_to_remote();

        self.resend_player_permission_context(player);

        self.send_ticking_state_to_player(player);

        player.send_packet(CGameEvent {
            event: GameEventType::ChangeGameMode,
            data: player.game_mode().into(),
        });
    }

    /// Resends the command tree and vanilla client permission-level projection.
    pub fn resend_player_permission_context(self: &Arc<Self>, player: &Arc<Player>) {
        let world = player.get_world();
        player.send_packet(CEntityEvent {
            entity_id: player.id(),
            event: client_permission_event(player, &world),
        });

        let server = player.server();
        if !Arc::ptr_eq(&server, self) {
            tracing::error!(
                player = %player.gameprofile.name,
                "cannot project commands from a different server"
            );
            return;
        }
        let Some(shared_player) = self.online_players.get_by_uuid(&player.gameprofile.id) else {
            tracing::error!(
                player = %player.gameprofile.name,
                "cannot project commands for a player outside the online player map"
            );
            return;
        };
        if !Arc::ptr_eq(&shared_player, player) {
            tracing::error!(
                player = %player.gameprofile.name,
                "cannot project commands for a stale player handle"
            );
            return;
        }
        let source = CommandSource::new(CommandSender::Player(shared_player), server);
        let commands = {
            let dispatcher = self.command_dispatcher.read();
            command_tree_packet(&dispatcher, &source)
        };
        match commands {
            Ok(commands) => player.send_packet(commands),
            Err(error) => tracing::error!(
                player = %player.gameprofile.name,
                %error,
                "failed to project the player's command tree"
            ),
        }
    }
    /// Queues a world change to be processed after the current tick.
    pub fn queue_world_change(&self, entity: SharedEntity, request: WorldChangeRequest) {
        self.pending_world_changes.lock().push((entity, request));
    }
}
