use std::sync::Arc;

use steel_macros::block_behavior;
use steel_registry::blocks::BlockRef;
use steel_registry::blocks::block_state_ext::BlockStateExt as _;
use steel_utils::{BlockPos, BlockStateId, Direction};

use crate::behavior::{BLOCK_BEHAVIORS, BlockBehavior, BlockPlaceContext, RailBehavior};
use crate::world::{LevelReader, ScheduledTickAccess, SignalQueryContext, World};

use super::base_rail_block::BaseRailBlock;
use super::rail_state::RailState;

/// Vanilla ordinary rail, including ordered curve and junction switching.
#[block_behavior]
pub struct RailBlock {
    base: BaseRailBlock,
}

impl RailBlock {
    /// Creates ordinary rail behavior for `block`.
    #[must_use]
    pub const fn new(block: BlockRef) -> Self {
        Self {
            base: BaseRailBlock::new(block, false),
        }
    }
}

impl RailBehavior for RailBlock {
    fn is_straight(&self) -> bool {
        self.base.is_straight()
    }
}

impl BlockBehavior for RailBlock {
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
        source_block: BlockRef,
        moved_by_piston: bool,
    ) {
        if !self
            .base
            .handle_neighbor_changed(state, world, pos, moved_by_piston)
        {
            return;
        }

        let source_behavior = BLOCK_BEHAVIORS.get_behavior(source_block);
        if !source_behavior
            .is_signal_source(source_block.default_state(), SignalQueryContext::DEFAULT)
        {
            return;
        }
        let Some(rail) = RailState::new(world, pos, state) else {
            return;
        };
        if rail.count_potential_connections() == 3 {
            let _ = BaseRailBlock::update_dir(world, pos, state, false);
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
    use steel_registry::blocks::properties::{BlockStateProperties, RailShape};
    use steel_registry::test_support::init_test_registry;
    use steel_registry::vanilla_blocks;
    use steel_utils::ChunkPos;
    use steel_utils::types::UpdateFlags;

    use super::*;
    use crate::behavior::{BLOCK_BEHAVIORS, init_behaviors};
    use crate::test_support::{fresh_test_world, insert_ready_full_chunk};

    #[test]
    fn powered_three_way_junction_uses_vanilla_curve_priority() {
        init_test_registry();
        init_behaviors();
        let world = fresh_test_world("rail_three_way_switch");
        let center = BlockPos::new(8, 64, 8);
        insert_ready_full_chunk(&world, ChunkPos::from_block_pos(center));
        let raw_flags = UpdateFlags::UPDATE_NONE | UpdateFlags::UPDATE_SKIP_ON_PLACE;

        for pos in [center, center.north(), center.south(), center.east()] {
            world.set_block(
                pos.below(),
                vanilla_blocks::STONE.default_state(),
                raw_flags,
            );
        }
        for (pos, shape) in [
            (center.north(), RailShape::NorthSouth),
            (center.south(), RailShape::NorthSouth),
            (center.east(), RailShape::EastWest),
        ] {
            world.set_block(
                pos,
                vanilla_blocks::RAIL
                    .default_state()
                    .set_value(&BlockStateProperties::RAIL_SHAPE, shape),
                raw_flags,
            );
        }
        let state = vanilla_blocks::RAIL
            .default_state()
            .set_value(&BlockStateProperties::RAIL_SHAPE, RailShape::SouthEast);
        world.set_block(center, state, raw_flags);
        world.set_block(
            center.west(),
            vanilla_blocks::REDSTONE_BLOCK.default_state(),
            raw_flags,
        );

        BLOCK_BEHAVIORS
            .get_behavior(&vanilla_blocks::RAIL)
            .handle_neighbor_changed(
                state,
                &world,
                center,
                &vanilla_blocks::REDSTONE_BLOCK,
                false,
            );

        assert_eq!(
            world
                .get_block_state(center)
                .get_value(&BlockStateProperties::RAIL_SHAPE),
            RailShape::NorthEast
        );
    }
}
