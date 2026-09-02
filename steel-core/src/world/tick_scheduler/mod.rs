//! Scheduled tick storage and selection for deterministic block and fluid updates.
//!
//! Scheduled ticks are stored in per-chunk priority queues against absolute world
//! game time. Within a chunk, the queue follows vanilla's
//! `ScheduledTick.DRAIN_ORDER`: trigger time, priority, then sub-tick order. Across
//! chunks, only each ready queue head participates in selection, following
//! vanilla's `LevelTicks` container-draining behavior.
//!
//! Saved and proto-chunk ticks remain pending until the chunk first reaches
//! confirmed block-ticking readiness. That transition anchors their saved delays
//! to the current game time, matching `LevelChunkTicks.unpack`. Later readiness
//! demotions do not pause or re-anchor those deadlines.
//!
//! ## Exact cross-chunk ties
//!
//! Each loaded chunk reconstructs saved ticks with its own negative sub-tick
//! order range, so two ready chunk heads can have the same priority and
//! sub-tick order. A `WorldGenRegion` also owns an independent counter, as in
//! Vanilla, and can retain that order when it schedules directly into an
//! already-Full dependency chunk. Vanilla's final order for these exact ties
//! follows iteration of fastutil's `Long2LongOpenHashMap` and then Java's
//! `PriorityQueue` heap behavior. Minecraft supplies no custom hash strategy
//! for that map. As an intentional performance tradeoff, Steel keeps the
//! optimized `scc` chunk traversal as the final tie order instead of reproducing
//! implementation-specific Java collection state. Ordinary live-world ticks
//! still use a world-global sub-tick counter.
//!
//! ## Exact intra-chunk ties
//!
//! Vanilla's `LevelChunkTicks` comparator leaves ticks with identical trigger
//! time, priority, and sub-tick order equal, so their final order depends on
//! Java collection and priority-queue history. Steel intentionally drains those
//! otherwise indistinguishable ticks in insertion order instead of reproducing
//! that implementation-specific state.

use std::{
    cmp::Ordering,
    collections::{BTreeSet, BinaryHeap},
    ptr,
    sync::{
        Arc, OnceLock,
        atomic::{AtomicI64, AtomicUsize, Ordering as AtomicOrdering},
    },
};

use rustc_hash::{FxHashMap, FxHashSet};
use steel_registry::blocks::BlockRef;
use steel_registry::fluid::FluidRef;
use steel_utils::{
    BlockPos, ChunkPos, PackedChunkPos,
    locks::{SyncMutex, SyncRwLock},
};

use crate::chunk::full_chunk::FullChunkRef;

