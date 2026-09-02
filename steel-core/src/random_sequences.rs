//! Domain-scoped persistent random sequences used by loot tables and other named RNG streams.

use std::{collections::BTreeMap, io, str::FromStr, sync::Arc};

use rustc_hash::FxHashMap;
use serde::{Deserialize, Serialize};
use steel_utils::{
    Identifier,
    locks::SyncMutex,
    random::{RandomSource, xoroshiro::Xoroshiro},
    saved_data::{SavedDataManager, names as saved_data_names},
};

use crate::{config::ResolvedDomainConfig, server::worlds::WorldMap, world::World};

/// Named random-sequence maps keyed by Steel domain.
pub(crate) struct DomainRandomSequences {
    domains: BTreeMap<String, Arc<RandomSequences>>,
}

/// Vanilla's logical-server-owned `RandomSequences` saved data.
pub(crate) struct RandomSequences {
    domain_seed: i64,
    saved_data: SavedDataManager,
    inner: SyncMutex<RandomSequencesInner>,
}

struct RandomSequencesInner {
    salt: i32,
    include_world_seed: bool,
    include_sequence_id: bool,
    sequences: FxHashMap<String, RandomSource>,
    revision: u64,
    saved_revision: u64,
}

#[derive(Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct PersistentRandomSequences {
    domain_seed: Option<i64>,
    salt: i32,
    include_world_seed: bool,
    include_sequence_id: bool,
    sequences: BTreeMap<String, PersistentRandomSequence>,
}

impl Default for PersistentRandomSequences {
    fn default() -> Self {
        Self {
            domain_seed: None,
            salt: 0,
            include_world_seed: true,
            include_sequence_id: true,
            sequences: BTreeMap::new(),
        }
    }
}

impl DomainRandomSequences {
    /// Loads one sequence map through each domain's default-world persistence boundary.
    pub(crate) async fn load(
        domains: &[ResolvedDomainConfig],
        worlds: &WorldMap,
    ) -> io::Result<Self> {
        let mut sequences = BTreeMap::new();
        for domain in domains {
            let world = domain_default_world(worlds, &domain.name)?;
            let random_sequences = RandomSequences::load(domain.seed, world.saved_data.clone())
                .await
                .map_err(|error| random_sequence_io_error(&domain.name, error))?;
            sequences.insert(domain.name.clone(), Arc::new(random_sequences));
        }
        Ok(Self { domains: sequences })
    }

    /// Creates unpersisted sequence maps for test and independently constructed servers.
    #[cfg(test)]
    pub(crate) fn ephemeral(domains: &[ResolvedDomainConfig]) -> Self {
        let domains = domains
            .iter()
            .map(|domain| {
                (
                    domain.name.clone(),
                    Arc::new(RandomSequences::ephemeral(domain.seed)),
                )
            })
            .collect();
        Self { domains }
    }

    /// Returns the named-sequence map owned by `domain`.
    pub(crate) fn get(&self, domain: &str) -> Option<&Arc<RandomSequences>> {
        self.domains.get(domain)
    }

