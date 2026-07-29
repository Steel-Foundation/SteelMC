use std::sync::Arc;

use steel_macros::block_behavior;
use steel_registry::blocks::BlockRef;
use steel_registry::blocks::block_state_ext::BlockStateExt as _;
use steel_registry::blocks::properties::{BlockStateProperties, RailShape};
use steel_utils::types::UpdateFlags;
use steel_utils::{BlockPos, BlockStateId, Direction};

use crate::behavior::{BlockBehavior, BlockPlaceContext, RailBehavior};
use crate::world::{LevelReader, ScheduledTickAccess, SignalGetter as _, World};

use super::base_rail_block::BaseRailBlock;

/// Vanilla powered-rail behavior, also used by activator rails.
#[block_behavior]
pub struct PoweredRailBlock {
    base: BaseRailBlock,
}

impl PoweredRailBlock {
    const MAX_SEARCH_DEPTH: i32 = 8;

    /// Creates powered or activator rail behavior for `block`.
    #[must_use]
    pub const fn new(block: BlockRef) -> Self {
        Self {
            base: BaseRailBlock::new(block, true),
        }
    }

    fn find_powered_rail_signal(
        &self,
        world: &Arc<World>,
        pos: BlockPos,
        state: BlockStateId,
        forward: bool,
        search_depth: i32,
    ) -> bool {
        if search_depth >= Self::MAX_SEARCH_DEPTH {
            return false;
        }

        let mut x = pos.x();
        let mut y = pos.y();
        let mut z = pos.z();
        let mut check_below = true;
        let expected_shape = match state.get_value(&BlockStateProperties::RAIL_SHAPE) {
            RailShape::NorthSouth => {
                z += if forward { 1 } else { -1 };
                RailShape::NorthSouth
            }
            RailShape::EastWest => {
                x += if forward { -1 } else { 1 };
                RailShape::EastWest
            }
            RailShape::AscendingEast => {
                if forward {
                    x -= 1;
                } else {
                    x += 1;
                    y += 1;
                    check_below = false;
                }
                RailShape::EastWest
            }
            RailShape::AscendingWest => {
                if forward {
                    x -= 1;
                    y += 1;
                    check_below = false;
                } else {
                    x += 1;
                }
                RailShape::EastWest
            }
            RailShape::AscendingNorth => {
                if forward {
                    z += 1;
                } else {
                    z -= 1;
                    y += 1;
                    check_below = false;
                }
                RailShape::NorthSouth
            }
            RailShape::AscendingSouth => {
                if forward {
                    z += 1;
                    y += 1;
                    check_below = false;
                } else {
                    z -= 1;
                }
                RailShape::NorthSouth
            }
            RailShape::SouthEast
            | RailShape::SouthWest
            | RailShape::NorthWest
            | RailShape::NorthEast => return false,
        };

        let next = BlockPos::new(x, y, z);
        self.is_same_rail_with_power(world, next, forward, search_depth, expected_shape)
            || (check_below
                && self.is_same_rail_with_power(
                    world,
                    next.below(),
                    forward,
                    search_depth,
                    expected_shape,
                ))
    }

    fn is_same_rail_with_power(
        &self,
        world: &Arc<World>,
        pos: BlockPos,
        forward: bool,
        search_depth: i32,
        expected_shape: RailShape,
    ) -> bool {
        let state = world.get_block_state(pos);
        if state.get_block() != self.base.block {
            return false;
        }

        let shape = state.get_value(&BlockStateProperties::RAIL_SHAPE);
        let incompatible = match expected_shape {
            RailShape::EastWest => matches!(
                shape,
                RailShape::NorthSouth | RailShape::AscendingNorth | RailShape::AscendingSouth
            ),
            RailShape::NorthSouth => matches!(
                shape,
                RailShape::EastWest | RailShape::AscendingEast | RailShape::AscendingWest
            ),
            RailShape::AscendingEast
            | RailShape::AscendingWest
            | RailShape::AscendingNorth
            | RailShape::AscendingSouth
            | RailShape::SouthEast
            | RailShape::SouthWest
            | RailShape::NorthWest
            | RailShape::NorthEast => true,
        };
        if incompatible || !state.get_value(&BlockStateProperties::POWERED) {
            return false;
        }

        world.has_neighbor_signal(pos)
            || self.find_powered_rail_signal(world, pos, state, forward, search_depth + 1)
    }

    fn update_powered_state(&self, state: BlockStateId, world: &Arc<World>, pos: BlockPos) {
        let was_powered = state.get_value(&BlockStateProperties::POWERED);
        let should_power = world.has_neighbor_signal(pos)
            || self.find_powered_rail_signal(world, pos, state, true, 0)
            || self.find_powered_rail_signal(world, pos, state, false, 0);
        if should_power == was_powered {
            return;
        }

        world.set_block(
            pos,
            state.set_value(&BlockStateProperties::POWERED, should_power),
            UpdateFlags::UPDATE_ALL,
        );
        world.update_neighbors_at(pos.below(), self.base.block);
        if state
            .get_value(&BlockStateProperties::RAIL_SHAPE)
            .is_slope()
        {
            world.update_neighbors_at(pos.above(), self.base.block);
        }
    }
}

impl RailBehavior for PoweredRailBlock {
    fn is_straight(&self) -> bool {
        self.base.is_straight()
    }
}

impl BlockBehavior for PoweredRailBlock {
    fn get_state_for_placement(&self, context: &BlockPlaceContext<'_>) -> Option<BlockStateId> {
        Some(self.base.state_for_placement(context))
    }

