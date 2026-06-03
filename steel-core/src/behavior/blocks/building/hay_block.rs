use std::sync::Arc;

use steel_macros::block_behavior;
use steel_registry::blocks::BlockRef;
use steel_registry::vanilla_damage_types;
use steel_utils::{BlockPos, BlockStateId};

use crate::{
    behavior::{
        BlockBehavior, BlockPlaceContext, EntityFallDamage, EntityFallOnContext,
        blocks::RotatedPillarBlock,
    },
    world::World,
};

use crate::entity::damage::DamageSource;

/// Behavior for hay blocks.
#[block_behavior]
pub struct HayBlock {
    block: BlockRef,
}

impl HayBlock {
    /// Creates a hay block behavior.
    #[must_use]
    pub const fn new(block: BlockRef) -> Self {
        Self { block }
    }

    #[must_use]
    fn fall_damage(fall_distance: f64) -> EntityFallDamage {
        EntityFallDamage::new(
            fall_distance,
            0.2,
            DamageSource::environment(&vanilla_damage_types::FALL),
        )
    }
}

impl BlockBehavior for HayBlock {
    fn get_state_for_placement(&self, context: &BlockPlaceContext<'_>) -> Option<BlockStateId> {
        Some(RotatedPillarBlock::placement_state(self.block, context))
    }

    fn fall_on(
        &self,
        _state: BlockStateId,
        _world: &Arc<World>,
        _pos: BlockPos,
        context: EntityFallOnContext<'_>,
    ) -> Option<EntityFallDamage> {
        Some(Self::fall_damage(context.fall_distance))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hay_reduces_fall_damage_like_vanilla() {
        let fall_damage = HayBlock::fall_damage(12.0);

        assert!((fall_damage.fall_distance - 12.0).abs() < f64::EPSILON);
        assert!((fall_damage.damage_modifier - 0.2).abs() < f32::EPSILON);
    }
}
