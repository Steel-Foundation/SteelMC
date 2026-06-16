use std::sync::Arc;

use steel_macros::block_behavior;
use steel_registry::blocks::block_state_ext::BlockStateExt;
use steel_registry::blocks::properties::{
    BlockStateProperties, DripstoneThickness, SpeleothemThickness,
};
use steel_registry::{vanilla_block_tags::BlockTag, vanilla_damage_types, vanilla_fluids};
use steel_utils::{BlockPos, BlockStateId, Direction};

use crate::behavior::block::{BlockBehavior, EntityFallDamage, EntityFallOnContext};
use crate::behavior::context::BlockPlaceContext;
use crate::entity::damage::DamageSource;
use crate::world::World;
use crate::world::{LevelReader, ScheduledTickAccess};

use super::BlockRef;

/// Vanilla `PointedDripstoneBlock` survival and thickness updates.
///
/// Survival mirrors vanilla's `isValidPointedDripstonePlacement`: the block
/// opposite the tip direction must be face-sturdy on the face pointing toward
/// us, or be another pointed dripstone with the same `vertical_direction`.
// TODO: Implement falling stalactites after falling block entities exist.
// TODO: Implement trident projectile breakage, fluid transfer, and growth.
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

    const fn speleothem(&self) -> SpeleothemBlockBehavior {
        SpeleothemBlockBehavior {
            block: self.block,
            kind: SpeleothemKind::PointedDripstone,
        }
    }
}

impl BlockBehavior for PointedDripstoneBlock {
    fn can_survive(&self, state: BlockStateId, world: &dyn LevelReader, pos: BlockPos) -> bool {
        self.speleothem().can_survive(state, world, pos)
    }

    fn get_state_for_placement(&self, context: &BlockPlaceContext<'_>) -> Option<BlockStateId> {
        self.speleothem().state_for_placement(context)
    }

    fn update_shape(
        &self,
        state: BlockStateId,
        world: &dyn ScheduledTickAccess,
        pos: BlockPos,
        direction: Direction,
        neighbor_pos: BlockPos,
        neighbor_state: BlockStateId,
    ) -> BlockStateId {
        self.speleothem()
            .update_shape(state, world, pos, direction, neighbor_pos, neighbor_state)
    }

