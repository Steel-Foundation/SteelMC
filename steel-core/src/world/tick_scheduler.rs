//! Scheduled tick system for deterministic block and fluid updates.
//!
//! Unlike random ticks, scheduled ticks fire after a precise delay and respect
//! priority ordering across all chunks. This is used for buttons, repeaters,
//! fluid flow, and other time-dependent block mechanics.
//!
//! ## Differences from Vanilla
//!
//! Vanilla stores an absolute `triggerTick` (game tick number) in memory and
//! converts to a relative `delay` on disk. We use **relative delay counters
//! everywhere** — each game tick decrements the delay by 1, and ticks fire when
//! their delay reaches 0. This prevents inter-chunk desync when chunks
//! load/unload at different times.

use rustc_hash::{FxBuildHasher, FxHashSet};
use std::ptr;
use steel_registry::blocks::BlockRef;
use steel_registry::fluid::FluidRef;
use steel_utils::BlockPos;

// ---------------------------------------------------------------------------
// TickPriority
// ---------------------------------------------------------------------------

/// Priority levels for scheduled ticks. Lower discriminant = higher priority.
///
/// Matches vanilla's `TickPriority` enum. `Ord` is derived so that
/// `ExtremelyHigh < Normal < ExtremelyLow`, which gives correct sort order.
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

// ---------------------------------------------------------------------------
// TickKey trait
// ---------------------------------------------------------------------------

/// Trait for types that can be used as the tick target in `ScheduledTick`.
///
/// Provides a `usize` key for deduplication (one tick per `(BlockPos, key)` pair).
pub trait TickKey: Copy {
    /// Returns a key suitable for dedup hashing.
    ///
    /// For `BlockRef` this is the pointer address (pointer identity).
    /// For `FluidRef` this is also the pointer address.
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

// ---------------------------------------------------------------------------
// ScheduledTick
// ---------------------------------------------------------------------------

/// A single scheduled tick targeting a block or fluid at a specific position.
pub struct ScheduledTick<T: TickKey> {
    /// The block or fluid type this tick targets.
    pub tick_type: T,
    /// The block position to tick.
    pub pos: BlockPos,
    /// Remaining delay in game ticks. Decremented each tick; fires when `<= 0`.
    pub delay: i32,
    /// Execution priority (lower = fires first within the same game tick).
    pub priority: TickPriority,
    /// Monotonic counter for stable ordering within the same priority.
    /// Newly scheduled ticks get positive values from `World::sub_tick_count`.
    /// Loaded ticks get negative values to execute before newly scheduled ones.
    pub sub_tick_order: i64,
}

// ---------------------------------------------------------------------------
// Type aliases
// ---------------------------------------------------------------------------

/// A scheduled tick targeting a block.
pub type BlockTick = ScheduledTick<BlockRef>;
/// A scheduled tick targeting a fluid.
pub type FluidTick = ScheduledTick<FluidRef>;
/// Per-chunk storage for scheduled block ticks.
pub type BlockTickList = TickList<BlockRef>;
/// Per-chunk storage for scheduled fluid ticks.
pub type FluidTickList = TickList<FluidRef>;

// ---------------------------------------------------------------------------
// TickList — per-chunk tick storage
// ---------------------------------------------------------------------------

/// Per-chunk storage for scheduled ticks of one type (block or fluid).
///
/// Maintains a deduplication set so that only one tick per `(BlockPos, type)`
/// pair can be active at a time.
pub struct TickList<T: TickKey> {
    ticks: Vec<ScheduledTick<T>>,
    /// Dedup set keyed by `(BlockPos, TickKey::key())`.
    scheduled: FxHashSet<(BlockPos, usize)>,
}

impl<T: TickKey> TickList<T> {
    /// Creates an empty tick list.
    #[must_use]
    pub fn new() -> Self {
        Self {
            ticks: Vec::new(),
            scheduled: FxHashSet::default(),
        }
    }

