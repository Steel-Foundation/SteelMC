use rand::{Rng, rng};
use std::sync::Arc;
use steel_registry::{
    REGISTRY,
    blocks::{BlockRef, block_state_ext::BlockStateExt},
    vanilla_fluids,
};
use steel_utils::{BlockPos, BlockStateId, Direction};

use crate::{
    behavior::{
        BLOCK_BEHAVIORS, BlockBehavior, BlockPlaceContext,
        block::default_can_be_replaced,
        blocks::vegetation::{
            bonemealable::{BonemealAction, Bonemealable},
            get_top_connected_block, growing_plant_can_survive,
            growing_plant_head_block::GrowingPlantHeadBlock,
        },
    },
    world::{LevelReader, ScheduledTickAccess, World},
};

/// Shared behavior for growing plant blocks.
pub struct GrowingPlantBodyBlock {
    block: BlockRef,
    growth_direction: Direction,
    schedule_fluid_ticks: bool,
    head_block: BlockRef,
    update_head_after_converted_from_body: fn(BlockStateId, BlockStateId) -> BlockStateId,
    can_grow_into: fn(BlockStateId) -> bool,
}

impl GrowingPlantBodyBlock {
    /// Creates a new growing plant body behavior.
    #[must_use]
    pub const fn new(
        block: BlockRef,
        growth_direction: Direction,
        schedule_fluid_ticks: bool,
        head_block: BlockRef,
        can_grow_into: fn(BlockStateId) -> bool,
    ) -> Self {
        Self {
            block,
            growth_direction,
            schedule_fluid_ticks,
            head_block,
            update_head_after_converted_from_body: Self::unchanged_converted_state,
            can_grow_into,
        }
    }

    /// Configures the vanilla `updateHeadAfterConvertedFromBody` specialization.
    #[must_use]
    pub const fn with_update_head_after_converted_from_body(
        mut self,
        update: fn(BlockStateId, BlockStateId) -> BlockStateId,
    ) -> Self {
        self.update_head_after_converted_from_body = update;
        self
    }

    const fn unchanged_converted_state(
        _body_state: BlockStateId,
        head_state: BlockStateId,
    ) -> BlockStateId {
        head_state
    }
    fn get_head_pos(
        &self,
        world: &dyn LevelReader,
        pos: BlockPos,
        body_block: BlockRef,
    ) -> Option<BlockPos> {
        get_top_connected_block(
            world,
            pos,
            body_block,
            self.growth_direction,
            self.head_block,
        )
    }
}

impl BlockBehavior for GrowingPlantBodyBlock {
    fn can_be_replaced(&self, state: BlockStateId, context: &BlockPlaceContext<'_>) -> bool {
        let default_result = default_can_be_replaced(state, context);
        if !default_result {
            return false;
        }

        context.with_item(|item| item.item() != REGISTRY.items.by_block(self.head_block))
    }

    fn can_survive(&self, state: BlockStateId, world: &dyn LevelReader, pos: BlockPos) -> bool {
        growing_plant_can_survive(
            world,
            pos,
            self.growth_direction,
            self.head_block,
            state.get_block(),
        )
    }

    fn update_shape(
        &self,
        state: BlockStateId,
        world: &dyn ScheduledTickAccess,
        pos: BlockPos,
        direction: Direction,
        _neighbor_pos: BlockPos,
        neighbor_state: BlockStateId,
    ) -> BlockStateId {
        if direction == self.growth_direction.opposite() && !self.can_survive(state, world, pos) {
            world.schedule_block_tick_default(pos, self.block, 1);
        }
        let head_block = self.head_block;
        if direction == self.growth_direction
            && neighbor_state.get_block() != self.block
            && neighbor_state.get_block() != head_block
        {
            let mut rng = rng();
            return (self.update_head_after_converted_from_body)(
                state,
                GrowingPlantHeadBlock::get_head_state(self.head_block, &mut rng),
            );
        }
        if self.schedule_fluid_ticks {
            world.schedule_fluid_tick_default(
                pos,
                &vanilla_fluids::WATER,
                vanilla_fluids::WATER.tick_delay as i32,
            );
        }
        state
    }
    fn tick(&self, state: BlockStateId, world: &Arc<World>, pos: BlockPos) {
        if !self.can_survive(state, world, pos) {
            world.destroy_block(pos, true);
        }
    }
    fn get_state_for_placement(&self, _context: &BlockPlaceContext<'_>) -> Option<BlockStateId> {
        Some(self.block.default_state())
    }
    fn as_bonemealable(&self) -> Option<&dyn Bonemealable> {
        Some(self)
    }
}
impl Bonemealable for GrowingPlantBodyBlock {
    fn is_valid_bonemeal_target(
        &self,
        state: BlockStateId,
        world: &dyn LevelReader,
        pos: BlockPos,
    ) -> bool {
        let Some(head_pos) = self.get_head_pos(world, pos, state.get_block()) else {
            return false;
        };
        let growth_pos = head_pos.relative(self.growth_direction);
        (self.can_grow_into)(world.get_block_state(growth_pos))
            && !world.is_outside_build_height(growth_pos.y())
    }

    fn is_bonemeal_success(
        &self,
        _state: BlockStateId,
        _world: &Arc<World>,
        _rng: &mut dyn Rng,
        _pos: BlockPos,
    ) -> bool {
        true
    }

    fn perform_bonemeal(
        &self,
        state: BlockStateId,
        world: &Arc<World>,
        rng: &mut dyn Rng,
        pos: BlockPos,
    ) {
        let Some(head_pos) = self.get_head_pos(world, pos, state.get_block()) else {
            return;
        };
        let forward_state = world.get_block_state(head_pos);
        let Some(behavior) = BLOCK_BEHAVIORS
            .get_behavior(forward_state.get_block())
            .as_bonemealable()
        else {
            return;
        };
        behavior.perform_bonemeal(forward_state, world, rng, head_pos);
    }

    fn bonemeal_action_type(&self) -> BonemealAction {
        BonemealAction::Grower
    }
}
