use std::sync::Arc;

use rand::Rng;
use steel_macros::block_behavior;
use steel_registry::blocks::{block_state_ext::BlockStateExt, properties::Direction};
use steel_registry::fluid::FluidRef;
use steel_registry::item_stack::ItemStack;
use steel_registry::{vanilla_blocks, vanilla_items};
use steel_utils::{BlockPos, BlockStateId};

use crate::behavior::blocks::KelpBlock;
use crate::behavior::blocks::vegetation::bonemealable::{BonemealAction, Bonemealable};
use crate::behavior::context::BlockPlaceContext;
use crate::behavior::{
    block::BlockBehavior, blocks::vegetation::growing_plant_body_block::GrowingPlantBodyBlock,
};
use crate::world::{LevelReader, ScheduledTickAccess, World};

use super::BlockRef;

/// Vanilla `KelpPlantBlock` survival and fluid state.
#[block_behavior]
pub struct KelpPlantBlock {
    base: GrowingPlantBodyBlock,
}

impl KelpPlantBlock {
    /// Creates a new kelp plant block behavior.
    #[must_use]
    pub const fn new(block: BlockRef) -> Self {
        Self {
            base: GrowingPlantBodyBlock::new(
                block,
                Direction::Up,
                true,
                &vanilla_blocks::KELP,
                Self::can_grow_into,
            )
            .with_update_head_after_converted_from_body(
                Self::update_head_after_converted_from_body,
            ),
        }
    }

    const fn update_head_after_converted_from_body(
        _body_state: BlockStateId,
        head_state: BlockStateId,
    ) -> BlockStateId {
        head_state
    }

    fn can_grow_into(state: BlockStateId) -> bool {
        state.get_block() == &vanilla_blocks::WATER
    }
}

impl BlockBehavior for KelpPlantBlock {
    fn can_survive(&self, _state: BlockStateId, world: &dyn LevelReader, pos: BlockPos) -> bool {
        KelpBlock::kelp_can_survive(world, pos)
    }

    fn get_clone_item_stack(
        &self,
        _block: BlockRef,
        _state: BlockStateId,
        _include_data: bool,
    ) -> Option<ItemStack> {
        Some(ItemStack::new(&vanilla_items::KELP))
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
        self.base.get_state_for_placement(context)
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
}

impl Bonemealable for KelpPlantBlock {
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
    use steel_registry::blocks::block_state_ext::BlockStateExt;
    use steel_registry::blocks::properties::{BlockStateProperties, IntProperty};
    use steel_registry::init_vanilla_registry;
    use steel_registry::vanilla_blocks;

    const AGE: &IntProperty = &BlockStateProperties::AGE_25;

    #[test]
    fn kelp_plant_update_shape_schedules_water_tick() {
        init_vanilla_registry();

        let kelp = KelpPlantBlock::new(&vanilla_blocks::KELP_PLANT);
        let level =
            TestLevel::default().with_default_block_state(vanilla_blocks::WATER.default_state());
        let state = vanilla_blocks::KELP_PLANT.default_state();

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
    fn kelp_plant_update_shape_schedules_break_tick_when_unsupported() {
        init_vanilla_registry();

        let kelp = KelpPlantBlock::new(&vanilla_blocks::KELP_PLANT);
        let level =
            TestLevel::default().with_default_block_state(vanilla_blocks::WATER.default_state());
        let state = vanilla_blocks::KELP_PLANT.default_state();

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
                .any(|tick| tick.block == &vanilla_blocks::KELP_PLANT && tick.delay == 1)
        );
    }

    #[test]
    fn kelp_plant_converts_to_random_aged_head_when_open_above() {
        init_vanilla_registry();

        let kelp = KelpPlantBlock::new(&vanilla_blocks::KELP_PLANT);
        let level =
            TestLevel::default().with_default_block_state(vanilla_blocks::WATER.default_state());
        let state = vanilla_blocks::KELP_PLANT.default_state();

        let updated = kelp.update_shape(
            state,
            &level,
            BlockPos::ZERO,
            Direction::Up,
            BlockPos::ZERO.above(),
            vanilla_blocks::WATER.default_state(),
        );

        assert_eq!(updated.get_block(), &vanilla_blocks::KELP);
        assert!(updated.get_value(AGE) < 25);
        assert!(level.scheduled_fluid_ticks.borrow().is_empty());
    }
}
