//! Scheduled tick storage and selection for deterministic block and fluid updates.
//!
//! Scheduled ticks are stored in per-chunk priority queues. Within a chunk, the
//! queue follows vanilla's `ScheduledTick.DRAIN_ORDER`: trigger time, priority,
//! then sub-tick order. Across chunks, only each ready queue head participates
//! in selection, following vanilla's `LevelTicks` container-draining behavior.
//!
//! ## Intentional difference from Vanilla
//!
//! Vanilla uses the world's absolute game time for in-memory trigger times. Game
//! time continues advancing while a loaded chunk is outside the block-ticking
//! range, so multiple repeater deadlines can become overdue and execute together
//! when the chunk starts ticking again. Steel intentionally advances a chunk's
//! scheduled-tick clock only while `ChunkMap` confirms that chunk is block-ticking.
//! This preserves the spacing and phase of repeater clocks across the loaded but
//! non-ticking zone. Remaining active-time delay is saved with the chunk and
//! re-anchored when loaded.

use std::{cmp::Ordering, collections::BinaryHeap, ptr};

use rustc_hash::FxHashSet;
use steel_registry::blocks::BlockRef;
use steel_registry::fluid::FluidRef;
use steel_utils::BlockPos;

/// Priority levels for scheduled ticks. Lower discriminant = higher priority.
///
/// Matches vanilla's `TickPriority` enum. `Ord` is derived so that
/// `ExtremelyHigh < Normal < ExtremelyLow`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(i8)]
pub enum TickPriority {
    /// Highest priority (-3). Fires before all others.
    ExtremelyHigh = -3,
    /// Very high priority (-2).
    VeryHigh = -2,
    /// High priority (-1).
    High = -1,
    /// Default priority (0).
    Normal = 0,
    /// Low priority (1).
    Low = 1,
    /// Very low priority (2).
    VeryLow = 2,
    /// Lowest priority (3). Fires after all others.
    ExtremelyLow = 3,
}

impl TickPriority {
    /// Converts from an `i8` value, returning `None` for out-of-range values.
    #[must_use]
    pub const fn from_i8(value: i8) -> Option<Self> {
        match value {
            -3 => Some(Self::ExtremelyHigh),
            -2 => Some(Self::VeryHigh),
            -1 => Some(Self::High),
            0 => Some(Self::Normal),
            1 => Some(Self::Low),
            2 => Some(Self::VeryLow),
            3 => Some(Self::ExtremelyLow),
            _ => None,
        }
    }
}

/// Trait for types that can be used as the tick target in `ScheduledTick`.
///
/// Provides a `usize` key for deduplication (one tick per `(BlockPos, key)` pair).
pub trait TickKey: Copy {
    /// Returns a key suitable for dedup hashing.
    fn key(self) -> usize;
}

impl TickKey for BlockRef {
    #[inline]
    fn key(self) -> usize {
        ptr::from_ref(self) as usize
    }
}

impl TickKey for FluidRef {
    #[inline]
    fn key(self) -> usize {
        ptr::from_ref(self) as usize
    }
}

/// A single scheduled tick targeting a block or fluid at a specific position.
#[derive(Debug, Clone, Copy)]
pub struct ScheduledTick<T: TickKey> {
    /// The block or fluid type this tick targets.
    pub tick_type: T,
    /// The block position to tick.
    pub pos: BlockPos,
    /// Deadline on the owning container's active-time clock.
    pub trigger_tick: i64,
    /// Execution priority (lower = fires first within the same active tick).
    pub priority: TickPriority,
    /// Monotonic counter for stable ordering within the same priority.
    /// Loaded ticks use negative values and therefore precede newly scheduled ticks.
    pub sub_tick_order: i64,
}

