//! Per-world saved data storage.
//!
//! Vanilla stores world-level saved data under each dimension's `data/`
//! directory. Steel uses the same per-world saved-data boundary for both its
//! human-readable TOML data and versioned binary data.

use std::{
    fmt::Display,
    fs as sync_fs, io,
    path::{Path, PathBuf},
};

use serde::{Serialize, de::DeserializeOwned};
use tokio::fs;
use wincode::{SchemaRead, SchemaWrite, config::DefaultConfig};

/// Built-in saved data entry names.
pub mod names {
    use super::{SavedDataName, WincodeSavedDataName};

    /// Vanilla `TicketStorage.TYPE`, persisted as `data/chunk_tickets.toml`.
    pub const CHUNK_TICKETS: SavedDataName = SavedDataName::trusted("chunk_tickets");
    /// Cached concentric-ring positions, persisted as `data/structure_rings.bin`.
    pub const STRUCTURE_RINGS: WincodeSavedDataName =
        WincodeSavedDataName::trusted("structure_rings", *b"STLR", 2);
    /// Domain command scoreboard, persisted through the domain default world.
    pub const SCOREBOARD: SavedDataName = SavedDataName::trusted("scoreboard");
    /// Domain command storage, persisted through the domain default world.
    pub const COMMAND_STORAGE: SavedDataName = SavedDataName::trusted("command_storage");
}

/// Name of a per-world saved data entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SavedDataName(&'static str);

impl SavedDataName {
    /// Creates a saved-data name from a static trusted identifier.
    #[must_use]
    pub(crate) const fn trusted(name: &'static str) -> Self {
        Self(name)
    }

    /// Creates a saved-data name after validating that it cannot escape `data/`.
    pub fn try_new(name: &'static str) -> Result<Self, String> {
        if is_valid_saved_data_name(name) {
            Ok(Self(name))
        } else {
            Err(format!("invalid saved data name {name}"))
        }
    }

    fn file_name(self) -> String {
        format!("{}.toml", self.0)
    }
}

/// Name and format header of a wincode-encoded per-world saved data entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WincodeSavedDataName {
    name: &'static str,
    magic: [u8; 4],
    version: u16,
}

impl WincodeSavedDataName {
    /// Creates a binary saved-data name from trusted format metadata.
    #[must_use]
    pub(crate) const fn trusted(name: &'static str, magic: [u8; 4], version: u16) -> Self {
        Self {
            name,
            magic,
            version,
        }
    }

    /// Creates a binary saved-data name after validating that it cannot escape `data/`.
    pub fn try_new(name: &'static str, magic: [u8; 4], version: u16) -> Result<Self, String> {
        if is_valid_saved_data_name(name) {
            Ok(Self {
                name,
                magic,
                version,
            })
        } else {
            Err(format!("invalid saved data name {name}"))
        }
    }

    fn file_name(self) -> String {
        format!("{}.bin", self.name)
    }
}

fn is_valid_saved_data_name(name: &str) -> bool {
    !name.is_empty()
        && !name.contains('/')
        && !name.contains('\\')
        && crate::Identifier::validate_path(name)
}

/// Typed saved-data storage for a loaded world.
#[derive(Debug, Clone)]
pub struct SavedDataManager {
    data_dir: Option<PathBuf>,
}

impl SavedDataManager {
    /// Creates saved-data storage rooted at `world_dir/data`.
    ///
    /// `None` means the world is ephemeral, matching Steel's RAM-only storage.
    #[must_use]
    pub fn new(world_dir: Option<&Path>) -> Self {
        Self {
            data_dir: world_dir.map(|path| path.join("data")),
        }
    }

