use std::sync::Arc;

use rand::Rng;
use steel_macros::block_behavior;
use steel_registry::{vanilla_block_tags::BlockTag, vanilla_blocks};
use steel_utils::{BlockPos, BlockStateId, Direction, types::UpdateFlags};

use crate::behavior::{
    block::BlockBehavior,
    blocks::vegetation::bonemealable::{
        BonemealAction, Bonemealable, find_spreadable_neighbor_pos, has_spreadable_neighbor_pos,
    },
    context::BlockPlaceContext,
};
use crate::world::{LevelReader, ScheduledTickAccess, World};

use super::{
    BlockRef, default_surviving_state, survives_on_tag, vegetation_block::survival_update_shape,
};

/// Vanilla `TallDryGrassBlock` survival
#[block_behavior]
pub struct TallDryGrassBlock {
    block: BlockRef,
}

impl TallDryGrassBlock {
    /// Creates a new tall dry grass block behavior.
    #[must_use]
    pub const fn new(block: BlockRef) -> Self {
        Self { block }
    }
}

impl BlockBehavior for TallDryGrassBlock {
    fn update_shape(
        &self,
        state: BlockStateId,
        world: &dyn ScheduledTickAccess,
        pos: BlockPos,
        _direction: Direction,
        _neighbor_pos: BlockPos,
        _neighbor_state: BlockStateId,
    ) -> BlockStateId {
        survival_update_shape(self, state, world, pos)
    }

    fn can_survive(&self, _state: BlockStateId, world: &dyn LevelReader, pos: BlockPos) -> bool {
        survives_on_tag(world, pos, &BlockTag::SUPPORTS_DRY_VEGETATION)
    }

    fn get_state_for_placement(&self, context: &BlockPlaceContext<'_>) -> Option<BlockStateId> {
        default_surviving_state(self.block, self, context)
    }

    fn as_bonemealable(&self) -> Option<&dyn Bonemealable> {
        Some(self)
    }
}

impl Bonemealable for TallDryGrassBlock {
    fn is_valid_bonemeal_target(
        &self,
        _state: BlockStateId,
        world: &dyn LevelReader,
        pos: BlockPos,
    ) -> bool {
        has_spreadable_neighbor_pos(world, pos, vanilla_blocks::SHORT_DRY_GRASS.default_state())
    }

    fn perform_bonemeal(
        &self,
        _state: BlockStateId,
        world: &Arc<World>,
        _rng: &mut dyn Rng,
        pos: BlockPos,
    ) {
        if let Some(spread_pos) = find_spreadable_neighbor_pos(
            world,
            pos,
            vanilla_blocks::SHORT_DRY_GRASS.default_state(),
        ) {
            world.set_block(
                spread_pos,
                vanilla_blocks::SHORT_DRY_GRASS.default_state(),
                UpdateFlags::UPDATE_ALL,
            );
        }
    }

    fn bonemeal_action_type(&self) -> BonemealAction {
        BonemealAction::NeighborSpreader
    }
}
