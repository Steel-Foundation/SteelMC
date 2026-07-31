use std::sync::Arc;

use steel_registry::blocks::BlockRef;
use steel_registry::blocks::block_state_ext::BlockStateExt as _;
use steel_registry::blocks::properties::{BlockStateProperties, RailShape};
use steel_registry::blocks::shapes::SupportType;
use steel_registry::{vanilla_block_tags::BlockTag, vanilla_fluids};
use steel_utils::{BlockPos, BlockStateId, Direction};

use crate::behavior::{BLOCK_BEHAVIORS, BlockPlaceContext};
use crate::world::{LevelReader, ScheduledTickAccess, SignalGetter as _, World};

use super::rail_state::RailState;

/// Shared server behavior inherited from vanilla's `BaseRailBlock`.
pub(super) struct BaseRailBlock {
    pub(super) block: BlockRef,
    is_straight: bool,
}

impl BaseRailBlock {
    #[must_use]
    pub(super) const fn new(block: BlockRef, is_straight: bool) -> Self {
        Self { block, is_straight }
    }

    #[must_use]
    pub(super) const fn is_straight(&self) -> bool {
        self.is_straight
    }

    #[must_use]
    pub(super) fn is_rail_state(state: BlockStateId) -> bool {
        let block = state.get_block();
        block.has_tag(&BlockTag::RAILS) && BLOCK_BEHAVIORS.get_behavior(block).as_rail().is_some()
    }

    fn can_support_rigid_block(level: &dyn LevelReader, pos: BlockPos) -> bool {
        level.is_face_sturdy_for(
            level.get_block_state(pos),
            pos,
            Direction::Up,
            SupportType::Rigid,
        )
    }

    #[must_use]
    pub(super) fn can_survive(level: &dyn LevelReader, pos: BlockPos) -> bool {
        Self::can_support_rigid_block(level, pos.below())
    }

    #[must_use]
    pub(super) fn state_for_placement(&self, context: &BlockPlaceContext<'_>) -> BlockStateId {
        let horizontal = context.horizontal_direction();
        let shape = if matches!(horizontal, Direction::East | Direction::West) {
            RailShape::EastWest
        } else {
            RailShape::NorthSouth
        };
        self.block
            .default_state()
            .set_value(&BlockStateProperties::RAIL_SHAPE, shape)
            .set_value(
                &BlockStateProperties::WATERLOGGED,
                context.is_water_source(),
            )
    }

    pub(super) fn update_shape(
        state: BlockStateId,
        level: &dyn ScheduledTickAccess,
        pos: BlockPos,
    ) -> BlockStateId {
        if state.get_value(&BlockStateProperties::WATERLOGGED) {
            let delay = level.fluid_tick_delay(&vanilla_fluids::WATER);
            level.schedule_fluid_tick_default(pos, &vanilla_fluids::WATER, delay);
        }
        state
    }

    /// Runs vanilla's initial topology update and straight-rail redstone check.
    #[must_use]
    pub(super) fn update_state_on_place(
        &self,
        state: BlockStateId,
        world: &Arc<World>,
        pos: BlockPos,
        moved_by_piston: bool,
    ) -> BlockStateId {
        let state = Self::update_dir(world, pos, state, true);
        if self.is_straight {
            world.neighbor_changed_with_state(state, pos, self.block, moved_by_piston);
        }
        state
    }

    #[must_use]
    pub(super) fn update_dir(
        world: &Arc<World>,
        pos: BlockPos,
        state: BlockStateId,
        first: bool,
    ) -> BlockStateId {
        let current = state.get_value(&BlockStateProperties::RAIL_SHAPE);
        let Some(mut rail) = RailState::new(world, pos, state) else {
            return state;
        };
        rail.place(world.has_neighbor_signal(pos), first, current)
    }

    fn should_be_removed(state: BlockStateId, world: &Arc<World>, pos: BlockPos) -> bool {
        if !Self::can_support_rigid_block(world.as_ref(), pos.below()) {
            return true;
        }

        match state.get_value(&BlockStateProperties::RAIL_SHAPE) {
            RailShape::AscendingEast => !Self::can_support_rigid_block(world.as_ref(), pos.east()),
            RailShape::AscendingWest => !Self::can_support_rigid_block(world.as_ref(), pos.west()),
            RailShape::AscendingNorth => {
                !Self::can_support_rigid_block(world.as_ref(), pos.north())
            }
            RailShape::AscendingSouth => {
                !Self::can_support_rigid_block(world.as_ref(), pos.south())
            }
            RailShape::NorthSouth
            | RailShape::EastWest
            | RailShape::SouthEast
            | RailShape::SouthWest
            | RailShape::NorthWest
            | RailShape::NorthEast => false,
        }
    }

    /// Returns whether the rail remains in place and subclass handling may run.
    pub(super) fn handle_neighbor_changed(
        &self,
        state: BlockStateId,
        world: &Arc<World>,
        pos: BlockPos,
        moved_by_piston: bool,
    ) -> bool {
        if world.get_block_state(pos).get_block() != self.block {
            return false;
        }
        if !Self::should_be_removed(state, world, pos) {
            return true;
        }

        world.drop_resources(state, pos);
        world.remove_block(pos, moved_by_piston);
        false
    }

    pub(super) fn affect_neighbors_after_removal(
        &self,
        state: BlockStateId,
        world: &Arc<World>,
        pos: BlockPos,
        moved_by_piston: bool,
    ) {
        if moved_by_piston {
            return;
        }
        if state
            .get_value(&BlockStateProperties::RAIL_SHAPE)
            .is_slope()
        {
            world.update_neighbors_at(pos.above(), self.block);
        }
        if self.is_straight {
            world.update_neighbors_at(pos, self.block);
            world.update_neighbors_at(pos.below(), self.block);
        }
    }
}

#[cfg(test)]
mod tests {
    use steel_registry::test_support::init_test_registry;
    use steel_registry::vanilla_blocks;

    use super::*;
    use crate::behavior::{BLOCK_BEHAVIORS, init_behaviors};

    #[test]
    fn rail_capability_requires_both_tag_and_behavior() {
        init_test_registry();
        init_behaviors();

        assert!(BaseRailBlock::is_rail_state(
            vanilla_blocks::RAIL.default_state()
        ));
        assert!(!BaseRailBlock::is_rail_state(
            vanilla_blocks::STONE.default_state()
        ));
        assert!(
            BLOCK_BEHAVIORS
                .get_behavior(&vanilla_blocks::POWERED_RAIL)
                .is_rail()
        );
    }
}
