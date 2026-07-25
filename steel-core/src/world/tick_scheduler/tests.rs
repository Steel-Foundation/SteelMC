use std::{
    sync::{Barrier, mpsc},
    thread,
    time::Duration,
};

use super::scheduler::{RegisteredChunkTicks, TickKind};
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

fn registered_container(scheduler: &WorldTickScheduler, pos: ChunkPos) -> Arc<ChunkTickContainer> {
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
