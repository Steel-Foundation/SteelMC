use super::known_players::{
    KnownPlayersFile, decode_known_players_file, encode_known_players_file,
};
use super::permissions::{PlayerPermissionsFile, serialize_player_permissions_file};
use super::stats::{PlayerStatsFile, serialize_player_stats_file};
use super::{
    GLOBAL_PLAYER_DATA_VERSION, GlobalPlayerData, GlobalPlayerDataFile, PlayerDataFile,
    decode_global_file, decode_player_file, encode_global_file, encode_player_file,
};
use crate::permission::PermissionSubjectIndex;
use crate::player::player_data::{PersistentPlayerData, PersistentStat};
use crate::player::{KnownPlayers, Player};
use rustc_hash::FxHashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use steel_utils::locks::{AsyncMutex, SyncMutex};
use tokio::io::AsyncWriteExt;
use tokio::sync::OwnedMutexGuard as OwnedAsyncMutexGuard;
use tokio::{fs, io};
use uuid::Uuid;

pub(crate) struct FilePlayerDataStorage {
    save_root: PathBuf,
    file_locks: SyncMutex<FxHashMap<PathBuf, Arc<AsyncMutex<()>>>>,
}

impl FilePlayerDataStorage {
    pub(crate) async fn new(save_root: PathBuf) -> io::Result<Self> {
        fs::create_dir_all(save_root.join("global").join("players")).await?;
        Ok(Self {
            save_root,
            file_locks: SyncMutex::new(FxHashMap::default()),
        })
    }

    pub(crate) async fn save_domain(&self, domain: &str, player: &Player) -> io::Result<()> {
        let uuid = player.gameprofile.id;
        let data = PersistentPlayerData::from_player(player);
        self.save_domain_data(domain, uuid, &data).await
    }

    pub(crate) async fn save_domain_data(
        &self,
        domain: &str,
        uuid: Uuid,
        data: &PersistentPlayerData,
    ) -> io::Result<()> {
        self.save_domain_player_data(domain, uuid, data).await?;
        self.save_domain_player_stats(domain, uuid, data).await?;
        Ok(())
    }

    pub(crate) async fn save_domain_player_data(
        &self,
        domain: &str,
        uuid: Uuid,
        data: &PersistentPlayerData,
    ) -> io::Result<()> {
        let file = PlayerDataFile::from_persistent(data)?;
        let bytes = encode_player_file(&file)?;
        self.write_atomic_player_data(&self.domain_players_dir(domain), uuid, &bytes)
            .await?;
        log::debug!("Saved player data for {uuid} in domain {domain}");

        Ok(())
    }

    pub(crate) async fn save_domain_player_stats(
        &self,
        domain: &str,
        uuid: Uuid,
        data: &PersistentPlayerData,
    ) -> io::Result<()> {
        let player_stats_file = PlayerStatsFile::from_persistent_stats(&data.stats)?;
        let toml_string = serialize_player_stats_file(&player_stats_file);
        let final_path = Self::player_stats_file(&self.domain_players_dir(domain), uuid);
        let _guard = self.file_lock(&final_path).await;
        Self::write_atomic_path_locked(&final_path, toml_string.as_bytes()).await?;
        log::debug!("Saved player stats for {uuid} in domain {domain}");

        Ok(())
    }

    pub(crate) async fn load_domain(
        &self,
        domain: &str,
        uuid: Uuid,
    ) -> io::Result<Option<PersistentPlayerData>> {
        let Some(mut data) = self.load_domain_player_data(domain, uuid).await? else {
            return Ok(None);
        };

        data.stats = match self.load_domain_player_stats(domain, uuid).await {
            Ok(Some(stats)) => stats,
            Ok(None) => {
                log::debug!("Using empty stats for {uuid} in domain {domain}");
                Vec::new()
            }
            Err(e) => {
                log::error!("Could not load stats for {uuid} in domain {domain}: {e}");
                Vec::new()
            }
        };

        Ok(Some(data))
    }

    pub(crate) async fn load_domain_player_data(
        &self,
        domain: &str,
        uuid: Uuid,
    ) -> io::Result<Option<PersistentPlayerData>> {
        let domain_dir = self.domain_players_dir(domain);
        let path = Self::player_data_file(&domain_dir, uuid);
        let _guard = self.file_lock(&path).await;
        let data = Self::load_recovering_from_backup_locked(&path, |bytes| {
            decode_player_file(bytes)?.into_persistent()
        })
        .await?;
        if data.is_some() {
            log::debug!("Loaded player data for {uuid} in domain {domain}");
        }

        Ok(data)
    }

