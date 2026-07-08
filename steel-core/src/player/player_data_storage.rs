//! Player data storage for global and domain-scoped player state.

use std::{
    io::Cursor,
    path::{Path, PathBuf},
    sync::Arc,
};

use rustc_hash::FxHashMap;
use simdnbt::{ToNbtTag, borrow::read_compound as read_borrowed_compound, owned::NbtTag};
use tokio::{fs, io};
use uuid::Uuid;
use wincode::{SchemaRead, SchemaWrite};

use super::player_data::{
    PLAYER_DATA_VERSION, PersistentAbilities, PersistentPlayerData, PersistentRootVehicle,
    PersistentSlot,
};
use crate::chunk_saver::PersistentEntity;
use crate::config::StorageSelection;
use crate::player::Player;
use steel_registry::item_stack::ItemStack;
use steel_utils::Identifier;
use steel_utils::locks::{AsyncMutex, SyncMutex};

const PLAYER_MAGIC: [u8; 4] = *b"STLP";
const GLOBAL_MAGIC: [u8; 4] = *b"STLG";
const PLAYER_STORAGE_VERSION: u16 = 7;
const GLOBAL_STORAGE_VERSION: u16 = 1;
const GLOBAL_PLAYER_DATA_VERSION: i32 = 1;

/// Server-wide player data.
#[derive(Debug, Clone)]
pub struct GlobalPlayerData {
    /// Last active domain for reconnects.
    pub last_active_domain: String,
}

/// Manages player data persistence.
pub struct PlayerDataStorage {
    backend: PlayerDataStorageBackend,
}

enum PlayerDataStorageBackend {
    File(FilePlayerDataStorage),
}

struct FilePlayerDataStorage {
    save_root: PathBuf,
    file_locks: SyncMutex<FxHashMap<PathBuf, Arc<AsyncMutex<()>>>>,
}

#[derive(SchemaWrite, SchemaRead)]
struct PlayerDataFile {
    data_version: i32,
    pos: [f64; 3],
    motion: [f64; 3],
    rotation: [f32; 2],
    on_ground: bool,
    fall_flying: bool,
    remaining_fire_ticks: i32,
    ticks_frozen: i32,
    is_in_powder_snow: bool,
    was_in_powder_snow: bool,
    has_visual_fire: bool,
    health: f32,
    game_mode: i32,
    prev_game_mode: Option<i32>,
    abilities: AbilitiesFile,
    inventory: Vec<SlotFile>,
    selected_slot: i32,
    world: String,
    food_level: i32,
    food_saturation_level: f32,
    food_exhaustion_level: f32,
    food_tick_timer: i32,
    experience_level: i32,
    experience_progress: f32,
    experience_total: i32,
    score: i32,
    seen_credits: bool,
    root_vehicle: Option<RootVehicleFile>,
}

#[derive(SchemaWrite, SchemaRead)]
struct RootVehicleFile {
    attach: [u8; 16],
    entity: PersistentEntity,
}

#[derive(SchemaWrite, SchemaRead)]
struct AbilitiesFile {
    invulnerable: bool,
    flying: bool,
    may_fly: bool,
    instabuild: bool,
    may_build: bool,
    flying_speed: f32,
    walking_speed: f32,
}

#[derive(SchemaWrite, SchemaRead)]
struct SlotFile {
    slot: i8,
    item_nbt: Vec<u8>,
}

#[derive(SchemaWrite, SchemaRead)]
struct GlobalPlayerDataFile {
    data_version: i32,
    last_active_domain: String,
}

