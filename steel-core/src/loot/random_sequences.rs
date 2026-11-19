//! Named vanilla random streams, shared and saved at Steel's domain boundary.

use std::{
    collections::BTreeMap,
    io,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
};

use rustc_hash::FxHashMap;
use serde::{Deserialize, Serialize};
use steel_utils::{
    Identifier,
    locks::{AsyncMutex, SyncRwLock},
    random::{RandomSource, xoroshiro::Xoroshiro},
    saved_data::names as saved_data_names,
};

use super::{
    LootLifecycle, OperationResult,
    random_stream::{RandomRequest, RandomStream},
};
use crate::server::worlds::WorldMap;

#[derive(Deserialize, Serialize)]
struct PersistentSequence {
    source: [i64; 2],
}

#[derive(Deserialize, Serialize)]
#[serde(default)]
struct PersistentRandomSequences {
    salt: i32,
    include_world_seed: bool,
    include_sequence_id: bool,
    sequences: BTreeMap<String, PersistentSequence>,
}

impl Default for PersistentRandomSequences {
    fn default() -> Self {
        Self {
            salt: 0,
            include_world_seed: true,
            include_sequence_id: true,
            sequences: BTreeMap::new(),
        }
    }
}

/// Saved vanilla Xoroshiro streams for one domain, using its default world's seed.
pub(crate) struct RandomSequences {
    pub(super) lifecycle: Arc<LootLifecycle>,
    seed: i64,
    salt: i32,
    include_world_seed: bool,
    include_sequence_id: bool,
    sequences: SyncRwLock<FxHashMap<Identifier, Arc<RandomStream>>>,
    revision: Arc<AtomicU64>,
    saved_revision: AtomicU64,
}

impl RandomSequences {
    #[cfg(test)]
    pub(crate) fn new_for_test(seed: i64) -> Self {
        Self::from_persistent(seed, PersistentRandomSequences::default())
            .expect("empty sequence data is valid")
    }

    fn from_persistent(seed: i64, persistent: PersistentRandomSequences) -> io::Result<Self> {
        let revision = Arc::new(AtomicU64::new(0));
        let mut sequences = FxHashMap::default();
        for (key, sequence) in persistent.sequences {
            let key = key
                .parse::<Identifier>()
                .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
            let random = Xoroshiro::from_state(sequence.source.map(i64::cast_unsigned));
            sequences.insert(
                key,
                Arc::new(RandomStream::new(
                    RandomSource::Xoroshiro(random),
                    Arc::clone(&revision),
                )),
            );
        }
        Ok(Self {
            lifecycle: Arc::new(LootLifecycle::default()),
            seed,
            salt: persistent.salt,
            include_world_seed: persistent.include_world_seed,
            include_sequence_id: persistent.include_sequence_id,
            sequences: SyncRwLock::new(sequences),
            revision,
            saved_revision: AtomicU64::new(0),
        })
    }

    fn create_sequence(&self, key: &Identifier) -> RandomSource {
        let seed = (if self.include_world_seed {
            self.seed
        } else {
            0
        }) ^ i64::from(self.salt);
        let random = if self.include_sequence_id {
            let digest = md5::compute(key.to_string());
            let mut lo = [0; 8];
            let mut hi = [0; 8];
            lo.copy_from_slice(&digest.0[..8]);
            hi.copy_from_slice(&digest.0[8..]);
            Xoroshiro::from_seed_with_hash(
                seed as u64,
                [u64::from_be_bytes(lo), u64::from_be_bytes(hi)],
            )
        } else {
            Xoroshiro::from_seed(seed as u64)
        };
        RandomSource::Xoroshiro(random)
    }

    fn sequence(&self, key: &Identifier) -> Arc<RandomStream> {
        if let Some(existing) = self.sequences.read().get(key) {
            return Arc::clone(existing);
        }
        Arc::clone(
            self.sequences
                .write()
                .entry(key.clone())
                .or_insert_with(|| {
                    Arc::new(RandomStream::new(
                        self.create_sequence(key),
                        Arc::clone(&self.revision),
                    ))
                }),
        )
    }

    pub(super) fn request(&self, key: &Identifier) -> RandomRequest {
        self.sequence(key).request()
    }

    pub(crate) fn with_random<T>(
        &self,
        key: &Identifier,
        use_random: impl FnOnce(&mut RandomSource) -> OperationResult<T>,
    ) -> OperationResult<T> {
        self.sequence(key).try_with_random(use_random)
    }

    fn pending_save(&self) -> io::Result<Option<(u64, PersistentRandomSequences)>> {
        // Capture the revision before reading any stream. A concurrent draw must
        // stay dirty even when it occurs between snapshotting different streams.
        let revision = self.revision.load(Ordering::Acquire);
        if revision == self.saved_revision.load(Ordering::Acquire) {
            return Ok(None);
        }
        let sources: Vec<_> = self
            .sequences
            .read()
            .iter()
            .map(|(key, source)| (key.to_string(), Arc::clone(source)))
            .collect();
        let mut sequences = BTreeMap::new();
        for (key, source) in sources {
            sequences.insert(
                key,
                PersistentSequence {
                    source: source.persistent_state()?,
                },
            );
        }
        Ok(Some((
            revision,
            PersistentRandomSequences {
                salt: self.salt,
                include_world_seed: self.include_world_seed,
                include_sequence_id: self.include_sequence_id,
                sequences,
            },
        )))
    }
}

/// Loads and saves one shared sequence collection per domain.
pub(crate) struct DomainRandomSequences {
    domains: BTreeMap<String, Arc<RandomSequences>>,
    save_lock: AsyncMutex<()>,
}

impl DomainRandomSequences {
    pub(crate) async fn load(worlds: &WorldMap) -> io::Result<Self> {
        let mut domains = BTreeMap::new();
        for domain in worlds.domain_names() {
            let world = worlds.default_world(domain).ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::NotFound,
                    format!("domain {domain} has no default world"),
                )
            })?;
            let persistent = world
                .saved_data
                .load_or_default(saved_data_names::RANDOM_SEQUENCES)
                .await?;
            let seed = world.level_data.read().seed();
            let sequences = Arc::new(RandomSequences::from_persistent(seed, persistent)?);
            for world in worlds.values().filter(|world| world.domain() == domain) {
                *world.random_sequences.write() = Some(Arc::clone(&sequences));
                *world.loot_lifecycle.write() = Arc::clone(&sequences.lifecycle);
            }
            domains.insert(domain.to_owned(), sequences);
        }
        Ok(Self {
            domains,
            save_lock: AsyncMutex::new(()),
        })
    }

    pub(crate) async fn save(&self, worlds: &WorldMap) -> io::Result<usize> {
        let _save_guard = self.save_lock.lock().await;
        let mut saved = 0;
        for (domain, sequences) in &self.domains {
            let _barrier = sequences.lifecycle.save_barrier().await;
            let Some((revision, persistent)) = sequences.pending_save()? else {
                continue;
            };
            let world = worlds.default_world(domain).ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::NotFound,
                    format!("domain {domain} has no default world"),
                )
            })?;
            world
                .saved_data
                .save(saved_data_names::RANDOM_SEQUENCES, &persistent)
                .await?;
            sequences
                .saved_revision
                .fetch_max(revision, Ordering::Release);
            saved += 1;
        }
        Ok(saved)
    }
}

#[cfg(test)]
#[path = "random_sequences_tests.rs"]
mod tests;
