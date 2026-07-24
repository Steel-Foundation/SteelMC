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

use crate::chunk::level_chunk::LevelChunk;

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

/// Block and fluid scheduled-tick queues belonging to one Full chunk.
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
    PrePublication,
    Registered,
    Finalized,
}

#[derive(Debug)]
struct ChunkTickContainerState {
    lifecycle: ChunkTickContainerLifecycle,
    lists: ChunkTickLists,
}

/// Stable scheduled-tick storage owned by one `LevelChunk`.
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
pub(crate) struct WorldTickScheduler {
    next_sub_tick_order: AtomicI64,
    phase: SyncRwLock<()>,
    state: SyncMutex<WorldTickSchedulerState>,
}

#[derive(Debug, Default)]
struct WorldTickSchedulerState {
    chunks: FxHashMap<ChunkPos, RegisteredChunkTicks>,
    active_block_deadlines: BTreeSet<(i64, PackedChunkPos)>,
    active_fluid_deadlines: BTreeSet<(i64, PackedChunkPos)>,
    active_generation: u64,
}

#[derive(Debug)]
struct RegisteredChunkTicks {
    container: Arc<ChunkTickContainer>,
    block_head: Option<i64>,
    fluid_head: Option<i64>,
    active: Option<ActiveChunkRank>,
}

#[derive(Debug, Clone, Copy)]
struct ActiveChunkRank {
    generation: u64,
    rank: usize,
}

#[derive(Clone, Copy)]
enum TickKind {
    Block,
    Fluid,
}

struct ActiveTickContainer {
    pos: ChunkPos,
    rank: usize,
    container: Arc<ChunkTickContainer>,
}

struct TickHeadUpdate {
    pos: ChunkPos,
    trigger_tick: Option<i64>,
}

impl RegisteredChunkTicks {
    const fn head(&self, kind: TickKind) -> Option<i64> {
        match kind {
            TickKind::Block => self.block_head,
            TickKind::Fluid => self.fluid_head,
        }
    }

    const fn set_head(&mut self, kind: TickKind, trigger_tick: Option<i64>) {
        match kind {
            TickKind::Block => self.block_head = trigger_tick,
            TickKind::Fluid => self.fluid_head = trigger_tick,
        }
    }

    const fn active_rank(&self, generation: u64) -> Option<usize> {
        match self.active {
            Some(active) if active.generation == generation => Some(active.rank),
            _ => None,
        }
    }
}

impl WorldTickSchedulerState {
    fn advance_active_generation(&mut self) -> u64 {
        let generation = if let Some(generation) = self.active_generation.checked_add(1) {
            generation
        } else {
            for registered in self.chunks.values_mut() {
                registered.active = None;
            }
            1
        };
        self.active_generation = generation;
        generation
    }

    const fn deadlines(&self, kind: TickKind) -> &BTreeSet<(i64, PackedChunkPos)> {
        match kind {
            TickKind::Block => &self.active_block_deadlines,
            TickKind::Fluid => &self.active_fluid_deadlines,
        }
    }

    const fn deadlines_mut(&mut self, kind: TickKind) -> &mut BTreeSet<(i64, PackedChunkPos)> {
        match kind {
            TickKind::Block => &mut self.active_block_deadlines,
            TickKind::Fluid => &mut self.active_fluid_deadlines,
        }
    }

    fn set_head(
        &mut self,
        pos: ChunkPos,
        kind: TickKind,
        trigger_tick: Option<i64>,
    ) -> Result<(), TickSchedulerError> {
        let Some(registered) = self.chunks.get(&pos) else {
            return Err(TickSchedulerError::MissingContainer(pos));
        };
        let active = registered.active_rank(self.active_generation).is_some();
        let previous = registered.head(kind);
        if previous == trigger_tick {
            return Ok(());
        }
        let packed = PackedChunkPos::from(pos);
        if active && let Some(previous) = previous {
            assert!(
                self.deadlines_mut(kind).remove(&(previous, packed)),
                "active scheduled-tick head was absent from its deadline index"
            );
        }
        let Some(registered) = self.chunks.get_mut(&pos) else {
            return Err(TickSchedulerError::MissingContainer(pos));
        };
        registered.set_head(kind, trigger_tick);
        if active && let Some(trigger_tick) = trigger_tick {
            assert!(
                self.deadlines_mut(kind).insert((trigger_tick, packed)),
                "active scheduled-tick deadline was already indexed"
            );
        }
        Ok(())
    }

    fn take_due(&mut self, kind: TickKind, current_tick: i64) -> Vec<ActiveTickContainer> {
        let mut due = Vec::new();
        while let Some((trigger_tick, packed)) = self.deadlines(kind).first().copied() {
            if trigger_tick > current_tick {
                break;
            }
            assert_eq!(
                self.deadlines_mut(kind).pop_first(),
                Some((trigger_tick, packed)),
                "due scheduled-tick deadline changed during collection"
            );
            let pos = packed.to_chunk_pos();
            let Some(registered) = self.chunks.get_mut(&pos) else {
                panic!("active scheduled-tick deadline lost its registered container");
            };
            assert_eq!(
                registered.head(kind),
                Some(trigger_tick),
                "active scheduled-tick deadline diverged from its container head"
            );
            let Some(rank) = registered.active_rank(self.active_generation) else {
                panic!("inactive scheduled-tick container retained an active deadline");
            };
            due.push(ActiveTickContainer {
                pos,
                rank,
                container: Arc::clone(&registered.container),
            });
            registered.set_head(kind, None);
        }
        due
    }
}

impl WorldTickScheduler {
    #[must_use]
    pub(crate) fn new() -> Self {
        Self {
            next_sub_tick_order: AtomicI64::new(0),
            phase: SyncRwLock::new(()),
            state: SyncMutex::new(WorldTickSchedulerState::default()),
        }
    }

