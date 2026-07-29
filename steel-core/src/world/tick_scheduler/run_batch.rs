use super::{
    AtomicOrdering, AtomicUsize, BlockPos, FxHashMap, OnceLock, ScheduledTick, ScheduledTickKey,
    TickKey,
};

/// Immutable ticks selected for one execution phase with a lazily built lookup index.
///
/// Vanilla creates its `willTickThisTick` hash set only on the first query. The executor advances
/// `next_index` before each callback, so a materialized key index needs no per-callback removal.
#[derive(Debug)]
pub(crate) struct ScheduledTickRunBatch<T: TickKey> {
    ticks: Vec<ScheduledTick<T>>,
    next_index: AtomicUsize,
    lookup: OnceLock<FxHashMap<ScheduledTickKey, usize>>,
}

impl<T: TickKey> ScheduledTickRunBatch<T> {
    #[must_use]
    pub(crate) const fn new(ticks: Vec<ScheduledTick<T>>) -> Self {
        Self {
            ticks,
            next_index: AtomicUsize::new(0),
            lookup: OnceLock::new(),
        }
    }

    #[must_use]
    pub(crate) fn ticks(&self) -> &[ScheduledTick<T>] {
        &self.ticks
    }

    pub(crate) fn start(&self, index: usize) {
        assert!(
            index < self.ticks.len(),
            "scheduled-tick batch index out of bounds"
        );
        self.next_index.store(index + 1, AtomicOrdering::Relaxed);
    }

    #[must_use]
    pub(crate) fn contains(&self, pos: BlockPos, tick_type: T) -> bool {
        let initial_index = self.next_index.load(AtomicOrdering::Relaxed);
        if initial_index >= self.ticks.len() {
            return false;
        }
        let lookup = self.lookup.get_or_init(|| {
            self.ticks[initial_index..]
                .iter()
                .enumerate()
                .map(|(index, tick)| (tick.key(), initial_index + index))
                .collect()
        });
        let next_index = self.next_index.load(AtomicOrdering::Relaxed);
        lookup
            .get(&(pos, tick_type.key()))
            .is_some_and(|&index| index >= next_index)
    }

    #[cfg(test)]
    pub(super) fn lookup_is_initialized(&self) -> bool {
        self.lookup.get().is_some()
    }
}
