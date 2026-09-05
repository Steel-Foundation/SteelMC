use smallvec::SmallVec;

use super::prelude::*;

const INLINE_CAPACITY: usize = 32;

/// Block-position set for feature code that vanilla models as `HashSet<BlockPos>`.
///
/// Java `HashSet` iteration order is implementation-defined, so the extractor normalizes these
/// worldgen sets to insertion order. Steel follows that deterministic oracle instead of depending
/// on JVM bucket ordering.
#[derive(Default)]
pub(super) struct JavaBlockPosSet {
    entries: SmallVec<[BlockPos; INLINE_CAPACITY]>,
}

impl JavaBlockPosSet {
    pub(super) fn insert(&mut self, pos: BlockPos) -> bool {
        if self.entries.contains(&pos) {
            return false;
        }

        self.entries.push(pos);
        true
    }

    pub(super) fn contains(&self, pos: BlockPos) -> bool {
        self.entries.contains(&pos)
    }

    pub(super) fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub(super) fn insertion_order(&self) -> impl Iterator<Item = &BlockPos> {
        self.entries.iter()
    }

    pub(super) fn java_ordered_positions(&self) -> Vec<BlockPos> {
        self.entries.to_vec()
    }

    pub(super) fn pop_java_ordered_position(&mut self) -> Option<BlockPos> {
        if self.entries.is_empty() {
            return None;
        }

        Some(self.entries.remove(0))
    }
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
    fn popped_positions_do_not_iterate() {
        let mut set = JavaBlockPosSet::default();
        for x in 0..4 {
            assert!(set.insert(BlockPos::new(x, 0, 0)));
        }

        assert_eq!(
            set.pop_java_ordered_position(),
            Some(BlockPos::new(0, 0, 0))
        );

        assert_eq!(
            set.java_ordered_positions(),
            [
                BlockPos::new(1, 0, 0),
                BlockPos::new(2, 0, 0),
                BlockPos::new(3, 0, 0)
            ]
        );
    }

    #[test]
    fn reinserted_position_uses_new_insertion_position() {
        let mut set = JavaBlockPosSet::default();
        let first = BlockPos::new(1, 0, 0);
        let second = BlockPos::new(17, 0, 0);

        assert!(set.insert(first));
        assert!(set.insert(second));
        assert_eq!(set.pop_java_ordered_position(), Some(first));
        assert!(set.insert(first));

        assert_eq!(set.java_ordered_positions(), [second, first]);
    }

    #[test]
    fn pop_uses_insertion_order() {
        let mut set = JavaBlockPosSet::default();
        let first = BlockPos::new(1, 0, 0);
        let second = BlockPos::new(2, 0, 0);
        assert!(set.insert(first));
        assert!(set.insert(second));

        assert_eq!(set.pop_java_ordered_position(), Some(first));
        assert_eq!(set.pop_java_ordered_position(), Some(second));
        assert_eq!(set.pop_java_ordered_position(), None);
    }
}
