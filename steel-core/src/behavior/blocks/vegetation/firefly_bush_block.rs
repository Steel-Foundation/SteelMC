use rand::Rng;
use std::sync::Arc;
use steel_macros::block_behavior;
use steel_registry::vanilla_block_tags::BlockTag;
use steel_utils::types::UpdateFlags;
use steel_utils::{BlockPos, BlockStateId};

use crate::behavior::blocks::vegetation::bonemealable::{
    BonemealAction, find_spreadable_neighbor_pos, has_spreadable_neighbor_pos,
};
use crate::behavior::context::BlockPlaceContext;
use crate::behavior::{block::BlockBehavior, blocks::vegetation::bonemealable::Bonemealable};
use crate::world::{LevelReader, World};

use super::{BlockRef, default_surviving_state, survives_on_tag};

/// Vanilla `FireflyBushBlock` survival.
#[block_behavior]
pub struct FireflyBushBlock {
    block: BlockRef,
}

impl FireflyBushBlock {
    /// Creates a new firefly bush block behavior.
    #[must_use]
    pub const fn new(block: BlockRef) -> Self {
        Self { block }
    }
}

impl BlockBehavior for FireflyBushBlock {
    fn can_survive(&self, _state: BlockStateId, world: &dyn LevelReader, pos: BlockPos) -> bool {
        survives_on_tag(world, pos, &BlockTag::SUPPORTS_VEGETATION)
    }

    fn get_state_for_placement(&self, context: &BlockPlaceContext<'_>) -> Option<BlockStateId> {
        default_surviving_state(self.block, self, context)
    }
    fn as_bonemealable(&self) -> Option<&dyn super::bonemealable::Bonemealable> {
        Some(self)
    }
}
impl Bonemealable for FireflyBushBlock {
    fn is_valid_bonemeal_target(
        &self,
        state: BlockStateId,
        world: &dyn LevelReader,
        pos: BlockPos,
    ) -> bool {
        has_spreadable_neighbor_pos(world, pos, state)
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
        let Some(block_pos) = find_spreadable_neighbor_pos(world, pos, state) else {
            return;
        };
        world.set_block(
            block_pos,
            self.block.default_state(),
            UpdateFlags::UPDATE_ALL,
        );
    }

    fn bonemeal_action_type(&self) -> BonemealAction {
        BonemealAction::NeighborSpreader
    }
}
