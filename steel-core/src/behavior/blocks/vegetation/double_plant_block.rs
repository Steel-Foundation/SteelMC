use std::sync::Arc;

use steel_macros::block_behavior;
use steel_registry::blocks::block_state_ext::BlockStateExt;
use steel_registry::blocks::properties::{BlockStateProperties, Direction, DoubleBlockHalf};
use steel_registry::item_stack::ItemStack;
use steel_registry::vanilla_blocks;
use steel_utils::{
    BlockPos, BlockStateId,
    axis::Axis,
    types::{InteractionHand, UpdateFlags},
};

use crate::behavior::block::BlockBehavior;
use crate::behavior::blocks::vegetation::Vegetation;
use crate::behavior::blocks::vegetation::default_surviving_state;
use crate::behavior::blocks::vegetation::vegetation_block::double_plant_can_survive;
use crate::behavior::context::{BlockPlaceContext, PlacementSource};
use crate::block_entity::BlockEntity;
use crate::entity::Entity;
use crate::fluid::{FluidStateExt as _, fluid_state_to_block, get_fluid_state};
use crate::player::Player;
use crate::world::{LevelReader, ScheduledTickAccess, World};

use super::BlockRef;

/// Behavior for vanilla two-block-tall plants.
#[block_behavior]
pub struct DoublePlantBlock {
    block: BlockRef,
}

impl DoublePlantBlock {
    /// Creates a new double plant block behavior.
    #[must_use]
    pub const fn new(block: BlockRef) -> Self {
        Self { block }
    }

    pub(super) fn copy_waterlogged_from(
        world: &Arc<World>,
        pos: BlockPos,
        state: BlockStateId,
    ) -> BlockStateId {
        if state
            .try_get_value(&BlockStateProperties::WATERLOGGED)
            .is_some()
        {
            state.set_value(
                &BlockStateProperties::WATERLOGGED,
                get_fluid_state(world, pos).is_water(),
            )
        } else {
            state
        }
    }

    pub(in crate::behavior::blocks) fn prevent_drop_from_bottom_part(
        world: &Arc<World>,
        pos: BlockPos,
        state: BlockStateId,
        player: &Player,
    ) {
        if state.get_value(&BlockStateProperties::DOUBLE_BLOCK_HALF) != DoubleBlockHalf::Upper {
            return;
        }

        let bottom_pos = pos.below();
        let bottom_state = world.get_block_state(bottom_pos);
        if bottom_state.get_block() != state.get_block()
            || bottom_state.get_value(&BlockStateProperties::DOUBLE_BLOCK_HALF)
                != DoubleBlockHalf::Lower
        {
            return;
        }

        let replacement = fluid_state_to_block(bottom_state.get_fluid_state());
        world.set_block(
            bottom_pos,
            replacement,
            UpdateFlags::UPDATE_ALL | UpdateFlags::UPDATE_SUPPRESS_DROPS,
        );
        world.destroy_block_effect(bottom_pos, u32::from(bottom_state.0), Some(player.id()));
    }

    pub(in crate::behavior::blocks) fn player_will_destroy_base(
        state: BlockStateId,
        world: &Arc<World>,
        pos: BlockPos,
        player: &Player,
    ) -> BlockStateId {
        if player.has_infinite_materials() {
            Self::prevent_drop_from_bottom_part(world, pos, state, player);
        } else {
            let destroyed_with = {
                let inventory = player.inventory.lock();
                let main_hand = inventory.get_item_in_hand(InteractionHand::MainHand);
                main_hand.copy_with_count(main_hand.count)
            };
            world.drop_resources_for_player(state, pos, player, None, &destroyed_with);
        }
        state
    }

    pub(in crate::behavior::blocks) fn player_destroy_base(
        world: &Arc<World>,
        pos: BlockPos,
        player: &Player,
        destroyed_with: &ItemStack,
    ) {
        world.drop_resources_for_player(
            vanilla_blocks::AIR.default_state(),
            pos,
            player,
            None,
            destroyed_with,
        );
    }

    /// Runs Vanilla `DoublePlantBlock.updateShape` while preserving virtual
    /// `canSurvive` dispatch for subclasses such as small dripleaf.
    pub(super) fn update_shape_with_survival(
        &self,
        survival_behavior: &dyn BlockBehavior,
        state: BlockStateId,
        world: &dyn ScheduledTickAccess,
        pos: BlockPos,
        direction: Direction,
        neighbor_state: BlockStateId,
    ) -> BlockStateId {
        let half = state.get_value(&BlockStateProperties::DOUBLE_BLOCK_HALF);
        let neighbor_is_matching_other_half = neighbor_state.get_block() == self.block
            && neighbor_state.get_value(&BlockStateProperties::DOUBLE_BLOCK_HALF) != half;

        if direction.get_axis() == Axis::Y
            && (half == DoubleBlockHalf::Lower) == (direction == Direction::Up)
            && !neighbor_is_matching_other_half
        {
            return vanilla_blocks::AIR.default_state();
        }

        if half == DoubleBlockHalf::Lower
            && direction == Direction::Down
            && !survival_behavior.can_survive(state, world, pos)
        {
            return vanilla_blocks::AIR.default_state();
        }

        state
    }
}

impl Vegetation for DoublePlantBlock {}

impl BlockBehavior for DoublePlantBlock {
    fn player_will_destroy(
        &self,
        state: BlockStateId,
        world: &Arc<World>,
        pos: BlockPos,
        player: &Player,
    ) -> BlockStateId {
        Self::player_will_destroy_base(state, world, pos, player)
    }

    fn player_destroy(
        &self,
        _state: BlockStateId,
        world: &Arc<World>,
        pos: BlockPos,
        player: &Player,
        _block_entity: Option<&dyn BlockEntity>,
        destroyed_with: &ItemStack,
    ) {
        Self::player_destroy_base(world, pos, player, destroyed_with);
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
        self.update_shape_with_survival(self, state, world, pos, direction, neighbor_state)
    }

    fn can_survive(&self, state: BlockStateId, world: &dyn LevelReader, pos: BlockPos) -> bool {
        double_plant_can_survive(self, state, world, pos)
    }

    fn set_placed_by(
        &self,
        _state: BlockStateId,
        world: &Arc<World>,
        pos: BlockPos,
        _source: &PlacementSource<'_>,
    ) {
        let upper_pos = pos.above();
        let upper_state = Self::copy_waterlogged_from(
            world,
            upper_pos,
            self.block.default_state().set_value(
                &BlockStateProperties::DOUBLE_BLOCK_HALF,
                DoubleBlockHalf::Upper,
            ),
        );
        world.set_block(upper_pos, upper_state, UpdateFlags::UPDATE_ALL);
    }

    fn get_state_for_placement(&self, context: &BlockPlaceContext<'_>) -> Option<BlockStateId> {
        if context.place_pos().y() >= context.world.max_y_exclusive() - 1 {
            return None;
        }
        if !context
            .world
            .get_block_state(context.place_pos().above())
            .is_replaceable()
        {
            return None;
        }
        default_surviving_state(self.block, self, context)
    }
}
