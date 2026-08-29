//! Explosion-local emulation of Java `HashSet<BlockPos>` iteration order.

use std::mem;
use std::vec::IntoIter;

use steel_utils::BlockPos;

const JAVA_HASH_MAP_TREEIFY_THRESHOLD: usize = 8;
const JAVA_HASH_MAP_MIN_TREEIFY_CAPACITY: usize = 64;
const JAVA_HASH_MAP_LOAD_FACTOR_NUMERATOR: usize = 3;
const JAVA_HASH_MAP_LOAD_FACTOR_DENOMINATOR: usize = 4;
const JAVA_BLOCK_POS_HASH_MULTIPLIER: i32 = 31;
const JAVA_HASH_MAP_SPREAD_SHIFT: u32 = 16;
const JAVA_BLOCK_POS_SET_EMPTY_INDEX: u32 = u32::MAX;

#[derive(Default)]
pub(super) struct JavaBlockPosSet {
    buckets: Vec<JavaBlockPosBucket>,
    entries: Vec<JavaBlockPosEntry>,
}

#[derive(Clone, Copy)]
struct JavaBlockPosBucket {
    head: u32,
    tail: u32,
}

impl JavaBlockPosBucket {
    const EMPTY: Self = Self {
        head: JAVA_BLOCK_POS_SET_EMPTY_INDEX,
        tail: JAVA_BLOCK_POS_SET_EMPTY_INDEX,
    };
}

struct JavaBlockPosEntry {
    pos: BlockPos,
    next: u32,
}

impl JavaBlockPosSet {
    pub(super) fn insert(&mut self, pos: BlockPos) -> bool {
        if self.buckets.is_empty() {
            self.buckets.resize(16, JavaBlockPosBucket::EMPTY);
            self.entries.reserve(16);
        }
        let index = java_block_pos_bucket(pos, self.buckets.len());
        let bucket = self.buckets[index];
        let mut current = bucket.head;
        let mut bin_len = 0;
        while current != JAVA_BLOCK_POS_SET_EMPTY_INDEX {
            let entry = &self.entries[current as usize];
            if entry.pos == pos {
                return false;
            }
            current = entry.next;
            bin_len += 1;
        }

        let Ok(entry_index) = u32::try_from(self.entries.len()) else {
            panic!("JavaBlockPosSet entry arena exceeded its u32 index space");
        };
        assert_ne!(
            entry_index, JAVA_BLOCK_POS_SET_EMPTY_INDEX,
            "JavaBlockPosSet entry arena exhausted its u32 index space"
        );
        self.entries.push(JavaBlockPosEntry {
            pos,
            next: JAVA_BLOCK_POS_SET_EMPTY_INDEX,
        });
        if bucket.tail == JAVA_BLOCK_POS_SET_EMPTY_INDEX {
            self.buckets[index] = JavaBlockPosBucket {
                head: entry_index,
                tail: entry_index,
            };
        } else {
            self.entries[bucket.tail as usize].next = entry_index;
            self.buckets[index].tail = entry_index;
        }

        // HashMap attempts to treeify after adding a ninth entry to one bin, but grows the
        // table instead while its capacity is below 64. That split changes iteration order.
        if self.buckets.len() < JAVA_HASH_MAP_MIN_TREEIFY_CAPACITY
            && bin_len >= JAVA_HASH_MAP_TREEIFY_THRESHOLD
        {
            self.resize();
        }
        // Steel intentionally keeps list bins at larger capacities. HashMap tree-bin order can
        // depend on JVM identity hashes and is not a reproducible Vanilla ordering contract.
        if self.entries.len()
            > self.buckets.len() * JAVA_HASH_MAP_LOAD_FACTOR_NUMERATOR
                / JAVA_HASH_MAP_LOAD_FACTOR_DENOMINATOR
        {
            self.resize();
        }
        true
    }

    #[cfg(test)]
    pub(super) const fn bucket_count(&self) -> usize {
        self.buckets.len()
    }

    fn resize(&mut self) {
        let new_capacity = self.buckets.len().saturating_mul(2);
        if new_capacity == self.buckets.len() {
            return;
        }
        let resized = vec![JavaBlockPosBucket::EMPTY; new_capacity];
        let old_buckets = mem::replace(&mut self.buckets, resized);
        for bucket in old_buckets {
            let mut current = bucket.head;
            while current != JAVA_BLOCK_POS_SET_EMPTY_INDEX {
                let entry_index = current as usize;
                let next = self.entries[entry_index].next;
                let index = java_block_pos_bucket(self.entries[entry_index].pos, new_capacity);
                let new_bucket = self.buckets[index];
                self.entries[entry_index].next = JAVA_BLOCK_POS_SET_EMPTY_INDEX;
                if new_bucket.tail == JAVA_BLOCK_POS_SET_EMPTY_INDEX {
                    self.buckets[index] = JavaBlockPosBucket {
                        head: current,
                        tail: current,
                    };
                } else {
                    self.entries[new_bucket.tail as usize].next = current;
                    self.buckets[index].tail = current;
                }
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
            while current != JAVA_BLOCK_POS_SET_EMPTY_INDEX {
                let entry = &self.entries[current as usize];
                ordered.push(entry.pos);
                current = entry.next;
            }
        }
        ordered.into_iter()
    }
}

const fn java_block_pos_bucket(pos: BlockPos, capacity: usize) -> usize {
    let hash = pos
        .y()
        .wrapping_add(pos.z().wrapping_mul(JAVA_BLOCK_POS_HASH_MULTIPLIER))
        .wrapping_mul(JAVA_BLOCK_POS_HASH_MULTIPLIER)
        .wrapping_add(pos.x()) as u32;
    let spread = hash ^ (hash >> JAVA_HASH_MAP_SPREAD_SHIFT);
    spread as usize & (capacity - 1)
}
