use std::sync::Arc;

use steel_macros::block_behavior;
use steel_registry::{
    blocks::{
        BlockRef,
        block_state_ext::BlockStateExt as _,
        properties::{BedPart, BlockStateProperties},
    },
    sound_events, vanilla_blocks, vanilla_custom_stats,
};
use steel_utils::{BlockPos, BlockStateId, Direction, axis::Axis, types::UpdateFlags};
use text_components::TextComponent;
use text_components::translation::TranslatedMessage;

use crate::{
    behavior::{
        BlockBehavior, BlockHitResult, BlockPlaceContext, EntityFallDamage, EntityFallOnContext,
        EntityLandingContext, InteractionResult, InventoryAccess, PlacementSource,
    },
    entity::ai::path::PathComputationType,
    player::Player,
    world::{LevelReader, ScheduledTickAccess, World},
};

use super::BedBlock;

/// straw bed
#[block_behavior]
pub struct StrawBedBlock {
    base: BedBlock,
}

impl StrawBedBlock {
    /// straw bed behaviour
    #[must_use]
    pub const fn new(block: BlockRef) -> Self {
        Self {
            base: BedBlock::new(block),
        }
    }

    fn destroy_bed(world: &Arc<World>, pos: BlockPos) {
        world.play_block_sound(
            &sound_events::BLOCK_STRAW_BED_BREAK_LEAVE,
            pos,
            1.0,
            1.0,
            None,
        );
        world.set_block(
            pos,
            vanilla_blocks::AIR.default_state(),
            UpdateFlags::UPDATE_ALL,
        );
    }
}

impl BlockBehavior for StrawBedBlock {
    fn get_state_for_placement(&self, context: &BlockPlaceContext<'_>) -> Option<BlockStateId> {
        self.base.get_state_for_placement(context)
    }

    fn fall_on(
        &self,
        state: BlockStateId,
        world: &Arc<World>,
        pos: BlockPos,
        context: EntityFallOnContext<'_>,
    ) -> Option<EntityFallDamage> {
        self.base.fall_on(state, world, pos, context)
    }

    fn update_entity_movement_after_fall_on(
        &self,
        state: BlockStateId,
        world: &Arc<World>,
        pos: BlockPos,
        context: EntityLandingContext,
    ) -> glam::DVec3 {
        self.base
            .update_entity_movement_after_fall_on(state, world, pos, context)
    }

    fn is_pathfindable(&self, state: BlockStateId, computation_type: PathComputationType) -> bool {
        self.base.is_pathfindable(state, computation_type)
    }

    fn is_bed(&self) -> bool {
        true
    }

    fn get_sleep_height(
        &self,
        state: BlockStateId,
        world: &dyn LevelReader,
        pos: BlockPos,
    ) -> Option<f64> {
        let foot_state = if state.get_value(&BlockStateProperties::BED_PART) == BedPart::Head {
            let foot_pos = state
                .get_value(&BlockStateProperties::HORIZONTAL_FACING)
                .opposite()
                .relative(pos);

            let foot_state = world.get_block_state(foot_pos);
            (foot_state.get_block() == self.base.block
                && foot_state.get_value(&BlockStateProperties::BED_PART) == BedPart::Foot)
                .then_some(foot_state)?
        } else {
            state
        };

        let shape = foot_state.get_static_outline_shape();
        (!shape.is_empty()).then(|| shape.max(Axis::Y))
    }

    fn on_stop_sleeping(&self, _state: BlockStateId, world: &Arc<World>, pos: BlockPos) {
        if world.dimension_type.straw_bed_rule.destroy_on_leave {
            Self::destroy_bed(world, pos);
        }
    }

    fn player_will_destroy(
        &self,
        state: BlockStateId,
        world: &Arc<World>,
        pos: BlockPos,
        player: &Player,
    ) -> BlockStateId {
        self.base.player_will_destroy(state, world, pos, player)
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
        self.base
            .update_shape(state, world, pos, direction, neighbor_pos, neighbor_state)
    }

    fn set_placed_by(
        &self,
        state: BlockStateId,
        world: &Arc<World>,
        pos: BlockPos,
        source: &PlacementSource<'_>,
    ) {
        self.base.set_placed_by(state, world, pos, source);
    }

    fn use_without_item(
        &self,
        state: BlockStateId,
        world: &Arc<World>,
        pos: BlockPos,
        player: &Player,
        _hit_result: &BlockHitResult,
        _inv: &mut InventoryAccess,
    ) -> InteractionResult {
        let Some((head_state, head_pos)) = self.base.head_state_and_pos(world, state, pos) else {
            return InteractionResult::Consume;
        };

        let rule = &world.dimension_type.straw_bed_rule;
        if rule.destroy_on_use {
            Self::destroy_bed(world, head_pos);
            return InteractionResult::SuccessServer;
        }

        if head_state.get_value(&BlockStateProperties::OCCUPIED) {
            player.send_overlay_message(&TextComponent::translated(TranslatedMessage {
                key: "block.minecraft.bed.occupied".into(),
                fallback: None,
                args: None,
            }));
            return InteractionResult::SuccessServer;
        }

        if let Err(problem) =
            player.start_sleep_in_bed(head_pos, rule, &vanilla_custom_stats::SLEEP_IN_STRAW_BED)
            && let Some(message) = problem.message()
        {
            player.send_overlay_message(message);
        }
        InteractionResult::SuccessServer
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use steel_registry::init_vanilla_registry;

    use crate::world::LevelReader;

    struct StrawBedLevel {
        foot_pos: BlockPos,
        foot_state: BlockStateId,
    }

    impl LevelReader for StrawBedLevel {
        fn get_block_state(&self, pos: BlockPos) -> BlockStateId {
            if pos == self.foot_pos {
                self.foot_state
            } else {
                vanilla_blocks::AIR.default_state()
            }
        }

        fn raw_brightness(&self, _pos: BlockPos, _sky_darkening: u8) -> u8 {
            0
        }

        fn min_y(&self) -> i32 {
            -64
        }

        fn height(&self) -> i32 {
            384
        }
    }

    #[test]
    fn sleeping_on_a_straw_bed_uses_the_foot_height() {
        init_vanilla_registry();
        let behavior = StrawBedBlock::new(&vanilla_blocks::STRAW_BED);
        let head_pos = BlockPos::ZERO;
        let facing = Direction::North;
        let head_state = vanilla_blocks::STRAW_BED
            .default_state()
            .set_value(&BlockStateProperties::HORIZONTAL_FACING, facing)
            .set_value(&BlockStateProperties::BED_PART, BedPart::Head);
        let foot_state = head_state.set_value(&BlockStateProperties::BED_PART, BedPart::Foot);
        let level = StrawBedLevel {
            foot_pos: facing.opposite().relative(head_pos),
            foot_state,
        };

        assert_eq!(
            behavior.get_sleep_height(head_state, &level, head_pos),
            Some(0.25)
        );
    }
}
