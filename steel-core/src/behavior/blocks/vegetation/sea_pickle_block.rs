use rand::RngExt;
use steel_macros::block_behavior;
use steel_registry::blocks::block_state_ext::BlockStateExt;
use steel_registry::blocks::properties::{
    BlockStateProperties, BoolProperty, Direction, IntProperty,
};
use steel_registry::fluid::FluidStateExt;
use steel_registry::vanilla_block_tags::BlockTag;
use steel_registry::{REGISTRY, vanilla_blocks};
use steel_utils::types::UpdateFlags;
use steel_utils::{BlockPos, BlockStateId};

use crate::behavior::block::{
    BlockBehavior, default_can_be_replaced, schedule_water_tick_if_waterlogged,
};
use crate::behavior::blocks::vegetation::bonemealable::Bonemealable;
use crate::behavior::context::BlockPlaceContext;
use crate::behavior::{BLOCK_BEHAVIORS, BlockCollisionContext};
use crate::entity::ai::path::PathComputationType;
use crate::fluid::get_fluid_state_from_block;
use crate::world::{LevelReader, ScheduledTickAccess};

use super::BlockRef;

/// Vanilla `SeaPickleBlock` survival.
#[block_behavior]
pub struct SeaPickleBlock {
    block: BlockRef,
}

const WATERLOGGED: &BoolProperty = &BlockStateProperties::WATERLOGGED;
const PICKLES: &IntProperty = &BlockStateProperties::PICKLES;

impl SeaPickleBlock {
    /// Creates a new sea pickle block behavior.
    #[must_use]
    pub const fn new(block: BlockRef) -> Self {
        Self { block }
    }

    fn may_place_on(world: &dyn LevelReader, state: BlockStateId, pos: BlockPos) -> bool {
        BLOCK_BEHAVIORS
            .get_behavior(state.get_block())
            .get_collision_boxes(state, world, pos, BlockCollisionContext::empty())
            .iter()
            .any(|aabb| !aabb.is_empty() && aabb.max_y() >= 1.0)
            || world.is_face_sturdy(state, pos, Direction::Up)
    }
    fn is_dead(state: BlockStateId) -> bool {
        !state.get_value(WATERLOGGED)
    }
}

impl BlockBehavior for SeaPickleBlock {
    fn update_shape(
        &self,
        state: BlockStateId,
        world: &dyn ScheduledTickAccess,
        pos: BlockPos,
        _direction: Direction,
        _neighbor_pos: BlockPos,
        _neighbor_state: BlockStateId,
    ) -> BlockStateId {
        if !self.can_survive(state, world, pos) {
            return vanilla_blocks::AIR.default_state();
        }

        schedule_water_tick_if_waterlogged(state, world, pos);
        state
    }

    fn can_survive(&self, _state: BlockStateId, world: &dyn LevelReader, pos: BlockPos) -> bool {
        let below_pos = pos.below();
        Self::may_place_on(world, world.get_block_state(below_pos), below_pos)
    }

    fn get_state_for_placement(&self, context: &BlockPlaceContext<'_>) -> Option<BlockStateId> {
        let clicked_pos = if context.replaces_clicked_block() {
            context.hit_pos()
        } else {
            context.place_pos()
        };
        let state = context.world.get_block_state(clicked_pos);
        if state.get_block() == self.block {
            return Some(state.set_value(PICKLES, 4.min(state.get_value(PICKLES) + 1)));
        }
        let replaced_fluid_state = get_fluid_state_from_block(state);
        let is_water_source = replaced_fluid_state.is_water();
        return Some(
            self.block
                .default_state()
                .set_value(WATERLOGGED, is_water_source),
        );
    }
    fn can_be_replaced(&self, state: BlockStateId, context: &BlockPlaceContext<'_>) -> bool {
        if !context.is_secondary_use_active()
            && context.with_item(|item| item.item() == REGISTRY.items.by_block(state.get_block()))
            && state.get_value(PICKLES) < 4
        {
            return true;
        }
        default_can_be_replaced(state, context)
    }
    fn is_pathfindable(
        &self,
        _state: BlockStateId,
        _computation_type: PathComputationType,
    ) -> bool {
        false
    }
    fn as_bonemealable(&self) -> Option<&dyn Bonemealable> {
        Some(self)
    }
}

