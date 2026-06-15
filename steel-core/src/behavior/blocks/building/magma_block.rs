use std::sync::Arc;

use steel_macros::block_behavior;
use steel_registry::blocks::BlockRef;
use steel_registry::vanilla_damage_types;
use steel_utils::{BlockPos, BlockStateId};

use crate::{
    behavior::{BlockBehavior, BlockPlaceContext},
    entity::{Entity, damage::DamageSource},
    world::World,
};

/// Behavior for magma blocks.
#[block_behavior]
pub struct MagmaBlock {
    block: BlockRef,
}

impl MagmaBlock {
    /// Creates a magma block behavior.
    #[must_use]
    pub const fn new(block: BlockRef) -> Self {
        Self { block }
    }

    #[must_use]
    const fn step_damage_amount(
        is_stepping_carefully: bool,
        is_living_entity: bool,
    ) -> Option<f32> {
        if !is_stepping_carefully && is_living_entity {
            Some(1.0)
        } else {
            None
        }
    }
}

impl BlockBehavior for MagmaBlock {
    fn get_state_for_placement(&self, _context: &BlockPlaceContext<'_>) -> Option<BlockStateId> {
        Some(self.block.default_state())
    }

    fn step_on(&self, state: BlockStateId, world: &Arc<World>, pos: BlockPos, entity: &dyn Entity) {
        if let Some(damage) =
            Self::step_damage_amount(entity.is_stepping_carefully(), entity.is_living_entity())
        {
            entity.hurt(
                &DamageSource::environment(&vanilla_damage_types::HOT_FLOOR),
                damage,
            );
        }

        self.default_step_on(state, world, pos, entity);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn magma_damages_non_careful_living_entities() {
        assert_eq!(MagmaBlock::step_damage_amount(false, true), Some(1.0));
    }

    #[test]
    fn magma_does_not_damage_careful_living_entities() {
        assert_eq!(MagmaBlock::step_damage_amount(true, true), None);
    }

    #[test]
    fn magma_does_not_damage_non_living_entities() {
        assert_eq!(MagmaBlock::step_damage_amount(false, false), None);
    }
}
