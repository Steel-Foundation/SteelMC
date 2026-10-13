//! Cross-chunk hand-off of noise cell-corner columns on chunk boundaries.
//!
//! Each chunk fills `(n + 1)²` corner columns but owns only `n²`: the columns
//! on its +X and +Z chunk boundaries are also filled by the neighboring chunks.
//! A corner column is a pure function of its cell column (corners sit on quart
//! boundaries, where the column cache's grid and raw paths agree), so a column
//! filled by one chunk can be copied by the others instead of re-evaluated.
//!
//! Vanilla recomputes these per chunk. Entries live only until every chunk
//! sharing the column has filled, so memory follows the frontier of chunks in
//! flight rather than the generated area.

use rustc_hash::{FxBuildHasher, FxHashMap};
use std::hash::BuildHasher;
use steel_utils::locks::SyncMutex;

const SHARD_COUNT: usize = 64;
/// Upper bound on stored values across all shards (~64 MiB of `f64`).
const MAX_STORED_VALUES: usize = 8 * 1024 * 1024;

struct Column {
    values: Box<[f64]>,
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

    /// If another chunk already filled column `key`, passes its values to
    /// `read` and records this chunk's use. Returns whether it was found.
    pub fn consume(&self, key: (i32, i32), read: impl FnOnce(&[f64])) -> bool {
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

    /// Records that this chunk filled column `key`, shared by `users` chunks in
    /// total, and keeps `values` for the others. If a concurrent chunk stored
    /// it first, only this chunk's use is recorded.
    pub fn produce(&self, key: (i32, i32), users: u8, values: impl FnOnce() -> Box<[f64]>) {
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
        // Entries whose other chunks never generate would otherwise stay
        // forever; dropping a full shard only costs recomputation.
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

    fn read(store: &CornerColumnStore, key: (i32, i32)) -> Option<Vec<f64>> {
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