    /// Allocates the world-global order before container lookup or deduplication.
    ///
    /// Vanilla creates the `ScheduledTick` before asking `LevelTicks` to store
    /// it, so failed and duplicate scheduling attempts consume an order too.
    pub(crate) fn next_sub_tick_order(&self) -> i64 {
        self.next_sub_tick_order
            .fetch_add(1, AtomicOrdering::Relaxed)
    }

    /// Registers an unpublished Full chunk's stable queues with the world index.
    pub(crate) fn register_chunk(&self, chunk: &LevelChunk) -> Result<(), TickSchedulerError> {
        let _phase = self.phase.read();
        let container = chunk.scheduled_tick_container();
        let mut container_state = container.state.lock();
        if container_state.lifecycle != ChunkTickContainerLifecycle::PrePublication {
            return Err(TickSchedulerError::AlreadyRegistered(chunk.pos));
        }
        let block_head = container_state
            .lists
            .block()
            .peek()
            .map(|tick| tick.trigger_tick);
        let fluid_head = container_state
            .lists
            .fluid()
            .peek()
            .map(|tick| tick.trigger_tick);
        let mut state = self.state.lock();
        if state.chunks.contains_key(&chunk.pos) {
            return Err(TickSchedulerError::AlreadyRegistered(chunk.pos));
        }
        state.chunks.insert(
            chunk.pos,
            RegisteredChunkTicks {
                container: Arc::clone(container),
                block_head,
                fluid_head,
                active: None,
            },
        );
        container_state.lifecycle = ChunkTickContainerLifecycle::Registered;
        Ok(())
    }

    /// Anchors saved/proto delays when a Full chunk first becomes block-ticking.
    ///
    /// `TickList::unpack` is idempotent, so later readiness promotions preserve
    /// the original absolute deadlines.
    pub(crate) fn unpack_chunk(
        &self,
        pos: ChunkPos,
        current_tick: i64,
    ) -> Result<(), TickSchedulerError> {
        let _phase = self.phase.read();
        let container = self
            .state
            .lock()
            .chunks
            .get(&pos)
            .map(|registered| Arc::clone(&registered.container))
            .ok_or(TickSchedulerError::MissingContainer(pos))?;
        let mut container_state = container.state.lock();
        if container_state.lifecycle != ChunkTickContainerLifecycle::Registered {
            return Err(TickSchedulerError::MissingContainer(pos));
        }
        container_state.lists.block_mut().unpack(current_tick);
        container_state.lists.fluid_mut().unpack(current_tick);
        let block_head = container_state
            .lists
            .block()
            .peek()
            .map(|tick| tick.trigger_tick);
        let fluid_head = container_state
            .lists
            .fluid()
            .peek()
            .map(|tick| tick.trigger_tick);
        let mut state = self.state.lock();
        let Some(registered) = state.chunks.get(&pos) else {
            return Err(TickSchedulerError::MissingContainer(pos));
        };
        if !Arc::ptr_eq(&registered.container, &container) {
            return Err(TickSchedulerError::ContainerMismatch(pos));
        }
        state.set_head(pos, TickKind::Block, block_head)?;
        state.set_head(pos, TickKind::Fluid, fluid_head)?;
        Ok(())
    }

    /// Removes a finally-unloaded Full chunk after its last save completed.
    pub(crate) fn unregister_chunk(&self, pos: ChunkPos) {
        let _phase = self.phase.write();
        let registered = {
            let mut state = self.state.lock();
            let active_generation = state.active_generation;
            let registered = state.chunks.remove(&pos);
            if let Some(registered) = &registered
                && registered.active_rank(active_generation).is_some()
            {
                let packed = PackedChunkPos::from(pos);
                if let Some(head) = registered.block_head {
                    assert!(
                        state.active_block_deadlines.remove(&(head, packed)),
                        "unloaded active block-tick head was absent from its deadline index"
                    );
                }
                if let Some(head) = registered.fluid_head {
                    assert!(
                        state.active_fluid_deadlines.remove(&(head, packed)),
                        "unloaded active fluid-tick head was absent from its deadline index"
                    );
                }
            }
            registered
        };
        if let Some(registered) = registered {
            registered.container.state.lock().lifecycle = ChunkTickContainerLifecycle::Finalized;
        }
    }

    #[cfg(test)]
    pub(crate) fn has_registered_chunk(&self, pos: ChunkPos) -> bool {
        self.state.lock().chunks.contains_key(&pos)
    }

    #[cfg(test)]
    pub(crate) fn has_indexed_head(&self, pos: ChunkPos) -> bool {
        let state = self.state.lock();
        state.chunks.get(&pos).is_some_and(|registered| {
            registered.block_head.is_some() || registered.fluid_head.is_some()
        })
    }

    /// Reconciles the active sparse deadline index at a rare ticking-snapshot rebuild.
    pub(crate) fn reconcile_active_chunks<I>(
        &self,
        active_chunks: I,
    ) -> Result<(), TickSchedulerError>
    where
        I: Iterator<Item = ChunkPos> + Clone,
    {
        let _phase = self.phase.write();
        let mut state = self.state.lock();
        for pos in active_chunks.clone() {
            if !state.chunks.contains_key(&pos) {
                return Err(TickSchedulerError::MissingContainer(pos));
            }
        }
        state.active_block_deadlines.clear();
        state.active_fluid_deadlines.clear();
        let generation = state.advance_active_generation();
        for (rank, pos) in active_chunks.enumerate() {
            let (block_head, fluid_head) = {
                let Some(registered) = state.chunks.get_mut(&pos) else {
                    return Err(TickSchedulerError::MissingContainer(pos));
                };
                assert!(
                    registered
                        .active
                        .is_none_or(|active| active.generation != generation),
                    "active scheduled-tick chunk appeared twice during reconciliation"
                );
                registered.active = Some(ActiveChunkRank { generation, rank });
                (registered.block_head, registered.fluid_head)
            };
            let packed = PackedChunkPos::from(pos);
            if let Some(block_head) = block_head {
                assert!(
                    state.active_block_deadlines.insert((block_head, packed)),
                    "active block-tick chunk appeared twice during reconciliation"
                );
            }
            if let Some(fluid_head) = fluid_head {
                assert!(
                    state.active_fluid_deadlines.insert((fluid_head, packed)),
                    "active fluid-tick chunk appeared twice during reconciliation"
                );
            }
        }
        Ok(())
    }