    /// Loads a versioned wincode value, or returns `None` when it is absent or
    /// this world has no persistent storage.
    pub fn sync_load_wincode<T>(&self, name: WincodeSavedDataName) -> io::Result<Option<T>>
    where
        for<'de> T: SchemaRead<'de, DefaultConfig, Dst = T>,
    {
        let Some(path) = self.wincode_path_for(name) else {
            return Ok(None);
        };
        if !path.exists() {
            return Ok(None);
        }

        let bytes = sync_fs::read(&path)?;
        let Some((magic, remainder)) = bytes.split_first_chunk::<4>() else {
            return Err(invalid_binary_data(&path, "missing magic header"));
        };
        if magic != &name.magic {
            return Err(invalid_binary_data(&path, "unexpected magic header"));
        }
        let Some((version, payload)) = remainder.split_first_chunk::<2>() else {
            return Err(invalid_binary_data(&path, "missing format version"));
        };
        if u16::from_le_bytes(*version) != name.version {
            return Err(invalid_binary_data(
                &path,
                format!(
                    "unsupported format version {}",
                    u16::from_le_bytes(*version)
                ),
            ));
        }

        wincode::deserialize_exact(payload)
            .map(Some)
            .map_err(|error| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("Invalid binary saved data {}: {error}", path.display()),
                )
            })
    }

    /// Loads saved data, or returns `T::default()` when the data file is absent
    /// or this world has no persistent storage.
    pub async fn load_or_default<T>(&self, name: SavedDataName) -> io::Result<T>
    where
        T: DeserializeOwned + Default,
    {
        let Some(path) = self.path_for(name) else {
            return Ok(T::default());
        };
        if !path.exists() {
            return Ok(T::default());
        }

        let content = fs::read_to_string(&path).await?;
        toml::from_str(&content).map_err(|error| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!("Invalid saved data {}: {error}", path.display()),
            )
        })
    }

    /// Saves a versioned wincode value.
    pub fn sync_save_wincode<T>(&self, name: WincodeSavedDataName, data: &T) -> io::Result<()>
    where
        T: SchemaWrite<DefaultConfig, Src = T>,
    {
        let Some(path) = self.wincode_path_for(name) else {
            return Ok(());
        };
        if let Some(parent) = path.parent() {
            sync_fs::create_dir_all(parent)?;
        }

        let payload = wincode::serialize(data)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error.to_string()))?;
        let mut bytes = Vec::with_capacity(6 + payload.len());
        bytes.extend_from_slice(&name.magic);
        bytes.extend_from_slice(&name.version.to_le_bytes());
        bytes.extend_from_slice(&payload);
        sync_fs::write(path, bytes)
    }

    /// Saves a typed saved-data value.
    pub async fn save<T>(&self, name: SavedDataName, data: &T) -> io::Result<()>
    where
        T: Serialize,
    {
        let Some(path) = self.path_for(name) else {
            return Ok(());
        };
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).await?;
        }

        let content = toml::to_string_pretty(data)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
        fs::write(path, content).await
    }

    fn path_for(&self, name: SavedDataName) -> Option<PathBuf> {
        self.data_dir
            .as_ref()
            .map(|data_dir| data_dir.join(name.file_name()))
    }

    fn wincode_path_for(&self, name: WincodeSavedDataName) -> Option<PathBuf> {
        self.data_dir
            .as_ref()
            .map(|data_dir| data_dir.join(name.file_name()))
    }
}

fn invalid_binary_data(path: &Path, message: impl Display) -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidData,
        format!("Invalid binary saved data {}: {message}", path.display()),
    )
}

#[cfg(test)]
mod tests {
    use std::{
        env::temp_dir,
        io::ErrorKind,
        path::PathBuf,
        time::{SystemTime, UNIX_EPOCH},
    };

    use serde::{Deserialize, Serialize};

    use wincode::{SchemaRead, SchemaWrite};

    use super::{SavedDataManager, SavedDataName, WincodeSavedDataName, sync_fs};

    const TEST_DATA: SavedDataName = SavedDataName::trusted("test_data");
    const TEST_BINARY_DATA: WincodeSavedDataName =
        WincodeSavedDataName::trusted("test_binary_data", *b"TEST", 3);

    #[derive(Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
    struct TestData {
        value: i32,
    }

    #[derive(Debug, PartialEq, Eq, SchemaWrite, SchemaRead)]
    struct TestBinaryData {
        value: i32,
    }

