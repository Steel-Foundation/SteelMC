//! Player data storage for global and domain-scoped player state.

mod known_players;
mod permissions;

use std::{
    io::Cursor,
    path::{Path, PathBuf},
    sync::Arc,
};

use rustc_hash::FxHashMap;
use simdnbt::{ToNbtTag, borrow::read_compound as read_borrowed_compound, owned::NbtTag};
use tokio::{
    fs,
    io::{self, AsyncWriteExt},
};
use uuid::Uuid;
use wincode::{SchemaRead, SchemaWrite};

#[cfg(test)]
use self::permissions::set_permission_subject;
use self::{
    known_players::{KnownPlayersFile, decode_known_players_file, encode_known_players_file},
    permissions::{PlayerPermissionsFile, serialize_player_permissions_file},
};
use super::player_data::{
    PLAYER_DATA_VERSION, PersistentAbilities, PersistentEnderPearl, PersistentPlayerData,
    PersistentRootVehicle, PersistentSlot,
};
use crate::chunk_saver::PersistentEntity;
use crate::config::StorageSelection;
use crate::permission::PermissionSubjectIndex;
#[cfg(test)]
use crate::permission::PermissionSubjectState;
use crate::player::Player;
use crate::player::known_players::KnownPlayers;
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
    ender_pearls: Vec<EnderPearlFile>,
}

#[derive(SchemaWrite, SchemaRead)]
struct RootVehicleFile {
    attach: [u8; 16],
    entity: PersistentEntity,
}

#[derive(SchemaWrite, SchemaRead)]
struct EnderPearlFile {
    world: String,
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
        if selection.kind != Identifier::from_steel("file") {
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

    /// Loads all persisted player permission snapshots.
    pub async fn load_permission_subjects(&self) -> io::Result<PermissionSubjectIndex> {
        match &self.backend {
            PlayerDataStorageBackend::File(storage) => storage.load_permission_subjects().await,
        }
    }

    /// Loads the rebuildable player identity cache, falling back to empty on failure.
    pub async fn load_known_players(&self) -> io::Result<KnownPlayers> {
        match &self.backend {
            PlayerDataStorageBackend::File(storage) => storage.load_known_players().await,
        }
    }

    /// Persists the identity cache when the caller's snapshot is still current.
    pub async fn save_known_players_if_current(
        &self,
        players: &KnownPlayers,
        is_current: impl FnOnce() -> bool + Send,
    ) -> io::Result<bool> {
        match &self.backend {
            PlayerDataStorageBackend::File(storage) => {
                storage
                    .save_known_players_if_current(players, is_current)
                    .await
            }
        }
    }

    /// Saves server-wide player data.
    pub async fn save_global(&self, uuid: Uuid, data: &GlobalPlayerData) -> io::Result<()> {
        match &self.backend {
            PlayerDataStorageBackend::File(storage) => storage.save_global(uuid, data).await,
        }
    }

    /// Persists the server's complete UUID-keyed permission snapshot.
    pub async fn save_permission_subjects(
        &self,
        subjects: &PermissionSubjectIndex,
    ) -> io::Result<()> {
        match &self.backend {
            PlayerDataStorageBackend::File(storage) => {
                storage.save_permission_subjects(subjects).await
            }
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
        if !Self::recover_missing_atomic_path_locked(&path).await? {
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
        if !Self::recover_missing_atomic_path_locked(&path).await? {
            return Ok(None);
        }
        let bytes = fs::read(&path).await?;
        let file = decode_global_file(&bytes)?;
        Ok(Some(GlobalPlayerData {
            last_active_domain: file.last_active_domain,
        }))
    }

    async fn load_permission_subjects(&self) -> io::Result<PermissionSubjectIndex> {
        self.load_player_permissions_file()
            .await?
            .into_subject_index()
    }

    async fn load_known_players(&self) -> io::Result<KnownPlayers> {
        let path = self.known_players_file();
        let lock = self.file_lock(&path);
        let _guard = lock.lock().await;
        match Self::read_known_players_file_locked(&path).await {
            Ok(players) => Ok(players),
            Err(error) => {
                log::warn!(
                    "Failed to load known player cache from {}: {error}. Starting with an empty cache",
                    path.display()
                );
                Ok(KnownPlayers::new())
            }
        }
    }

    async fn read_known_players_file_locked(path: &Path) -> io::Result<KnownPlayers> {
        if !Self::recover_missing_atomic_path_locked(path).await? {
            return Ok(KnownPlayers::new());
        }
        let bytes = fs::read(path).await?;
        decode_known_players_file(&bytes)?.into_known_players()
    }

    async fn save_known_players_if_current(
        &self,
        players: &KnownPlayers,
        is_current: impl FnOnce() -> bool + Send,
    ) -> io::Result<bool> {
        let path = self.known_players_file();
        let lock = self.file_lock(&path);
        let _guard = lock.lock().await;
        if !is_current() {
            return Ok(false);
        }
        let bytes = encode_known_players_file(&KnownPlayersFile::from_known_players(players))?;
        Self::write_atomic_path_locked(&path, bytes).await?;
        Ok(true)
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

    async fn save_permission_subjects(&self, subjects: &PermissionSubjectIndex) -> io::Result<()> {
        let path = self.player_permissions_file();
        let lock = self.file_lock(&path);
        let _guard = lock.lock().await;
        let file = PlayerPermissionsFile::from_subject_index(subjects);
        self.write_player_permissions_file_locked(&path, &file)
            .await
    }

    async fn load_player_permissions_file(&self) -> io::Result<PlayerPermissionsFile> {
        let path = self.player_permissions_file();
        let lock = self.file_lock(&path);
        let _guard = lock.lock().await;
        self.read_player_permissions_file_locked(&path).await
    }

    async fn read_player_permissions_file_locked(
        &self,
        path: &Path,
    ) -> io::Result<PlayerPermissionsFile> {
        if !Self::recover_missing_atomic_path_locked(path).await? {
            return Ok(PlayerPermissionsFile::default());
        }
        let contents = fs::read_to_string(path).await?;
        let file = toml::from_str::<PlayerPermissionsFile>(&contents).map_err(|error| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!(
                    "invalid player permissions TOML in {}: {error}",
                    path.display()
                ),
            )
        })?;
        file.validate()?;
        Ok(file)
    }

    async fn write_player_permissions_file_locked(
        &self,
        path: &Path,
        file: &PlayerPermissionsFile,
    ) -> io::Result<()> {
        let contents = serialize_player_permissions_file(file).map_err(|error| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!("failed to serialize player permissions TOML: {error}"),
            )
        })?;
        Self::write_atomic_path_locked(path, contents.into_bytes()).await
    }