impl PlayerDataStorage {
    /// Creates player data storage from config.
    pub async fn new(save_root: PathBuf, selection: StorageSelection) -> io::Result<Self> {
        if selection.kind != Identifier::new("steel", "file") {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("unknown player storage {}", selection.kind),
            ));
        }
        let backend = PlayerDataStorageBackend::File(FilePlayerDataStorage::new(save_root).await?);
        Ok(Self { backend })
    }

    /// Saves a player's current domain data and global last-active-domain.
    pub async fn save(&self, player: &Player) -> io::Result<()> {
        let domain = player.get_world().domain().to_owned();
        self.save_domain(&domain, player).await?;
        self.save_global(
            player.gameprofile.id,
            &GlobalPlayerData {
                last_active_domain: domain,
            },
        )
        .await
    }

    /// Saves a player's data for a specific domain.
    pub async fn save_domain(&self, domain: &str, player: &Player) -> io::Result<()> {
        match &self.backend {
            PlayerDataStorageBackend::File(storage) => storage.save_domain(domain, player).await,
        }
    }

    /// Saves an already captured player data snapshot for a specific domain.
    pub async fn save_domain_data(
        &self,
        domain: &str,
        uuid: Uuid,
        data: &PersistentPlayerData,
    ) -> io::Result<()> {
        match &self.backend {
            PlayerDataStorageBackend::File(storage) => {
                storage.save_domain_data(domain, uuid, data).await
            }
        }
    }

    /// Loads a player's data for a specific domain.
    pub async fn load_domain(
        &self,
        domain: &str,
        uuid: Uuid,
    ) -> io::Result<Option<PersistentPlayerData>> {
        match &self.backend {
            PlayerDataStorageBackend::File(storage) => storage.load_domain(domain, uuid).await,
        }
    }

    /// Loads server-wide player data.
    pub async fn load_global(&self, uuid: Uuid) -> io::Result<Option<GlobalPlayerData>> {
        match &self.backend {
            PlayerDataStorageBackend::File(storage) => storage.load_global(uuid).await,
        }
    }

    /// Saves server-wide player data.
    pub async fn save_global(&self, uuid: Uuid, data: &GlobalPlayerData) -> io::Result<()> {
        match &self.backend {
            PlayerDataStorageBackend::File(storage) => storage.save_global(uuid, data).await,
        }
    }

    /// Saves multiple players' data.
    pub async fn save_all(&self, players: &[Arc<Player>]) -> io::Result<usize> {
        let mut saved = 0;
        for player in players {
            match self.save(player).await {
                Ok(()) => saved += 1,
                Err(e) => {
                    log::error!("Failed to save player {}: {e}", player.gameprofile.id);
                }
            }
        }
        Ok(saved)
    }
}

impl FilePlayerDataStorage {
    async fn new(save_root: PathBuf) -> io::Result<Self> {
        fs::create_dir_all(save_root.join("global").join("players")).await?;
        Ok(Self {
            save_root,
            file_locks: SyncMutex::new(FxHashMap::default()),
        })
    }

    async fn save_domain(&self, domain: &str, player: &Player) -> io::Result<()> {
        let uuid = player.gameprofile.id;
        let data = PersistentPlayerData::from_player(player);
        self.save_domain_data(domain, uuid, &data).await
    }

    async fn save_domain_data(
        &self,
        domain: &str,
        uuid: Uuid,
        data: &PersistentPlayerData,
    ) -> io::Result<()> {
        let file = PlayerDataFile::from_persistent(data)?;
        let bytes = encode_player_file(&file)?;
        self.write_atomic(&self.domain_players_dir(domain), uuid, bytes)
            .await?;
        log::debug!("Saved player data for {uuid} in domain {domain}");
        Ok(())
    }

    async fn load_domain(
        &self,
        domain: &str,
        uuid: Uuid,
    ) -> io::Result<Option<PersistentPlayerData>> {
        let path = Self::player_file(&self.domain_players_dir(domain), uuid);
        let lock = self.file_lock(&path);
        let _guard = lock.lock().await;
        if !path.exists() {
            return Ok(None);
        }
        let bytes = fs::read(&path).await?;
        let file = decode_player_file(&bytes)?;
        let data = file.into_persistent()?;
        log::debug!("Loaded player data for {uuid} in domain {domain}");
        Ok(Some(data))
    }

    async fn load_global(&self, uuid: Uuid) -> io::Result<Option<GlobalPlayerData>> {
        let path = Self::player_file(&self.global_players_dir(), uuid);
        let lock = self.file_lock(&path);
        let _guard = lock.lock().await;
        if !path.exists() {
            return Ok(None);
        }
        let bytes = fs::read(&path).await?;
        let file = decode_global_file(&bytes)?;
        Ok(Some(GlobalPlayerData {
            last_active_domain: file.last_active_domain,
        }))
    }

    async fn save_global(&self, uuid: Uuid, data: &GlobalPlayerData) -> io::Result<()> {
        let file = GlobalPlayerDataFile {
            data_version: GLOBAL_PLAYER_DATA_VERSION,
            last_active_domain: data.last_active_domain.clone(),
        };
        let bytes = encode_global_file(&file)?;
        self.write_atomic(&self.global_players_dir(), uuid, bytes)
            .await
    }

    fn global_players_dir(&self) -> PathBuf {
        self.save_root.join("global").join("players")
    }

