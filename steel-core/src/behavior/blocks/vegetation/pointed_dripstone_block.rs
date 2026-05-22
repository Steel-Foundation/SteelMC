use std::sync::Arc;

use steel_macros::block_behavior;
use steel_registry::blocks::block_state_ext::BlockStateExt;
use steel_registry::blocks::properties::{BlockStateProperties, Direction, DripstoneThickness};
use steel_registry::{vanilla_blocks, vanilla_damage_types};
use steel_utils::{BlockPos, BlockStateId};

use crate::behavior::block::BlockBehavior;
use crate::behavior::context::BlockPlaceContext;
use crate::entity::Entity;
use crate::entity::damage::DamageSource;
use crate::world::{LevelReader, World};

use super::BlockRef;

/// Vanilla `PointedDripstoneBlock`
// TODO: Implement thickness recalculation, scheduled-tick stalagmite breakage,
// trident projectile breakage, fluid transfer, and growth.
#[block_behavior]
pub struct PointedDripstoneBlock {
    block: BlockRef,
}

impl PointedDripstoneBlock {
    /// Creates a new pointed dripstone block behavior
    #[must_use]
    pub const fn new(block: BlockRef) -> Self {
        Self { block }
    }
}

impl BlockBehavior for PointedDripstoneBlock {
    fn can_survive(&self, state: BlockStateId, world: &dyn LevelReader, pos: BlockPos) -> bool {
        let tip_direction = state.get_value(&BlockStateProperties::VERTICAL_DIRECTION);
        let behind_pos = pos.relative(tip_direction.opposite());
        let behind_state = world.get_block_state(behind_pos);

        if behind_state.is_face_sturdy(tip_direction) {
            return true;
        }

        behind_state.get_block() == &vanilla_blocks::POINTED_DRIPSTONE
            && behind_state.get_value(&BlockStateProperties::VERTICAL_DIRECTION) == tip_direction
    }

    fn fall_on(
        &self,
        state: BlockStateId,
        _world: &Arc<World>,
        _pos: BlockPos,
        entity: &dyn Entity,
        fall_distance: f32,
    ) {
        let is_upward_tip = state.get_value(&BlockStateProperties::VERTICAL_DIRECTION)
            == Direction::Up
            && state.get_value(&BlockStateProperties::DRIPSTONE_THICKNESS)
                == DripstoneThickness::Tip;

        if is_upward_tip {
            entity.cause_fall_damage(
                fall_distance + 2.5,
                2.0,
                &DamageSource::environment(&vanilla_damage_types::STALAGMITE),
            );
        } else {
            entity.cause_fall_damage(
                fall_distance,
                1.0,
                &DamageSource::environment(&vanilla_damage_types::FALL),
            );
        }
    }

    fn get_state_for_placement(&self, context: &BlockPlaceContext<'_>) -> Option<BlockStateId> {
        // TODO: Vanilla picks tip direction from clicked-face/looking direction
        // and computes thickness. Placeholder: default state if it survives.
        let state = self.block.default_state();
        self.can_survive(state, context.world, context.relative_pos)
            .then_some(state.set_value(
                &BlockStateProperties::WATERLOGGED,
                context.is_water_source(),
            ))
    }
}
