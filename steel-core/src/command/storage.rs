//! Persistent command storage isolated by Steel domain.

use std::{
    collections::BTreeMap,
    io::{self, Cursor},
    sync::atomic::{AtomicU64, Ordering},
};

use rustc_hash::FxHashMap;
use serde::{Deserialize, Serialize};
use simdnbt::owned::{NbtCompound, read_compound};
use steel_utils::{
    Identifier,
    locks::{AsyncMutex, SyncRwLock},
};

use crate::{server::worlds::WorldMap, world::World};
use steel_utils::saved_data::names as saved_data_names;

#[derive(Default, Deserialize, Serialize)]
struct PersistentCommandStorage {
    entries: BTreeMap<String, Vec<u8>>,
}

struct CommandStorageSaveSnapshot {
    revision: u64,
    state: PersistentCommandStorage,
}

/// Vanilla command storage for one Steel domain.
pub(crate) struct CommandStorage {
    entries: SyncRwLock<FxHashMap<Identifier, NbtCompound>>,
    revision: AtomicU64,
    saved_revision: AtomicU64,
}

impl CommandStorage {
    /// Creates an empty, clean command storage.
    #[must_use]
    pub(crate) fn new() -> Self {
        Self {
            entries: SyncRwLock::new(FxHashMap::default()),
            revision: AtomicU64::new(0),
            saved_revision: AtomicU64::new(0),
        }
    }

    fn from_persistent(persistent: PersistentCommandStorage) -> io::Result<Self> {
        let mut entries = FxHashMap::default();
        for (raw_key, bytes) in persistent.entries {
            let key = raw_key.parse::<Identifier>().map_err(|error| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("invalid command storage key '{raw_key}': {error}"),
                )
            })?;
            let mut cursor = Cursor::new(bytes.as_slice());
            let compound = read_compound(&mut cursor).map_err(|error| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("invalid NBT for command storage key '{key}': {error:?}"),
                )
            })?;
            if cursor.position() != bytes.len() as u64 {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("trailing NBT data for command storage key '{key}'"),
                ));
            }
            if !compound.is_empty() {
                entries.insert(key, compound);
            }
        }
        Ok(Self {
            entries: SyncRwLock::new(entries),
            revision: AtomicU64::new(0),
            saved_revision: AtomicU64::new(0),
        })
    }

    /// Returns the compound stored at `id`, or an empty compound when absent.
    #[must_use]
    pub(crate) fn get(&self, id: &Identifier) -> NbtCompound {
        self.entries
            .read()
            .get(id)
            .cloned()
            .unwrap_or_else(NbtCompound::new)
    }

    /// Stores a compound, removing the key when the compound is empty.
    pub(crate) fn set(&self, id: Identifier, contents: NbtCompound) {
        let mut entries = self.entries.write();
        if contents.is_empty() {
            entries.remove(&id);
        } else {
            entries.insert(id, contents);
        }
        self.revision.fetch_add(1, Ordering::Release);
    }

    /// Returns stored keys in stable resource-location order.
    #[must_use]
    pub(crate) fn keys(&self) -> Vec<Identifier> {
        let mut keys = self.entries.read().keys().cloned().collect::<Vec<_>>();
        keys.sort_by_cached_key(ToString::to_string);
        keys
    }

    fn pending_save(&self) -> Option<CommandStorageSaveSnapshot> {
        let entries = self.entries.read();
        let revision = self.revision.load(Ordering::Acquire);
        if revision == self.saved_revision.load(Ordering::Acquire) {
            return None;
        }

        let entries = entries
            .iter()
            .map(|(key, compound)| {
                let mut bytes = Vec::new();
                compound.write(&mut bytes);
                (key.to_string(), bytes)
            })
            .collect();
        Some(CommandStorageSaveSnapshot {
            revision,
            state: PersistentCommandStorage { entries },
        })
    }

    fn mark_saved(&self, revision: u64) {
        self.saved_revision.fetch_max(revision, Ordering::Release);
    }
}

impl Default for CommandStorage {
    fn default() -> Self {
        Self::new()
    }
}

/// Loaded command storages keyed by Steel domain.
pub(crate) struct DomainCommandStorage {
    storages: BTreeMap<String, CommandStorage>,
    save_lock: AsyncMutex<()>,
}

impl DomainCommandStorage {
    /// Loads one command storage through each domain's default world.
    pub(crate) async fn load(worlds: &WorldMap) -> io::Result<Self> {
        let mut domains = worlds.domain_names().collect::<Vec<_>>();
        domains.sort_unstable();
        let mut storages = BTreeMap::new();
        for domain in domains {
            let world = domain_default_world(worlds, domain)?;
            let persistent: PersistentCommandStorage = world
                .saved_data
                .load_or_default(saved_data_names::COMMAND_STORAGE)
                .await
                .map_err(|error| storage_io_error(domain, error))?;
            let storage = CommandStorage::from_persistent(persistent)
                .map_err(|error| storage_io_error(domain, error))?;
            storages.insert(domain.to_owned(), storage);
        }
        Ok(Self {
            storages,
            save_lock: AsyncMutex::new(()),
        })
    }

