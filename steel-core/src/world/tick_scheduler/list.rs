use super::{
    BinaryHeap, BlockPos, FxHashSet, Ordering, SavedTick, ScheduledTick, ScheduledTickKey, TickKey,
    TickPriority,
};

impl<T: TickKey> ScheduledTick<T> {
    /// Returns the position/type identity used to deduplicate this tick.
    #[must_use]
    pub fn key(&self) -> ScheduledTickKey {
        (self.pos, self.tick_type.key())
    }

    fn drain_order(&self, other: &Self) -> Ordering {
        self.trigger_tick.cmp(&other.trigger_tick).then_with(|| {
            intra_tick_drain_order(
                self.priority,
                self.sub_tick_order,
                other.priority,
                other.sub_tick_order,
            )
        })
    }
}

pub(super) fn intra_tick_drain_order(
    left_priority: TickPriority,
    left_sub_tick_order: i64,
    right_priority: TickPriority,
    right_sub_tick_order: i64,
) -> Ordering {
    left_priority
        .cmp(&right_priority)
        .then_with(|| left_sub_tick_order.cmp(&right_sub_tick_order))
}

#[derive(Debug)]
struct QueuedTick<T: TickKey> {
    tick: ScheduledTick<T>,
    insertion_order: u64,
}

impl<T: TickKey> PartialEq for QueuedTick<T> {
    fn eq(&self, other: &Self) -> bool {
        self.tick.drain_order(&other.tick) == Ordering::Equal
            && self.insertion_order == other.insertion_order
    }
}

impl<T: TickKey> Eq for QueuedTick<T> {}

impl<T: TickKey> PartialOrd for QueuedTick<T> {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl<T: TickKey> Ord for QueuedTick<T> {
    fn cmp(&self, other: &Self) -> Ordering {
        self.tick
            .drain_order(&other.tick)
            .reverse()
            .then_with(|| other.insertion_order.cmp(&self.insertion_order))
    }
}

/// Per-chunk storage for scheduled ticks of one type (block or fluid).
///
/// Saved and proto-chunk entries remain in `pending_ticks` until the chunk first
/// reaches block-ticking readiness. Live entries use absolute game-time deadlines.
/// A priority queue keeps live work ordered without scanning every tick.
#[derive(Debug)]
pub struct TickList<T: TickKey> {
    pending_ticks: Option<Vec<SavedTick<T>>>,
    ticks: BinaryHeap<QueuedTick<T>>,
    scheduled: FxHashSet<ScheduledTickKey>,
    next_insertion_order: u64,
}

pub(super) struct TickListPackingSnapshot<T: TickKey> {
    pending_ticks: Vec<SavedTick<T>>,
    live_ticks: Vec<(ScheduledTick<T>, u64)>,
}

impl<T: TickKey> TickListPackingSnapshot<T> {
    pub(super) fn pack(mut self, current_tick: i64) -> Vec<SavedTick<T>> {
        self.live_ticks.sort_by(|left, right| {
            left.0
                .sub_tick_order
                .cmp(&right.0.sub_tick_order)
                .then_with(|| left.1.cmp(&right.1))
        });
        self.pending_ticks
            .extend(self.live_ticks.into_iter().map(|(tick, _)| SavedTick {
                tick_type: tick.tick_type,
                pos: tick.pos,
                delay: tick.trigger_tick.wrapping_sub(current_tick) as i32,
                priority: tick.priority,
            }));
        self.pending_ticks
    }
}

impl<T: TickKey> TickList<T> {
    /// Creates an empty tick list.
    #[must_use]
    pub fn new() -> Self {
        Self {
            pending_ticks: None,
            ticks: BinaryHeap::new(),
            scheduled: FxHashSet::default(),
            next_insertion_order: 0,
        }
    }

    /// Creates an empty proto-chunk list whose entries remain relative until
    /// the promoted Full chunk first becomes block-ticking.
    #[must_use]
    pub(crate) fn new_pending() -> Self {
        Self {
            pending_ticks: Some(Vec::new()),
            ticks: BinaryHeap::new(),
            scheduled: FxHashSet::default(),
            next_insertion_order: 0,
        }
    }

    /// Creates a tick list from relative-delay ticks loaded from chunk storage.
    ///
    /// Vanilla assigns loaded entries the range `-len..-1` in saved list order,
    /// ensuring they execute before newly scheduled entries with equal timing
    /// once the list is unpacked.
    #[must_use]
    pub(crate) fn from_saved_ticks(saved_ticks: Vec<SavedTick<T>>) -> Self {
        let mut result = Self::new_pending();
        result.scheduled.reserve(saved_ticks.len());
        for saved_tick in &saved_ticks {
            result
                .scheduled
                .insert((saved_tick.pos, saved_tick.tick_type.key()));
        }
        result.pending_ticks = Some(saved_ticks);
        result
    }

    /// Creates a proto-chunk tick list from relative-delay storage entries.
    ///
    /// `ProtoChunkTicks.load` schedules saved entries individually, so duplicate
    /// `(pos, type)` keys are discarded while preserving the first entry. Full
    /// chunk loading intentionally uses [`Self::from_saved_ticks`] instead because
    /// `LevelChunkTicks` retains its saved list exactly as stored.
    #[must_use]
    pub(crate) fn from_proto_saved_ticks(saved_ticks: Vec<SavedTick<T>>) -> Self {
        let mut result = Self::new_pending();
        result.scheduled.reserve(saved_ticks.len());
        for saved_tick in saved_ticks {
            result.schedule_saved_pending(saved_tick);
        }
        result
    }