    fn domain_players_dir(&self, domain: &str) -> PathBuf {
        self.save_root.join(domain).join("players")
    }

    fn player_file(players_dir: &Path, uuid: Uuid) -> PathBuf {
        players_dir.join(format!("{uuid}.dat"))
    }

    fn temp_file(players_dir: &Path, uuid: Uuid) -> PathBuf {
        players_dir.join(format!("{uuid}.dat.tmp"))
    }

    fn backup_file(players_dir: &Path, uuid: Uuid) -> PathBuf {
        players_dir.join(format!("{uuid}.dat_old"))
    }

    fn file_lock(&self, path: &Path) -> Arc<AsyncMutex<()>> {
        let mut locks = self.file_locks.lock();
        locks
            .entry(path.to_path_buf())
            .or_insert_with(|| Arc::new(AsyncMutex::new(())))
            .clone()
    }

    async fn write_atomic(&self, players_dir: &Path, uuid: Uuid, bytes: Vec<u8>) -> io::Result<()> {
        fs::create_dir_all(players_dir).await?;
        let temp_path = Self::temp_file(players_dir, uuid);
        let final_path = Self::player_file(players_dir, uuid);
        let backup_path = Self::backup_file(players_dir, uuid);
        let lock = self.file_lock(&final_path);
        let _guard = lock.lock().await;

        fs::write(&temp_path, bytes).await?;
        if final_path.exists() {
            if backup_path.exists() {
                let _ = fs::remove_file(&backup_path).await;
            }
            fs::rename(&final_path, &backup_path).await?;
        }
        fs::rename(&temp_path, &final_path).await
    }
}

impl PlayerDataFile {
    fn from_persistent(data: &PersistentPlayerData) -> io::Result<Self> {
        let mut inventory = Vec::with_capacity(data.inventory.len());
        for slot in &data.inventory {
            inventory.push(SlotFile {
                slot: slot.slot,
                item_nbt: item_to_nbt_bytes(&slot.item)?,
            });
        }

        Ok(Self {
            data_version: data.data_version,
            pos: data.pos,
            motion: data.motion,
            rotation: data.rotation,
            on_ground: data.on_ground,
            fall_flying: data.fall_flying,
            remaining_fire_ticks: data.remaining_fire_ticks,
            ticks_frozen: data.ticks_frozen,
            is_in_powder_snow: data.is_in_powder_snow,
            was_in_powder_snow: data.was_in_powder_snow,
            has_visual_fire: data.has_visual_fire,
            health: data.health,
            game_mode: data.game_mode,
            prev_game_mode: data.prev_game_mode,
            abilities: AbilitiesFile {
                invulnerable: data.abilities.invulnerable,
                flying: data.abilities.flying,
                may_fly: data.abilities.may_fly,
                instabuild: data.abilities.instabuild,
                may_build: data.abilities.may_build,
                flying_speed: data.abilities.flying_speed,
                walking_speed: data.abilities.walking_speed,
            },
            inventory,
            selected_slot: data.selected_slot,
            world: data.world.clone(),
            food_level: data.food_level,
            food_saturation_level: data.food_saturation_level,
            food_exhaustion_level: data.food_exhaustion_level,
            food_tick_timer: data.food_tick_timer,
            experience_level: data.experience_level,
            experience_progress: data.experience_progress,
            experience_total: data.experience_total,
            score: data.score,
            seen_credits: data.seen_credits,
            root_vehicle: data
                .root_vehicle
                .clone()
                .map(|root_vehicle| RootVehicleFile {
                    attach: root_vehicle.attach,
                    entity: root_vehicle.entity,
                }),
        })
    }