    /// Returns command storage for a domain.
    #[must_use]
    pub(crate) fn get(&self, domain: &str) -> Option<&CommandStorage> {
        self.storages.get(domain)
    }

    /// Saves every dirty domain storage and returns the number written.
    pub(crate) async fn save(&self, worlds: &WorldMap) -> io::Result<usize> {
        let _save_guard = self.save_lock.lock().await;
        let mut saved = 0;
        for (domain, storage) in &self.storages {
            let Some(snapshot) = storage.pending_save() else {
                continue;
            };
            let world = domain_default_world(worlds, domain)?;
            world
                .saved_data
                .save(saved_data_names::COMMAND_STORAGE, &snapshot.state)
                .await
                .map_err(|error| storage_io_error(domain, error))?;
            storage.mark_saved(snapshot.revision);
            saved += 1;
        }
        Ok(saved)
    }
}

fn domain_default_world<'a>(worlds: &'a WorldMap, domain: &str) -> io::Result<&'a World> {
    worlds
        .default_world(domain)
        .map(AsRef::as_ref)
        .ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::NotFound,
                format!("domain '{domain}' has no loaded default world"),
            )
        })
}

fn storage_io_error(domain: &str, error: io::Error) -> io::Error {
    io::Error::new(
        error.kind(),
        format!("command storage I/O failed for domain '{domain}': {error}"),
    )
}

#[cfg(test)]
mod tests {
    use std::{
        env::temp_dir,
        slice,
        time::{SystemTime, UNIX_EPOCH},
    };

    use simdnbt::owned::{NbtCompound, NbtTag};
    use steel_utils::Identifier;
    use tokio::fs;

    use steel_utils::saved_data::SavedDataManager;

    use super::*;

    #[test]
    fn missing_and_empty_values_match_vanilla_storage_semantics() {
        let storage = CommandStorage::new();
        let key = Identifier::from_steel("data");

        assert!(storage.get(&key).is_empty());
        let mut value = NbtCompound::new();
        value.insert("value", 3);
        storage.set(key.clone(), value);
        assert_eq!(storage.keys(), slice::from_ref(&key));

        storage.set(key.clone(), NbtCompound::new());
        assert!(storage.get(&key).is_empty());
        assert!(storage.keys().is_empty());
    }

    #[tokio::test]
    async fn persistent_storage_round_trips_binary_nbt() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time should be after Unix epoch")
            .as_nanos();
        let path = temp_dir().join(format!("steel-command-storage-{unique}"));
        let manager = SavedDataManager::new(Some(&path));
        let storage = CommandStorage::new();
        let key = Identifier::from_steel("nested");
        let mut nested = NbtCompound::new();
        nested.insert("bytes", NbtTag::ByteArray(vec![1, 2, 255]));
        let mut value = NbtCompound::new();
        value.insert("nested", nested);
        storage.set(key.clone(), value.clone());

        let snapshot = storage.pending_save().expect("storage should become dirty");
        manager
            .save(saved_data_names::COMMAND_STORAGE, &snapshot.state)
            .await
            .expect("command storage should save");
        storage.mark_saved(snapshot.revision);
        assert!(storage.pending_save().is_none());

        let persistent: PersistentCommandStorage = manager
            .load_or_default(saved_data_names::COMMAND_STORAGE)
            .await
            .expect("command storage should load");
        let restored =
            CommandStorage::from_persistent(persistent).expect("stored NBT should validate");
        assert_eq!(restored.get(&key), value);

        fs::remove_dir_all(path)
            .await
            .expect("temporary command storage directory should be removed");
    }

    #[test]
    fn mutation_after_snapshot_remains_dirty() {
        let storage = CommandStorage::new();
        let first = Identifier::from_steel("first");
        let second = Identifier::from_steel("second");
        let mut value = NbtCompound::new();
        value.insert("value", 1);
        storage.set(first, value.clone());
        let snapshot = storage.pending_save().expect("storage should become dirty");

        storage.set(second.clone(), value);
        storage.mark_saved(snapshot.revision);

        let pending = storage
            .pending_save()
            .expect("newer mutation should remain dirty");
        assert!(pending.revision > snapshot.revision);
        assert!(pending.state.entries.contains_key(&second.to_string()));
    }

    #[test]
    fn domains_keep_independent_command_storage() {
        let storages = DomainCommandStorage {
            storages: [
                ("alpha".to_owned(), CommandStorage::new()),
                ("beta".to_owned(), CommandStorage::new()),
            ]
            .into_iter()
            .collect(),
            save_lock: AsyncMutex::new(()),
        };
        let key = Identifier::from_steel("data");
        let mut value = NbtCompound::new();
        value.insert("value", 1);
        storages
            .get("alpha")
            .expect("alpha storage should exist")
            .set(key.clone(), value);

        assert!(
            storages
                .get("beta")
                .expect("beta storage should exist")
                .get(&key)
                .is_empty()
        );
    }

    #[test]
    fn invalid_persisted_nbt_is_rejected() {
        let persistent = PersistentCommandStorage {
            entries: [("steel:data".to_owned(), vec![10])].into_iter().collect(),
        };

        let error = CommandStorage::from_persistent(persistent)
            .err()
            .expect("invalid NBT should fail loading");
        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
    }
}