    /// Schedules a live tick with an absolute world game-time deadline.
    ///
    /// Returns `true` if the tick was added, or `false` when the same `(pos, type)`
    /// is already scheduled.
    pub(crate) fn schedule(
        &mut self,
        tick_type: T,
        pos: BlockPos,
        trigger_tick: i64,
        priority: TickPriority,
        sub_tick_order: i64,
    ) -> bool {
        let key = (pos, tick_type.key());
        if !self.scheduled.insert(key) {
            return false;
        }

        self.push_unchecked(ScheduledTick {
            tick_type,
            pos,
            trigger_tick,
            priority,
            sub_tick_order,
        });
        true
    }

    /// Stores a proto-chunk tick with Vanilla's fixed zero delay.
    pub(crate) fn schedule_pending(
        &mut self,
        tick_type: T,
        pos: BlockPos,
        priority: TickPriority,
    ) -> bool {
        self.schedule_saved_pending(SavedTick {
            tick_type,
            pos,
            delay: 0,
            priority,
        })
    }

    fn schedule_saved_pending(&mut self, saved_tick: SavedTick<T>) -> bool {
        let key = (saved_tick.pos, saved_tick.tick_type.key());
        if !self.scheduled.insert(key) {
            return false;
        }
        let pending_ticks = self.pending_ticks.get_or_insert_default();
        pending_ticks.push(saved_tick);
        true
    }

    /// Returns `true` if a tick is scheduled for the given `(pos, type)`.
    #[must_use]
    pub(crate) fn has_tick(&self, pos: BlockPos, tick_type: T) -> bool {
        self.scheduled.contains(&(pos, tick_type.key()))
    }

    /// Returns the saved entries that have not yet been anchored to game time.
    #[must_use]
    pub(crate) fn pending_entries(&self) -> &[SavedTick<T>] {
        self.pending_ticks.as_deref().unwrap_or_default()
    }

    /// Removes pending entries matching `predicate` while keeping deduplication in sync.
    pub(crate) fn remove_pending_matching(
        &mut self,
        mut predicate: impl FnMut(&SavedTick<T>) -> bool,
    ) -> usize {
        let Self {
            pending_ticks,
            scheduled,
            ..
        } = self;
        let Some(pending_ticks) = pending_ticks.as_mut() else {
            return 0;
        };

        let old_len = pending_ticks.len();
        pending_ticks.retain(|tick| {
            if !predicate(tick) {
                return true;
            }

            scheduled.remove(&(tick.pos, tick.tick_type.key()));
            false
        });
        old_len - pending_ticks.len()
    }

    /// Packs pending entries followed by live entries in Vanilla saved-list order.
    #[must_use]
    pub(crate) fn pack(&self, current_tick: i64) -> Vec<SavedTick<T>> {
        self.packing_snapshot().pack(current_tick)
    }

    pub(super) fn packing_snapshot(&self) -> TickListPackingSnapshot<T> {
        let mut pending_ticks = Vec::with_capacity(self.len());
        if let Some(pending) = &self.pending_ticks {
            pending_ticks.extend_from_slice(pending);
        }
        let live_ticks = self
            .ticks
            .iter()
            .map(|queued| (queued.tick, queued.insertion_order))
            .collect();
        TickListPackingSnapshot {
            pending_ticks,
            live_ticks,
        }
    }

    /// Converts pending saved/proto ticks into live absolute-time ordering.
    ///
    /// This mirrors `LevelChunkTicks.unpack`: delays are anchored to `current_tick`
    /// and entries receive negative sub-tick orders in saved-list order. Repeated
    /// calls are no-ops, so later readiness changes cannot re-anchor deadlines.
    pub(crate) fn unpack(&mut self, current_tick: i64) {
        let Some(pending_ticks) = self.pending_ticks.take() else {
            return;
        };
        let tick_count = pending_ticks.len() as i64;
        self.ticks.reserve(pending_ticks.len());
        for (index, saved_tick) in pending_ticks.into_iter().enumerate() {
            self.push_unchecked(ScheduledTick {
                tick_type: saved_tick.tick_type,
                pos: saved_tick.pos,
                trigger_tick: current_tick.wrapping_add(i64::from(saved_tick.delay)),
                priority: saved_tick.priority,
                sub_tick_order: -tick_count + index as i64,
            });
        }
    }

    /// Returns the number of scheduled ticks.
    #[must_use]
    pub(crate) fn len(&self) -> usize {
        self.ticks.len() + self.pending_ticks.as_ref().map_or(0, Vec::len)
    }

    fn push_unchecked(&mut self, tick: ScheduledTick<T>) {
        let insertion_order = self.next_insertion_order;
        self.next_insertion_order = self.next_insertion_order.wrapping_add(1);
        self.ticks.push(QueuedTick {
            tick,
            insertion_order,
        });
    }

    pub(super) fn peek(&self) -> Option<ScheduledTick<T>> {
        Some(self.ticks.peek()?.tick)
    }

    pub(super) fn peek_ready(&self, current_tick: i64) -> Option<ScheduledTick<T>> {
        let tick = self.ticks.peek()?.tick;
        (tick.trigger_tick <= current_tick).then_some(tick)
    }

    pub(super) fn pop_ready(&mut self, current_tick: i64) -> Option<ScheduledTick<T>> {
        self.peek_ready(current_tick)?;
        let tick = self.ticks.pop()?.tick;
        self.scheduled.remove(&tick.key());
        Some(tick)
    }

    #[cfg(test)]
    pub(super) fn drain_ready(&mut self, current_tick: i64) -> Vec<ScheduledTick<T>> {
        let mut ready = Vec::new();
        while let Some(tick) = self.pop_ready(current_tick) {
            ready.push(tick);
        }
        ready
    }
}

impl<T: TickKey> Default for TickList<T> {
    fn default() -> Self {
        Self::new()
    }
}