    fn tick(&self, state: BlockStateId, world: &Arc<World>, pos: BlockPos) {
        self.speleothem().tick(state, world, pos);
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

#[block_behavior]
/// Vanilla `SulfurSpikeBlock` behavior
pub struct SulfurSpikeBlock {
    block: BlockRef,
}

impl SulfurSpikeBlock {
    /// Creates a new sulfur spike block
    #[must_use]
    pub const fn new(block: BlockRef) -> Self {
        Self { block }
    }

    const fn speleothem(&self) -> SpeleothemBlockBehavior {
        SpeleothemBlockBehavior {
            block: self.block,
            kind: SpeleothemKind::Sulfur,
        }
    }
}

impl BlockBehavior for SulfurSpikeBlock {
    fn can_survive(&self, state: BlockStateId, world: &dyn LevelReader, pos: BlockPos) -> bool {
        self.speleothem().can_survive(state, world, pos)
    }

    fn get_state_for_placement(&self, context: &BlockPlaceContext<'_>) -> Option<BlockStateId> {
        self.speleothem().state_for_placement(context)
    }

    fn update_shape(
        &self,
        state: BlockStateId,
        world: &dyn ScheduledTickAccess,
        pos: BlockPos,
        direction: Direction,
        neighbor_pos: BlockPos,
        neighbor_state: BlockStateId,
    ) -> BlockStateId {
        self.speleothem()
            .update_shape(state, world, pos, direction, neighbor_pos, neighbor_state)
    }

    fn tick(&self, state: BlockStateId, world: &Arc<World>, pos: BlockPos) {
        self.speleothem().tick(state, world, pos);
    }
}

struct SpeleothemBlockBehavior {
    block: BlockRef,
    kind: SpeleothemKind,
}

#[derive(Clone, Copy)]
enum SpeleothemKind {
    PointedDripstone,
    Sulfur,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum SpeleothemThicknessValue {
    TipMerge,
    Tip,
    Frustum,
    Middle,
    Base,
}

impl SpeleothemBlockBehavior {
    fn state_for_placement(&self, context: &BlockPlaceContext<'_>) -> Option<BlockStateId> {
        let default_tip_direction = context.get_nearest_looking_vertical_direction().opposite();
        let tip_direction = self.calculate_tip_direction(
            context.world.as_ref(),
            context.relative_pos,
            default_tip_direction,
        )?;
        let merge_opposing_tips = !context.is_secondary_use_active;
        let thickness = self.calculate_thickness(
            context.world.as_ref(),
            context.relative_pos,
            tip_direction,
            merge_opposing_tips,
        );
        let state = self
            .block
            .default_state()
            .set_value(&BlockStateProperties::VERTICAL_DIRECTION, tip_direction)
            .set_value(
                &BlockStateProperties::WATERLOGGED,
                context.is_water_source(),
            );

        Some(self.with_thickness(state, thickness))
    }

    fn can_survive(&self, state: BlockStateId, world: &dyn LevelReader, pos: BlockPos) -> bool {
        let tip_direction = state.get_value(&BlockStateProperties::VERTICAL_DIRECTION);
        let behind_pos = pos.relative(tip_direction.opposite());
        let behind_state = world.get_block_state(behind_pos);

        behind_state.is_face_sturdy(tip_direction)
            || (Self::is_speleothem_with_direction(behind_state, tip_direction)
                && behind_state.get_block() == self.block)
    }

    fn update_shape(
        &self,
        state: BlockStateId,
        world: &dyn ScheduledTickAccess,
        pos: BlockPos,
        direction: Direction,
        _neighbor_pos: BlockPos,
        _neighbor_state: BlockStateId,
    ) -> BlockStateId {
        if state.get_value(&BlockStateProperties::WATERLOGGED) {
            let delay = world.fluid_tick_delay(&vanilla_fluids::WATER);
            let _ = world.schedule_fluid_tick_default(pos, &vanilla_fluids::WATER, delay);
        }

        if direction != Direction::Up && direction != Direction::Down {
            return state;
        }

        let tip_direction = state.get_value(&BlockStateProperties::VERTICAL_DIRECTION);
        if tip_direction == Direction::Down && world.has_scheduled_block_tick(pos, self.block) {
            return state;
        }

        if direction == tip_direction.opposite() && !self.can_survive(state, world, pos) {
            let delay = if tip_direction == Direction::Down {
                2
            } else {
                1
            };
            let _ = world.schedule_block_tick_default(pos, self.block, delay);
            return state;
        }

        let merge_opposing_tips = self.thickness(state) == SpeleothemThicknessValue::TipMerge;
        let thickness = self.calculate_thickness(world, pos, tip_direction, merge_opposing_tips);
        self.with_thickness(state, thickness)
    }

    fn tick(&self, state: BlockStateId, world: &Arc<World>, pos: BlockPos) {
        if Self::is_stalagmite(state) && !self.can_survive(state, world.as_ref(), pos) {
            world.destroy_block(pos, true);
        }
    }

    fn calculate_tip_direction(
        &self,
        world: &dyn LevelReader,
        pos: BlockPos,
        default_tip_direction: Direction,
    ) -> Option<Direction> {
        let default_state = self.block.default_state().set_value(
            &BlockStateProperties::VERTICAL_DIRECTION,
            default_tip_direction,
        );
        if self.can_survive(default_state, world, pos) {
            return Some(default_tip_direction);
        }

        let opposite_tip_direction = default_tip_direction.opposite();
        let opposite_state = self.block.default_state().set_value(
            &BlockStateProperties::VERTICAL_DIRECTION,
            opposite_tip_direction,
        );
        self.can_survive(opposite_state, world, pos)
            .then_some(opposite_tip_direction)
    }

    fn calculate_thickness(
        &self,
        world: &dyn LevelReader,
        pos: BlockPos,
        tip_direction: Direction,
        merge_opposing_tips: bool,
    ) -> SpeleothemThicknessValue {
        let base_direction = tip_direction.opposite();
        let in_front_state = world.get_block_state(pos.relative(tip_direction));
        if Self::is_speleothem_with_direction(in_front_state, base_direction)
            && in_front_state.get_block() == self.block
        {
            if merge_opposing_tips
                || self.thickness(in_front_state) == SpeleothemThicknessValue::TipMerge
            {
                return SpeleothemThicknessValue::TipMerge;
            }
            return SpeleothemThicknessValue::Tip;
        }

        if !Self::is_speleothem_with_direction(in_front_state, tip_direction) {
            return SpeleothemThicknessValue::Tip;
        }

        let in_front_thickness = self.thickness(in_front_state);
        if matches!(
            in_front_thickness,
            SpeleothemThicknessValue::Tip | SpeleothemThicknessValue::TipMerge
        ) {
            return SpeleothemThicknessValue::Frustum;
        }

        let behind_state = world.get_block_state(pos.relative(base_direction));
        if !Self::is_speleothem_with_direction(behind_state, tip_direction) {
            return SpeleothemThicknessValue::Base;
        }
        SpeleothemThicknessValue::Middle
    }

    fn is_speleothem_with_direction(state: BlockStateId, tip_direction: Direction) -> bool {
        state.get_block().has_tag(&BlockTag::SPELEOTHEMS)
            && state.get_value(&BlockStateProperties::VERTICAL_DIRECTION) == tip_direction
    }

    fn is_stalagmite(state: BlockStateId) -> bool {
        Self::is_speleothem_with_direction(state, Direction::Up)
    }

    fn thickness(&self, state: BlockStateId) -> SpeleothemThicknessValue {
        match self.kind {
            SpeleothemKind::PointedDripstone => {
                match state.get_value(&BlockStateProperties::DRIPSTONE_THICKNESS) {
                    DripstoneThickness::TipMerge => SpeleothemThicknessValue::TipMerge,
                    DripstoneThickness::Tip => SpeleothemThicknessValue::Tip,
                    DripstoneThickness::Frustum => SpeleothemThicknessValue::Frustum,
                    DripstoneThickness::Middle => SpeleothemThicknessValue::Middle,
                    DripstoneThickness::Base => SpeleothemThicknessValue::Base,
                }
            }
            SpeleothemKind::Sulfur => {
                match state.get_value(&BlockStateProperties::SPELEOTHEM_THICKNESS) {
                    SpeleothemThickness::TipMerge => SpeleothemThicknessValue::TipMerge,
                    SpeleothemThickness::Tip => SpeleothemThicknessValue::Tip,
                    SpeleothemThickness::Frustum => SpeleothemThicknessValue::Frustum,
                    SpeleothemThickness::Middle => SpeleothemThicknessValue::Middle,
                    SpeleothemThickness::Base => SpeleothemThicknessValue::Base,
                }
            }
        }
    }

    fn with_thickness(
        &self,
        state: BlockStateId,
        thickness: SpeleothemThicknessValue,
    ) -> BlockStateId {
        match self.kind {
            SpeleothemKind::PointedDripstone => state.set_value(
                &BlockStateProperties::DRIPSTONE_THICKNESS,
                match thickness {
                    SpeleothemThicknessValue::TipMerge => DripstoneThickness::TipMerge,
                    SpeleothemThicknessValue::Tip => DripstoneThickness::Tip,
                    SpeleothemThicknessValue::Frustum => DripstoneThickness::Frustum,
                    SpeleothemThicknessValue::Middle => DripstoneThickness::Middle,
                    SpeleothemThicknessValue::Base => DripstoneThickness::Base,
                },
            ),
            SpeleothemKind::Sulfur => state.set_value(
                &BlockStateProperties::SPELEOTHEM_THICKNESS,
                match thickness {
                    SpeleothemThicknessValue::TipMerge => SpeleothemThickness::TipMerge,
                    SpeleothemThicknessValue::Tip => SpeleothemThickness::Tip,
                    SpeleothemThicknessValue::Frustum => SpeleothemThickness::Frustum,
                    SpeleothemThicknessValue::Middle => SpeleothemThickness::Middle,
                    SpeleothemThicknessValue::Base => SpeleothemThickness::Base,
                },
            ),
        }
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