    pub(crate) fn schedule_block(
        &self,
        chunk: &LevelChunk,
        block: BlockRef,
        pos: BlockPos,
        trigger_tick: i64,
        priority: TickPriority,
        sub_tick_order: i64,
    ) -> Result<bool, TickSchedulerError> {
        let _phase = self.phase.read();
        let container = chunk.scheduled_tick_container();
        let mut container_state = container.state.lock();
        if container_state.lifecycle == ChunkTickContainerLifecycle::PrePublication {
            return Ok(container_state.lists.block_mut().schedule(
                block,
                pos,
                trigger_tick,
                priority,
                sub_tick_order,
            ));
        }
        if container_state.lifecycle == ChunkTickContainerLifecycle::Finalized {
            return Err(TickSchedulerError::MissingContainer(chunk.pos));
        }
        let previous_head = container_state
            .lists
            .block()
            .peek()
            .map(|tick| tick.trigger_tick);
        let added = container_state.lists.block_mut().schedule(
            block,
            pos,
            trigger_tick,
            priority,
            sub_tick_order,
        );
        if !added {
            return Ok(false);
        }
        let head = container_state
            .lists
            .block()
            .peek()
            .map(|tick| tick.trigger_tick);
        if head == previous_head {
            return Ok(true);
        }
        let mut state = self.state.lock();
        let Some(registered) = state.chunks.get(&chunk.pos) else {
            return Err(TickSchedulerError::MissingContainer(chunk.pos));
        };
        if !Arc::ptr_eq(&registered.container, container) {
            return Err(TickSchedulerError::ContainerMismatch(chunk.pos));
        }
        state.set_head(chunk.pos, TickKind::Block, head)?;
        Ok(true)
    }

    pub(crate) fn schedule_fluid(
        &self,
        chunk: &LevelChunk,
        fluid: FluidRef,
        pos: BlockPos,
        trigger_tick: i64,
        priority: TickPriority,
        sub_tick_order: i64,
    ) -> Result<bool, TickSchedulerError> {
        let _phase = self.phase.read();
        let container = chunk.scheduled_tick_container();
        let mut container_state = container.state.lock();
        if container_state.lifecycle == ChunkTickContainerLifecycle::PrePublication {
            return Ok(container_state.lists.fluid_mut().schedule(
                fluid,
                pos,
                trigger_tick,
                priority,
                sub_tick_order,
            ));
        }
        if container_state.lifecycle == ChunkTickContainerLifecycle::Finalized {
            return Err(TickSchedulerError::MissingContainer(chunk.pos));
        }
        let previous_head = container_state
            .lists
            .fluid()
            .peek()
            .map(|tick| tick.trigger_tick);
        let added = container_state.lists.fluid_mut().schedule(
            fluid,
            pos,
            trigger_tick,
            priority,
            sub_tick_order,
        );
        if !added {
            return Ok(false);
        }
        let head = container_state
            .lists
            .fluid()
            .peek()
            .map(|tick| tick.trigger_tick);
        if head == previous_head {
            return Ok(true);
        }
        let mut state = self.state.lock();
        let Some(registered) = state.chunks.get(&chunk.pos) else {
            return Err(TickSchedulerError::MissingContainer(chunk.pos));
        };
        if !Arc::ptr_eq(&registered.container, container) {
            return Err(TickSchedulerError::ContainerMismatch(chunk.pos));
        }
        state.set_head(chunk.pos, TickKind::Fluid, head)?;
        Ok(true)
    }

    /// Selects the ready block batch from sparse live-container heads.
    pub(crate) fn begin_tick(
        &self,
        current_tick: i64,
        max_ticks: usize,
    ) -> ScheduledTickBatch<BlockRef> {
        self.collect_ticks(
            TickKind::Block,
            ChunkTickLists::block_mut,
            current_tick,
            max_ticks,
        )
    }

    /// Selects fluids after block callbacks using the same captured game time.
    pub(crate) fn collect_fluid_ticks(
        &self,
        current_tick: i64,
        max_ticks: usize,
    ) -> ScheduledTickBatch<FluidRef> {
        self.collect_ticks(
            TickKind::Fluid,
            ChunkTickLists::fluid_mut,
            current_tick,
            max_ticks,
        )
    }

    fn collect_ticks<T: TickKey>(
        &self,
        kind: TickKind,
        select: fn(&mut ChunkTickLists) -> &mut TickList<T>,
        current_tick: i64,
        max_ticks: usize,
    ) -> ScheduledTickBatch<T> {
        if max_ticks == 0 {
            return ScheduledTickBatch {
                ticks: Vec::new(),
                changed_containers: Vec::new(),
            };
        }
        let _phase = self.phase.write();
        let due = self.state.lock().take_due(kind, current_tick);
        if due.is_empty() {
            return ScheduledTickBatch {
                ticks: Vec::new(),
                changed_containers: Vec::new(),
            };
        }
        let (batch, head_updates) = collect_registered_ticks(due, select, current_tick, max_ticks);
        let mut state = self.state.lock();
        for update in head_updates {
            if let Err(error) = state.set_head(update.pos, kind, update.trigger_tick) {
                panic!("scheduled-tick head index invariant failed: {error:?}");
            }
        }
        batch
    }
}

