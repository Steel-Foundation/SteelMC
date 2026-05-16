use super::prelude::*;

/// Block-position set for feature code that consumes Java `HashSet` iteration order.
///
/// Vanilla uses `HashSet<BlockPos>` in a few feature paths before consuming random from the
/// resulting iteration order. Rust's hash sets intentionally do not match that order, so this
/// type keeps insertion order for membership and reconstructs Java's final bucket order on demand.
#[derive(Default)]
pub(super) struct JavaBlockPosSet {
    entries: Vec<BlockPos>,
    present: FxHashSet<BlockPos>,
    capacity: usize,
}

impl JavaBlockPosSet {
    pub(super) fn insert(&mut self, pos: BlockPos) -> bool {
        if !self.present.insert(pos) {
            return false;
        }

        self.entries.push(pos);
        self.ensure_capacity();
        true
    }

    pub(super) fn remove(&mut self, pos: BlockPos) -> bool {
        if !self.present.remove(&pos) {
            return false;
        }

        self.entries.retain(|entry| *entry != pos);
        true
    }

    pub(super) fn contains(&self, pos: BlockPos) -> bool {
        self.present.contains(&pos)
    }

    pub(super) fn is_empty(&self) -> bool {
        self.present.is_empty()
    }

    pub(super) fn insertion_order(&self) -> impl Iterator<Item = &BlockPos> {
        self.entries
            .iter()
            .filter(|pos| self.present.contains(*pos))
    }

    pub(super) fn java_ordered_positions(&self) -> Vec<BlockPos> {
        if self.capacity == 0 {
            return Vec::new();
        }

        let mut indexed = self
            .entries
            .iter()
            .copied()
            .enumerate()
            .filter(|&(_, pos)| self.present.contains(&pos))
            .map(|(insertion_index, pos)| {
                (
                    java_hash_bucket(java_block_pos_hash(pos), self.capacity),
                    insertion_index,
                    pos,
                )
            })
            .collect::<Vec<_>>();
        indexed.sort_by_key(|&(bucket, insertion_index, _)| (bucket, insertion_index));
        indexed.into_iter().map(|(_, _, pos)| pos).collect()
    }

    pub(super) fn pop_java_ordered_position(&mut self) -> Option<BlockPos> {
        let pos = self.java_ordered_positions().into_iter().next()?;
        self.remove(pos);
        Some(pos)
    }

    fn ensure_capacity(&mut self) {
        if self.capacity == 0 {
            self.capacity = 16;
        }

        while self.present.len() > java_hash_set_resize_threshold(self.capacity) {
            self.capacity *= 2;
        }
    }
}

#[cfg(test)]
fn java_hash_set_capacity(len: usize) -> usize {
    if len == 0 {
        return 0;
    }

    let min_capacity = len.saturating_mul(4).saturating_add(2) / 3;
    min_capacity.next_power_of_two().max(16)
}

fn java_hash_set_resize_threshold(capacity: usize) -> usize {
    capacity.saturating_mul(3) / 4
}

fn java_hash_bucket(hash: i32, capacity: usize) -> usize {
    let hash = hash as u32;
    let spread = hash ^ (hash >> 16);
    (spread as usize) & (capacity - 1)
}

fn java_block_pos_hash(pos: BlockPos) -> i32 {
    pos.y()
        .wrapping_add(pos.z().wrapping_mul(31))
        .wrapping_mul(31)
        .wrapping_add(pos.x())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn duplicate_positions_keep_first_insertion() {
        let mut set = JavaBlockPosSet::default();
        assert!(set.insert(BlockPos::new(1, 2, 3)));
        assert!(!set.insert(BlockPos::new(1, 2, 3)));
        assert_eq!(set.java_ordered_positions(), [BlockPos::new(1, 2, 3)]);
    }

    #[test]
    fn capacity_matches_default_java_hash_set_thresholds() {
        assert_eq!(java_hash_set_capacity(0), 0);
        assert_eq!(java_hash_set_capacity(1), 16);
        assert_eq!(java_hash_set_capacity(12), 16);
        assert_eq!(java_hash_set_capacity(13), 32);
        assert_eq!(java_hash_set_capacity(24), 32);
        assert_eq!(java_hash_set_capacity(25), 64);
    }

    #[test]
    fn removed_positions_do_not_shrink_java_backing_capacity() {
        let mut set = JavaBlockPosSet::default();
        for x in 0..13 {
            assert!(set.insert(BlockPos::new(x, 0, 0)));
        }
        assert_eq!(set.capacity, 32);

        for x in 0..12 {
            assert!(set.remove(BlockPos::new(x, 0, 0)));
        }

        assert_eq!(set.capacity, 32);
        assert_eq!(set.java_ordered_positions(), [BlockPos::new(12, 0, 0)]);
    }

    #[test]
    fn reinserted_position_uses_new_bucket_chain_order() {
        let mut set = JavaBlockPosSet::default();
        let first = BlockPos::new(1, 0, 0);
        let second = BlockPos::new(17, 0, 0);
        assert_eq!(java_hash_bucket(java_block_pos_hash(first), 16), 1);
        assert_eq!(java_hash_bucket(java_block_pos_hash(second), 16), 1);

        assert!(set.insert(first));
        assert!(set.insert(second));
        assert!(set.remove(first));
        assert!(set.insert(first));

        assert_eq!(set.java_ordered_positions(), [second, first]);
    }
}