    pub(crate) async fn load_domain_player_stats(
        &self,
        domain: &str,
        uuid: Uuid,
    ) -> io::Result<Option<Vec<PersistentStat>>> {
        let domain_dir = self.domain_players_dir(domain);
        let path = Self::player_stats_file(&domain_dir, uuid);
        let _guard = self.file_lock(&path).await;
        if !Self::recover_missing_atomic_path_locked(&path).await? {
            return Ok(None);
        }

        let string = fs::read_to_string(&path).await?;
        let stats_file: PlayerStatsFile = toml::from_str(&string).map_err(|e| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!("Invalid player stats for {uuid} in domain {domain}: {e}"),
            )
        })?;
        let persistent_stats = stats_file.into_persistent_stats();
        log::debug!("Loaded player stats for {uuid} in domain {domain}");

        Ok(Some(persistent_stats))
    }

    pub(crate) async fn load_global(&self, uuid: Uuid) -> io::Result<Option<GlobalPlayerData>> {
        let path = Self::player_data_file(&self.global_players_dir(), uuid);
        let _guard = self.file_lock(&path).await;
        Self::load_recovering_from_backup_locked(&path, |bytes| {
            decode_global_file(bytes).map(|file| GlobalPlayerData {
                last_active_domain: file.last_active_domain,
            })
        })
        .await
    }

    pub(crate) async fn load_permission_subjects(&self) -> io::Result<PermissionSubjectIndex> {
        self.load_player_permissions_file()
            .await?
            .into_subject_index()
    }

    pub(crate) async fn load_known_players(&self) -> io::Result<KnownPlayers> {
        let path = self.known_players_file();
        let _guard = self.file_lock(&path).await;
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

    pub(crate) async fn save_known_players_if_current(
        &self,
        players: &KnownPlayers,
        is_current: impl FnOnce() -> bool + Send,
    ) -> io::Result<bool> {
        let path = self.known_players_file();
        let _guard = self.file_lock(&path).await;
        if !is_current() {
            return Ok(false);
        }
        let bytes = encode_known_players_file(&KnownPlayersFile::from_known_players(players))?;
        Self::write_atomic_path_locked(&path, &bytes).await?;
        Ok(true)
    }

    pub(crate) async fn save_global(&self, uuid: Uuid, data: &GlobalPlayerData) -> io::Result<()> {
        let file = GlobalPlayerDataFile {
            data_version: GLOBAL_PLAYER_DATA_VERSION,
            last_active_domain: data.last_active_domain.clone(),
        };
        let bytes = encode_global_file(&file)?;
        self.write_atomic_player_data(&self.global_players_dir(), uuid, &bytes)
            .await
    }

    pub(crate) async fn save_permission_subjects(
        &self,
        subjects: &PermissionSubjectIndex,
    ) -> io::Result<()> {
        let path = self.player_permissions_file();
        let _guard = self.file_lock(&path).await;
        let file = PlayerPermissionsFile::from_subject_index(subjects);
        self.write_player_permissions_file_locked(&path, &file)
            .await
    }

    pub(crate) async fn load_player_permissions_file(&self) -> io::Result<PlayerPermissionsFile> {
        let path = self.player_permissions_file();
        let _guard = self.file_lock(&path).await;
        self.read_player_permissions_file_locked(&path).await
    }

    pub(crate) async fn read_player_permissions_file_locked(
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

    pub(crate) async fn write_player_permissions_file_locked(
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
        Self::write_atomic_path_locked(path, contents.as_bytes()).await
    }

    pub(crate) fn global_dir(&self) -> PathBuf {
        self.save_root.join("global")
    }

    pub(crate) fn global_players_dir(&self) -> PathBuf {
        self.global_dir().join("players")
    }

    pub(crate) fn player_permissions_file(&self) -> PathBuf {
        self.global_dir().join("player_permissions.toml")
    }

    pub(crate) fn known_players_file(&self) -> PathBuf {
        self.global_dir().join("known_players.dat")
    }

    pub(crate) fn domain_players_dir(&self, domain: &str) -> PathBuf {
        self.save_root.join(domain).join("players")
    }

    pub(crate) fn player_data_file(players_dir: &Path, uuid: Uuid) -> PathBuf {
        players_dir.join(format!("data/{uuid}"))
    }

    pub(crate) fn player_stats_file(players_dir: &Path, uuid: Uuid) -> PathBuf {
        players_dir.join(format!("stats/{uuid}.toml"))
    }

    async fn file_lock(&self, path: &Path) -> OwnedAsyncMutexGuard<()> {
        let mutex = self
            .file_locks
            .lock()
            .entry(path.to_path_buf())
            .or_insert_with(|| Arc::new(AsyncMutex::new(())))
            .clone();

        mutex.lock_owned().await
    }

    async fn write_atomic_player_data(
        &self,
        players_dir: &Path,
        uuid: Uuid,
        bytes: &[u8],
    ) -> io::Result<()> {
        let final_path = Self::player_data_file(players_dir, uuid);
        let _guard = self.file_lock(&final_path).await;
        Self::write_atomic_path_locked(&final_path, bytes).await
    }

    pub(crate) async fn write_atomic_path_locked(
        final_path: &Path,
        bytes: &[u8],
    ) -> io::Result<()> {
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

        Self::write_synced_file(&temp_path, bytes).await?;
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

    pub(crate) fn atomic_temp_path(path: &Path) -> PathBuf {
        let extension = path.extension().and_then(|value| value.to_str());
        path.with_extension(match extension {
            Some(extension) => format!("{extension}.tmp"),
            None => "tmp".to_owned(),
        })
    }

    pub(crate) fn atomic_backup_path(path: &Path) -> PathBuf {
        let extension = path.extension().and_then(|value| value.to_str());
        path.with_extension(match extension {
            Some(extension) => format!("{extension}_old"),
            None => "old".to_owned(),
        })
    }

    /// Where a damaged file is set aside. Timestamped so a later corruption
    /// cannot overwrite the evidence from an earlier one.
    pub(crate) fn quarantine_path(path: &Path, stamp: u64) -> PathBuf {
        let extension = path.extension().and_then(|value| value.to_str());
        path.with_extension(match extension {
            Some(extension) => format!("{extension}_corrupt_{stamp}"),
            None => format!("corrupt_{stamp}"),
        })
    }

    /// Reads and decodes `path`, falling back to the atomic-write backup when
    /// the live file's bytes are damaged.
    ///
    /// The backup is only consulted for [`io::ErrorKind::InvalidData`], which
    /// `decode_file` reserves for damage. An [`io::ErrorKind::Unsupported`]
    /// file is intact but written by another format version, and the backup
    /// carries that same version, so it propagates untouched.
    ///
    /// On a successful recovery the damaged file is quarantined rather than
    /// deleted and the backup is promoted into its place, so the next read
    /// finds the recovered generation. Quarantining happens first: if the
    /// process stops between the two renames, the live path is missing while
    /// the backup is intact, which is the state
    /// `recover_missing_atomic_path_locked` repairs on the next read.
    ///
    /// The recovered generation is one save behind.
    async fn load_recovering_from_backup_locked<T, F>(
        path: &Path,
        decode: F,
    ) -> io::Result<Option<T>>
    where
        F: Fn(&[u8]) -> io::Result<T>,
    {
        if !Self::recover_missing_atomic_path_locked(path).await? {
            return Ok(None);
        }

        let bytes = fs::read(path).await?;
        let damage = match decode(&bytes) {
            Ok(value) => return Ok(Some(value)),
            Err(error) if error.kind() == io::ErrorKind::InvalidData => error,
            Err(error) => return Err(error),
        };

        let backup_path = Self::atomic_backup_path(path);
        if !fs::try_exists(&backup_path).await? {
            tracing::error!(
                path = %path.display(),
                error = %damage,
                "Player data file is corrupt and has no backup to recover from"
            );
            return Err(damage);
        }

        let backup_bytes = fs::read(&backup_path).await?;
        let recovered = match decode(&backup_bytes) {
            Ok(value) => value,
            Err(backup_error) => {
                tracing::error!(
                    path = %path.display(),
                    error = %damage,
                    backup_error = %backup_error,
                    "Player data file and its backup are both unreadable"
                );
                return Err(damage);
            }
        };

        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |elapsed| elapsed.as_secs());
        let quarantine = Self::quarantine_path(path, stamp);
        fs::rename(path, &quarantine).await?;
        fs::rename(&backup_path, path).await?;
        if let Some(parent) = path.parent() {
            Self::sync_parent(parent).await?;
        }

        tracing::warn!(
            path = %path.display(),
            quarantined = %quarantine.display(),
            error = %damage,
            "Recovered a corrupt player data file from its backup; the restored \
             data is one save behind and the damaged file was set aside"
        );

        Ok(Some(recovered))
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
        // Runtime check so the `.await`s stay present on all platforms for clippy;
        // on Windows the branch never runs (directory fsync is unix-only).
        if cfg!(unix) {
            fs::File::open(parent).await?.sync_all().await?;
        }
        Ok(())
    }
}