/// A scheduled tick in the chunk persistence representation.
///
/// Like vanilla's `SavedTick`, this stores relative delay but not sub-tick order.
/// Loaded ticks receive negative sub-tick orders in their saved list order.
#[derive(Debug, Clone, Copy)]
pub(crate) struct SavedTick<T: TickKey> {
    /// The block or fluid type this tick targets.
    pub(crate) tick_type: T,
    /// The block position to tick.
    pub(crate) pos: BlockPos,
    /// Remaining active-time delay when the chunk was saved.
    pub(crate) delay: i32,
    /// Execution priority.
    pub(crate) priority: TickPriority,
}

/// A scheduled tick targeting a block.
pub type BlockTick = ScheduledTick<BlockRef>;
/// A scheduled tick targeting a fluid.
pub type FluidTick = ScheduledTick<FluidRef>;
/// Deduplication key used by scheduled tick containers and execution snapshots.
pub type ScheduledTickKey = (BlockPos, usize);
/// Per-chunk storage for scheduled block ticks.
pub type BlockTickList = TickList<BlockRef>;
/// Per-chunk storage for scheduled fluid ticks.
pub type FluidTickList = TickList<FluidRef>;

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

fn intra_tick_drain_order(
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ReadyContainer {
    index: usize,
    priority: TickPriority,
    sub_tick_order: i64,
}

impl ReadyContainer {
    const fn new<T: TickKey>(index: usize, tick: ScheduledTick<T>) -> Self {
        Self {
            index,
            priority: tick.priority,
            sub_tick_order: tick.sub_tick_order,
        }
    }
}

impl PartialOrd for ReadyContainer {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for ReadyContainer {
    fn cmp(&self, other: &Self) -> Ordering {
        intra_tick_drain_order(
            self.priority,
            self.sub_tick_order,
            other.priority,
            other.sub_tick_order,
        )
        .reverse()
        .then_with(|| other.index.cmp(&self.index))
    }
}

/// Per-chunk storage for scheduled ticks of one type (block or fluid).
///
/// The active-time clock advances only when this container's chunk is eligible
/// for block ticks. A priority queue keeps future work ordered without scanning
/// every pending tick each game tick.
#[derive(Debug)]
pub struct TickList<T: TickKey> {
    active_tick: i64,
    ticks: BinaryHeap<QueuedTick<T>>,
    scheduled: FxHashSet<ScheduledTickKey>,
    next_insertion_order: u64,
}

impl<T: TickKey> TickList<T> {
    /// Creates an empty tick list.
    #[must_use]
    pub fn new() -> Self {
        Self {
            active_tick: 0,
            ticks: BinaryHeap::new(),
            scheduled: FxHashSet::default(),
            next_insertion_order: 0,
        }
    }

    /// Creates a tick list from relative-delay ticks loaded from chunk storage.
    ///
    /// Vanilla assigns loaded entries the range `-len..-1` in saved list order,
    /// ensuring they execute before newly scheduled entries with equal timing.
    #[must_use]
    pub(crate) fn from_saved_ticks(saved_ticks: Vec<SavedTick<T>>) -> Self {
        let tick_count = saved_ticks.len() as i64;
        let mut result = Self::new();
        result.ticks.reserve(saved_ticks.len());
        result.scheduled.reserve(saved_ticks.len());

        for (index, saved_tick) in saved_ticks.into_iter().enumerate() {
            let tick = ScheduledTick {
                tick_type: saved_tick.tick_type,
                pos: saved_tick.pos,
                trigger_tick: i64::from(saved_tick.delay),
                priority: saved_tick.priority,
                sub_tick_order: -tick_count + index as i64,
            };
            result.scheduled.insert(tick.key());
            result.push_unchecked(tick);
        }

        result
    }

    /// Schedules a tick relative to this container's current active time.
    ///
    /// Returns `true` if the tick was added, or `false` when the same `(pos, type)`
    /// is already scheduled.
    pub fn schedule(
        &mut self,
        tick_type: T,
        pos: BlockPos,
        delay: i32,
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
            trigger_tick: self.active_tick.wrapping_add(i64::from(delay)),
            priority,
            sub_tick_order,
        });
        true
    }

    /// Returns `true` if a tick is scheduled for the given `(pos, type)`.
    #[must_use]
    pub fn has_tick(&self, pos: BlockPos, tick_type: T) -> bool {
        self.scheduled.contains(&(pos, tick_type.key()))
    }

    /// Packs ticks as relative active-time delays in Vanilla saved-list order.
    #[must_use]
    pub(crate) fn pack(&self) -> Vec<SavedTick<T>> {
        let mut ticks: Vec<_> = self.ticks.iter().collect();
        ticks.sort_by(|a, b| {
            a.tick
                .sub_tick_order
                .cmp(&b.tick.sub_tick_order)
                .then_with(|| a.insertion_order.cmp(&b.insertion_order))
        });

        ticks
            .into_iter()
            .map(|queued| SavedTick {
                tick_type: queued.tick.tick_type,
                pos: queued.tick.pos,
                delay: queued.tick.trigger_tick.wrapping_sub(self.active_tick) as i32,
                priority: queued.tick.priority,
            })
            .collect()
    }

    /// Converts proto-chunk pending ticks into live loaded-tick ordering.
    ///
    /// This mirrors `LevelChunkTicks.unpack`: remaining delays are re-anchored and
    /// all entries receive negative sub-tick orders in their packed list order.
    pub(crate) fn unpack(&mut self) {
        let saved_ticks = self.pack();
        *self = Self::from_saved_ticks(saved_ticks);
    }

    /// Returns the number of scheduled ticks.
    #[must_use]
    pub fn len(&self) -> usize {
        self.ticks.len()
    }

    /// Returns `true` if no ticks are scheduled.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.ticks.is_empty()
    }

    fn push_unchecked(&mut self, tick: ScheduledTick<T>) {
        let insertion_order = self.next_insertion_order;
        self.next_insertion_order = self.next_insertion_order.wrapping_add(1);
        self.ticks.push(QueuedTick {
            tick,
            insertion_order,
        });
    }

    const fn advance_active_time(&mut self) {
        self.active_tick = self.active_tick.wrapping_add(1);
    }

    fn peek_ready(&self) -> Option<ScheduledTick<T>> {
        let tick = self.ticks.peek()?.tick;
        (tick.trigger_tick <= self.active_tick).then_some(tick)
    }

    fn pop_ready(&mut self) -> Option<ScheduledTick<T>> {
        self.peek_ready()?;
        let tick = self.ticks.pop()?.tick;
        self.scheduled.remove(&tick.key());
        Some(tick)
    }

    #[cfg(test)]
    fn drain_ready(&mut self) -> Vec<ScheduledTick<T>> {
        self.advance_active_time();
        let mut ready = Vec::new();
        while let Some(tick) = self.pop_ready() {
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

pub(crate) struct CollectedTicks<T: TickKey> {
    pub(crate) ticks: Vec<ScheduledTick<T>>,
    pub(crate) changed_containers: Vec<usize>,
}

/// Advances the active-time clock of each eligible container once.
///
/// Returns the indices of non-empty containers whose packed remaining delays
/// changed and therefore need to be persisted.
pub(crate) fn advance_tick_containers<T: TickKey>(
    containers: &mut [&mut TickList<T>],
) -> Vec<usize> {
    let mut changed_containers = Vec::new();
    for (index, container) in containers.iter_mut().enumerate() {
        if !container.is_empty() {
            changed_containers.push(index);
        }
        container.advance_active_time();
    }
    changed_containers
}

/// Selects at most `max_ticks` ready entries from already-advanced containers.
///
/// Only queue heads compete globally. Revealing the next head after each pop is
/// what preserves Vanilla's per-chunk deadline ordering when several ticks are
/// already overdue.
pub(crate) fn collect_ticks_to_run<T: TickKey>(
    containers: &mut [&mut TickList<T>],
    max_ticks: usize,
) -> CollectedTicks<T> {
    let mut ready_containers = BinaryHeap::with_capacity(containers.len());
    for (index, container) in containers.iter_mut().enumerate() {
        if let Some(tick) = container.peek_ready() {
            ready_containers.push(ReadyContainer::new(index, tick));
        }
    }

    let mut ticks = Vec::with_capacity(max_ticks.min(ready_containers.len()));
    let mut changed = vec![false; containers.len()];
    while ticks.len() < max_ticks {
        let Some(ready_container) = ready_containers.pop() else {
            break;
        };
        let container = &mut *containers[ready_container.index];
        let Some(tick) = container.pop_ready() else {
            continue;
        };

        changed[ready_container.index] = true;
        ticks.push(tick);

        // Vanilla keeps draining the current container while its next head is
        // no later in intra-tick order than the best competing container. In
        // particular, an exact tie stays with the current container.
        let next_competing_container = ready_containers.peek().copied();
        while ticks.len() < max_ticks {
            let Some(next_tick) = container.peek_ready() else {
                break;
            };
            if next_competing_container.is_some_and(|competitor| {
                intra_tick_drain_order(
                    next_tick.priority,
                    next_tick.sub_tick_order,
                    competitor.priority,
                    competitor.sub_tick_order,
                ) == Ordering::Greater
            }) {
                break;
            }
            let Some(next_tick) = container.pop_ready() else {
                break;
            };
            ticks.push(next_tick);
        }

        if ticks.len() < max_ticks
            && let Some(next_tick) = container.peek_ready()
        {
            ready_containers.push(ReadyContainer::new(ready_container.index, next_tick));
        }
    }

    let changed_containers = changed
        .into_iter()
        .enumerate()
        .filter_map(|(index, changed)| changed.then_some(index))
        .collect();
    CollectedTicks {
        ticks,
        changed_containers,
    }
}

/// Remaining ticks in the currently collected execution snapshot.
///
/// Vanilla removes a tick from this set immediately before its callback. Earlier
/// callbacks can therefore detect a later tick selected for the same game tick.
#[derive(Debug, Default)]
pub(crate) struct ScheduledTickRunSet {
    remaining: FxHashSet<ScheduledTickKey>,
}

impl ScheduledTickRunSet {
    pub(crate) fn begin<T: TickKey>(&mut self, ticks: &[ScheduledTick<T>]) {
        self.remaining.clear();
        self.remaining.extend(ticks.iter().map(ScheduledTick::key));
    }

    pub(crate) fn start<T: TickKey>(&mut self, tick: &ScheduledTick<T>) {
        self.remaining.remove(&tick.key());
    }

    #[must_use]
    pub(crate) fn contains<T: TickKey>(&self, pos: BlockPos, tick_type: T) -> bool {
        self.remaining.contains(&(pos, tick_type.key()))
    }

    pub(crate) fn clear(&mut self) {
        self.remaining.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use steel_registry::blocks::Block;
    use steel_registry::blocks::behavior::BlockConfig;
    use steel_utils::Identifier;

    fn test_block() -> BlockRef {
        static BLOCK: Block = Block::new(
            Identifier::vanilla_static("test_block"),
            BlockConfig::new(),
            &[],
        );
        &BLOCK
    }

    fn test_block_2() -> BlockRef {
        static BLOCK: Block = Block::new(
            Identifier::vanilla_static("test_block_2"),
            BlockConfig::new(),
            &[],
        );
        &BLOCK
    }

    fn schedule(
        list: &mut BlockTickList,
        block: BlockRef,
        pos: BlockPos,
        delay: i32,
        priority: TickPriority,
        sub_tick_order: i64,
    ) -> bool {
        list.schedule(block, pos, delay, priority, sub_tick_order)
    }

    #[test]
    fn schedule_deduplicates_by_position_and_type() {
        let mut list = BlockTickList::new();
        let block = test_block();
        let pos = BlockPos::new(1, 2, 3);

        assert!(schedule(&mut list, block, pos, 5, TickPriority::Normal, 0));
        assert!(!schedule(&mut list, block, pos, 10, TickPriority::High, 1));
        assert!(schedule(
            &mut list,
            test_block_2(),
            pos,
            5,
            TickPriority::Normal,
            2
        ));
        assert!(schedule(
            &mut list,
            block,
            BlockPos::new(4, 5, 6),
            5,
            TickPriority::Normal,
            3
        ));
        assert_eq!(list.len(), 3);
    }

    #[test]
    fn active_time_preserves_spacing_across_a_pause() {
        let mut list = BlockTickList::new();
        let first_pos = BlockPos::new(0, 0, 0);
        let fourth_pos = BlockPos::new(1, 0, 0);
        assert!(schedule(
            &mut list,
            test_block(),
            first_pos,
            1,
            TickPriority::Normal,
            0
        ));
        assert!(schedule(
            &mut list,
            test_block(),
            fourth_pos,
            4,
            TickPriority::Normal,
            1
        ));

        assert_eq!(list.drain_ready()[0].pos, first_pos);

        // No call while the chunk is outside the block-ticking range: its clock is paused.
        assert!(list.drain_ready().is_empty());
        assert!(list.drain_ready().is_empty());
        assert_eq!(list.drain_ready()[0].pos, fourth_pos);
    }

    #[test]
    fn global_cap_retains_ready_overflow() {
        let mut list = BlockTickList::new();
        let high_pos = BlockPos::new(0, 0, 0);
        let normal_pos = BlockPos::new(1, 0, 0);
        let overflow_pos = BlockPos::new(2, 0, 0);
        for (pos, priority, order) in [
            (overflow_pos, TickPriority::Normal, 10),
            (high_pos, TickPriority::High, 20),
            (normal_pos, TickPriority::Normal, 5),
        ] {
            assert!(schedule(&mut list, test_block(), pos, 1, priority, order));
        }

        let mut containers = [&mut list];
        assert_eq!(advance_tick_containers(&mut containers), vec![0]);
        let selected = collect_ticks_to_run(&mut containers, 2);
        assert_eq!(
            selected
                .ticks
                .iter()
                .map(|tick| tick.pos)
                .collect::<Vec<_>>(),
            vec![high_pos, normal_pos]
        );
        assert_eq!(selected.changed_containers, vec![0]);
        assert!(list.has_tick(overflow_pos, test_block()));

        let mut containers = [&mut list];
        assert_eq!(advance_tick_containers(&mut containers), vec![0]);
        let selected = collect_ticks_to_run(&mut containers, 2);
        assert_eq!(selected.ticks.len(), 1);
        assert_eq!(selected.ticks[0].pos, overflow_pos);
    }

    #[test]
    fn scheduling_after_clock_advance_anchors_to_the_current_active_tick() {
        let mut list = BlockTickList::new();
        {
            let mut containers = [&mut list];
            assert!(advance_tick_containers(&mut containers).is_empty());
        }

        let immediate_pos = BlockPos::new(0, 0, 0);
        let delayed_pos = BlockPos::new(1, 0, 0);
        assert!(schedule(
            &mut list,
            test_block(),
            immediate_pos,
            0,
            TickPriority::Normal,
            0
        ));
        assert!(schedule(
            &mut list,
            test_block(),
            delayed_pos,
            1,
            TickPriority::Normal,
            1
        ));

        let mut containers = [&mut list];
        let selected = collect_ticks_to_run(&mut containers, 2);
        assert_eq!(selected.ticks.len(), 1);
        assert_eq!(selected.ticks[0].pos, immediate_pos);

        assert_eq!(advance_tick_containers(&mut containers), vec![0]);
        let selected = collect_ticks_to_run(&mut containers, 2);
        assert_eq!(selected.ticks.len(), 1);
        assert_eq!(selected.ticks[0].pos, delayed_pos);
    }

    #[test]
    fn selection_respects_each_chunks_deadline_head() {
        let mut first_chunk = BlockTickList::new();
        let mut second_chunk = BlockTickList::new();
        let old_low_pos = BlockPos::new(0, 0, 0);
        let later_high_pos = BlockPos::new(1, 0, 0);
        let other_normal_pos = BlockPos::new(16, 0, 0);

        assert!(schedule(
            &mut first_chunk,
            test_block(),
            old_low_pos,
            1,
            TickPriority::Low,
            0
        ));
        assert!(schedule(
            &mut first_chunk,
            test_block(),
            later_high_pos,
            2,
            TickPriority::ExtremelyHigh,
            1
        ));
        assert!(schedule(
            &mut second_chunk,
            test_block(),
            other_normal_pos,
            1,
            TickPriority::Normal,
            2
        ));

        let mut containers = [&mut first_chunk, &mut second_chunk];
        assert_eq!(advance_tick_containers(&mut containers), vec![0, 1]);
        // Leave the first due heads queued so that all three are overdue next active tick.
        assert!(collect_ticks_to_run(&mut containers, 0).ticks.is_empty());

        let mut containers = [&mut first_chunk, &mut second_chunk];
        assert_eq!(advance_tick_containers(&mut containers), vec![0, 1]);
        let selected = collect_ticks_to_run(&mut containers, 3);
        assert_eq!(
            selected
                .ticks
                .iter()
                .map(|tick| tick.pos)
                .collect::<Vec<_>>(),
            vec![other_normal_pos, old_low_pos, later_high_pos]
        );
    }

    #[test]
    fn exact_intra_tick_ties_keep_draining_the_current_chunk() {
        let current_high_pos = BlockPos::new(16, 0, 0);
        let current_normal_pos = BlockPos::new(17, 0, 0);
        let competing_normal_pos = BlockPos::new(0, 0, 0);
        let mut current_chunk = BlockTickList::from_saved_ticks(vec![
            SavedTick {
                tick_type: test_block(),
                pos: current_high_pos,
                delay: 1,
                priority: TickPriority::High,
            },
            SavedTick {
                tick_type: test_block(),
                pos: current_normal_pos,
                delay: 1,
                priority: TickPriority::Normal,
            },
        ]);
        let mut competing_chunk = BlockTickList::from_saved_ticks(vec![SavedTick {
            tick_type: test_block(),
            pos: competing_normal_pos,
            delay: 1,
            priority: TickPriority::Normal,
        }]);

        // Put the competitor first so its container-index tie-break would win if
        // the current container were reinserted after every pop.
        let mut containers = [&mut competing_chunk, &mut current_chunk];
        assert_eq!(advance_tick_containers(&mut containers), vec![0, 1]);
        let selected = collect_ticks_to_run(&mut containers, 3);

        assert_eq!(
            selected
                .ticks
                .iter()
                .map(|tick| tick.pos)
                .collect::<Vec<_>>(),
            vec![current_high_pos, current_normal_pos, competing_normal_pos]
        );
    }

    #[test]
    fn advancing_pending_ticks_reports_a_persistence_change() {
        let mut empty = BlockTickList::new();
        let mut pending = BlockTickList::new();
        assert!(schedule(
            &mut pending,
            test_block(),
            BlockPos::new(0, 0, 0),
            3,
            TickPriority::Normal,
            0
        ));
        assert_eq!(pending.pack()[0].delay, 3);

        {
            let mut containers = [&mut empty, &mut pending];
            assert_eq!(advance_tick_containers(&mut containers), vec![1]);
        }

        assert_eq!(pending.pack()[0].delay, 2);
    }

    #[test]
    fn persistence_saves_remaining_active_delay_and_rebuilds_loaded_order() {
        let mut list = BlockTickList::new();
        let first_pos = BlockPos::new(0, 0, 0);
        let second_pos = BlockPos::new(1, 0, 0);
        assert!(schedule(
            &mut list,
            test_block(),
            first_pos,
            5,
            TickPriority::Normal,
            100
        ));
        assert!(schedule(
            &mut list,
            test_block(),
            second_pos,
            5,
            TickPriority::Normal,
            101
        ));
        assert!(list.drain_ready().is_empty());
        assert!(list.drain_ready().is_empty());

        let saved = list.pack();
        assert_eq!(
            saved.iter().map(|tick| tick.delay).collect::<Vec<_>>(),
            vec![3, 3]
        );

        let mut loaded = BlockTickList::from_saved_ticks(saved);
        assert!(schedule(
            &mut loaded,
            test_block(),
            BlockPos::new(2, 0, 0),
            3,
            TickPriority::Normal,
            0
        ));
        assert!(loaded.drain_ready().is_empty());
        assert!(loaded.drain_ready().is_empty());
        let ready = loaded.drain_ready();

        assert_eq!(
            ready
                .iter()
                .map(|tick| tick.sub_tick_order)
                .collect::<Vec<_>>(),
            vec![-2, -1, 0]
        );
        assert_eq!(ready[0].pos, first_pos);
        assert_eq!(ready[1].pos, second_pos);
    }

    #[test]
    fn unpack_preserves_proto_tick_insertion_order() {
        let mut proto_ticks = BlockTickList::new();
        let first_pos = BlockPos::new(0, 0, 0);
        let second_pos = BlockPos::new(1, 0, 0);
        assert!(schedule(
            &mut proto_ticks,
            test_block(),
            first_pos,
            0,
            TickPriority::Normal,
            0
        ));
        assert!(schedule(
            &mut proto_ticks,
            test_block(),
            second_pos,
            0,
            TickPriority::Normal,
            0
        ));

        proto_ticks.unpack();
        let ready = proto_ticks.drain_ready();
        assert_eq!(
            ready.iter().map(|tick| tick.pos).collect::<Vec<_>>(),
            vec![first_pos, second_pos]
        );
        assert_eq!(
            ready
                .iter()
                .map(|tick| tick.sub_tick_order)
                .collect::<Vec<_>>(),
            vec![-2, -1]
        );
    }

    #[test]
    fn execution_snapshot_contains_only_ticks_that_have_not_started() {
        let first = BlockTick {
            tick_type: test_block(),
            pos: BlockPos::new(0, 0, 0),
            trigger_tick: 1,
            priority: TickPriority::Normal,
            sub_tick_order: 0,
        };
        let second = BlockTick {
            tick_type: test_block(),
            pos: BlockPos::new(1, 0, 0),
            trigger_tick: 1,
            priority: TickPriority::Normal,
            sub_tick_order: 1,
        };
        let mut run_set = ScheduledTickRunSet::default();
        run_set.begin(&[first, second]);

        assert!(run_set.contains(first.pos, first.tick_type));
        assert!(run_set.contains(second.pos, second.tick_type));
        run_set.start(&first);
        assert!(!run_set.contains(first.pos, first.tick_type));
        assert!(run_set.contains(second.pos, second.tick_type));
        run_set.clear();
        assert!(!run_set.contains(second.pos, second.tick_type));
    }

    #[test]
    fn can_reschedule_after_ready_tick_is_removed() {
        let mut list = BlockTickList::new();
        let block = test_block();
        let pos = BlockPos::new(0, 0, 0);
        assert!(schedule(&mut list, block, pos, 1, TickPriority::Normal, 0));
        assert_eq!(list.drain_ready().len(), 1);
        assert!(schedule(&mut list, block, pos, 5, TickPriority::Normal, 1));
    }

    #[test]
    fn priority_ordering_matches_vanilla_discriminants() {
        assert!(TickPriority::ExtremelyHigh < TickPriority::Normal);
        assert!(TickPriority::Normal < TickPriority::ExtremelyLow);
        assert!(TickPriority::High < TickPriority::Low);
    }
}