    /// Creates a tick list pre-populated with ticks (used when loading from disk).
    #[must_use]
    pub fn from_ticks(ticks: Vec<ScheduledTick<T>>) -> Self {
        let mut scheduled = FxHashSet::with_capacity_and_hasher(ticks.len(), FxBuildHasher);
        for tick in &ticks {
            scheduled.insert((tick.pos, tick.tick_type.key()));
        }
        Self { ticks, scheduled }
    }

    /// Schedules a tick if one isn't already scheduled for the same `(pos, type)`.
    ///
    /// Returns `true` if the tick was added, `false` if a duplicate exists.
    pub fn schedule(&mut self, tick: ScheduledTick<T>) -> bool {
        let key = (tick.pos, tick.tick_type.key());
        if self.scheduled.insert(key) {
            self.ticks.push(tick);
            true
        } else {
            false
        }
    }

    /// Returns `true` if a tick is scheduled for the given `(pos, type)`.
    #[must_use]
    pub fn has_tick(&self, pos: BlockPos, tick_type: T) -> bool {
        self.scheduled.contains(&(pos, tick_type.key()))
    }

    /// Decrements all delays by 1 and drains ticks that are ready (delay <= 0).
    ///
    /// Ready ticks are removed from both the tick vec and the dedup set.
    pub fn drain_ready(&mut self) -> Vec<ScheduledTick<T>> {
        // Decrement all delays
        for tick in &mut self.ticks {
            tick.delay -= 1;
        }

        // Partition: ready ticks have delay <= 0
        let mut ready = Vec::new();
        self.ticks.retain(|tick| {
            if tick.delay <= 0 {
                ready.push(ScheduledTick {
                    tick_type: tick.tick_type,
                    pos: tick.pos,
                    delay: tick.delay,
                    priority: tick.priority,
                    sub_tick_order: tick.sub_tick_order,
                });
                false
            } else {
                true
            }
        });

        // Remove ready ticks from dedup set
        for tick in &ready {
            self.scheduled.remove(&(tick.pos, tick.tick_type.key()));
        }

        ready
    }

    /// Returns the number of scheduled ticks.
    #[must_use]
    pub const fn len(&self) -> usize {
        self.ticks.len()
    }

    /// Returns `true` if no ticks are scheduled.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.ticks.is_empty()
    }

    /// Iterates over all scheduled ticks (for persistence).
    pub fn iter(&self) -> impl Iterator<Item = &ScheduledTick<T>> {
        self.ticks.iter()
    }
}

impl<T: TickKey> Default for TickList<T> {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use steel_registry::blocks::Block;
    use steel_registry::blocks::behaviour::BlockConfig;
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

    #[test]
    fn schedule_adds_tick() {
        let mut list = BlockTickList::new();
        let tick = BlockTick {
            tick_type: test_block(),
            pos: BlockPos::new(1, 2, 3),
            delay: 5,
            priority: TickPriority::Normal,
            sub_tick_order: 0,
        };
        assert!(list.schedule(tick));
        assert_eq!(list.len(), 1);
    }

    #[test]
    fn schedule_deduplicates() {
        let mut list = BlockTickList::new();
        let block = test_block();
        let pos = BlockPos::new(1, 2, 3);

        let tick1 = BlockTick {
            tick_type: block,
            pos,
            delay: 5,
            priority: TickPriority::Normal,
            sub_tick_order: 0,
        };
        let tick2 = BlockTick {
            tick_type: block,
            pos,
            delay: 10,
            priority: TickPriority::High,
            sub_tick_order: 1,
        };

        assert!(list.schedule(tick1));
        assert!(!list.schedule(tick2)); // duplicate
        assert_eq!(list.len(), 1);
    }

    #[test]
    fn different_pos_same_block_not_duplicate() {
        let mut list = BlockTickList::new();
        let block = test_block();

        assert!(list.schedule(BlockTick {
            tick_type: block,
            pos: BlockPos::new(1, 2, 3),
            delay: 5,
            priority: TickPriority::Normal,
            sub_tick_order: 0,
        }));
        assert!(list.schedule(BlockTick {
            tick_type: block,
            pos: BlockPos::new(4, 5, 6),
            delay: 5,
            priority: TickPriority::Normal,
            sub_tick_order: 1,
        }));
        assert_eq!(list.len(), 2);
    }