    fn into_persistent(self) -> io::Result<PersistentPlayerData> {
        if self.data_version != PLAYER_DATA_VERSION {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!(
                    "unsupported player data payload version {}",
                    self.data_version
                ),
            ));
        }

        let mut inventory = Vec::with_capacity(self.inventory.len());
        for slot in self.inventory {
            inventory.push(PersistentSlot {
                slot: slot.slot,
                item: item_from_nbt_bytes(&slot.item_nbt)?,
            });
        }

        Ok(PersistentPlayerData {
            pos: self.pos,
            motion: self.motion,
            rotation: self.rotation,
            on_ground: self.on_ground,
            fall_flying: self.fall_flying,
            remaining_fire_ticks: self.remaining_fire_ticks,
            ticks_frozen: self.ticks_frozen,
            is_in_powder_snow: self.is_in_powder_snow,
            was_in_powder_snow: self.was_in_powder_snow,
            has_visual_fire: self.has_visual_fire,
            health: self.health,
            game_mode: self.game_mode,
            prev_game_mode: self.prev_game_mode,
            abilities: PersistentAbilities {
                invulnerable: self.abilities.invulnerable,
                flying: self.abilities.flying,
                may_fly: self.abilities.may_fly,
                instabuild: self.abilities.instabuild,
                may_build: self.abilities.may_build,
                flying_speed: self.abilities.flying_speed,
                walking_speed: self.abilities.walking_speed,
            },
            inventory,
            selected_slot: self.selected_slot,
            world: self.world,
            food_level: self.food_level,
            food_saturation_level: self.food_saturation_level,
            food_exhaustion_level: self.food_exhaustion_level,
            food_tick_timer: self.food_tick_timer,
            data_version: self.data_version,
            experience_level: self.experience_level,
            experience_progress: self.experience_progress,
            experience_total: self.experience_total,
            score: self.score,
            seen_credits: self.seen_credits,
            root_vehicle: self.root_vehicle.map(|root_vehicle| PersistentRootVehicle {
                attach: root_vehicle.attach,
                entity: root_vehicle.entity,
            }),
        })
    }
}

fn item_to_nbt_bytes(item: &ItemStack) -> io::Result<Vec<u8>> {
    let NbtTag::Compound(compound) = item.clone().to_nbt_tag() else {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "item stack did not serialize to a compound",
        ));
    };
    let mut bytes = Vec::new();
    compound.write(&mut bytes);
    Ok(bytes)
}

fn item_from_nbt_bytes(bytes: &[u8]) -> io::Result<ItemStack> {
    let nbt = read_borrowed_compound(&mut Cursor::new(bytes)).map_err(|e| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("failed to parse item NBT: {e}"),
        )
    })?;
    let compound = simdnbt::borrow::NbtCompound::from(&nbt);
    ItemStack::from_borrowed_compound(&compound)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "invalid item stack data"))
}

fn encode_player_file(file: &PlayerDataFile) -> io::Result<Vec<u8>> {
    encode_file(
        PLAYER_MAGIC,
        PLAYER_STORAGE_VERSION,
        wincode::serialize(file),
    )
}

fn decode_player_file(bytes: &[u8]) -> io::Result<PlayerDataFile> {
    let payload = decode_file(PLAYER_MAGIC, PLAYER_STORAGE_VERSION, bytes)?;
    wincode::deserialize(&payload)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e.to_string()))
}

fn encode_global_file(file: &GlobalPlayerDataFile) -> io::Result<Vec<u8>> {
    encode_file(
        GLOBAL_MAGIC,
        GLOBAL_STORAGE_VERSION,
        wincode::serialize(file),
    )
}

fn decode_global_file(bytes: &[u8]) -> io::Result<GlobalPlayerDataFile> {
    let payload = decode_file(GLOBAL_MAGIC, GLOBAL_STORAGE_VERSION, bytes)?;
    wincode::deserialize(&payload)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e.to_string()))
}

fn encode_file(
    magic: [u8; 4],
    version: u16,
    serialized: wincode::WriteResult<Vec<u8>>,
) -> io::Result<Vec<u8>> {
    let payload =
        serialized.map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e.to_string()))?;
    let compressed = zstd::encode_all(&payload[..], 3)?;
    let mut bytes = Vec::with_capacity(6 + compressed.len());
    bytes.extend_from_slice(&magic);
    bytes.extend_from_slice(&version.to_le_bytes());
    bytes.extend_from_slice(&compressed);
    Ok(bytes)
}

fn decode_file(
    expected_magic: [u8; 4],
    expected_version: u16,
    bytes: &[u8],
) -> io::Result<Vec<u8>> {
    if bytes.len() < 6 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "player data file is too short",
        ));
    }
    if bytes[0..4] != expected_magic {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "invalid player data magic",
        ));
    }
    let version = u16::from_le_bytes([bytes[4], bytes[5]]);
    if version != expected_version {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("unsupported player data storage version {version}"),
        ));
    }
    zstd::decode_all(&bytes[6..])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entity::DEFAULT_MAX_AIR_SUPPLY;

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
    fn stale_player_payload_version_is_rejected() {
        let file = sample_player_file(PLAYER_DATA_VERSION - 1);

        let error = file
            .into_persistent()
            .expect_err("stale payload should fail");

        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
    }
}
