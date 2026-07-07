//! Per-world saved data storage.
//!
//! Vanilla stores world-level saved data under each dimension's `data/`
//! directory. Steel keeps its on-disk format TOML like `level_data`, but uses
//! the same per-world saved-data boundary.

use std::{
    io,
    path::{Path, PathBuf},
};

use serde::{Serialize, de::DeserializeOwned};
use tokio::fs;

/// Built-in saved data entry names.
pub mod names {
    use super::SavedDataName;

    /// Vanilla `TicketStorage.TYPE`, persisted as `data/chunk_tickets.toml`.
    pub const CHUNK_TICKETS: SavedDataName = SavedDataName::trusted("chunk_tickets");
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

fn is_valid_saved_data_name(name: &str) -> bool {
    !name.is_empty()
        && !name.contains('/')
        && !name.contains('\\')
        && steel_utils::Identifier::validate_path(name)
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
}

#[cfg(test)]
mod tests {
    use std::{
        env::temp_dir,
        path::PathBuf,
        time::{SystemTime, UNIX_EPOCH},
    };

    use serde::{Deserialize, Serialize};

    use super::{SavedDataManager, SavedDataName};

    const TEST_DATA: SavedDataName = SavedDataName::trusted("test_data");

    #[derive(Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
    struct TestData {
        value: i32,
    }

    #[test]
    fn saved_data_name_rejects_paths() {
        assert!(SavedDataName::try_new("valid_name").is_ok());
        assert!(SavedDataName::try_new("../outside").is_err());
        assert!(SavedDataName::try_new("nested/name").is_err());
        assert!(SavedDataName::try_new("nested\\name").is_err());
        assert!(SavedDataName::try_new("").is_err());
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
}
