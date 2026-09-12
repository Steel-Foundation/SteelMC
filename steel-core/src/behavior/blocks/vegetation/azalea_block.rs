use std::sync::Arc;

use rand::{Rng, RngExt};
use steel_macros::block_behavior;
use steel_registry::{
    blocks::{BlockRef, block_state_ext::BlockStateExt},
    vanilla_block_tags::BlockTag,
};
use steel_utils::{BlockPos, BlockStateId, Direction};

use crate::{
    behavior::{
        BlockBehavior, BlockPlaceContext,
        blocks::vegetation::{
            TreeGrower, Vegetation,
            bonemealable::Bonemealable,
            default_surviving_state,
            vegetation_block::{survival_update_shape, vegetation_can_survive},
        },
    },
    world::{LevelReader, ScheduledTickAccess, World},
};

const BONEMEAL_SUCCESS_CHANCE: f32 = 0.45;
/// Extra height vanilla requires above the tree's minimum height for bonemeal.
const BONEMEAL_HEIGHT_MARGIN: i32 = 2;

/// Vanilla `AzaleaBlock`: a bonemealable bush that grows into `TreeGrower.AZALEA`.
#[block_behavior]
pub struct AzaleaBlock {
    block: BlockRef,
}

impl AzaleaBlock {
    /// Creates a new azalea block behavior.
    #[must_use]
    pub const fn new(block: BlockRef) -> Self {
        Self { block }
    }
}

impl BlockBehavior for AzaleaBlock {
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

    fn can_survive(&self, state: BlockStateId, world: &dyn LevelReader, pos: BlockPos) -> bool {
        vegetation_can_survive(self, state, world, pos)
    }

    fn get_state_for_placement(&self, context: &BlockPlaceContext<'_>) -> Option<BlockStateId> {
        default_surviving_state(self.block, self, context)
    }

    fn as_bonemealable(&self) -> Option<&dyn Bonemealable> {
        Some(self)
    }
}

impl Bonemealable for AzaleaBlock {
    fn is_valid_bonemeal_target(
        &self,
        _state: BlockStateId,
        world: &dyn LevelReader,
        pos: BlockPos,
    ) -> bool {
        let min_height = TreeGrower::Azalea.minimum_height().unwrap_or(0);
        !world.is_outside_build_height(pos.above_n(min_height + BONEMEAL_HEIGHT_MARGIN).y())
            && world
                .get_block_state(pos.above())
                .get_fluid_state()
                .is_empty()
    }

    fn is_bonemeal_success(
        &self,
        _state: BlockStateId,
        _world: &Arc<World>,
        rng: &mut dyn Rng,
        _pos: BlockPos,
    ) -> bool {
        rng.random::<f32>() < BONEMEAL_SUCCESS_CHANCE
    }

    fn perform_bonemeal(
        &self,
        state: BlockStateId,
        world: &Arc<World>,
        rng: &mut dyn Rng,
        pos: BlockPos,
    ) {
        TreeGrower::Azalea.grow_tree(world, pos, state, rng);
    }
}

impl Vegetation for AzaleaBlock {
    fn may_place_on(&self, state: BlockStateId, _world: &dyn LevelReader, _pos: BlockPos) -> bool {
        state.get_block().has_tag(&BlockTag::SUPPORTS_AZALEA)
    }
}
