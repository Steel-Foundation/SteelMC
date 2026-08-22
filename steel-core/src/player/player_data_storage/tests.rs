use crate::entity::DEFAULT_MAX_AIR_SUPPLY;
use crate::permission::PermissionSet;
use crate::player::KnownPlayer;
use crate::player::player_data::PersistentStat;
use crate::player::player_data_storage::file_storage::FilePlayerDataStorage;
use crate::player::player_data_storage::known_players::{
    KnownPlayersFile, decode_known_players_file, encode_known_players_file,
};
use crate::player::player_data_storage::permissions::{
    PlayerPermissionsFile, serialize_player_permissions_file,
};
use crate::player::player_data_storage::stats::PlayerStatsFile;
use crate::player::player_data_storage::*;
use std::collections::BTreeMap;
use std::{
    env,
    time::{SystemTime, UNIX_EPOCH},
};
use steel_registry::stat::vanilla_stat_types;
use steel_registry::{init_vanilla_registry, vanilla_blocks, vanilla_custom_stats, vanilla_items};
use tokio::fs;

fn temp_storage_root(name: &str) -> PathBuf {
    let suffix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock should be after unix epoch")
        .as_nanos();
    env::temp_dir().join(format!("steelmc-player-storage-{name}-{suffix}"))
}

fn sample_player_file(data_version: i32) -> PlayerDataFile {
    PlayerDataFile {
        data_version,
        pos: [1.0, 2.0, 3.0],
        motion: [0.0, 0.0, 0.0],
        rotation: [90.0, 10.0],
        on_ground: true,
        fall_flying: false,
        remaining_fire_ticks: 0,
        ticks_frozen: 0,
        is_in_powder_snow: false,
        was_in_powder_snow: false,
        has_visual_fire: false,
        health: 20.0,
        game_mode: 2,
        prev_game_mode: Some(0),
        abilities: AbilitiesFile {
            invulnerable: false,
            flying: false,
            may_fly: false,
            instabuild: false,
            may_build: true,
            flying_speed: 0.05,
            walking_speed: 0.1,
        },
        inventory: Vec::new(),
        selected_slot: 4,
        world: "lobby:void".to_owned(),
        food_level: 20,
        food_saturation_level: 5.0,
        food_exhaustion_level: 0.0,
        food_tick_timer: 0,
        experience_level: 7,
        experience_progress: 0.5,
        experience_total: 32,
        score: 9,
        seen_credits: true,
        root_vehicle: None,
        respawn_config: None,
        ender_pearls: Vec::new(),
    }
}

