//! Vanilla collecting neighbor-update ordering.
//!
//! The feature-gated experimental redstone `Orientation` is intentionally absent.

use std::sync::Arc;

use steel_registry::blocks::BlockRef;
use steel_utils::locks::SyncMutex;
use steel_utils::types::UpdateFlags;
use steel_utils::{BlockPos, BlockStateId, Direction};

use super::World;

/// Vanilla `NeighborUpdater.UPDATE_ORDER`.
pub(super) const UPDATE_ORDER: [Direction; 6] = [
    Direction::West,
    Direction::East,
    Direction::Down,
    Direction::Up,
    Direction::North,
    Direction::South,
];

pub(super) struct CollectingNeighborUpdater {
    max_chained_neighbor_updates: i32,
    state: SyncMutex<CollectingState<NeighborUpdate>>,
}

pub(super) struct ShapeUpdate {
    direction: Direction,
    neighbor_state: BlockStateId,
    pos: BlockPos,
    neighbor_pos: BlockPos,
    flags: UpdateFlags,
    update_limit: i32,
}

impl ShapeUpdate {
    pub(super) const fn new(
        direction: Direction,
        neighbor_state: BlockStateId,
        pos: BlockPos,
        neighbor_pos: BlockPos,
        flags: UpdateFlags,
        update_limit: i32,
    ) -> Self {
        Self {
            direction,
            neighbor_state,
            pos,
            neighbor_pos,
            flags,
            update_limit,
        }
    }
}

impl CollectingNeighborUpdater {
    pub(super) fn new(max_chained_neighbor_updates: i32) -> Self {
        Self {
            max_chained_neighbor_updates,
            state: SyncMutex::new(CollectingState::default()),
        }
    }

    pub(super) fn shape_update(&self, world: &Arc<World>, update: ShapeUpdate) {
        self.add_and_run(world, update.pos, NeighborUpdate::Shape(update));
    }

    pub(super) fn neighbor_changed(
        &self,
        world: &Arc<World>,
        pos: BlockPos,
        source_block: BlockRef,
    ) {
        self.add_and_run(world, pos, NeighborUpdate::Simple { pos, source_block });
    }

    pub(super) fn neighbor_changed_with_state(
        &self,
        world: &Arc<World>,
        state: BlockStateId,
        pos: BlockPos,
        source_block: BlockRef,
        moved_by_piston: bool,
    ) {
        self.add_and_run(
            world,
            pos,
            NeighborUpdate::Full {
                state,
                pos,
                source_block,
                moved_by_piston,
            },
        );
    }

    pub(super) fn update_neighbors_at_except_from_facing(
        &self,
        world: &Arc<World>,
        pos: BlockPos,
        source_block: BlockRef,
        skip_direction: Option<Direction>,
    ) {
        self.add_and_run(
            world,
            pos,
            NeighborUpdate::Multi {
                source_pos: pos,
                source_block,
                skip_direction,
                index: usize::from(skip_direction == Some(UPDATE_ORDER[0])),
            },
        );
    }

    fn add_and_run(&self, world: &Arc<World>, pos: BlockPos, update: NeighborUpdate) {
        let result = self
            .state
            .lock()
            .enqueue(self.max_chained_neighbor_updates, update);
        if result.first_skipped {
            log::error!(
                "Too many chained neighbor updates. Skipping the rest. First skipped position: {}, {}, {}",
                pos.x(),
                pos.y(),
                pos.z()
            );
        }
        if result.should_run {
            self.run_updates(world);
        }
    }

    fn run_updates(&self, world: &Arc<World>) {
        let mut reset_guard = ResetGuard {
            updater: self,
            armed: true,
        };
        loop {
            let next = {
                let mut state = self.state.lock();
                let next = state.take_next();
                if next.is_none() {
                    state.reset();
                    reset_guard.armed = false;
                }
                next
            };
            let Some(mut update) = next else {
                return;
            };

            if update.run_next(world) {
                self.state.lock().return_unfinished(update);
            }
        }
    }

    fn reset(&self) {
        self.state.lock().reset();
    }
}

struct ResetGuard<'a> {
    updater: &'a CollectingNeighborUpdater,
    armed: bool,
}

impl Drop for ResetGuard<'_> {
    fn drop(&mut self) {
        if self.armed {
            self.updater.reset();
        }
    }
}

enum NeighborUpdate {
    Full {
        state: BlockStateId,
        pos: BlockPos,
        source_block: BlockRef,
        moved_by_piston: bool,
    },
    Multi {
        source_pos: BlockPos,
        source_block: BlockRef,
        skip_direction: Option<Direction>,
        index: usize,
    },
    Shape(ShapeUpdate),
    Simple {
        pos: BlockPos,
        source_block: BlockRef,
    },
}

