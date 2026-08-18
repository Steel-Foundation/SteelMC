use std::sync::Arc;

use steel_macros::block_behavior;
use steel_registry::blocks::block_state_ext::BlockStateExt;
use steel_registry::vanilla_block_tags::BlockTag;
use steel_registry::vanilla_fluid_tags::FluidTag;
use steel_utils::{BlockPos, BlockStateId};

use crate::behavior::block::BlockBehavior;
use crate::behavior::blocks::vegetation::vegetation_block::survival_update_shape;
use crate::behavior::context::BlockPlaceContext;
use crate::entity::{Entity, InsideBlockEffectCollector};
use crate::fluid::get_fluid_state_from_block;
use crate::world::{LevelReader, World};

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

        log::info!("=========");
        log::info!(
            "{}",
            below_fluid.fluid_id.has_tag(&FluidTag::SUPPORTS_LILY_PAD)
        );
        log::info!(
            "{}",
            below.get_block().has_tag(&BlockTag::SUPPORTS_LILY_PAD)
        );
        log::info!("{}", above_fluid.is_empty());

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
        log::info!("333");
        if entity.entity_type().is_abstract_boat {
            world.destroy_block_by_entity(pos, true, entity);
        }
    }

    fn update_shape(
        &self,
        state: BlockStateId,
        world: &dyn crate::world::ScheduledTickAccess,
        pos: BlockPos,
        _direction: steel_utils::Direction,
        _neighbor_pos: BlockPos,
        _neighbor_state: BlockStateId,
    ) -> BlockStateId {
        log::info!("222");
        survival_update_shape(self, state, world, pos)
    }

    fn can_survive(&self, _state: BlockStateId, world: &dyn LevelReader, pos: BlockPos) -> bool {
        log::info!("1");
        let below = pos.below();
        Self::may_place_on(world, below)
    }

    fn get_state_for_placement(&self, _context: &BlockPlaceContext<'_>) -> Option<BlockStateId> {
        log::info!("111");
        Some(self.block.default_state())
    }
}