    fn global_dir(&self) -> PathBuf {
        self.save_root.join("global")
    }

    fn global_players_dir(&self) -> PathBuf {
        self.global_dir().join("players")
    }

    fn player_permissions_file(&self) -> PathBuf {
        self.global_dir().join("player_permissions.toml")
    }

    fn known_players_file(&self) -> PathBuf {
        self.global_dir().join("known_players.dat")
    }

    fn domain_players_dir(&self, domain: &str) -> PathBuf {
        self.save_root.join(domain).join("players")
    }

    fn player_file(players_dir: &Path, uuid: Uuid) -> PathBuf {
        players_dir.join(format!("{uuid}.dat"))
    }

    fn file_lock(&self, path: &Path) -> Arc<AsyncMutex<()>> {
        let mut locks = self.file_locks.lock();
        locks
            .entry(path.to_path_buf())
            .or_insert_with(|| Arc::new(AsyncMutex::new(())))
            .clone()
    }

    async fn write_atomic(&self, players_dir: &Path, uuid: Uuid, bytes: Vec<u8>) -> io::Result<()> {
        let final_path = Self::player_file(players_dir, uuid);
        let lock = self.file_lock(&final_path);
        let _guard = lock.lock().await;
        Self::write_atomic_path_locked(&final_path, bytes).await
    }

