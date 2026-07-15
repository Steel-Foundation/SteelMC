use rand::Rng;
use steel_macros::block_behavior;
use steel_registry::blocks::block_state_ext::BlockStateExt;
use steel_registry::blocks::properties::{BlockStateProperties, BoolProperty, EnumProperty};
use steel_registry::fluid::{FluidState, FluidStateExt};
use steel_registry::vanilla_block_tags::BlockTag;
use steel_registry::{vanilla_blocks, vanilla_fluids};
use steel_utils::types::UpdateFlags;
use steel_utils::{BlockPos, BlockStateId, Direction};

use crate::behavior::BlockStateBehaviorExt;
use crate::behavior::blocks::BigDripleafBlock;
use crate::behavior::blocks::vegetation::bonemealable::BonemealAction;
use crate::behavior::context::BlockPlaceContext;
use crate::behavior::{block::BlockBehavior, blocks::vegetation::bonemealable::Bonemealable};
use crate::world::{LevelReader, ScheduledTickAccess, World};
use std::sync::Arc;

use super::BlockRef;

const WATERLOGGED: BoolProperty = BlockStateProperties::WATERLOGGED;
const FACING: EnumProperty<Direction> = BlockStateProperties::FACING;
/// Vanilla `BigDripleafStemBlock` survival.
///
/// Below must be stem or in `SUPPORTS_BIG_DRIPLEAF`; above must be stem or big
/// dripleaf head.
// TODO: Implement scheduled break on shape update and tick.
#[block_behavior]
pub struct BigDripleafStemBlock {
    block: BlockRef,
}

impl BigDripleafStemBlock {
    /// Creates a new big dripleaf stem block behavior.
    #[must_use]
    pub const fn new(block: BlockRef) -> Self {
        Self { block }
    }
    /// places a big dripleaf stem block in target position with specific properties
    pub fn place(
        world: &Arc<World>,
        pos: BlockPos,
        fluid_state: FluidState,
        facing: Direction,
    ) -> bool {
        let new_state = vanilla_blocks::BIG_DRIPLEAF_STEM
            .default_state()
            .set_value(
                &WATERLOGGED,
                fluid_state.is_source() && fluid_state.is_water(),
            )
            .set_value(&FACING, facing);
        world.set_block(pos, new_state, UpdateFlags::UPDATE_ALL)
    }
    fn get_top_connected_block(
        world: &dyn LevelReader,
        pos: BlockPos,
        body_block: BlockRef,
        growth_direction: Direction,
        head_block: BlockRef,
    ) -> Option<BlockPos> {
        let mut forward_pos = pos;
        let mut forward_state;

        loop {
            forward_pos = forward_pos.relative(growth_direction);
            forward_state = world.get_block_state(forward_pos);

            if forward_state.get_block() != body_block {
                break;
            }
        }

        if forward_state.get_block() == head_block {
            Some(forward_pos)
        } else {
            None
        }
    }
}

impl BlockBehavior for BigDripleafStemBlock {
    fn can_survive(&self, _state: BlockStateId, world: &dyn LevelReader, pos: BlockPos) -> bool {
        let below = world.get_block_state(pos.below());
        let below_block = below.get_block();

        let above = world.get_block_state(pos.above());
        let above_block = above.get_block();

        let below_check =
            below_block == self.block || below_block.has_tag(&BlockTag::SUPPORTS_BIG_DRIPLEAF);
        let above_check = above_block == self.block || above_block == &vanilla_blocks::BIG_DRIPLEAF;

        below_check && above_check
    }
    fn as_bonemealable(&self) -> Option<&dyn Bonemealable> {
        Some(self)
    }
    fn get_state_for_placement(&self, _context: &BlockPlaceContext<'_>) -> Option<BlockStateId> {
        Some(self.block.default_state())
    }

    fn update_shape(
        &self,
        state: BlockStateId,
        world: &dyn ScheduledTickAccess,
        pos: BlockPos,
        direction: Direction,
        _neighbor_pos: BlockPos,
        _neighbor_state: BlockStateId,
    ) -> BlockStateId {
        if (direction == Direction::Down || direction == Direction::Up)
            && !self.can_survive(state, world, pos)
        {
            world.schedule_block_tick_default(pos, self.block, 1);
        }
        if state.get_value(&WATERLOGGED) {
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
}
impl Bonemealable for BigDripleafStemBlock {
    fn is_valid_bonemeal_target(
        &self,
        _state: BlockStateId,
        world: &dyn LevelReader,
        pos: BlockPos,
    ) -> bool {
        let head_pos = Self::get_top_connected_block(
            world,
            pos,
            self.block,
            Direction::Up,
            &vanilla_blocks::BIG_DRIPLEAF,
        );
        match head_pos {
            Some(head_pos) => BigDripleafBlock::can_grow_into(world, head_pos.above()),
            None => false,
        }
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
        _rng: &mut dyn Rng,
        pos: BlockPos,
    ) {
        let forward_pos = Self::get_top_connected_block(
            world,
            pos,
            self.block,
            Direction::Up,
            &vanilla_blocks::BIG_DRIPLEAF,
        );
        let Some(head_pos) = forward_pos else {
            return;
        };
        let place_head_pos = head_pos.above();
        let facing = state.get_value(&FACING);
        Self::place(
            world,
            head_pos,
            world.get_block_state(head_pos).get_fluid_state(),
            facing,
        );
        BigDripleafBlock::place(
            world,
            place_head_pos,
            world.get_block_state(place_head_pos).get_fluid_state(),
            facing,
        );
    }

    fn bonemeal_action_type(&self) -> BonemealAction {
        BonemealAction::Grower
    }
}