    fn can_survive(&self, _state: BlockStateId, world: &dyn LevelReader, pos: BlockPos) -> bool {
        BaseRailBlock::can_survive(world, pos)
    }

    fn update_shape(
        &self,
        state: BlockStateId,
        world: &dyn ScheduledTickAccess,
        pos: BlockPos,
        _direction: Direction,
        _neighbor_pos: BlockPos,
        _neighbor_state: BlockStateId,
    ) -> BlockStateId {
        BaseRailBlock::update_shape(state, world, pos)
    }

    fn on_place(
        &self,
        state: BlockStateId,
        world: &Arc<World>,
        pos: BlockPos,
        old_state: BlockStateId,
        moved_by_piston: bool,
    ) {
        if old_state.get_block() != self.base.block {
            let _ = self
                .base
                .update_state_on_place(state, world, pos, moved_by_piston);
        }
    }

    fn handle_neighbor_changed(
        &self,
        state: BlockStateId,
        world: &Arc<World>,
        pos: BlockPos,
        _source_block: BlockRef,
        moved_by_piston: bool,
    ) {
        if self
            .base
            .handle_neighbor_changed(state, world, pos, moved_by_piston)
        {
            self.update_powered_state(state, world, pos);
        }
    }

    fn affect_neighbors_after_removal(
        &self,
        state: BlockStateId,
        world: &Arc<World>,
        pos: BlockPos,
        moved_by_piston: bool,
    ) {
        self.base
            .affect_neighbors_after_removal(state, world, pos, moved_by_piston);
    }

    fn as_rail(&self) -> Option<&dyn RailBehavior> {
        Some(self)
    }
}

#[cfg(test)]
mod tests {
    use steel_registry::test_support::init_test_registry;
    use steel_registry::vanilla_blocks;
    use steel_utils::ChunkPos;

    use super::*;
    use crate::behavior::init_behaviors;
    use crate::test_support::{fresh_test_world, insert_ready_full_chunk};

    fn raw_flags() -> UpdateFlags {
        UpdateFlags::UPDATE_NONE | UpdateFlags::UPDATE_SKIP_ON_PLACE
    }

    fn powered_chain_world(key: &'static str, last_x: i32) -> (Arc<World>, BlockPos) {
        init_test_registry();
        init_behaviors();
        let world = fresh_test_world(key);
        let start = BlockPos::new(8, 64, 8);
        insert_ready_full_chunk(&world, ChunkPos::from_block_pos(start));
        let end_chunk = ChunkPos::from_block_pos(start.offset(last_x, 0, 0));
        if end_chunk != ChunkPos::from_block_pos(start) {
            insert_ready_full_chunk(&world, end_chunk);
        }
        for x in 0..=last_x {
            let pos = start.offset(x, 0, 0);
            world.set_block(
                pos.below(),
                vanilla_blocks::STONE.default_state(),
                raw_flags(),
            );
            let state = vanilla_blocks::POWERED_RAIL
                .default_state()
                .set_value(&BlockStateProperties::RAIL_SHAPE, RailShape::EastWest)
                .set_value(&BlockStateProperties::POWERED, x != 0);
            world.set_block(pos, state, raw_flags());
        }
        world.set_block(
            start.offset(last_x, 1, 0),
            vanilla_blocks::REDSTONE_BLOCK.default_state(),
            raw_flags(),
        );
        (world, start)
    }

    #[test]
    fn powered_signal_reaches_exact_vanilla_depth_limit() {
        let behavior = PoweredRailBlock::new(&vanilla_blocks::POWERED_RAIL);

        let (within_world, start) = powered_chain_world("powered_rail_depth_eight", 8);
        let start_state = within_world.get_block_state(start);
        assert!(behavior.find_powered_rail_signal(&within_world, start, start_state, false, 0,));

        let (outside_world, start) = powered_chain_world("powered_rail_depth_nine", 9);
        let start_state = outside_world.get_block_state(start);
        assert!(!behavior.find_powered_rail_signal(&outside_world, start, start_state, false, 0,));
    }

    #[test]
    fn powered_propagation_requires_exact_block_identity() {
        init_test_registry();
        init_behaviors();
        let world = fresh_test_world("powered_rail_activator_isolation");
        let start = BlockPos::new(8, 64, 8);
        insert_ready_full_chunk(&world, ChunkPos::from_block_pos(start));
        for pos in [start, start.east()] {
            world.set_block(
                pos.below(),
                vanilla_blocks::STONE.default_state(),
                raw_flags(),
            );
        }
        let start_state = vanilla_blocks::POWERED_RAIL
            .default_state()
            .set_value(&BlockStateProperties::RAIL_SHAPE, RailShape::EastWest);
        world.set_block(start, start_state, raw_flags());
        world.set_block(
            start.east(),
            vanilla_blocks::ACTIVATOR_RAIL
                .default_state()
                .set_value(&BlockStateProperties::RAIL_SHAPE, RailShape::EastWest)
                .set_value(&BlockStateProperties::POWERED, true),
            raw_flags(),
        );
        world.set_block(
            start.east().above(),
            vanilla_blocks::REDSTONE_BLOCK.default_state(),
            raw_flags(),
        );

        let behavior = PoweredRailBlock::new(&vanilla_blocks::POWERED_RAIL);
        assert!(!behavior.find_powered_rail_signal(&world, start, start_state, false, 0,));
    }
}