    /// Persists one domain's sequence map when it advanced.
    pub(crate) async fn save(&self, domain: &str) -> io::Result<bool> {
        let Some(sequences) = self.domains.get(domain) else {
            return Err(io::Error::new(
                io::ErrorKind::NotFound,
                format!("domain '{domain}' has no random-sequence map"),
            ));
        };
        sequences
            .save()
            .await
            .map_err(|error| random_sequence_io_error(domain, error))
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

fn random_sequence_io_error(domain: &str, error: io::Error) -> io::Error {
    io::Error::new(
        error.kind(),
        format!("random-sequence I/O failed for domain '{domain}': {error}"),
    )
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct PersistentRandomSequence {
    /// The two signed longs encoded by Vanilla's xoroshiro codec.
    source: [i64; 2],
}

impl RandomSequences {
    /// Loads a domain's sequence map through its saved-data boundary.
    async fn load(domain_seed: i64, saved_data: SavedDataManager) -> io::Result<Self> {
        let persistent = saved_data
            .load_or_default(saved_data_names::RANDOM_SEQUENCES)
            .await?;
        Self::from_persistent(domain_seed, saved_data, persistent)
    }

    /// Creates an unpersisted sequence map for an independently constructed world.
    pub(crate) fn ephemeral(domain_seed: i64) -> Self {
        Self {
            domain_seed,
            saved_data: SavedDataManager::new(None),
            inner: SyncMutex::new(RandomSequencesInner {
                salt: 0,
                include_world_seed: true,
                include_sequence_id: true,
                sequences: FxHashMap::default(),
                revision: 0,
                saved_revision: 0,
            }),
        }
    }

    fn from_persistent(
        configured_domain_seed: i64,
        saved_data: SavedDataManager,
        persistent: PersistentRandomSequences,
    ) -> io::Result<Self> {
        let domain_seed = persistent.domain_seed.unwrap_or(configured_domain_seed);
        let seed_needs_save = persistent.domain_seed.is_none();
        let mut sequences = FxHashMap::default();
        for (key, sequence) in persistent.sequences {
            let identifier = Identifier::from_str(&key).map_err(|error| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("invalid random-sequence identifier {key:?}: {error}"),
                )
            })?;
            let [seed_lo, seed_hi] = sequence.source;
            sequences.insert(
                identifier.to_string(),
                RandomSource::Xoroshiro(Xoroshiro::from_state(seed_lo as u64, seed_hi as u64)),
            );
        }

        Ok(Self {
            domain_seed,
            saved_data,
            inner: SyncMutex::new(RandomSequencesInner {
                salt: persistent.salt,
                include_world_seed: persistent.include_world_seed,
                include_sequence_id: persistent.include_sequence_id,
                sequences,
                revision: u64::from(seed_needs_save),
                saved_revision: 0,
            }),
        })
    }

    /// Runs an operation against the persistent stream for `key`.
    pub(crate) fn with_sequence<T>(
        &self,
        key: &Identifier,
        operation: impl FnOnce(&mut RandomSource) -> T,
    ) -> T {
        let mut inner = self.inner.lock();
        let salt = inner.salt;
        let include_world_seed = inner.include_world_seed;
        let include_sequence_id = inner.include_sequence_id;
        let key = key.to_string();
        let random = inner.sequences.entry(key.clone()).or_insert_with(|| {
            Self::create_sequence(
                self.domain_seed,
                salt,
                include_world_seed,
                include_sequence_id,
                &key,
            )
        });
        let result = operation(random);
        inner.revision = inner.revision.wrapping_add(1);
        result
    }

    /// Runs a fallible operation transactionally against the persistent stream.
    ///
    /// Failed loot evaluation must not consume a named sequence: the caller can
    /// correct the missing context and retry without changing Vanilla's result.
    pub(crate) fn try_with_sequence<T, E>(
        &self,
        key: &Identifier,
        operation: impl FnOnce(&mut RandomSource) -> Result<T, E>,
    ) -> Result<T, E> {
        let mut inner = self.inner.lock();
        let salt = inner.salt;
        let include_world_seed = inner.include_world_seed;
        let include_sequence_id = inner.include_sequence_id;
        let key = key.to_string();
        let random = inner.sequences.entry(key.clone()).or_insert_with(|| {
            Self::create_sequence(
                self.domain_seed,
                salt,
                include_world_seed,
                include_sequence_id,
                &key,
            )
        });
        let mut candidate = random.clone();
        let result = operation(&mut candidate)?;
        *random = candidate;
        inner.revision = inner.revision.wrapping_add(1);
        Ok(result)
    }

    fn create_sequence(
        domain_seed: i64,
        salt: i32,
        include_world_seed: bool,
        include_sequence_id: bool,
        key: &str,
    ) -> RandomSource {
        let seed = (if include_world_seed { domain_seed } else { 0 }) ^ i64::from(salt);
        let random = if include_sequence_id {
            Xoroshiro::from_seed_with_key(seed as u64, key)
        } else {
            Xoroshiro::from_seed(seed as u64)
        };
        RandomSource::Xoroshiro(random)
    }

    /// Persists changed sequence states. Returns whether a write was needed.
    pub(crate) async fn save(&self) -> io::Result<bool> {
        let Some((revision, persistent)) = self.persistent_snapshot()? else {
            return Ok(false);
        };
        self.saved_data
            .save(saved_data_names::RANDOM_SEQUENCES, &persistent)
            .await?;

        let mut inner = self.inner.lock();
        if inner.revision == revision {
            inner.saved_revision = revision;
        }
        Ok(true)
    }

    fn persistent_snapshot(&self) -> io::Result<Option<(u64, PersistentRandomSequences)>> {
        let inner = self.inner.lock();
        if inner.revision == inner.saved_revision {
            return Ok(None);
        }

        let mut sequences = BTreeMap::new();
        for (key, source) in &inner.sequences {
            let RandomSource::Xoroshiro(random) = source else {
                return Err(io::Error::other(
                    "random-sequence map contained a non-xoroshiro source",
                ));
            };
            let (seed_lo, seed_hi) = random.state();
            sequences.insert(
                key.clone(),
                PersistentRandomSequence {
                    source: [seed_lo as i64, seed_hi as i64],
                },
            );
        }

        Ok(Some((
            inner.revision,
            PersistentRandomSequences {
                domain_seed: Some(self.domain_seed),
                salt: inner.salt,
                include_world_seed: inner.include_world_seed,
                include_sequence_id: inner.include_sequence_id,
                sequences,
            },
        )))
    }
}

#[cfg(test)]
mod tests {
    use std::{
        env::temp_dir,
        path::PathBuf,
        time::{SystemTime, UNIX_EPOCH},
    };

