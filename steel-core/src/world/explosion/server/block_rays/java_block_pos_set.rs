//! Explosion-local emulation of Java `HashSet<BlockPos>` iteration order.
//!
//! Distinct hashes follow `OpenJDK` 25's linked tree-bin order. `BlockPos` inherits
//! `Comparable<Vec3i>`, so equal-hash ties use JVM identities. Here ties use insertion
//! indices, matching increasing JVM identities for plain `BlockPos` keys. Different
//! identities can also change other entries' order in that bin and its later splits.

use std::mem;
use std::vec::IntoIter;

use rustc_hash::FxHashMap;
use steel_utils::BlockPos;

#[cfg(test)]
mod tests;
mod tree;

const EMPTY: u32 = u32::MAX;
const TREEIFY_THRESHOLD: usize = 8;
const MIN_TREEIFY_CAPACITY: usize = 64;

#[derive(Default)]
pub(super) struct JavaBlockPosSet {
    buckets: Vec<Bucket>,
    entries: Vec<Entry>,
    // Allocate tree links only for entries that have actually entered a tree bin.
    trees: FxHashMap<u32, tree::Links>,
}

#[derive(Clone, Copy)]
struct Bucket {
    head: u32,
    // EMPTY marks a tree bin; empty list bins use a dummy tail and an EMPTY head.
    tail: u32,
}

impl Bucket {
    const EMPTY: Self = Self {
        head: EMPTY,
        tail: 0,
    };

    const fn is_tree(self) -> bool {
        self.tail == EMPTY
    }
}

struct Entry {
    pos: BlockPos,
    next: u32,
}

impl JavaBlockPosSet {
    #[inline]
    pub(super) fn insert(&mut self, pos: BlockPos) -> bool {
        if self.buckets.is_empty() {
            self.buckets.resize(16, Bucket::EMPTY);
            self.entries.reserve(16);
        }
        let hash = spread(pos);
        let index = hash as u32 as usize & (self.buckets.len() - 1);
        let bucket = self.buckets[index];
        let mut bin_len = 0;
        if bucket.is_tree() {
            if self.find_in_tree(bucket.head, hash, pos) {
                return false;
            }
        } else {
            let mut current = bucket.head;
            while current != EMPTY {
                let entry = &self.entries[current as usize];
                if entry.pos == pos {
                    return false;
                }
                current = entry.next;
                bin_len += 1;
            }
        }

        let Ok(entry_index) = u32::try_from(self.entries.len()) else {
            panic!("JavaBlockPosSet entry arena exceeded its u32 index space");
        };
        assert_ne!(
            entry_index, EMPTY,
            "JavaBlockPosSet entry arena exhausted its u32 index space"
        );
        self.entries.push(Entry { pos, next: EMPTY });
        if bucket.is_tree() {
            self.insert_tree(index, entry_index);
        } else {
            self.append(index, entry_index);
            // Java treeifies when adding the ninth bin entry.
            if bin_len >= TREEIFY_THRESHOLD {
                if self.buckets.len() < MIN_TREEIFY_CAPACITY {
                    self.resize();
                } else {
                    self.treeify(index);
                }
            }
        }
        if self.entries.len() > self.buckets.len() * 3 / 4 {
            self.resize();
        }
        true
    }

    #[cfg(test)]
    pub(super) const fn bucket_count(&self) -> usize {
        self.buckets.len()
    }

    fn append(&mut self, bucket_index: usize, entry_index: u32) {
        debug_assert!(!self.buckets[bucket_index].is_tree());
        let bucket = &mut self.buckets[bucket_index];
        self.entries[entry_index as usize].next = EMPTY;
        if bucket.head == EMPTY {
            bucket.head = entry_index;
        } else {
            self.entries[bucket.tail as usize].next = entry_index;
        }
        bucket.tail = entry_index;
    }

    fn resize(&mut self) {
        let old_capacity = self.buckets.len();
        // HashMap stops growing at MAXIMUM_CAPACITY.
        if old_capacity >= 1 << 30 {
            return;
        }
        let old_buckets = mem::replace(&mut self.buckets, vec![Bucket::EMPTY; old_capacity * 2]);
        for (index, bucket) in old_buckets.into_iter().enumerate() {
            if bucket.is_tree() {
                self.split_tree(index, bucket.head, old_capacity);
                continue;
            }
            let mut current = bucket.head;
            while current != EMPTY {
                let next = self.entries[current as usize].next;
                let target = spread(self.entries[current as usize].pos) as u32 as usize
                    & (self.buckets.len() - 1);
                self.append(target, current);
                current = next;
            }
        }
    }
}

impl IntoIterator for JavaBlockPosSet {
    type Item = BlockPos;
    type IntoIter = IntoIter<BlockPos>;

    fn into_iter(self) -> Self::IntoIter {
        let mut ordered = Vec::with_capacity(self.entries.len());
        for bucket in self.buckets {
            let mut current = bucket.head;
            while current != EMPTY {
                let entry = &self.entries[current as usize];
                ordered.push(entry.pos);
                current = entry.next;
            }
        }
        ordered.into_iter()
    }
}

const fn spread(pos: BlockPos) -> i32 {
    let hash = pos.java_hash_code() as u32;
    (hash ^ (hash >> 16)) as i32
}