    async fn write_atomic_path_locked(final_path: &Path, bytes: Vec<u8>) -> io::Result<()> {
        let Some(parent) = final_path.parent() else {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "atomic write path has no parent",
            ));
        };
        fs::create_dir_all(parent).await?;
        let temp_path = Self::atomic_temp_path(final_path);
        let backup_path = Self::atomic_backup_path(final_path);
        let backup_temp_path = Self::atomic_temp_path(&backup_path);

        Self::write_synced_file(&temp_path, &bytes).await?;
        if fs::try_exists(final_path).await? {
            Self::copy_synced_file(final_path, &backup_temp_path).await?;
            fs::rename(&backup_temp_path, &backup_path).await?;
        }
        fs::rename(&temp_path, final_path).await?;
        if let Err(error) = Self::sync_parent(parent).await {
            tracing::error!(
                %error,
                path = %final_path.display(),
                "Atomic data-file replacement committed, but directory sync failed; crash durability is uncertain"
            );
        }
        Ok(())
    }

    fn atomic_temp_path(path: &Path) -> PathBuf {
        let extension = path.extension().and_then(|value| value.to_str());
        path.with_extension(match extension {
            Some(extension) => format!("{extension}.tmp"),
            None => "tmp".to_owned(),
        })
    }

    fn atomic_backup_path(path: &Path) -> PathBuf {
        let extension = path.extension().and_then(|value| value.to_str());
        path.with_extension(match extension {
            Some(extension) => format!("{extension}_old"),
            None => "old".to_owned(),
        })
    }

    async fn recover_missing_atomic_path_locked(final_path: &Path) -> io::Result<bool> {
        if fs::try_exists(final_path).await? {
            return Ok(true);
        }

        let backup_path = Self::atomic_backup_path(final_path);
        if fs::try_exists(&backup_path).await? {
            fs::rename(&backup_path, final_path).await?;
            let Some(parent) = final_path.parent() else {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "atomic recovery path has no parent",
                ));
            };
            Self::sync_parent(parent).await?;
            let temp_path = Self::atomic_temp_path(final_path);
            if fs::try_exists(&temp_path).await?
                && let Err(error) = fs::remove_file(&temp_path).await
            {
                tracing::warn!(
                    %error,
                    path = %temp_path.display(),
                    "Failed to remove an uncommitted atomic-write temporary file"
                );
            }
            tracing::warn!(
                path = %final_path.display(),
                backup = %backup_path.display(),
                "Recovered a missing data file from its last committed backup"
            );
            return Ok(true);
        }

        let temp_path = Self::atomic_temp_path(final_path);
        if fs::try_exists(&temp_path).await? {
            if let Err(error) = fs::remove_file(&temp_path).await {
                tracing::warn!(
                    %error,
                    path = %temp_path.display(),
                    "Failed to remove an uncommitted atomic-write temporary file"
                );
            }
            tracing::warn!(
                path = %final_path.display(),
                temporary = %temp_path.display(),
                "Discarded an interrupted data-file publication with no committed generation"
            );
        }

        Ok(false)
    }

    async fn write_synced_file(path: &Path, bytes: &[u8]) -> io::Result<()> {
        let mut file = fs::File::create(path).await?;
        file.write_all(bytes).await?;
        file.sync_all().await
    }

    async fn copy_synced_file(source: &Path, destination: &Path) -> io::Result<()> {
        let mut source = fs::File::open(source).await?;
        let mut destination = fs::File::create(destination).await?;
        io::copy(&mut source, &mut destination).await?;
        destination.sync_all().await
    }

    async fn sync_parent(parent: &Path) -> io::Result<()> {
        #[cfg(unix)]
        fs::File::open(parent).await?.sync_all().await?;
        #[cfg(not(unix))]
        let _ = parent;
        Ok(())
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
            ender_pearls: data
                .ender_pearls
                .iter()
                .map(|pearl| EnderPearlFile {
                    world: pearl.world.clone(),
                    entity: pearl.entity.clone(),
                })
                .collect(),
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
            ender_pearls: self
                .ender_pearls
                .into_iter()
                .map(|pearl| PersistentEnderPearl {
                    world: pearl.world,
                    entity: pearl.entity,
                })
                .collect(),
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
    use crate::permission::PermissionSet;
    use crate::player::known_players::KnownPlayer;
    use std::{
        env,
        time::{SystemTime, UNIX_EPOCH},
    };

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

    #[tokio::test]
    async fn atomic_path_replacement_retains_the_last_committed_generation() {
        let root = temp_storage_root("atomic-replacement");
        let path = root.join("state.dat");

        FilePlayerDataStorage::write_atomic_path_locked(&path, b"first".to_vec())
            .await
            .expect("first generation should publish");
        FilePlayerDataStorage::write_atomic_path_locked(&path, b"second".to_vec())
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
        let contents = serialize_player_permissions_file(&file)
            .expect("uncommitted permissions should serialize");
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
            KnownPlayer::with_expiration(
                Uuid::from_u128(value),
                format!("Player{value}"),
                1_234_567,
            )
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
    fn player_file_roundtrip_preserves_ender_pearls() {
        let mut file = sample_player_file(PLAYER_DATA_VERSION);
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
}