    use steel_utils::random::Random;
    use tokio::fs;

    use super::*;

    #[test]
    fn restored_sequence_continues_from_its_persisted_state() {
        let key = Identifier::vanilla_static("chests/simple_dungeon");
        let sequences = RandomSequences::ephemeral(12_345);
        sequences.with_sequence(&key, |random| {
            random.next_i64();
            random.next_i32();
        });
        let snapshot = sequences.persistent_snapshot();
        let Ok(Some((_, persistent))) = snapshot else {
            panic!("used sequence should produce a persistent snapshot");
        };
        let expected = sequences.with_sequence(&key, Random::next_i64);

        let restored =
            RandomSequences::from_persistent(12_345, SavedDataManager::new(None), persistent);
        let Ok(restored) = restored else {
            panic!("valid sequence snapshot should restore");
        };
        assert_eq!(restored.with_sequence(&key, Random::next_i64), expected);
    }

    #[test]
    fn persisted_domain_seed_controls_new_sequences_after_restart() {
        let first_key = Identifier::vanilla_static("chests/simple_dungeon");
        let later_key = Identifier::vanilla_static("chests/shipwreck_supply");
        let sequences = RandomSequences::ephemeral(12_345);
        sequences.with_sequence(&first_key, Random::next_i64);
        let snapshot = sequences.persistent_snapshot();
        let Ok(Some((_, persistent))) = snapshot else {
            panic!("used sequence should produce a persistent snapshot");
        };

        let restored =
            RandomSequences::from_persistent(98_765, SavedDataManager::new(None), persistent);
        let Ok(restored) = restored else {
            panic!("valid sequence snapshot should restore");
        };
        let expected =
            RandomSequences::ephemeral(12_345).with_sequence(&later_key, Random::next_i64);

        assert_eq!(
            restored.with_sequence(&later_key, Random::next_i64),
            expected
        );
    }

    #[test]
    fn domain_sequence_maps_advance_independently() {
        let key = Identifier::vanilla_static("chests/simple_dungeon");
        let domain_a = RandomSequences::ephemeral(42);
        let domain_b = RandomSequences::ephemeral(42);

        let first_a = domain_a.with_sequence(&key, Random::next_i64);
        let second_a = domain_a.with_sequence(&key, Random::next_i64);
        let first_b = domain_b.with_sequence(&key, Random::next_i64);

        assert_eq!(first_a, first_b);
        assert_ne!(second_a, first_b);
    }