    #[test]
    fn same_pos_different_block_not_duplicate() {
        let mut list = BlockTickList::new();
        let pos = BlockPos::new(1, 2, 3);

        assert!(list.schedule(BlockTick {
            tick_type: test_block(),
            pos,
            delay: 5,
            priority: TickPriority::Normal,
            sub_tick_order: 0,
        }));
        assert!(list.schedule(BlockTick {
            tick_type: test_block_2(),
            pos,
            delay: 5,
            priority: TickPriority::Normal,
            sub_tick_order: 1,
        }));
        assert_eq!(list.len(), 2);
    }

    #[test]
    fn drain_ready_after_delay() {
        let mut list = BlockTickList::new();
        list.schedule(BlockTick {
            tick_type: test_block(),
            pos: BlockPos::new(0, 0, 0),
            delay: 3,
            priority: TickPriority::Normal,
            sub_tick_order: 0,
        });

        // Tick 1: delay 3 -> 2
        let ready = list.drain_ready();
        assert!(ready.is_empty());
        assert_eq!(list.len(), 1);

        // Tick 2: delay 2 -> 1
        let ready = list.drain_ready();
        assert!(ready.is_empty());

        // Tick 3: delay 1 -> 0, fires
        let ready = list.drain_ready();
        assert_eq!(ready.len(), 1);
        assert_eq!(list.len(), 0);
        assert!(!list.has_tick(BlockPos::new(0, 0, 0), test_block()));
    }

    #[test]
    fn drain_ready_respects_different_delays() {
        let mut list = BlockTickList::new();
        list.schedule(BlockTick {
            tick_type: test_block(),
            pos: BlockPos::new(0, 0, 0),
            delay: 1,
            priority: TickPriority::Normal,
            sub_tick_order: 0,
        });
        list.schedule(BlockTick {
            tick_type: test_block_2(),
            pos: BlockPos::new(0, 0, 0),
            delay: 3,
            priority: TickPriority::Normal,
            sub_tick_order: 1,
        });

        // Tick 1: first fires (delay 1->0), second stays (delay 3->2)
        let ready = list.drain_ready();
        assert_eq!(ready.len(), 1);
        assert_eq!(list.len(), 1);

        // Tick 2: delay 2->1
        let ready = list.drain_ready();
        assert!(ready.is_empty());

        // Tick 3: delay 1->0, fires
        let ready = list.drain_ready();
        assert_eq!(ready.len(), 1);
        assert_eq!(list.len(), 0);
    }

    #[test]
    fn has_tick_works() {
        let mut list = BlockTickList::new();
        let block = test_block();
        let pos = BlockPos::new(1, 2, 3);

        assert!(!list.has_tick(pos, block));
        list.schedule(BlockTick {
            tick_type: block,
            pos,
            delay: 5,
            priority: TickPriority::Normal,
            sub_tick_order: 0,
        });
        assert!(list.has_tick(pos, block));
    }

    #[test]
    fn can_reschedule_after_drain() {
        let mut list = BlockTickList::new();
        let block = test_block();
        let pos = BlockPos::new(0, 0, 0);

        list.schedule(BlockTick {
            tick_type: block,
            pos,
            delay: 1,
            priority: TickPriority::Normal,
            sub_tick_order: 0,
        });

        // Drain it
        let ready = list.drain_ready();
        assert_eq!(ready.len(), 1);

        // Should be able to reschedule
        assert!(list.schedule(BlockTick {
            tick_type: block,
            pos,
            delay: 5,
            priority: TickPriority::Normal,
            sub_tick_order: 1,
        }));
    }

    #[test]
    fn priority_ordering() {
        assert!(TickPriority::ExtremelyHigh < TickPriority::Normal);
        assert!(TickPriority::Normal < TickPriority::ExtremelyLow);
        assert!(TickPriority::High < TickPriority::Low);
    }
}
