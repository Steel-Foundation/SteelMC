//! share boundary corner columns with neighboring chunks
//!
//! quart-aligned corners agree on cached and raw paths, so reuse is exact
//! entries leave once all sharing chunks have used them; a size cap handles stragglers

use rustc_hash::{FxBuildHasher, FxHashMap};
use std::hash::BuildHasher;
use steel_utils::locks::SyncMutex;

const SHARD_COUNT: usize = 64;
/// Upper bound on stored values across all shards (~32 MiB of `f32`).
const MAX_STORED_VALUES: usize = 8 * 1024 * 1024;

struct Column {
    values: Box<[f32]>,
    /// Chunks sharing this column that have not yet filled it.
    remaining: u8,
}

#[derive(Default)]
struct Shard {
    columns: FxHashMap<(i32, i32), Column>,
    stored_values: usize,
}

/// Generator-wide store of boundary corner columns, keyed by cell column.
pub struct CornerColumnStore {
    shards: Box<[SyncMutex<Shard>]>,
}

impl Default for CornerColumnStore {
    fn default() -> Self {
        Self {
            shards: (0..SHARD_COUNT)
                .map(|_| SyncMutex::new(Shard::default()))
                .collect(),
        }
    }
}

impl CornerColumnStore {
    fn shard(&self, key: (i32, i32)) -> &SyncMutex<Shard> {
        &self.shards[FxBuildHasher.hash_one(key) as usize % SHARD_COUNT]
    }

    /// read a shared column and count this chunk's use; false on a miss
    pub fn consume(&self, key: (i32, i32), read: impl FnOnce(&[f32])) -> bool {
        let mut shard = self.shard(key).lock();
        let Some(column) = shard.columns.get_mut(&key) else {
            return false;
        };

        read(&column.values);
        column.remaining -= 1;
        if column.remaining == 0 {
            shard.remove(key);
        }

        true
    }

    /// keep a column for the other chunk a race just counts this chunk use
    pub fn produce(&self, key: (i32, i32), users: u8, values: impl FnOnce() -> Box<[f32]>) {
        debug_assert!(users > 1, "only shared columns are stored");
        let mut shard = self.shard(key).lock();
        if let Some(column) = shard.columns.get_mut(&key) {
            column.remaining -= 1;
            if column.remaining == 0 {
                shard.remove(key);
            }

            return;
        }

        let values = values();
        if shard.stored_values + values.len() > MAX_STORED_VALUES / SHARD_COUNT {
            shard.columns.clear();
            shard.stored_values = 0;
        }

        shard.stored_values += values.len();
        shard.columns.insert(
            key,
            Column {
                values,
                remaining: users - 1,
            },
        );
    }
}

impl Shard {
    fn remove(&mut self, key: (i32, i32)) {
        if let Some(column) = self.columns.remove(&key) {
            self.stored_values -= column.values.len();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::CornerColumnStore;

    fn read(store: &CornerColumnStore, key: (i32, i32)) -> Option<Vec<f32>> {
        let mut out = None;
        store.consume(key, |values| out = Some(values.to_vec()));
        out
    }

    #[test]
    fn last_user_removes_the_column() {
        let store = CornerColumnStore::default();
        assert_eq!(read(&store, (0, 0)), None);

        store.produce((0, 0), 2, || vec![1.0, 2.0].into());
        assert_eq!(read(&store, (0, 0)), Some(vec![1.0, 2.0]));
        assert_eq!(read(&store, (0, 0)), None);

        // Chunk corner: shared by four chunks.
        store.produce((4, 4), 4, || vec![3.0].into());
        for _ in 0..3 {
            assert_eq!(read(&store, (4, 4)), Some(vec![3.0]));
        }
        assert_eq!(read(&store, (4, 4)), None);
    }

    #[test]
    fn concurrent_producers_count_as_uses() {
        let store = CornerColumnStore::default();
        // Both sharing chunks missed and computed the column themselves.
        store.produce((0, 8), 2, || vec![1.0].into());
        store.produce((0, 8), 2, || unreachable!("already stored"));
        assert_eq!(read(&store, (0, 8)), None);
    }
}