    #[tokio::test]
    async fn domain_sequence_files_restore_independently() {
        let root = temp_save_root("domain-round-trip");
        let alpha_path = root.join("alpha");
        let beta_path = root.join("beta");
        let alpha =
            RandomSequences::load(42, SavedDataManager::new(Some(alpha_path.as_path()))).await;
        let beta =
            RandomSequences::load(42, SavedDataManager::new(Some(beta_path.as_path()))).await;
        let (Ok(alpha), Ok(beta)) = (alpha, beta) else {
            panic!("fresh domain sequence maps should load");
        };
        let maps = DomainRandomSequences {
            domains: BTreeMap::from([
                ("alpha".to_owned(), Arc::new(alpha)),
                ("beta".to_owned(), Arc::new(beta)),
            ]),
        };
        let key = Identifier::vanilla_static("chests/simple_dungeon");
        let Some(alpha) = maps.get("alpha") else {
            panic!("alpha sequence map should exist");
        };
        let Some(beta) = maps.get("beta") else {
            panic!("beta sequence map should exist");
        };
        alpha.with_sequence(&key, Random::next_i64);
        beta.with_sequence(&key, Random::next_i64);
        beta.with_sequence(&key, Random::next_i64);

        assert!(matches!(maps.save("alpha").await, Ok(true)));
        assert!(matches!(maps.save("beta").await, Ok(true)));
        let expected_alpha = alpha.with_sequence(&key, Random::next_i64);
        let expected_beta = beta.with_sequence(&key, Random::next_i64);
        drop(maps);

        let restored_alpha =
            RandomSequences::load(999, SavedDataManager::new(Some(alpha_path.as_path()))).await;
        let restored_beta =
            RandomSequences::load(999, SavedDataManager::new(Some(beta_path.as_path()))).await;
        let (Ok(restored_alpha), Ok(restored_beta)) = (restored_alpha, restored_beta) else {
            panic!("saved domain sequence maps should reload");
        };
        assert_eq!(
            restored_alpha.with_sequence(&key, Random::next_i64),
            expected_alpha
        );
        assert_eq!(
            restored_beta.with_sequence(&key, Random::next_i64),
            expected_beta
        );

        if let Err(error) = fs::remove_dir_all(root).await {
            panic!("sequence test directory should be removed: {error}");
        }
    }

    #[tokio::test]
    async fn one_domain_save_error_does_not_prevent_another_domain_save() {
        let root = temp_save_root("domain-save-error");
        let broken_path = root.join("broken");
        if let Err(error) = fs::create_dir_all(&root).await {
            panic!("sequence test root should be created: {error}");
        }
        if let Err(error) = fs::write(&broken_path, b"not a directory").await {
            panic!("broken persistence path should be created: {error}");
        }
        let healthy_path = root.join("healthy");
        let broken =
            RandomSequences::load(42, SavedDataManager::new(Some(broken_path.as_path()))).await;
        let healthy =
            RandomSequences::load(42, SavedDataManager::new(Some(healthy_path.as_path()))).await;
        let (Ok(broken), Ok(healthy)) = (broken, healthy) else {
            panic!("missing sequence files should load before their first save");
        };
        let maps = DomainRandomSequences {
            domains: BTreeMap::from([
                ("broken".to_owned(), Arc::new(broken)),
                ("healthy".to_owned(), Arc::new(healthy)),
            ]),
        };

        assert!(maps.save("broken").await.is_err());
        assert!(matches!(maps.save("healthy").await, Ok(true)));
        assert!(healthy_path.join("data/random_sequences.toml").is_file());

        if let Err(error) = fs::remove_dir_all(root).await {
            panic!("sequence test directory should be removed: {error}");
        }
    }

    fn temp_save_root(name: &str) -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |duration| duration.as_nanos());
        temp_dir().join(format!("steel-random-sequences-{name}-{unique}"))
    }
}
