use std::sync::Arc;

use crate::behavior::block::BlockBehavior;
use crate::behavior::blocks::vegetation::bonemealable::{BonemealAction, Bonemealable};
use crate::behavior::blocks::vegetation::growing_plant_block;
use crate::behavior::blocks::vegetation::growing_plant_head_block::{
    GrowingPlantHeadBehavior, GrowingPlantHeadBlock,
};
use crate::behavior::context::BlockPlaceContext;
use crate::world::{LevelReader, ScheduledTickAccess, World};

use rand::Rng;
use steel_macros::block_behavior;
use steel_registry::blocks::block_state_ext::BlockStateExt;
use steel_registry::blocks::properties::Direction;
use steel_registry::fluid::{FluidRef, FluidStateExt};
use steel_registry::item_stack::ItemStack;
use steel_registry::vanilla_block_tags::BlockTag;
use steel_registry::{vanilla_blocks, vanilla_items};
use steel_utils::{BlockPos, BlockStateId};

use super::BlockRef;

/// Vanilla `KelpBlock` survival and fluid state.
#[block_behavior]
pub struct KelpBlock {
    base: GrowingPlantHeadBlock,
}

const GROW_PER_TICK_PROBABILITY: f64 = 0.14;

impl KelpBlock {
    /// Creates a new kelp block behavior.
    #[must_use]
    pub const fn new(block: BlockRef) -> Self {
        Self {
            base: GrowingPlantHeadBlock::new(
                block,
                Direction::Up,
                true,
                GROW_PER_TICK_PROBABILITY,
                &vanilla_blocks::KELP_PLANT,
                Some(Self::get_blocks_to_grow_when_bonemealed),
                Self::can_grow_into,
            ),
        }
    }

    fn can_grow_into(state: BlockStateId) -> bool {
        state.get_block() == &vanilla_blocks::WATER
    }

    fn get_blocks_to_grow_when_bonemealed(_rng: &mut dyn Rng) -> i32 {
        1
    }
    pub(crate) fn kelp_can_survive(world: &dyn LevelReader, pos: BlockPos) -> bool {
        let attached_pos = pos.below();
        let attached_state = world.get_block_state(attached_pos);
        if attached_state
            .get_block()
            .has_tag(&BlockTag::CANNOT_SUPPORT_KELP)
        {
            return false;
        }
        growing_plant_block::can_survive(
            world,
            pos,
            Direction::Up,
            &vanilla_blocks::KELP,
            &vanilla_blocks::KELP_PLANT,
        )
    }
}

impl BlockBehavior for KelpBlock {
    fn can_survive(&self, _state: BlockStateId, world: &dyn LevelReader, pos: BlockPos) -> bool {
        Self::kelp_can_survive(world, pos)
    }

    fn get_clone_item_stack(
        &self,
        _block: BlockRef,
        _state: BlockStateId,
        _include_data: bool,
    ) -> Option<ItemStack> {
        Some(ItemStack::new(&vanilla_items::KELP))
    }

    fn random_tick(&self, state: BlockStateId, world: &Arc<World>, pos: BlockPos) {
        self.base.random_tick(state, world, pos);
    }

    fn update_shape(
        &self,
        state: BlockStateId,
        world: &dyn ScheduledTickAccess,
        pos: BlockPos,
        direction: Direction,
        neighbor_pos: BlockPos,
        neighbor_state: BlockStateId,
    ) -> BlockStateId {
        self.base
            .update_shape(state, world, pos, direction, neighbor_pos, neighbor_state)
    }

    fn get_state_for_placement(&self, context: &BlockPlaceContext<'_>) -> Option<BlockStateId> {
        let fluid_state = context
            .world
            .get_block_state(context.place_pos())
            .get_fluid_state();
        if fluid_state.is_water() && fluid_state.is_full() {
            return self.base.get_state_for_placement(context);
        }
        None
    }

    fn tick(&self, state: BlockStateId, world: &Arc<World>, pos: BlockPos) {
        self.base.tick(state, world, pos);
    }

    fn is_liquid_container(&self, _state: BlockStateId) -> bool {
        true
    }

    fn can_place_liquid(&self, _state: BlockStateId, _fluid: FluidRef) -> bool {
        false
    }

    fn as_bonemealable(&self) -> Option<&dyn Bonemealable> {
        Some(self)
    }

    fn as_growing_plant_head(&self) -> Option<&dyn GrowingPlantHeadBehavior> {
        Some(&self.base)
    }
}

impl Bonemealable for KelpBlock {
    fn is_valid_bonemeal_target(
        &self,
        state: BlockStateId,
        world: &dyn LevelReader,
        pos: BlockPos,
    ) -> bool {
        self.base.is_valid_bonemeal_target(state, world, pos)
    }

    fn perform_bonemeal(
        &self,
        state: BlockStateId,
        world: &Arc<World>,
        rng: &mut dyn Rng,
        pos: BlockPos,
    ) {
        self.base.perform_bonemeal(state, world, rng, pos);
    }

    fn bonemeal_action_type(&self) -> BonemealAction {
        BonemealAction::Grower
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::TestLevel;
    use steel_registry::init_vanilla_registry;

    #[test]
    fn kelp_update_shape_schedules_water_tick() {
        init_vanilla_registry();

        let kelp = KelpBlock::new(&vanilla_blocks::KELP);
        let level =
            TestLevel::default().with_default_block_state(vanilla_blocks::WATER.default_state());
        let state = vanilla_blocks::KELP.default_state();

        assert_eq!(
            kelp.update_shape(
                state,
                &level,
                BlockPos::ZERO,
                Direction::North,
                Direction::North.relative(BlockPos::ZERO),
                vanilla_blocks::WATER.default_state(),
            ),
            state
        );
        assert!(level.scheduled_water_tick());
    }

    #[test]
    fn kelp_head_update_shape_schedules_break_tick_when_unsupported() {
        init_vanilla_registry();

        let kelp = KelpBlock::new(&vanilla_blocks::KELP);
        let level =
            TestLevel::default().with_default_block_state(vanilla_blocks::WATER.default_state());
        let state = vanilla_blocks::KELP.default_state();

        let updated = kelp.update_shape(
            state,
            &level,
            BlockPos::ZERO,
            Direction::Down,
            BlockPos::ZERO.below(),
            vanilla_blocks::WATER.default_state(),
        );

        assert_eq!(updated, state);
        assert!(
            level
                .scheduled_block_ticks
                .borrow()
                .iter()
                .any(|tick| tick.block == &vanilla_blocks::KELP && tick.delay == 1)
        );
    }

    #[test]
    fn kelp_head_converts_to_body_when_connected_above() {
        init_vanilla_registry();

        let kelp = KelpBlock::new(&vanilla_blocks::KELP);
        let level =
            TestLevel::default().with_default_block_state(vanilla_blocks::WATER.default_state());
        let state = vanilla_blocks::KELP.default_state();

        let updated = kelp.update_shape(
            state,
            &level,
            BlockPos::ZERO,
            Direction::Up,
            BlockPos::ZERO.above(),
            vanilla_blocks::KELP_PLANT.default_state(),
        );

        assert_eq!(updated.get_block(), &vanilla_blocks::KELP_PLANT);
        assert!(level.scheduled_fluid_ticks.borrow().is_empty());
    }
}
