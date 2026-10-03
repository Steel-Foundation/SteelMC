use steel_macros::item_behavior;
use steel_registry::blocks::{BlockRef, block_state_ext::BlockStateExt, properties::Direction};

use super::block_item::SurvivalCheck;
use crate::behavior::blocks::ScaffoldingBlock;
use crate::behavior::{
    BlockItem, BlockPlaceContext, BlockStateBehaviorExt, InteractionResult, ItemBehavior,
    UseOnContext,
};

/// Vanilla scaffolding's directional extension placement.
#[item_behavior]
pub struct ScaffoldingBlockItem {
    #[json_arg(vanilla_blocks, json = "block")]
    block: BlockRef,
    base: BlockItem,
}

impl ScaffoldingBlockItem {
    /// Creates a scaffolding block item behavior.
    #[must_use]
    pub const fn new(block: BlockRef) -> Self {
        Self {
            block,
            base: BlockItem::new(block),
        }
    }

    fn place(&self, context: BlockPlaceContext<'_>) -> InteractionResult {
        self.base.place_with_policy(
            context,
            |place_context| self.update_placement_context(place_context),
            SurvivalCheck::Skipped,
            BlockItem::place_block,
            self.block.config.sound_type.place_sound,
        )
    }

    fn update_placement_context<'a>(
        &self,
        context: BlockPlaceContext<'a>,
    ) -> Option<BlockPlaceContext<'a>> {
        let pos = context.place_pos();
        let state = context.world.get_block_state(pos);
        if state.get_block() != self.block {
            return (ScaffoldingBlock::get_distance(context.world.as_ref(), pos)
                < ScaffoldingBlock::STABILITY_MAX_DISTANCE)
                .then_some(context);
        }

        let direction = if context.is_secondary_use_active() {
            if context.is_inside() {
                context.clicked_face().opposite()
            } else {
                context.clicked_face()
            }
        } else if context.clicked_face() == Direction::Up {
            context.horizontal_direction()
        } else {
            Direction::Up
        };

        let mut horizontal_distance = 0;
        let mut placement_pos = direction.relative(pos);
        while horizontal_distance < ScaffoldingBlock::STABILITY_MAX_DISTANCE {
            if !context.world.is_in_world_bounds(placement_pos) {
                if placement_pos.y() > context.world.get_max_y()
                    && let Some(player) = context.player()
                {
                    player.send_build_limit_too_high_message(context.world.get_max_y());
                }
                break;
            }

            let state = context.world.get_block_state(placement_pos);
            if state.get_block() != self.block {
                if state.can_be_replaced(&context) {
                    return Some(context.at(placement_pos, direction));
                }
                break;
            }

            placement_pos = direction.relative(placement_pos);
            if direction.is_horizontal() {
                horizontal_distance += 1;
            }
        }

        None
    }
}