mod world;

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
    /// Resolves Vanilla's serialized priority value, clamping invalid values to an extreme.
    #[must_use]
    pub const fn by_value(value: i32) -> Self {
        match value {
            -3 => Self::ExtremelyHigh,
            -2 => Self::VeryHigh,
            -1 => Self::High,
            0 => Self::Normal,
            1 => Self::Low,
            2 => Self::VeryLow,
            3 => Self::ExtremelyLow,
            value if value < Self::ExtremelyHigh as i32 => Self::ExtremelyHigh,
            _ => Self::ExtremelyLow,
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
    /// Absolute world game-time deadline.
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
    /// Delay relative to the game time at which the chunk was saved.
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

/// Block and fluid scheduled-tick queues belonging to one chunk.
#[derive(Debug, Default)]
pub(crate) struct ChunkTickLists {
    block: BlockTickList,
    fluid: FluidTickList,
}

impl ChunkTickLists {
    #[must_use]
    pub(crate) const fn new(block: BlockTickList, fluid: FluidTickList) -> Self {
        Self { block, fluid }
    }

    pub(crate) const fn block(&self) -> &BlockTickList {
        &self.block
    }

    pub(crate) const fn block_mut(&mut self) -> &mut BlockTickList {
        &mut self.block
    }

    pub(crate) const fn fluid(&self) -> &FluidTickList {
        &self.fluid
    }

    pub(crate) const fn fluid_mut(&mut self) -> &mut FluidTickList {
        &mut self.fluid
    }

    fn packing_snapshot(&self) -> ChunkTickPackingSnapshot {
        ChunkTickPackingSnapshot {
            block: self.block.packing_snapshot(),
            fluid: self.fluid.packing_snapshot(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ChunkTickContainerLifecycle {
    Proto,
    PrePublication,
    Registered,
    Finalized,
}

#[derive(Debug)]
struct ChunkTickContainerState {
    lifecycle: ChunkTickContainerLifecycle,
    lists: ChunkTickLists,
}

/// Stable scheduled-tick storage owned by one chunk from creation through unload.
///
/// The world scheduler retains a shared handle only while the chunk is registered and indexes
/// its current heads. Persistence reads this container directly, so packing one chunk never
/// retains the world scheduler metadata lock.
#[derive(Debug)]
pub(crate) struct ChunkTickContainer {
    state: SyncMutex<ChunkTickContainerState>,
}

impl ChunkTickContainer {
    #[must_use]
    pub(crate) const fn new(lists: ChunkTickLists) -> Self {
        Self {
            state: SyncMutex::new(ChunkTickContainerState {
                lifecycle: ChunkTickContainerLifecycle::PrePublication,
                lists,
            }),
        }
    }

    #[must_use]
    pub(crate) const fn new_proto(lists: ChunkTickLists) -> Self {
        Self {
            state: SyncMutex::new(ChunkTickContainerState {
                lifecycle: ChunkTickContainerLifecycle::Proto,
                lists,
            }),
        }
    }

    /// Atomically closes proto-only mutations and opens unpublished Full scheduling.
    pub(crate) fn promote_to_full(&self) -> bool {
        let mut state = self.state.lock();
        if state.lifecycle != ChunkTickContainerLifecycle::Proto {
            return false;
        }
        state.lifecycle = ChunkTickContainerLifecycle::PrePublication;
        true
    }

    pub(crate) fn schedule_unregistered_block(
        &self,
        block: BlockRef,
        pos: BlockPos,
        trigger_tick: i64,
        priority: TickPriority,
        sub_tick_order: i64,
    ) -> Option<bool> {
        let mut state = self.state.lock();
        (state.lifecycle == ChunkTickContainerLifecycle::PrePublication).then(|| {
            state
                .lists
                .block_mut()
                .schedule(block, pos, trigger_tick, priority, sub_tick_order)
        })
    }

    pub(crate) fn schedule_unregistered_fluid(
        &self,
        fluid: FluidRef,
        pos: BlockPos,
        trigger_tick: i64,
        priority: TickPriority,
        sub_tick_order: i64,
    ) -> Option<bool> {
        let mut state = self.state.lock();
        (state.lifecycle == ChunkTickContainerLifecycle::PrePublication).then(|| {
            state
                .lists
                .fluid_mut()
                .schedule(fluid, pos, trigger_tick, priority, sub_tick_order)
        })
    }

    pub(crate) fn schedule_pending_block(
        &self,
        block: BlockRef,
        pos: BlockPos,
        priority: TickPriority,
    ) -> Option<bool> {
        let mut state = self.state.lock();
        (state.lifecycle == ChunkTickContainerLifecycle::Proto).then(|| {
            state
                .lists
                .block_mut()
                .schedule_pending(block, pos, priority)
        })
    }

    pub(crate) fn schedule_pending_fluid(
        &self,
        fluid: FluidRef,
        pos: BlockPos,
        priority: TickPriority,
    ) -> Option<bool> {
        let mut state = self.state.lock();
        (state.lifecycle == ChunkTickContainerLifecycle::Proto).then(|| {
            state
                .lists
                .fluid_mut()
                .schedule_pending(fluid, pos, priority)
        })
    }

    pub(crate) fn pending_block_snapshot(&self) -> Option<Vec<SavedTick<BlockRef>>> {
        let state = self.state.lock();
        (state.lifecycle == ChunkTickContainerLifecycle::Proto)
            .then(|| state.lists.block().pending_entries().to_vec())
    }

    #[cfg(test)]
    pub(crate) fn pending_fluid_snapshot(&self) -> Option<Vec<SavedTick<FluidRef>>> {
        let state = self.state.lock();
        (state.lifecycle == ChunkTickContainerLifecycle::Proto)
            .then(|| state.lists.fluid().pending_entries().to_vec())
    }

    pub(crate) fn remove_pending_blocks_matching(
        &self,
        predicate: impl FnMut(&SavedTick<BlockRef>) -> bool,
    ) -> Option<usize> {
        let mut state = self.state.lock();
        (state.lifecycle == ChunkTickContainerLifecycle::Proto)
            .then(|| state.lists.block_mut().remove_pending_matching(predicate))
    }

    pub(crate) fn has_block(&self, pos: BlockPos, block: BlockRef) -> Option<bool> {
        let state = self.state.lock();
        (state.lifecycle != ChunkTickContainerLifecycle::Finalized)
            .then(|| state.lists.block().has_tick(pos, block))
    }

    pub(crate) fn has_fluid(&self, pos: BlockPos, fluid: FluidRef) -> Option<bool> {
        let state = self.state.lock();
        (state.lifecycle != ChunkTickContainerLifecycle::Finalized)
            .then(|| state.lists.fluid().has_tick(pos, fluid))
    }

    pub(crate) fn snapshot(&self, current_tick: i64) -> Option<ScheduledTickSnapshot> {
        let packing = {
            let state = self.state.lock();
            (state.lifecycle != ChunkTickContainerLifecycle::Finalized)
                .then(|| state.lists.packing_snapshot())
        }?;
        Some(packing.pack(current_tick))
    }
}

struct ChunkTickPackingSnapshot {
    block: TickListPackingSnapshot<BlockRef>,
    fluid: TickListPackingSnapshot<FluidRef>,
}

impl ChunkTickPackingSnapshot {
    fn pack(self, current_tick: i64) -> ScheduledTickSnapshot {
        ScheduledTickSnapshot {
            block: self.block.pack(current_tick),
            fluid: self.fluid.pack(current_tick),
        }
    }
}

/// Owned persistence snapshot of both scheduled-tick queues for a Full chunk.
pub(crate) struct ScheduledTickSnapshot {
    pub(crate) block: Vec<SavedTick<BlockRef>>,
    pub(crate) fluid: Vec<SavedTick<FluidRef>>,
}

/// A violated Full-chunk scheduled-tick ownership invariant.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TickSchedulerError {
    /// A second Full chunk attempted to register the same position.
    AlreadyRegistered(ChunkPos),
    /// The required chunk-owned container is unavailable or already finalized.
    MissingContainer(ChunkPos),
    /// The world index and chunk refer to different containers at the same position.
    ContainerMismatch(ChunkPos),
}

/// Ready ticks and the active containers whose persisted delays changed.
pub(crate) struct ScheduledTickBatch<T: TickKey> {
    pub(crate) ticks: Vec<ScheduledTick<T>>,
    pub(crate) changed_containers: Vec<usize>,
}

/// World index for registered Full-chunk scheduled block and fluid containers.
///
/// Chunks own the queues used for persistence. The metadata mutex retains shared handles, current
/// heads, and active deadline sets; packing never acquires it. The phase lock gives collection a
/// single world-wide cutoff without retaining any scheduler lock during callbacks.
mod list;
mod run_batch;
mod scheduler;

use list::{TickList, TickListPackingSnapshot, intra_tick_drain_order};
pub(crate) use run_batch::*;
pub(crate) use scheduler::*;

#[cfg(test)]
mod tests;