impl Default for WorldTickScheduler {
    fn default() -> Self {
        Self::new()
    }
}

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

#[derive(Debug)]
struct ReadyContainer {
    pos: ChunkPos,
    rank: usize,
    container: Arc<ChunkTickContainer>,
    priority: TickPriority,
    sub_tick_order: i64,
    dirty_reported: bool,
}

impl PartialEq for ReadyContainer {
    fn eq(&self, other: &Self) -> bool {
        self.priority == other.priority
            && self.sub_tick_order == other.sub_tick_order
            && self.rank == other.rank
    }
}

impl Eq for ReadyContainer {}

impl ReadyContainer {
    fn new<T: TickKey>(
        active: ActiveTickContainer,
        tick: ScheduledTick<T>,
        dirty_reported: bool,
    ) -> Self {
        Self {
            pos: active.pos,
            rank: active.rank,
            container: active.container,
            priority: tick.priority,
            sub_tick_order: tick.sub_tick_order,
            dirty_reported,
        }
    }

    const fn refresh<T: TickKey>(&mut self, tick: ScheduledTick<T>) {
        self.priority = tick.priority;
        self.sub_tick_order = tick.sub_tick_order;
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
        .then_with(|| other.rank.cmp(&self.rank))
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

struct TickListPackingSnapshot<T: TickKey> {
    pending_ticks: Vec<SavedTick<T>>,
    live_ticks: Vec<(ScheduledTick<T>, u64)>,
}

impl<T: TickKey> TickListPackingSnapshot<T> {
    fn pack(mut self, current_tick: i64) -> Vec<SavedTick<T>> {
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

    fn packing_snapshot(&self) -> TickListPackingSnapshot<T> {
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

    fn peek(&self) -> Option<ScheduledTick<T>> {
        Some(self.ticks.peek()?.tick)
    }

    fn peek_ready(&self, current_tick: i64) -> Option<ScheduledTick<T>> {
        let tick = self.ticks.peek()?.tick;
        (tick.trigger_tick <= current_tick).then_some(tick)
    }

    fn pop_ready(&mut self, current_tick: i64) -> Option<ScheduledTick<T>> {
        self.peek_ready(current_tick)?;
        let tick = self.ticks.pop()?.tick;
        self.scheduled.remove(&tick.key());
        Some(tick)
    }

    #[cfg(test)]
    fn drain_ready(&mut self, current_tick: i64) -> Vec<ScheduledTick<T>> {
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

fn prepare_ready_containers<T: TickKey>(
    due_containers: Vec<ActiveTickContainer>,
    select: fn(&mut ChunkTickLists) -> &mut TickList<T>,
    current_tick: i64,
) -> (BinaryHeap<ReadyContainer>, Vec<TickHeadUpdate>) {
    let mut ready_containers = BinaryHeap::with_capacity(due_containers.len());
    let mut head_updates = Vec::with_capacity(due_containers.len());
    for active in due_containers {
        let head = {
            let mut state = active.container.state.lock();
            assert_eq!(
                state.lifecycle,
                ChunkTickContainerLifecycle::Registered,
                "active scheduled-tick container was not registered"
            );
            select(&mut state.lists).peek()
        };
        if let Some(tick) = head
            && tick.trigger_tick <= current_tick
        {
            ready_containers.push(ReadyContainer::new(active, tick, false));
        } else {
            head_updates.push(TickHeadUpdate {
                pos: active.pos,
                trigger_tick: head.map(|tick| tick.trigger_tick),
            });
        }
    }
    (ready_containers, head_updates)
}

/// Selects at most `max_ticks` ready entries from sparse live-container heads.
///
/// Only queue heads compete globally. Revealing the next head after each pop is
/// what preserves Vanilla's per-chunk deadline ordering when several ticks are
/// already overdue.
fn collect_registered_ticks<T: TickKey>(
    due_containers: Vec<ActiveTickContainer>,
    select: fn(&mut ChunkTickLists) -> &mut TickList<T>,
    current_tick: i64,
    max_ticks: usize,
) -> (ScheduledTickBatch<T>, Vec<TickHeadUpdate>) {
    let (mut ready_containers, mut head_updates) =
        prepare_ready_containers(due_containers, select, current_tick);

    let mut ticks = Vec::with_capacity(max_ticks.min(ready_containers.len()));
    let mut changed_containers = Vec::with_capacity(ready_containers.len());
    while ticks.len() < max_ticks {
        let Some(mut ready_container) = ready_containers.pop() else {
            break;
        };
        let mut container_state = ready_container.container.state.lock();
        assert_eq!(
            container_state.lifecycle,
            ChunkTickContainerLifecycle::Registered,
            "ready scheduled-tick container was not registered"
        );
        let container = select(&mut container_state.lists);
        let Some(tick) = container.pop_ready(current_tick) else {
            head_updates.push(TickHeadUpdate {
                pos: ready_container.pos,
                trigger_tick: container.peek().map(|tick| tick.trigger_tick),
            });
            continue;
        };

        if !ready_container.dirty_reported {
            changed_containers.push(ready_container.rank);
            ready_container.dirty_reported = true;
        }
        ticks.push(tick);

        // Vanilla keeps draining the current container while its next head is
        // no later in intra-tick order than the best competing container. In
        // particular, an exact tie stays with the current container.
        let next_competing_container = ready_containers
            .peek()
            .map(|competitor| (competitor.priority, competitor.sub_tick_order));
        while ticks.len() < max_ticks {
            let Some(next_tick) = container.peek_ready(current_tick) else {
                break;
            };
            if next_competing_container.is_some_and(|(priority, sub_tick_order)| {
                intra_tick_drain_order(
                    next_tick.priority,
                    next_tick.sub_tick_order,
                    priority,
                    sub_tick_order,
                ) == Ordering::Greater
            }) {
                break;
            }
            let Some(next_tick) = container.pop_ready(current_tick) else {
                break;
            };
            ticks.push(next_tick);
        }

        let next_tick = container.peek();
        drop(container_state);
        if let Some(next_tick) = next_tick {
            if ticks.len() < max_ticks && next_tick.trigger_tick <= current_tick {
                ready_container.refresh(next_tick);
                ready_containers.push(ready_container);
            } else {
                head_updates.push(TickHeadUpdate {
                    pos: ready_container.pos,
                    trigger_tick: Some(next_tick.trigger_tick),
                });
            }
        } else {
            head_updates.push(TickHeadUpdate {
                pos: ready_container.pos,
                trigger_tick: None,
            });
        }
    }

    for ready_container in ready_containers {
        let next_tick = {
            let mut state = ready_container.container.state.lock();
            select(&mut state.lists).peek()
        };
        head_updates.push(TickHeadUpdate {
            pos: ready_container.pos,
            trigger_tick: next_tick.map(|tick| tick.trigger_tick),
        });
    }

    (
        ScheduledTickBatch {
            ticks,
            changed_containers,
        },
        head_updates,
    )
}

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
    fn lookup_is_initialized(&self) -> bool {
        self.lookup.get().is_some()
    }
}

#[cfg(test)]
mod tests {
    use std::{
        sync::{Barrier, mpsc},
        thread,
        time::Duration,
    };

    use super::*;
    use steel_registry::blocks::Block;
    use steel_registry::blocks::behavior::BlockConfig;
    use steel_registry::test_support::init_test_registry;
    use steel_registry::vanilla_fluids;
    use steel_utils::Identifier;

    use crate::behavior::init_behaviors;
    use crate::chunk::chunk_access::ChunkStatus;
    use crate::test_support::{fresh_test_world, insert_ready_full_chunk};

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
        list.schedule(block, pos, i64::from(delay), priority, sub_tick_order)
    }

    fn scheduler_with_block_lists(
        chunks: impl IntoIterator<Item = (ChunkPos, BlockTickList)>,
    ) -> WorldTickScheduler {
        let scheduler = WorldTickScheduler::new();
        {
            let mut state = scheduler.state.lock();
            for (pos, block) in chunks {
                let block_head = block.peek().map(|tick| tick.trigger_tick);
                let container = Arc::new(ChunkTickContainer::new(ChunkTickLists::new(
                    block,
                    FluidTickList::new(),
                )));
                container.state.lock().lifecycle = ChunkTickContainerLifecycle::Registered;
                state.chunks.insert(
                    pos,
                    RegisteredChunkTicks {
                        container,
                        block_head,
                        fluid_head: None,
                        active: None,
                    },
                );
            }
        }
        scheduler
    }

    fn begin_block_tick_at(
        scheduler: &WorldTickScheduler,
        current_tick: i64,
        active_chunks: &[ChunkPos],
        max_ticks: usize,
    ) -> ScheduledTickBatch<BlockRef> {
        if let Err(error) = scheduler.reconcile_active_chunks(active_chunks.iter().copied()) {
            panic!("test scheduler invariant failed: {error:?}");
        }
        scheduler.begin_tick(current_tick, max_ticks)
    }

    fn registered_container(
        scheduler: &WorldTickScheduler,
        pos: ChunkPos,
    ) -> Arc<ChunkTickContainer> {
        let state = scheduler.state.lock();
        let Some(registered) = state.chunks.get(&pos) else {
            panic!("test chunk must remain registered");
        };
        Arc::clone(&registered.container)
    }

    fn block_head(scheduler: &WorldTickScheduler, pos: ChunkPos) -> Option<i64> {
        scheduler
            .state
            .lock()
            .chunks
            .get(&pos)
            .and_then(|registered| registered.block_head)
    }

    fn begin_block_tick(
        scheduler: &WorldTickScheduler,
        active_chunks: &[ChunkPos],
        max_ticks: usize,
    ) -> ScheduledTickBatch<BlockRef> {
        begin_block_tick_at(scheduler, 1, active_chunks, max_ticks)
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
    fn chunk_snapshot_does_not_wait_for_world_scheduler_metadata() {
        init_test_registry();
        init_behaviors();
        let world = fresh_test_world("chunk_tick_snapshot_lock_scope");
        let chunk_pos = ChunkPos::new(0, 0);
        let tick_pos = BlockPos::new(1, 64, 1);
        let holder = insert_ready_full_chunk(&world, chunk_pos);
        world.schedule_block_tick(tick_pos, test_block(), 5, TickPriority::Normal);

        let metadata = world.scheduled_ticks.state.lock();
        let barrier = Arc::new(Barrier::new(2));
        let worker_barrier = Arc::clone(&barrier);
        let (sender, receiver) = mpsc::channel();
        let worker = thread::spawn(move || {
            worker_barrier.wait();
            let Some(chunk) = holder.try_chunk(ChunkStatus::Full) else {
                return;
            };
            let Some(full) = chunk.as_full() else {
                return;
            };
            let _ = sender.send(full.scheduled_tick_snapshot().block.len());
        });
        barrier.wait();

        let snapshot_len = receiver.recv_timeout(Duration::from_secs(2));
        drop(metadata);
        assert!(
            worker.join().is_ok(),
            "scheduled-tick snapshot worker panicked"
        );
        assert_eq!(
            snapshot_len,
            Ok(1),
            "packing one chunk must not acquire world scheduler metadata"
        );
    }

    #[test]
    fn pending_ticks_are_unindexed_until_idempotent_unpack() {
        let chunk_pos = ChunkPos::new(0, 0);
        let tick_pos = BlockPos::new(1, 2, 3);
        let pending = BlockTickList::from_saved_ticks(vec![SavedTick {
            tick_type: test_block(),
            pos: tick_pos,
            delay: 5,
            priority: TickPriority::Normal,
        }]);
        let scheduler = scheduler_with_block_lists([(chunk_pos, pending)]);

        assert_eq!(block_head(&scheduler, chunk_pos), None);
        if let Err(error) = scheduler.unpack_chunk(chunk_pos, 100) {
            panic!("test scheduler invariant failed: {error:?}");
        }
        assert_eq!(block_head(&scheduler, chunk_pos), Some(105));

        // A later readiness promotion cannot re-anchor the existing deadline.
        if let Err(error) = scheduler.unpack_chunk(chunk_pos, 200) {
            panic!("test scheduler invariant failed: {error:?}");
        }
        assert_eq!(block_head(&scheduler, chunk_pos), Some(105));
        assert!(
            begin_block_tick_at(&scheduler, 104, &[chunk_pos], 1)
                .ticks
                .is_empty()
        );
        assert_eq!(
            begin_block_tick_at(&scheduler, 105, &[chunk_pos], 1).ticks[0].pos,
            tick_pos
        );
    }

    #[test]
    fn pending_and_live_ticks_share_dedup_before_unpack() {
        let pending_pos = BlockPos::new(1, 2, 3);
        let live_pos = BlockPos::new(2, 2, 3);
        let mut list = BlockTickList::from_saved_ticks(vec![SavedTick {
            tick_type: test_block(),
            pos: pending_pos,
            delay: 5,
            priority: TickPriority::Normal,
        }]);

        assert!(!list.schedule(test_block(), pending_pos, 101, TickPriority::High, 10));
        assert!(list.schedule(test_block(), live_pos, 101, TickPriority::Normal, 11));
        assert_eq!(list.peek().map(|tick| tick.pos), Some(live_pos));
        list.unpack(100);
        assert_eq!(list.drain_ready(101)[0].pos, live_pos);
        assert_eq!(list.drain_ready(105)[0].pos, pending_pos);
    }

    #[test]
    fn absolute_time_makes_ineligible_deadlines_overdue() {
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

        assert_eq!(list.drain_ready(1)[0].pos, first_pos);
        // No collection occurs while the chunk is ineligible, but world game
        // time continues. The later deadline is overdue upon re-entry.
        assert_eq!(list.drain_ready(100)[0].pos, fourth_pos);
    }

    #[test]
    fn global_cap_retains_ready_overflow() {
        let mut list = BlockTickList::new();
        let chunk_pos = ChunkPos::new(0, 0);
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

        let scheduler = scheduler_with_block_lists([(chunk_pos, list)]);
        let selected = begin_block_tick(&scheduler, &[chunk_pos], 2);
        assert_eq!(
            selected
                .ticks
                .iter()
                .map(|tick| tick.pos)
                .collect::<Vec<_>>(),
            vec![high_pos, normal_pos]
        );
        assert_eq!(selected.changed_containers, vec![0]);
        let container = registered_container(&scheduler, chunk_pos);
        assert!(
            container
                .state
                .lock()
                .lists
                .block()
                .has_tick(overflow_pos, test_block())
        );

        let selected = begin_block_tick(&scheduler, &[chunk_pos], 2);
        assert_eq!(selected.ticks.len(), 1);
        assert_eq!(selected.ticks[0].pos, overflow_pos);
    }

    #[test]
    fn block_and_fluid_collection_use_the_same_absolute_time() {
        let chunk_pos = ChunkPos::new(0, 0);
        let block_pos = BlockPos::new(0, 0, 0);
        let fluid_pos = BlockPos::new(1, 0, 0);
        let scheduler = scheduler_with_block_lists([(chunk_pos, BlockTickList::new())]);

        let container = registered_container(&scheduler, chunk_pos);
        let (block_head, fluid_head) = {
            let mut container_state = container.state.lock();
            assert!(container_state.lists.block_mut().schedule(
                test_block(),
                block_pos,
                20,
                TickPriority::Normal,
                0
            ));
            assert!(container_state.lists.fluid_mut().schedule(
                &vanilla_fluids::WATER,
                fluid_pos,
                20,
                TickPriority::Normal,
                1
            ));
            (
                container_state
                    .lists
                    .block()
                    .peek()
                    .map(|tick| tick.trigger_tick),
                container_state
                    .lists
                    .fluid()
                    .peek()
                    .map(|tick| tick.trigger_tick),
            )
        };
        {
            let mut state = scheduler.state.lock();
            if let Err(error) = state.set_head(chunk_pos, TickKind::Block, block_head) {
                panic!("test scheduler invariant failed: {error:?}");
            }
            if let Err(error) = state.set_head(chunk_pos, TickKind::Fluid, fluid_head) {
                panic!("test scheduler invariant failed: {error:?}");
            }
        }

        if let Err(error) = scheduler.reconcile_active_chunks([chunk_pos].into_iter()) {
            panic!("test scheduler invariant failed: {error:?}");
        }
        let blocks = scheduler.begin_tick(20, 2);
        let fluids = scheduler.collect_fluid_ticks(20, 2);
        assert_eq!(
            fluids.ticks.iter().map(|tick| tick.pos).collect::<Vec<_>>(),
            [fluid_pos]
        );
        assert_eq!(
            blocks.ticks.iter().map(|tick| tick.pos).collect::<Vec<_>>(),
            [block_pos]
        );
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

        let first_pos = ChunkPos::new(0, 0);
        let second_pos = ChunkPos::new(1, 0);
        let scheduler =
            scheduler_with_block_lists([(first_pos, first_chunk), (second_pos, second_chunk)]);
        // Leave the first due heads queued so that all three are overdue next active tick.
        assert!(
            begin_block_tick(&scheduler, &[first_pos, second_pos], 0)
                .ticks
                .is_empty()
        );

        let selected = begin_block_tick_at(&scheduler, 2, &[first_pos, second_pos], 3);
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
        let current_chunk = BlockTickList::from_saved_ticks(vec![
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
        let competing_chunk = BlockTickList::from_saved_ticks(vec![SavedTick {
            tick_type: test_block(),
            pos: competing_normal_pos,
            delay: 1,
            priority: TickPriority::Normal,
        }]);

        // Put the competitor first so its container-index tie-break would win if
        // the current container were reinserted after every pop.
        let competing_chunk_pos = ChunkPos::new(0, 0);
        let current_chunk_pos = ChunkPos::new(1, 0);
        let scheduler = scheduler_with_block_lists([
            (competing_chunk_pos, competing_chunk),
            (current_chunk_pos, current_chunk),
        ]);
        for pos in [competing_chunk_pos, current_chunk_pos] {
            if let Err(error) = scheduler.unpack_chunk(pos, 0) {
                panic!("test scheduler invariant failed: {error:?}");
            }
        }
        let selected = begin_block_tick(&scheduler, &[competing_chunk_pos, current_chunk_pos], 3);

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
    fn exact_loaded_head_ties_follow_the_active_scc_order() {
        let first_tick_pos = BlockPos::new(0, 0, 0);
        let second_tick_pos = BlockPos::new(16, 0, 0);
        let first_chunk_pos = ChunkPos::new(0, 0);
        let second_chunk_pos = ChunkPos::new(1, 0);
        let first_chunk = BlockTickList::from_saved_ticks(vec![SavedTick {
            tick_type: test_block(),
            pos: first_tick_pos,
            delay: 1,
            priority: TickPriority::Normal,
        }]);
        let second_chunk = BlockTickList::from_saved_ticks(vec![SavedTick {
            tick_type: test_block(),
            pos: second_tick_pos,
            delay: 1,
            priority: TickPriority::Normal,
        }]);
        let scheduler = scheduler_with_block_lists([
            (first_chunk_pos, first_chunk),
            (second_chunk_pos, second_chunk),
        ]);
        for pos in [first_chunk_pos, second_chunk_pos] {
            if let Err(error) = scheduler.unpack_chunk(pos, 0) {
                panic!("test scheduler invariant failed: {error:?}");
            }
        }

        let selected = begin_block_tick(&scheduler, &[second_chunk_pos, first_chunk_pos], 2);
        assert_eq!(
            selected
                .ticks
                .iter()
                .map(|tick| tick.pos)
                .collect::<Vec<_>>(),
            [second_tick_pos, first_tick_pos]
        );
    }

    #[test]
    fn ineligible_live_head_stays_indexed_until_reentry() {
        let registered_pos = ChunkPos::new(0, 0);
        let mut pending = BlockTickList::new();
        assert!(schedule(
            &mut pending,
            test_block(),
            BlockPos::new(0, 0, 0),
            3,
            TickPriority::Normal,
            0
        ));
        let scheduler = scheduler_with_block_lists([(registered_pos, pending)]);

        if let Err(error) = scheduler.reconcile_active_chunks([registered_pos].into_iter()) {
            panic!("test scheduler invariant failed: {error:?}");
        }
        assert!(
            scheduler
                .state
                .lock()
                .active_block_deadlines
                .contains(&(3, PackedChunkPos::from(registered_pos)))
        );

        if let Err(error) = scheduler.reconcile_active_chunks([].into_iter()) {
            panic!("test scheduler invariant failed: {error:?}");
        }
        assert!(scheduler.state.lock().active_block_deadlines.is_empty());
        let inactive_batch = scheduler.begin_tick(100, 1);
        assert!(inactive_batch.ticks.is_empty());
        assert_eq!(block_head(&scheduler, registered_pos), Some(3));

        if let Err(error) = scheduler.reconcile_active_chunks([registered_pos].into_iter()) {
            panic!("test scheduler invariant failed: {error:?}");
        }
        assert!(
            scheduler
                .state
                .lock()
                .active_block_deadlines
                .contains(&(3, PackedChunkPos::from(registered_pos)))
        );
        let selected = scheduler.begin_tick(100, 1);
        assert_eq!(selected.ticks.len(), 1);
        assert_eq!(selected.changed_containers, [0]);
    }

    #[test]
    fn failed_active_reconciliation_preserves_the_published_index() {
        let registered_pos = ChunkPos::new(0, 0);
        let missing_pos = ChunkPos::new(1, 0);
        let mut ticks = BlockTickList::new();
        assert!(schedule(
            &mut ticks,
            test_block(),
            BlockPos::new(0, 0, 0),
            3,
            TickPriority::Normal,
            0
        ));
        let scheduler = scheduler_with_block_lists([(registered_pos, ticks)]);
        if let Err(error) = scheduler.reconcile_active_chunks([registered_pos].into_iter()) {
            panic!("test scheduler invariant failed: {error:?}");
        }
        let generation = scheduler.state.lock().active_generation;

        assert_eq!(
            scheduler.reconcile_active_chunks([registered_pos, missing_pos].into_iter()),
            Err(TickSchedulerError::MissingContainer(missing_pos))
        );
        let state = scheduler.state.lock();
        assert_eq!(state.active_generation, generation);
        assert!(
            state
                .active_block_deadlines
                .contains(&(3, PackedChunkPos::from(registered_pos)))
        );
        assert_eq!(
            state
                .chunks
                .get(&registered_pos)
                .and_then(|registered| registered.active_rank(generation)),
            Some(0)
        );
    }

    #[test]
    fn only_popped_containers_report_a_persistence_change() {
        let empty = BlockTickList::new();
        let mut pending = BlockTickList::new();
        assert!(schedule(
            &mut pending,
            test_block(),
            BlockPos::new(0, 0, 0),
            3,
            TickPriority::Normal,
            0
        ));
        assert_eq!(pending.pack(0)[0].delay, 3);

        let empty_pos = ChunkPos::new(0, 0);
        let pending_pos = ChunkPos::new(1, 0);
        let scheduler = scheduler_with_block_lists([(empty_pos, empty), (pending_pos, pending)]);
        let before_deadline = begin_block_tick_at(&scheduler, 1, &[empty_pos, pending_pos], 1);
        assert!(before_deadline.changed_containers.is_empty());
        let selected = begin_block_tick_at(&scheduler, 3, &[empty_pos, pending_pos], 1);
        assert_eq!(selected.changed_containers, vec![1]);
    }

    #[test]
    fn persistence_uses_absolute_time_and_rebuilds_loaded_order() {
        let mut list = BlockTickList::new();
        let first_pos = BlockPos::new(0, 0, 0);
        let second_pos = BlockPos::new(1, 0, 0);
        assert!(list.schedule(test_block(), first_pos, 105, TickPriority::Normal, 100));
        assert!(list.schedule(test_block(), second_pos, 105, TickPriority::Normal, 101));

        let saved = list.pack(102);
        assert_eq!(
            saved.iter().map(|tick| tick.delay).collect::<Vec<_>>(),
            vec![3, 3]
        );

        let mut loaded = BlockTickList::from_saved_ticks(saved);
        assert!(loaded.schedule(
            test_block(),
            BlockPos::new(2, 0, 0),
            203,
            TickPriority::Normal,
            0
        ));
        loaded.unpack(200);
        assert!(loaded.drain_ready(202).is_empty());
        let ready = loaded.drain_ready(203);

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
    fn proto_saved_ticks_deduplicate_in_first_occurrence_order() {
        let pos = BlockPos::new(1, 2, 3);
        let proto = BlockTickList::from_proto_saved_ticks(vec![
            SavedTick {
                tick_type: test_block(),
                pos,
                delay: 7,
                priority: TickPriority::High,
            },
            SavedTick {
                tick_type: test_block(),
                pos,
                delay: 2,
                priority: TickPriority::Low,
            },
        ]);

        let saved = proto.pack(0);
        assert_eq!(saved.len(), 1);
        assert_eq!(saved[0].delay, 7);
        assert_eq!(saved[0].priority, TickPriority::High);
    }

    #[test]
    fn removing_pending_ticks_releases_their_deduplication_keys() {
        let mut ticks = BlockTickList::new_pending();
        let removed_pos = BlockPos::new(1, 2, 3);
        let retained_pos = BlockPos::new(4, 5, 6);
        assert!(ticks.schedule_pending(test_block(), removed_pos, TickPriority::Normal));
        assert!(ticks.schedule_pending(test_block_2(), retained_pos, TickPriority::Low));

        let removed = ticks.remove_pending_matching(|tick| tick.pos == removed_pos);

        assert_eq!(removed, 1);
        assert_eq!(ticks.pending_entries().len(), 1);
        assert_eq!(ticks.pending_entries()[0].pos, retained_pos);
        assert!(ticks.schedule_pending(test_block(), removed_pos, TickPriority::High));
    }

    #[test]
    fn unpack_preserves_proto_tick_insertion_order() {
        let mut proto_ticks = BlockTickList::new_pending();
        let first_pos = BlockPos::new(0, 0, 0);
        let second_pos = BlockPos::new(1, 0, 0);
        assert!(proto_ticks.schedule_pending(test_block(), first_pos, TickPriority::Normal));
        assert!(proto_ticks.schedule_pending(test_block(), second_pos, TickPriority::Normal));

        proto_ticks.unpack(50);
        let ready = proto_ticks.drain_ready(50);
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
        let batch = ScheduledTickRunBatch::new(vec![first, second]);
        assert!(!batch.lookup_is_initialized());
        assert!(batch.contains(first.pos, first.tick_type));
        assert!(batch.lookup_is_initialized());
        assert!(batch.contains(second.pos, second.tick_type));
        batch.start(0);
        assert!(!batch.contains(first.pos, first.tick_type));
        assert!(batch.contains(second.pos, second.tick_type));
        batch.start(1);
        assert!(!batch.contains(second.pos, second.tick_type));

        let late_query_batch = ScheduledTickRunBatch::new(vec![first, second]);
        late_query_batch.start(0);
        assert!(!late_query_batch.lookup_is_initialized());
        assert!(!late_query_batch.contains(first.pos, first.tick_type));
        assert!(late_query_batch.contains(second.pos, second.tick_type));

        let completed_batch = ScheduledTickRunBatch::new(vec![first]);
        completed_batch.start(0);
        assert!(!completed_batch.contains(first.pos, first.tick_type));
        assert!(!completed_batch.lookup_is_initialized());
    }

    #[test]
    fn can_reschedule_after_ready_tick_is_removed() {
        let mut list = BlockTickList::new();
        let block = test_block();
        let pos = BlockPos::new(0, 0, 0);
        assert!(schedule(&mut list, block, pos, 1, TickPriority::Normal, 0));
        assert_eq!(list.drain_ready(1).len(), 1);
        assert!(schedule(&mut list, block, pos, 5, TickPriority::Normal, 1));
    }

    #[test]
    fn priority_ordering_matches_vanilla_discriminants() {
        assert!(TickPriority::ExtremelyHigh < TickPriority::Normal);
        assert!(TickPriority::Normal < TickPriority::ExtremelyLow);
        assert!(TickPriority::High < TickPriority::Low);
    }
}