impl ItemBehavior for ScaffoldingBlockItem {
    fn use_on(&self, context: &mut UseOnContext) -> InteractionResult {
        self.place(context.build_place_context())
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use glam::DVec3;
    use steel_registry::blocks::{
        block_state_ext::BlockStateExt,
        properties::{BlockStateProperties, BoolProperty, IntProperty},
    };
    use steel_registry::item_stack::ItemStack;
    use steel_registry::{init_vanilla_registry, vanilla_blocks, vanilla_items};
    use steel_utils::types::{InteractionHand, UpdateFlags};
    use steel_utils::{BlockPos, ChunkPos};

    use super::*;
    use crate::behavior::{BlockHitResult, PlacementOrientation, PlacementSource, init_behaviors};
    use crate::test_support::{fresh_test_world, insert_ready_full_chunk};
    use crate::world::{LevelReader, World};

    const BOTTOM: &BoolProperty = &BlockStateProperties::BOTTOM;
    const DISTANCE: &IntProperty = &BlockStateProperties::STABILITY_DISTANCE;
    const EAST_FACING_YAW_DEGREES: f32 = 270.0;
    const VANILLA_STABILITY_MAX_DISTANCE: u8 = 7;
    const WATERLOGGED: &BoolProperty = &BlockStateProperties::WATERLOGGED;

    fn test_world(key: &'static str) -> Arc<World> {
        init_vanilla_registry();
        init_behaviors();
        let world = fresh_test_world(key);
        insert_ready_full_chunk(&world, ChunkPos::new(0, 0));
        world
    }

    fn set_block(world: &Arc<World>, pos: BlockPos, state: steel_utils::BlockStateId) {
        assert!(world.set_block(pos, state, UpdateFlags::UPDATE_NONE));
    }

    fn scaffolding_state(distance: u8) -> steel_utils::BlockStateId {
        vanilla_blocks::SCAFFOLDING
            .default_state()
            .set_value(DISTANCE, distance)
            .set_value(BOTTOM, distance > 0)
    }

    fn place_context<'a>(
        world: &'a Arc<World>,
        stack: &'a mut ItemStack,
        hit_pos: BlockPos,
        clicked_face: Direction,
        inside: bool,
        secondary_use: bool,
        yaw: f32,
    ) -> BlockPlaceContext<'a> {
        let source = PlacementSource::direct(
            None,
            InteractionHand::MainHand,
            stack,
            PlacementOrientation::Player {
                rotation: yaw,
                pitch: 0.0,
            },
            secondary_use,
        );
        BlockPlaceContext::new(
            world,
            source,
            &BlockHitResult {
                location: DVec3::new(
                    f64::from(hit_pos.x()) + 0.5,
                    f64::from(hit_pos.y()) + 0.5,
                    f64::from(hit_pos.z()) + 0.5,
                ),
                direction: clicked_face,
                block_pos: hit_pos,
                miss: false,
                inside,
                world_border_hit: false,
            },
        )
    }

    fn routed_pos(
        item: &ScaffoldingBlockItem,
        world: &Arc<World>,
        hit_pos: BlockPos,
        clicked_face: Direction,
        inside: bool,
        secondary_use: bool,
        yaw: f32,
    ) -> Option<BlockPos> {
        let mut stack = ItemStack::new(&vanilla_items::SCAFFOLDING);
        let context = place_context(
            world,
            &mut stack,
            hit_pos,
            clicked_face,
            inside,
            secondary_use,
            yaw,
        );
        item.update_placement_context(context)
            .map(|context| context.place_pos())
    }

    #[test]
    fn routing_matches_normal_secondary_and_inside_direction_rules() {
        let world = test_world("scaffolding_item_directions");
        let pos = BlockPos::new(8, 64, 8);
        set_block(&world, pos, scaffolding_state(0));
        let item = ScaffoldingBlockItem::new(&vanilla_blocks::SCAFFOLDING);

        assert_eq!(
            routed_pos(
                &item,
                &world,
                pos,
                Direction::Up,
                false,
                false,
                EAST_FACING_YAW_DEGREES,
            ),
            Some(pos.east())
        );
        assert_eq!(
            routed_pos(
                &item,
                &world,
                pos,
                Direction::North,
                false,
                false,
                EAST_FACING_YAW_DEGREES,
            ),
            Some(pos.above())
        );
        assert_eq!(
            routed_pos(
                &item,
                &world,
                pos,
                Direction::North,
                false,
                true,
                EAST_FACING_YAW_DEGREES,
            ),
            Some(pos.north())
        );
        assert_eq!(
            routed_pos(
                &item,
                &world,
                pos,
                Direction::North,
                true,
                true,
                EAST_FACING_YAW_DEGREES,
            ),
            Some(pos.south())
        );
    }

    #[test]
    fn horizontal_routing_allows_seventh_position_but_not_eighth() {
        let world = test_world("scaffolding_item_horizontal_limit");
        let start = BlockPos::new(4, 64, 8);
        let item = ScaffoldingBlockItem::new(&vanilla_blocks::SCAFFOLDING);
        let max_distance = i32::from(VANILLA_STABILITY_MAX_DISTANCE);
        for offset in 0..max_distance {
            set_block(
                &world,
                BlockPos::new(start.x() + offset, start.y(), start.z()),
                scaffolding_state(offset as u8),
            );
        }

        let seventh = BlockPos::new(start.x() + max_distance, start.y(), start.z());
        assert_eq!(
            routed_pos(
                &item,
                &world,
                start,
                Direction::Up,
                false,
                false,
                EAST_FACING_YAW_DEGREES,
            ),
            Some(seventh)
        );

        set_block(
            &world,
            seventh,
            scaffolding_state(VANILLA_STABILITY_MAX_DISTANCE),
        );
        assert_eq!(
            routed_pos(
                &item,
                &world,
                start,
                Direction::Up,
                false,
                false,
                EAST_FACING_YAW_DEGREES,
            ),
            None
        );
    }

    #[test]
    fn distance_seven_extension_places_then_waits_for_stability_tick() {
        let world = test_world("scaffolding_item_unstable_extension");
        let start = BlockPos::new(4, 64, 8);
        set_block(&world, start.below(), vanilla_blocks::STONE.default_state());
        let last_stable_offset = i32::from(VANILLA_STABILITY_MAX_DISTANCE) - 1;
        for offset in 0..=last_stable_offset {
            set_block(
                &world,
                BlockPos::new(start.x() + offset, start.y(), start.z()),
                scaffolding_state(offset as u8),
            );
        }

        let item = ScaffoldingBlockItem::new(&vanilla_blocks::SCAFFOLDING);
        let end = BlockPos::new(start.x() + last_stable_offset, start.y(), start.z());
        let placed = end.east();
        let mut stack = ItemStack::with_count(&vanilla_items::SCAFFOLDING, 2);
        let context = place_context(
            &world,
            &mut stack,
            end,
            Direction::Up,
            false,
            false,
            EAST_FACING_YAW_DEGREES,
        );

        assert_eq!(item.place(context), InteractionResult::Success);
        let state = world.get_block_state(placed);
        assert_eq!(state.get_block(), &vanilla_blocks::SCAFFOLDING);
        assert_eq!(state.get_value(DISTANCE), VANILLA_STABILITY_MAX_DISTANCE);
        assert!(state.get_value(BOTTOM));
        assert_eq!(stack.count(), 1);
    }

    #[test]
    fn unsupported_direct_target_is_rejected_without_consuming_item() {
        let world = test_world("scaffolding_item_unsupported_direct");
        let pos = BlockPos::new(8, 64, 8);
        let item = ScaffoldingBlockItem::new(&vanilla_blocks::SCAFFOLDING);
        let mut stack = ItemStack::with_count(&vanilla_items::SCAFFOLDING, 2);
        let context = place_context(&world, &mut stack, pos, Direction::Up, false, false, 0.0);

        assert_eq!(item.place(context), InteractionResult::Fail);
        assert!(world.get_block_state(pos).is_air());
        assert_eq!(stack.count(), 2);
    }

    #[test]
    fn supported_source_water_placement_is_waterlogged() {
        let world = test_world("scaffolding_item_waterlogged");
        let pos = BlockPos::new(8, 64, 8);
        set_block(&world, pos.below(), vanilla_blocks::STONE.default_state());
        set_block(&world, pos, vanilla_blocks::WATER.default_state());
        let item = ScaffoldingBlockItem::new(&vanilla_blocks::SCAFFOLDING);
        let mut stack = ItemStack::new(&vanilla_items::SCAFFOLDING);
        let context = place_context(&world, &mut stack, pos, Direction::Up, false, false, 0.0);

        assert_eq!(item.place(context), InteractionResult::Success);
        let state = world.get_block_state(pos);
        assert_eq!(state.get_block(), &vanilla_blocks::SCAFFOLDING);
        assert_eq!(state.get_value(DISTANCE), 0);
        assert!(!state.get_value(BOTTOM));
        assert!(state.get_value(WATERLOGGED));
        assert!(stack.is_empty());
    }
}