    #[test]
    fn saved_data_name_rejects_paths() {
        assert!(SavedDataName::try_new("valid_name").is_ok());
        assert!(SavedDataName::try_new("../outside").is_err());
        assert!(SavedDataName::try_new("nested/name").is_err());
        assert!(SavedDataName::try_new("nested\\name").is_err());
        assert!(SavedDataName::try_new("").is_err());
        assert!(WincodeSavedDataName::try_new("valid_name", *b"TEST", 1).is_ok());
        assert!(WincodeSavedDataName::try_new("../outside", *b"TEST", 1).is_err());
    }

    fn temp_world_dir(test_name: &str) -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time should be after Unix epoch")
            .as_nanos();
        temp_dir().join(format!("steel-saved-data-{test_name}-{unique}"))
    }

    #[tokio::test]
    async fn missing_saved_data_loads_default() {
        let dir = temp_world_dir("missing");
        let manager = SavedDataManager::new(Some(dir.as_path()));

        let loaded: TestData = manager
            .load_or_default(TEST_DATA)
            .await
            .expect("missing saved data should load default");

        assert_eq!(loaded, TestData::default());
    }

    #[tokio::test]
    async fn saved_data_round_trips_through_world_data_dir() {
        let dir = temp_world_dir("round-trip");
        let manager = SavedDataManager::new(Some(dir.as_path()));

        manager
            .save(TEST_DATA, &TestData { value: 42 })
            .await
            .expect("saved data should write");
        let loaded: TestData = manager
            .load_or_default(TEST_DATA)
            .await
            .expect("saved data should load");

        assert_eq!(loaded, TestData { value: 42 });
        assert!(dir.join("data").join("test_data.toml").exists());
    }

    #[tokio::test]
    async fn ephemeral_saved_data_does_not_write() {
        let manager = SavedDataManager::new(None);

        manager
            .save(TEST_DATA, &TestData { value: 42 })
            .await
            .expect("ephemeral save should be a no-op");
        let loaded: TestData = manager
            .load_or_default(TEST_DATA)
            .await
            .expect("ephemeral load should return default");

        assert_eq!(loaded, TestData::default());
    }

    #[test]
    fn wincode_saved_data_round_trips_with_header() {
        let dir = temp_world_dir("binary-round-trip");
        let manager = SavedDataManager::new(Some(dir.as_path()));

        manager
            .sync_save_wincode(TEST_BINARY_DATA, &TestBinaryData { value: 42 })
            .expect("binary saved data should write");
        let loaded: TestBinaryData = manager
            .sync_load_wincode(TEST_BINARY_DATA)
            .expect("binary saved data should load")
            .expect("binary saved data should exist");

        assert_eq!(loaded, TestBinaryData { value: 42 });
        let bytes = sync_fs::read(dir.join("data").join("test_binary_data.bin"))
            .expect("binary saved data file should exist");
        assert_eq!(&bytes[..6], b"TEST\x03\x00");

        let newer_format = WincodeSavedDataName::trusted("test_binary_data", *b"TEST", 4);
        let error = manager
            .sync_load_wincode::<TestBinaryData>(newer_format)
            .expect_err("mismatched binary format version should fail");
        assert_eq!(error.kind(), ErrorKind::InvalidData);
    }

    #[test]
    fn missing_and_ephemeral_wincode_data_return_none() {
        let dir = temp_world_dir("binary-missing");
        let persistent = SavedDataManager::new(Some(dir.as_path()));
        let ephemeral = SavedDataManager::new(None);

        assert!(
            persistent
                .sync_load_wincode::<TestBinaryData>(TEST_BINARY_DATA)
                .expect("missing binary data should load")
                .is_none()
        );
        assert!(
            ephemeral
                .sync_load_wincode::<TestBinaryData>(TEST_BINARY_DATA)
                .expect("ephemeral binary data should load")
                .is_none()
        );
        ephemeral
            .sync_save_wincode(TEST_BINARY_DATA, &TestBinaryData { value: 42 })
            .expect("ephemeral binary save should be a no-op");
    }
}
