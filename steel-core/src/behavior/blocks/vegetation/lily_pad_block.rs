use std::sync::Arc;

use steel_macros::block_behavior;
use steel_registry::blocks::block_state_ext::BlockStateExt;
use steel_registry::vanilla_block_tags::BlockTag;
use steel_registry::vanilla_fluid_tags::FluidTag;
use steel_utils::{BlockPos, BlockStateId, Direction};

use crate::behavior::block::BlockBehavior;
use crate::behavior::blocks::vegetation::vegetation_block::survival_update_shape;
use crate::behavior::context::BlockPlaceContext;
use crate::entity::{Entity, InsideBlockEffectCollector};
use crate::fluid::get_fluid_state_from_block;
use crate::world::{LevelReader, ScheduledTickAccess, World};

use super::BlockRef;

/// Vanilla `LilyPadBlock` survival.
#[block_behavior]
pub struct LilyPadBlock {
    block: BlockRef,
}

impl LilyPadBlock {
    /// Creates a new lily-pad block behavior.
    #[must_use]
    pub const fn new(block: BlockRef) -> Self {
        Self { block }
    }

    fn may_place_on(world: &dyn LevelReader, pos: BlockPos) -> bool {
        let below = world.get_block_state(pos);
        let below_fluid = get_fluid_state_from_block(below);
        let above_fluid = get_fluid_state_from_block(world.get_block_state(pos.above()));

        (below_fluid.fluid_id.has_tag(&FluidTag::SUPPORTS_LILY_PAD)
            || below.get_block().has_tag(&BlockTag::SUPPORTS_LILY_PAD))
            && above_fluid.is_empty()
    }
}

impl BlockBehavior for LilyPadBlock {
    fn entity_inside(
        &self,
        _state: BlockStateId,
        world: &Arc<World>,
        pos: BlockPos,
        entity: &dyn Entity,
        _effect_collector: &mut InsideBlockEffectCollector,
        _is_precise: bool,
    ) {
        if entity.entity_type().is_abstract_boat {
            world.destroy_block_by_entity(pos, true, entity);
        }
    }

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
        let below = pos.below();
        Self::may_place_on(world, below)
    }

    fn get_state_for_placement(&self, _context: &BlockPlaceContext<'_>) -> Option<BlockStateId> {
        Some(self.block.default_state())
    }
}
