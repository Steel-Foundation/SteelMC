use super::prelude::*;

/// Block-position set for feature code that vanilla models as `HashSet<BlockPos>`.
///
/// Java `HashSet` iteration order is implementation-defined, so the extractor normalizes these
/// worldgen sets to insertion order. Steel follows that deterministic oracle instead of depending
/// on JVM bucket ordering.
#[derive(Default)]
pub(super) struct JavaBlockPosSet {
    entries: Vec<BlockPos>,
    present: FxHashSet<BlockPos>,
}

impl JavaBlockPosSet {
    pub(super) fn insert(&mut self, pos: BlockPos) -> bool {
        if !self.present.insert(pos) {
            return false;
        }

        self.entries.push(pos);
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
        self.insertion_order().copied().collect()
    }

    pub(super) fn pop_java_ordered_position(&mut self) -> Option<BlockPos> {
        let pos = self.java_ordered_positions().into_iter().next()?;
        self.remove(pos);
        Some(pos)
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
    fn removed_positions_do_not_iterate() {
        let mut set = JavaBlockPosSet::default();
        for x in 0..4 {
            assert!(set.insert(BlockPos::new(x, 0, 0)));
        }

        assert!(set.remove(BlockPos::new(1, 0, 0)));

        assert_eq!(
            set.java_ordered_positions(),
            [
                BlockPos::new(0, 0, 0),
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
        assert!(set.remove(first));
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