fn sample_persistent_entity() -> PersistentEntity {
    PersistentEntity {
        entity_type: Identifier::vanilla_static("minecart"),
        uuid: [7; 16],
        pos: [4.0, 65.0, 6.0],
        motion: [0.0, 0.0, 0.0],
        rotation: [45.0, 0.0],
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

fn sample_player_stats_file() -> PlayerStatsFile {
    let mut custom_stats = BTreeMap::new();
    custom_stats.insert(vanilla_custom_stats::JUMP.key.clone(), 14);
    custom_stats.insert(vanilla_custom_stats::WALK_ONE_CM.key.clone(), 3);
    custom_stats.insert(vanilla_custom_stats::TOTAL_WORLD_TIME.key.clone(), 5555);

    let mut broken_stats = BTreeMap::new();
    broken_stats.insert(vanilla_blocks::DIAMOND_BLOCK.key.clone(), 5);
    broken_stats.insert(vanilla_blocks::REINFORCED_DEEPSLATE.key.clone(), 1);

    let mut stats = BTreeMap::new();
    stats.insert(vanilla_stat_types::CUSTOM.key.clone(), custom_stats);
    stats.insert(vanilla_stat_types::BLOCK_MINED.key.clone(), broken_stats);

    PlayerStatsFile { stats }
}

#[tokio::test]
async fn atomic_path_replacement_retains_the_last_committed_generation() {
    let root = temp_storage_root("atomic-replacement");
    let path = root.join("state.dat");

    FilePlayerDataStorage::write_atomic_path_locked(&path, b"first".as_slice())
        .await
        .expect("first generation should publish");
    FilePlayerDataStorage::write_atomic_path_locked(&path, b"second".as_slice())
        .await
        .expect("second generation should publish");

    assert_eq!(
        fs::read(&path).await.expect("live file should be readable"),
        b"second"
    );
    assert_eq!(
        fs::read(FilePlayerDataStorage::atomic_backup_path(&path))
            .await
            .expect("backup should be readable"),
        b"first"
    );

    fs::remove_dir_all(root)
        .await
        .expect("temporary storage should be removable");
}

#[tokio::test]
async fn interrupted_permission_publication_recovers_before_the_next_update() {
    let root = temp_storage_root("permission-recovery");
    let storage = FilePlayerDataStorage::new(root.clone())
        .await
        .expect("test storage should initialize");
    let mut subjects = PermissionSubjectIndex::new();
    for (uuid, group) in [
        (Uuid::from_u128(10), "builder"),
        (Uuid::from_u128(20), "moderator"),
    ] {
        subjects.set(
            uuid,
            PermissionSubjectState::new(vec![group.to_owned()], PermissionSet::new()),
        );
    }
    storage
        .save_permission_subjects(&subjects)
        .await
        .expect("permission subjects should persist");

    let path = storage.player_permissions_file();
    let backup = FilePlayerDataStorage::atomic_backup_path(&path);
    let temporary = FilePlayerDataStorage::atomic_temp_path(&path);
    fs::rename(&path, &backup)
        .await
        .expect("legacy publication should reach its interrupted state");
    fs::write(&temporary, b"uncommitted replacement")
        .await
        .expect("uncommitted replacement should be staged");

    let mut recovered = storage
        .load_permission_subjects()
        .await
        .expect("last committed permissions should recover");
    assert_eq!(recovered.len(), 2);
    assert_eq!(
        recovered
            .get(Uuid::from_u128(10))
            .map(PermissionSubjectState::groups),
        Some(["builder".to_owned()].as_slice())
    );
    assert_eq!(
        recovered
            .get(Uuid::from_u128(20))
            .map(PermissionSubjectState::groups),
        Some(["moderator".to_owned()].as_slice())
    );
    assert!(!temporary.exists());

    recovered.set(
        Uuid::from_u128(30),
        PermissionSubjectState::new(vec!["operator".to_owned()], PermissionSet::new()),
    );
    storage
        .save_permission_subjects(&recovered)
        .await
        .expect("an update after recovery should preserve existing subjects");
    let updated = storage
        .load_permission_subjects()
        .await
        .expect("updated permissions should load");
    assert_eq!(updated.len(), 3);

    fs::remove_dir_all(root)
        .await
        .expect("temporary storage should be removable");
}

#[tokio::test]
async fn corrupt_live_permission_file_does_not_fall_back_to_its_backup() {
    let root = temp_storage_root("corrupt-live-permissions");
    let storage = FilePlayerDataStorage::new(root.clone())
        .await
        .expect("test storage should initialize");
    let mut subjects = PermissionSubjectIndex::new();
    subjects.set(
        Uuid::from_u128(42),
        PermissionSubjectState::new(vec!["op".to_owned()], PermissionSet::new()),
    );
    storage
        .save_permission_subjects(&subjects)
        .await
        .expect("permission subject should persist");
    let path = storage.player_permissions_file();
    let backup = FilePlayerDataStorage::atomic_backup_path(&path);
    fs::copy(&path, &backup)
        .await
        .expect("valid backup should be staged");
    fs::write(&path, b"not valid permission TOML")
        .await
        .expect("live permission file should be corrupted for the test");

    let error = storage
        .load_permission_subjects()
        .await
        .expect_err("a corrupt live permission file must remain startup-fatal");
    assert_eq!(error.kind(), io::ErrorKind::InvalidData);
    assert_eq!(
        fs::read_to_string(&path)
            .await
            .expect("corrupt live file should remain in place"),
        "not valid permission TOML"
    );

    fs::remove_dir_all(root)
        .await
        .expect("temporary storage should be removable");
}

#[tokio::test]
async fn interrupted_first_known_player_publication_discards_its_temporary_file() {
    let root = temp_storage_root("known-player-interrupted-first-write");
    let storage = FilePlayerDataStorage::new(root.clone())
        .await
        .expect("test storage should initialize");
    let uuid = Uuid::from_u128(42);
    let players =
        KnownPlayers::from_entries([KnownPlayer::with_expiration(uuid, "Steve", 1_234_567)]);
    let path = storage.known_players_file();
    let temporary = FilePlayerDataStorage::atomic_temp_path(&path);
    let bytes = encode_known_players_file(&KnownPlayersFile::from_known_players(&players))
        .expect("known players should encode");
    fs::write(&temporary, bytes)
        .await
        .expect("first publication should reach its interrupted state");

    let loaded = storage
        .load_known_players()
        .await
        .expect("uncommitted known-player state should be ignored");
    assert!(loaded.entries().is_empty());
    assert!(!path.exists());
    assert!(!temporary.exists());

    fs::remove_dir_all(root)
        .await
        .expect("temporary storage should be removable");
}

#[tokio::test]
async fn corrupt_known_player_cache_loads_as_empty() {
    let root = temp_storage_root("corrupt-known-players");
    let storage = FilePlayerDataStorage::new(root.clone())
        .await
        .expect("test storage should initialize");
    let path = storage.known_players_file();
    fs::write(&path, b"not a known-player cache")
        .await
        .expect("known-player cache should be corrupted for the test");

    let loaded = storage
        .load_known_players()
        .await
        .expect("a corrupt optional cache should not prevent startup");
    assert!(loaded.entries().is_empty());
    assert_eq!(
        fs::read(&path)
            .await
            .expect("the corrupt cache should remain available for diagnosis"),
        b"not a known-player cache"
    );

    fs::remove_dir_all(root)
        .await
        .expect("temporary storage should be removable");
}

#[tokio::test]
async fn incompatible_known_player_cache_version_loads_as_empty() {
    let root = temp_storage_root("incompatible-known-player-version");
    let storage = FilePlayerDataStorage::new(root.clone())
        .await
        .expect("test storage should initialize");
    let uuid = Uuid::from_u128(42);
    let players = KnownPlayers::from_entries([KnownPlayer::new(uuid, "Steve")]);
    let mut bytes = encode_known_players_file(&KnownPlayersFile::from_known_players(&players))
        .expect("known-player cache should encode");
    bytes[4..6].copy_from_slice(&u16::MAX.to_le_bytes());
    fs::write(storage.known_players_file(), bytes)
        .await
        .expect("incompatible known-player cache should be seeded");

    let loaded = storage
        .load_known_players()
        .await
        .expect("an incompatible optional cache should not prevent startup");
    assert!(loaded.entries().is_empty());
    assert!(loaded.by_uuid(uuid).is_none());

    fs::remove_dir_all(root)
        .await
        .expect("temporary storage should be removable");
}

#[tokio::test]
async fn interrupted_first_permission_publication_does_not_apply_uncommitted_access() {
    let root = temp_storage_root("permission-interrupted-first-write");
    let storage = FilePlayerDataStorage::new(root.clone())
        .await
        .expect("test storage should initialize");
    let path = storage.player_permissions_file();
    let temporary = FilePlayerDataStorage::atomic_temp_path(&path);
    let mut file = PlayerPermissionsFile::default();
    set_permission_subject(
        &mut file,
        Uuid::from_u128(42),
        &PermissionSubjectState::new(vec!["op".to_owned()], PermissionSet::new()),
    );
    let contents =
        serialize_player_permissions_file(&file).expect("uncommitted permissions should serialize");
    fs::write(&temporary, contents)
        .await
        .expect("uncommitted permissions should be staged");

    let loaded = storage
        .load_permission_subjects()
        .await
        .expect("uncommitted permissions should be ignored");
    assert!(loaded.is_empty());
    assert!(!path.exists());
    assert!(!temporary.exists());

    fs::remove_dir_all(root)
        .await
        .expect("temporary storage should be removable");
}

#[tokio::test]
async fn known_player_cache_round_trips_and_rejects_stale_writes() {
    let root = temp_storage_root("known-players");
    let storage = match FilePlayerDataStorage::new(root.clone()).await {
        Ok(storage) => storage,
        Err(error) => panic!("test storage should initialize: {error}"),
    };
    let uuid = Uuid::from_u128(42);
    let players =
        KnownPlayers::from_entries([KnownPlayer::with_expiration(uuid, "Steve", 1_234_567)]);

    let stale = storage
        .save_known_players_if_current(&players, || false)
        .await;
    assert!(matches!(stale, Ok(false)));
    assert!(!storage.known_players_file().exists());

    let saved = storage
        .save_known_players_if_current(&players, || true)
        .await;
    assert!(matches!(saved, Ok(true)));
    let loaded = storage.load_known_players().await;
    let Ok(loaded) = loaded else {
        panic!("known players should load");
    };
    assert_eq!(
        loaded.by_uuid(uuid).map(KnownPlayer::last_known_name),
        Some("Steve")
    );
    assert_eq!(
        loaded.by_uuid(uuid).map(KnownPlayer::expires_at_millis),
        Some(1_234_567)
    );

    fs::remove_dir_all(root)
        .await
        .expect("temporary storage should be removable");
}

#[test]
fn known_player_cache_persists_vanillas_mru_limit() {
    let players = KnownPlayers::from_entries((0_u128..=1_000).map(|value| {
        KnownPlayer::with_expiration(Uuid::from_u128(value), format!("Player{value}"), 1_234_567)
    }));
    let encoded = encode_known_players_file(&KnownPlayersFile::from_known_players(&players));
    let Ok(encoded) = encoded else {
        panic!("known player cache should encode");
    };
    let decoded =
        decode_known_players_file(&encoded).and_then(KnownPlayersFile::into_known_players);
    let Ok(decoded) = decoded else {
        panic!("known player cache should decode");
    };

    assert_eq!(decoded.entries().len(), 1_000);
    assert!(decoded.by_uuid(Uuid::from_u128(999)).is_some());
    assert!(decoded.by_uuid(Uuid::from_u128(1_000)).is_none());
}

#[test]
fn player_file_roundtrip_preserves_domain_world_data() {
    let file = sample_player_file(PLAYER_DATA_VERSION);

    let encoded = encode_player_file(&file).expect("player file should encode");
    let decoded = decode_player_file(&encoded).expect("player file should decode");

    assert_eq!(
        u16::from_le_bytes([encoded[4], encoded[5]]),
        PLAYER_STORAGE_VERSION
    );
    assert_eq!(decoded.world, "lobby:void");
    assert_eq!(decoded.game_mode, 2);
    assert_eq!(decoded.selected_slot, 4);
    assert_eq!(decoded.experience_level, 7);
    assert_eq!(decoded.experience_progress.to_bits(), 0.5_f32.to_bits());
    assert_eq!(decoded.experience_total, 32);
    assert_eq!(decoded.score, 9);
    assert!(decoded.seen_credits);
}

#[test]
fn player_file_roundtrip_preserves_absent_previous_game_mode() {
    let mut file = sample_player_file(PLAYER_DATA_VERSION);
    file.prev_game_mode = None;

    let encoded = encode_player_file(&file).expect("player file should encode");
    let decoded = decode_player_file(&encoded).expect("player file should decode");
    let persistent = decoded
        .into_persistent()
        .expect("player file should convert");

    assert_eq!(persistent.prev_game_mode, None);
}

#[test]
fn global_file_roundtrip_preserves_last_active_domain() {
    let file = GlobalPlayerDataFile {
        data_version: GLOBAL_PLAYER_DATA_VERSION,
        last_active_domain: "minecraft".to_owned(),
    };

    let encoded = encode_global_file(&file).expect("global file should encode");
    let decoded = decode_global_file(&encoded).expect("global file should decode");

    assert_eq!(
        u16::from_le_bytes([encoded[4], encoded[5]]),
        GLOBAL_STORAGE_VERSION
    );
    assert_eq!(decoded.last_active_domain, "minecraft");
}

#[tokio::test]
async fn permission_subject_snapshot_removes_noncanonical_uuid_key() {
    let root = temp_storage_root("permission-uuid-key");
    let storage = match FilePlayerDataStorage::new(root.clone()).await {
        Ok(storage) => storage,
        Err(error) => panic!("test storage should initialize: {error}"),
    };
    let target_uuid = Uuid::from_u128(42);
    let control_uuid = Uuid::from_u128(84);
    let mut seed = PermissionSubjectIndex::new();
    seed.set(
        target_uuid,
        PermissionSubjectState::new(vec!["op".to_owned()], PermissionSet::new()),
    );
    seed.set(
        control_uuid,
        PermissionSubjectState::new(vec!["builder".to_owned()], PermissionSet::new()),
    );
    let file = PlayerPermissionsFile::from_subject_index(&seed);
    let canonical = target_uuid.to_string();
    let noncanonical = target_uuid.simple().to_string();
    let contents = serialize_player_permissions_file(&file)
        .expect("permission subjects should serialize")
        .replace(&canonical, &noncanonical);
    fs::write(storage.player_permissions_file(), contents)
        .await
        .expect("noncanonical permission UUID should be seeded");

    let mut subjects = storage
        .load_permission_subjects()
        .await
        .expect("valid UUID spellings should load");
    assert_eq!(subjects.len(), 2);
    let removed = subjects
        .remove(target_uuid)
        .expect("target should be indexed by UUID");
    assert_eq!(removed.groups(), ["op"]);
    storage
        .save_permission_subjects(&subjects)
        .await
        .expect("updated UUID index should persist");

    let reloaded = storage
        .load_permission_subjects()
        .await
        .expect("updated permission subjects should load");
    assert!(reloaded.get(target_uuid).is_none());
    assert_eq!(
        reloaded
            .get(control_uuid)
            .map(PermissionSubjectState::groups),
        Some(["builder".to_owned()].as_slice())
    );
    let persisted = fs::read_to_string(storage.player_permissions_file())
        .await
        .expect("updated permissions should be readable");
    assert!(!persisted.contains(&canonical));
    assert!(!persisted.contains(&noncanonical));

    fs::remove_dir_all(root)
        .await
        .expect("temporary storage should be removable");
}

#[test]
fn player_file_roundtrip_preserves_root_vehicle() {
    let mut file = sample_player_file(PLAYER_DATA_VERSION);
    file.root_vehicle = Some(RootVehicleFile {
        attach: [3; 16],
        entity: sample_persistent_entity(),
    });

    let encoded = encode_player_file(&file).expect("player file should encode");
    let decoded = decode_player_file(&encoded).expect("player file should decode");
    let persistent = decoded
        .into_persistent()
        .expect("player file should convert");

    let Some(root_vehicle) = persistent.root_vehicle else {
        panic!("root vehicle should survive roundtrip");
    };
    assert_eq!(root_vehicle.attach, [3; 16]);
    assert_eq!(root_vehicle.entity.uuid, [7; 16]);
    assert_eq!(
        root_vehicle.entity.entity_type,
        Identifier::vanilla_static("minecart")
    );
    assert_eq!(
        root_vehicle.entity.pos.map(f64::to_bits),
        [4.0_f64.to_bits(), 65.0_f64.to_bits(), 6.0_f64.to_bits()]
    );
}

#[test]
fn player_file_roundtrip_preserves_respawn_config() {
    let mut file = sample_player_file(PLAYER_DATA_VERSION);
    file.respawn_config = Some(RespawnConfigFile {
        dimension: "minecraft:overworld".to_owned(),
        pos: [10, 64, -3],
        yaw: 181.0,
        pitch: -120.0,
        forced: false,
    });
    file.ender_pearls = vec![
        EnderPearlFile {
            world: "minecraft:overworld".to_owned(),
            entity: sample_persistent_entity(),
        },
        EnderPearlFile {
            world: "minecraft:the_nether".to_owned(),
            entity: sample_persistent_entity(),
        },
    ];

    let encoded = encode_player_file(&file).expect("player file should encode");
    let decoded = decode_player_file(&encoded).expect("player file should decode");
    let persistent = decoded
        .into_persistent()
        .expect("player file should convert");

    let Some(respawn_config) = persistent.respawn_config else {
        panic!("respawn config should survive roundtrip");
    };
    assert_eq!(
        respawn_config.respawn_data.dimension(),
        &Identifier::vanilla_static("overworld")
    );
    assert_eq!(respawn_config.respawn_data.pos(), BlockPos::new(10, 64, -3));
    assert_eq!(
        respawn_config.respawn_data.yaw.to_bits(),
        (-179.0_f32).to_bits()
    );
    assert_eq!(
        respawn_config.respawn_data.pitch.to_bits(),
        (-90.0_f32).to_bits()
    );
    assert!(!respawn_config.forced);

    assert_eq!(persistent.ender_pearls.len(), 2);
    assert_eq!(persistent.ender_pearls[0].world, "minecraft:overworld");
    assert_eq!(persistent.ender_pearls[1].world, "minecraft:the_nether");
    assert_eq!(persistent.ender_pearls[0].entity.uuid, [7; 16]);
    assert_eq!(
        persistent.ender_pearls[0].entity.pos.map(f64::to_bits),
        [4.0_f64.to_bits(), 65.0_f64.to_bits(), 6.0_f64.to_bits()]
    );
}

#[test]
fn stale_player_payload_version_is_rejected() {
    let file = sample_player_file(PLAYER_DATA_VERSION - 1);

    let error = file
        .into_persistent()
        .expect_err("stale payload should fail");

    assert_eq!(error.kind(), io::ErrorKind::InvalidData);
}

#[test]
fn player_stats_file_roundtrips() {
    init_vanilla_registry();

    let file = sample_player_stats_file();

    let mut stats = file.into_persistent_stats();

    assert_eq!(stats.len(), 5);

    stats.push(PersistentStat {
        stat: vanilla_stat_types::ITEM_PICKED_UP.get(&vanilla_items::DIAMOND),
        count: 934,
    });
    stats.push(PersistentStat {
        stat: vanilla_stat_types::BLOCK_MINED.get(&vanilla_blocks::EMERALD_BLOCK),
        count: 111,
    });

    let file =
        PlayerStatsFile::from_persistent_stats(&stats).expect("conversion should not have failed");
    assert_eq!(file.stats.len(), 3); // We added a new stat type to serialize

    let block_stats = &file.stats[&Identifier::vanilla_static("mined")];
    let expected = [
        ("minecraft:diamond_block", 5),
        ("minecraft:emerald_block", 111),
        ("minecraft:reinforced_deepslate", 1),
    ];

    for (i, (block, &count)) in block_stats.iter().enumerate() {
        assert_eq!(block.to_string(), expected[i].0);
        assert_eq!(count, expected[i].1);
    }
}

#[test]
fn duplicate_stat_fails_conversion() {
    init_vanilla_registry();

    let stats = vec![
        PersistentStat {
            stat: vanilla_stat_types::ITEM_PICKED_UP.get(&vanilla_items::DIAMOND),
            count: 123,
        },
        PersistentStat {
            stat: vanilla_stat_types::ITEM_PICKED_UP.get(&vanilla_items::DIAMOND),
            count: 456,
        },
    ];

    PlayerStatsFile::from_persistent_stats(&stats)
        .expect_err("conversion should have failed with duplicate stat");
}
