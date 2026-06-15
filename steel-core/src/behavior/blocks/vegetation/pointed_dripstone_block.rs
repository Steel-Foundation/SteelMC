use std::sync::Arc;

use steel_macros::block_behavior;
use steel_registry::blocks::block_state_ext::BlockStateExt;
use steel_registry::blocks::properties::{BlockStateProperties, DripstoneThickness};
use steel_registry::{vanilla_blocks, vanilla_damage_types};
use steel_utils::Direction;
use steel_utils::{BlockPos, BlockStateId};

use crate::behavior::block::{BlockBehavior, EntityFallDamage, EntityFallOnContext};
use crate::behavior::context::BlockPlaceContext;
use crate::entity::damage::DamageSource;
use crate::world::LevelReader;
use crate::world::World;

use super::BlockRef;

/// Vanilla `PointedDripstoneBlock` survival.
///
/// Survival mirrors vanilla's `isValidPointedDripstonePlacement`: the block
/// opposite the tip direction must be face-sturdy on the face pointing toward
/// us, or be another pointed dripstone with the same `vertical_direction`.
// TODO: Implement thickness recalculation, scheduled-tick stalagmite breakage,
// trident projectile breakage, fluid transfer, and growth.
#[block_behavior]
pub struct PointedDripstoneBlock {
    block: BlockRef,
}

impl PointedDripstoneBlock {
    /// Creates a new pointed dripstone block behavior.
    #[must_use]
    pub const fn new(block: BlockRef) -> Self {
        Self { block }
    }

    #[must_use]
    fn fall_damage_for_state(state: BlockStateId, fall_distance: f64) -> Option<EntityFallDamage> {
        if state.get_value(&BlockStateProperties::VERTICAL_DIRECTION) != Direction::Up
            || state.get_value(&BlockStateProperties::DRIPSTONE_THICKNESS)
                != DripstoneThickness::Tip
        {
            return None;
        }

        Some(EntityFallDamage::new(
            fall_distance + 2.5,
            2.0,
            DamageSource::environment(&vanilla_damage_types::STALAGMITE),
        ))
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

        // Behind is pointed dripstone with the same tip direction.
        behind_state.get_block() == &vanilla_blocks::POINTED_DRIPSTONE
            && behind_state.get_value(&BlockStateProperties::VERTICAL_DIRECTION) == tip_direction
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

    fn fall_on(
        &self,
        state: BlockStateId,
        world: &Arc<World>,
        pos: BlockPos,
        context: EntityFallOnContext<'_>,
    ) -> Option<EntityFallDamage> {
        Self::fall_damage_for_state(state, context.fall_distance)
            .or_else(|| self.default_fall_on(state, world, pos, context))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use steel_registry::{test_support::init_test_registry, vanilla_blocks, vanilla_damage_types};

    fn pointed_dripstone_state(
        direction: Direction,
        thickness: DripstoneThickness,
    ) -> BlockStateId {
        init_test_registry();
        vanilla_blocks::POINTED_DRIPSTONE
            .default_state()
            .set_value(&BlockStateProperties::VERTICAL_DIRECTION, direction)
            .set_value(&BlockStateProperties::DRIPSTONE_THICKNESS, thickness)
    }

    #[test]
    fn upward_tip_uses_stalagmite_fall_damage() {
        let state = pointed_dripstone_state(Direction::Up, DripstoneThickness::Tip);
        let fall_damage = PointedDripstoneBlock::fall_damage_for_state(state, 4.0)
            .expect("upward tip should request stalagmite damage");

        assert!((fall_damage.fall_distance - 6.5).abs() < f64::EPSILON);
        assert!((fall_damage.damage_modifier - 2.0).abs() < f32::EPSILON);
        assert_eq!(
            &fall_damage.source.damage_type.key,
            &vanilla_damage_types::STALAGMITE.key,
        );
    }

    #[test]
    fn non_tip_uses_default_fall_damage() {
        let state = pointed_dripstone_state(Direction::Up, DripstoneThickness::Frustum);

        assert!(PointedDripstoneBlock::fall_damage_for_state(state, 4.0).is_none());
    }

    #[test]
    fn downward_tip_uses_default_fall_damage() {
        let state = pointed_dripstone_state(Direction::Down, DripstoneThickness::Tip);

        assert!(PointedDripstoneBlock::fall_damage_for_state(state, 4.0).is_none());
    }
}