impl NeighborUpdate {
    fn run_next(&mut self, world: &Arc<World>) -> bool {
        match self {
            Self::Full {
                state,
                pos,
                source_block,
                moved_by_piston,
            } => {
                world.execute_neighbor_update(*state, *pos, source_block, *moved_by_piston);
                false
            }
            Self::Multi {
                source_pos,
                source_block,
                skip_direction,
                index,
            } => {
                let direction = UPDATE_ORDER[*index];
                *index += 1;
                let neighbor_pos = source_pos.relative(direction);
                let state = world.get_block_state(neighbor_pos);
                world.execute_neighbor_update(state, neighbor_pos, source_block, false);
                if *index < UPDATE_ORDER.len() && Some(UPDATE_ORDER[*index]) == *skip_direction {
                    *index += 1;
                }
                *index < UPDATE_ORDER.len()
            }
            Self::Shape(update) => {
                world.execute_neighbor_shape_update(
                    update.direction,
                    update.pos,
                    update.neighbor_pos,
                    update.neighbor_state,
                    update.flags,
                    update.update_limit,
                );
                false
            }
            Self::Simple { pos, source_block } => {
                let state = world.get_block_state(*pos);
                world.execute_neighbor_update(state, *pos, source_block, false);
                false
            }
        }
    }
}

struct AddResult {
    should_run: bool,
    first_skipped: bool,
}

struct CollectingState<U> {
    stack: Vec<U>,
    added_this_layer: Vec<U>,
    count: i32,
}

impl<U> Default for CollectingState<U> {
    fn default() -> Self {
        Self {
            stack: Vec::new(),
            added_this_layer: Vec::new(),
            count: 0,
        }
    }
}

impl<U> CollectingState<U> {
    fn enqueue(&mut self, max_chained_neighbor_updates: i32, update: U) -> AddResult {
        let running_already = self.count > 0;
        let too_many_updates =
            max_chained_neighbor_updates >= 0 && self.count >= max_chained_neighbor_updates;
        self.count = self.count.wrapping_add(1);

        if !too_many_updates {
            if running_already {
                self.added_this_layer.push(update);
            } else {
                self.stack.push(update);
            }
        }

        AddResult {
            should_run: !running_already,
            first_skipped: too_many_updates
                && self.count.wrapping_sub(1) == max_chained_neighbor_updates,
        }
    }

    fn take_next(&mut self) -> Option<U> {
        while let Some(update) = self.added_this_layer.pop() {
            self.stack.push(update);
        }
        self.stack.pop()
    }

    fn return_unfinished(&mut self, update: U) {
        self.stack.push(update);
    }

    fn reset(&mut self) {
        self.stack.clear();
        self.added_this_layer.clear();
        self.count = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    struct TestUpdate(char);

    #[test]
    fn nested_updates_interrupt_multi_update_and_keep_sibling_fifo_order() {
        let mut state = CollectingState::default();
        assert!(state.enqueue(1_000_000, TestUpdate('A')).should_run);
        let multi = state.take_next();
        assert_eq!(multi, Some(TestUpdate('A')));

        assert!(!state.enqueue(1_000_000, TestUpdate('B')).should_run);
        assert!(!state.enqueue(1_000_000, TestUpdate('C')).should_run);
        if let Some(multi) = multi {
            state.return_unfinished(multi);
        }

        assert_eq!(state.take_next(), Some(TestUpdate('B')));
        assert!(!state.enqueue(1_000_000, TestUpdate('D')).should_run);
        assert_eq!(state.take_next(), Some(TestUpdate('D')));
        assert_eq!(state.take_next(), Some(TestUpdate('C')));
        assert_eq!(state.take_next(), Some(TestUpdate('A')));
        assert_eq!(state.take_next(), None);
    }

    #[test]
    fn chained_update_limit_counts_enqueued_tasks_and_reports_only_first_skip() {
        let mut state = CollectingState::default();
        let first = state.enqueue(2, TestUpdate('A'));
        let second = state.enqueue(2, TestUpdate('B'));
        let skipped = state.enqueue(2, TestUpdate('C'));
        let also_skipped = state.enqueue(2, TestUpdate('D'));

        assert!(first.should_run);
        assert!(!first.first_skipped);
        assert!(!second.first_skipped);
        assert!(skipped.first_skipped);
        assert!(!also_skipped.first_skipped);
        assert_eq!(state.take_next(), Some(TestUpdate('B')));
        assert_eq!(state.take_next(), Some(TestUpdate('A')));
        assert_eq!(state.take_next(), None);
    }

    #[test]
    fn zero_limit_still_starts_and_resets_a_root_run_without_executing_it() {
        let mut state = CollectingState::default();
        let result = state.enqueue(0, TestUpdate('A'));

        assert!(result.should_run);
        assert!(result.first_skipped);
        assert_eq!(state.take_next(), None);
        state.reset();
        assert!(state.enqueue(0, TestUpdate('B')).should_run);
    }
}
