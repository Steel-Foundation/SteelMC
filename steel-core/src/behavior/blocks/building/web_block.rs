use crate::behavior::{BlockBehavior, BlockPlaceContext};
use crate::entity::{Entity, InsideBlockEffectCollector};
use crate::world::World;
use glam::DVec3;
use std::sync::Arc;
use steel_macros::block_behavior;
use steel_registry::blocks::BlockRef;
use steel_registry::vanilla_mob_effects::WEAVING;
use steel_utils::{BlockPos, BlockStateId};

/// Behavior for cobwebs.
#[block_behavior]
pub struct WebBlock {
    block: BlockRef,
}

impl WebBlock {
    /// Creates a new web block behavior.
    #[must_use]
    pub const fn new(block: BlockRef) -> Self {
        Self { block }
    }
}

impl BlockBehavior for WebBlock {
    fn get_state_for_placement(&self, _context: &BlockPlaceContext<'_>) -> Option<BlockStateId> {
        Some(self.block.default_state())
    }

    fn entity_inside(
        &self,
        state: BlockStateId,
        _world: &Arc<World>,
        _pos: BlockPos,
        entity: &dyn Entity,
        _effect_collector: &mut InsideBlockEffectCollector,
        _is_precise: bool,
    ) {
        // Entities with the Weaving mob effect move faster in a cobweb.
        let multiplier = if let Some(living) = entity.as_living_entity()
            && living.has_mob_effect(WEAVING)
        {
            DVec3::new(0.5, 0.25, 0.5)
        } else {
            DVec3::new(0.25, 0.05, 0.25)
        };

        entity.make_stuck_in_block(state, multiplier);
    }
}