impl Bonemealable for SeaPickleBlock {
    fn is_valid_bonemeal_target(
        &self,
        state: BlockStateId,
        world: &dyn LevelReader,
        pos: BlockPos,
    ) -> bool {
        !Self::is_dead(state)
            && world
                .get_block_state(pos.below())
                .get_block()
                .has_tag(&BlockTag::CORAL_BLOCKS)
    }
    fn perform_bonemeal(
        &self,
        state: BlockStateId,
        world: &std::sync::Arc<crate::world::World>,
        rng: &mut dyn rand::prelude::Rng,
        pos: BlockPos,
    ) {
        let _span = 5;
        let mut z_span = 1;
        let _height = 2;
        let mut count = 0;

        let x_start = pos.x() - 2;
        let mut z_offset = 0;

        for x in 0..5 {
            for z in 0..z_span {
                let end_y = 2 + pos.y() - 1;

                for start_y in (end_y - 2)..end_y {
                    let position = BlockPos::new(x_start + x, start_y, pos.z() - z_offset + z);

                    if position != pos
                        && rng.random_range(0..6) == 0
                        && world.get_block_state(position).get_block() == &vanilla_blocks::WATER
                    {
                        let below_state = world.get_block_state(position.below());

                        if below_state.get_block().has_tag(&BlockTag::CORAL_BLOCKS) {
                            let sea_pickle_state = vanilla_blocks::SEA_PICKLE
                                .default_state()
                                .set_value(PICKLES, rng.random_range(0..4) + 1);

                            world.set_block(position, sea_pickle_state, UpdateFlags::UPDATE_ALL);
                        }
                    }
                }
            }

            if count < 2 {
                z_span += 2;
                z_offset += 1;
            } else {
                z_span -= 2;
                z_offset -= 1;
            }

            count += 1;
        }

        let final_state = state.set_value(PICKLES, 4);

        world.set_block(pos, final_state, UpdateFlags::UPDATE_CLIENTS);
    }
}

#[cfg(test)]
mod tests {
    use steel_registry::blocks::properties::BlockStateProperties;
    use steel_registry::{init_vanilla_registry, vanilla_fluids};

    use super::*;
    use crate::behavior::init_behaviors;
    use crate::test_support::TestLevel;

    #[test]
    fn sea_pickle_checks_survival_before_scheduling_water() {
        init_vanilla_registry();
        init_behaviors();
        let behavior = SeaPickleBlock::new(&vanilla_blocks::SEA_PICKLE);
        let state = vanilla_blocks::SEA_PICKLE
            .default_state()
            .set_value(&BlockStateProperties::WATERLOGGED, true);
        let pos = BlockPos::new(0, 64, 0);
        let unsupported = TestLevel::default();

        assert!(
            behavior
                .update_shape(
                    state,
                    &unsupported,
                    pos,
                    Direction::North,
                    pos.north(),
                    vanilla_blocks::AIR.default_state(),
                )
                .is_air()
        );
        assert!(unsupported.scheduled_fluid_ticks.borrow().is_empty());

        let supported =
            TestLevel::default().with_block(pos.below(), vanilla_blocks::STONE.default_state());
        assert_eq!(
            behavior.update_shape(
                state,
                &supported,
                pos,
                Direction::North,
                pos.north(),
                vanilla_blocks::AIR.default_state(),
            ),
            state
        );
        assert_eq!(
            supported
                .scheduled_fluid_ticks
                .borrow()
                .iter()
                .map(|tick| (tick.fluid, tick.delay))
                .collect::<Vec<_>>(),
            vec![(&vanilla_fluids::WATER, 5)]
        );
    }
}
