use std::sync::Arc;

use steel_registry::blocks::{BlockRef, block_state_ext::BlockStateExt as _};
use steel_registry::vanilla_block_tags::BlockTag;
use steel_utils::{BlockPos, BlockStateId};

use crate::entity::entities::FallingBlockEntity;
use crate::world::{ScheduledTickAccess, World};

const FALL_DELAY: i32 = 2;

/// Shared server-side behavior of vanilla `FallingBlock`.
///
/// Vanilla's falling-dust `animateTick` particle is client-local.
pub struct FallingBlock {
    block: BlockRef,
}

impl FallingBlock {
    /// Creates the shared behavior for one registered falling block.
    #[must_use]
    pub const fn new(block: BlockRef) -> Self {
        Self { block }
    }

    /// Returns this behavior's registered block.
    #[must_use]
    pub const fn block(&self) -> BlockRef {
        self.block
    }

    /// Schedules the vanilla two-tick fall check after placement.
    pub fn on_place(&self, world: &Arc<World>, pos: BlockPos) {
        let _ = world.schedule_block_tick_default(pos, self.block, FALL_DELAY);
    }

    /// Schedules the vanilla two-tick fall check after every shape update.
    #[must_use]
    pub fn update_shape(
        &self,
        state: BlockStateId,
        ticks: &dyn ScheduledTickAccess,
        pos: BlockPos,
    ) -> BlockStateId {
        let _ = ticks.schedule_block_tick_default(pos, self.block, FALL_DELAY);
        state
    }

    /// Runs the scheduled fall check and returns the spawned entity, if any.
    #[must_use]
    pub fn tick(
        state: BlockStateId,
        world: &Arc<World>,
        pos: BlockPos,
    ) -> Option<Arc<FallingBlockEntity>> {
        if pos.y() < world.get_min_y() || !Self::is_free(world.get_block_state(pos.below())) {
            return None;
        }

        Some(FallingBlockEntity::fall(world, pos, state))
    }

    /// Vanilla `FallingBlock.isFree`.
    #[must_use]
    pub fn is_free(state: BlockStateId) -> bool {
        let block = state.get_block();
        state.is_air()
            || block.has_tag(&BlockTag::FIRE)
            || block.config.liquid
            || state.is_replaceable()
    }
}
