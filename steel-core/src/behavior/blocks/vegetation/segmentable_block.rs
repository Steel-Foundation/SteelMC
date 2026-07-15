use steel_registry::REGISTRY;
use steel_registry::blocks::BlockRef;
use steel_registry::blocks::block_state_ext::BlockStateExt;
use steel_registry::blocks::properties::{BlockStateProperties, EnumProperty, IntProperty};
use steel_utils::{BlockStateId, Direction};

use crate::behavior::{BlockPlaceContext, block::default_can_be_replaced};
use crate::world::LevelReader;

pub const MAX_SEGMENT_AMOUNT: u8 = 4;
const FACING_PROPERTY: EnumProperty<Direction> = BlockStateProperties::HORIZONTAL_FACING;

pub fn segmentable_get_state_for_placement(
    block_ref: BlockRef,
    segment_property: &IntProperty,
    context: &BlockPlaceContext<'_>,
) -> BlockStateId {
    let existing_state = context.world.get_block_state(context.place_pos());
    segmentable_placement_state(
        block_ref,
        segment_property,
        existing_state,
        context.horizontal_direction(),
    )
}

pub fn segmentable_can_be_replaced(
    segment_property: &IntProperty,
    state: BlockStateId,
    context: &BlockPlaceContext<'_>,
) -> bool {
    (!context.is_secondary_use_active()
        && context.with_item(|item| item.item() == REGISTRY.items.by_block(state.get_block()))
        && state.get_value(segment_property) < MAX_SEGMENT_AMOUNT)
        || default_can_be_replaced(state, context)
}

fn segmentable_placement_state(
    block_ref: BlockRef,
    segment_property: &IntProperty,
    existing_state: BlockStateId,
    horizontal_direction: Direction,
) -> BlockStateId {
    if existing_state.get_block() == block_ref {
        let amount = (existing_state.get_value(segment_property) + 1).min(MAX_SEGMENT_AMOUNT);
        existing_state.set_value(segment_property, amount)
    } else {
        block_ref
            .default_state()
            .set_value(&FACING_PROPERTY, horizontal_direction.opposite())
    }
}

#[cfg(test)]
mod tests {
    use glam::DVec3;
    use steel_registry::item_stack::ItemStack;
    use steel_registry::{test_support::init_test_registry, vanilla_blocks, vanilla_items};
    use steel_utils::{BlockPos, types::InteractionHand};

    use super::*;
    use crate::{
        behavior::{
            BLOCK_BEHAVIORS, BlockHitResult, BlockStateBehaviorExt, PlacementOrientation,
            PlacementSource, init_behaviors,
        },
        test_support::test_world,
    };

    fn place_context(
        item_in_hand: &mut ItemStack,
        is_secondary_use_active: bool,
    ) -> BlockPlaceContext<'_> {
        let hit_result = BlockHitResult {
            location: DVec3::ZERO,
            direction: Direction::Up,
            block_pos: BlockPos::ZERO,
            miss: false,
            inside: false,
            world_border_hit: false,
        };
        let source = PlacementSource::direct(
            None,
            InteractionHand::MainHand,
            item_in_hand,
            PlacementOrientation::Player {
                rotation: 0.0,
                pitch: 0.0,
            },
            is_secondary_use_active,
        );
        BlockPlaceContext::new(test_world(), source, &hit_result)
    }

    #[test]
    fn placement_uses_horizontal_direction_and_preserves_existing_facing() {
        init_test_registry();

        let placed = segmentable_placement_state(
            &vanilla_blocks::LEAF_LITTER,
            &BlockStateProperties::SEGMENT_AMOUNT,
            vanilla_blocks::AIR.default_state(),
            Direction::East,
        );
        assert_eq!(
            placed.get_value(&BlockStateProperties::HORIZONTAL_FACING),
            Direction::West
        );

        let existing = vanilla_blocks::LEAF_LITTER
            .default_state()
            .set_value(&BlockStateProperties::HORIZONTAL_FACING, Direction::North)
            .set_value(&BlockStateProperties::SEGMENT_AMOUNT, 2);
        let stacked = segmentable_placement_state(
            &vanilla_blocks::LEAF_LITTER,
            &BlockStateProperties::SEGMENT_AMOUNT,
            existing,
            Direction::East,
        );
        assert_eq!(
            stacked.get_value(&BlockStateProperties::HORIZONTAL_FACING),
            Direction::North
        );
        assert_eq!(stacked.get_value(&BlockStateProperties::SEGMENT_AMOUNT), 3);
    }

    #[test]
    fn replacement_matches_vanilla_segmentable_and_default_rules() {
        init_test_registry();
        init_behaviors();

        let leaf_litter = vanilla_blocks::LEAF_LITTER.default_state();
        let mut leaf_litter_item = ItemStack::new(&vanilla_items::LEAF_LITTER);
        assert!(leaf_litter.can_be_replaced(&place_context(&mut leaf_litter_item, false)));
        assert!(!leaf_litter.can_be_replaced(&place_context(&mut leaf_litter_item, true)));

        let full_leaf_litter =
            leaf_litter.set_value(&BlockStateProperties::SEGMENT_AMOUNT, MAX_SEGMENT_AMOUNT);
        assert!(!full_leaf_litter.can_be_replaced(&place_context(&mut leaf_litter_item, false,)));
        let mut stone = ItemStack::new(&vanilla_items::STONE);
        assert!(leaf_litter.can_be_replaced(&place_context(&mut stone, false)));
        let mut empty = ItemStack::empty();
        assert!(leaf_litter.can_be_replaced(&place_context(&mut empty, false)));
    }

    #[test]
    fn only_flower_beds_are_bonemealable() {
        init_test_registry();
        init_behaviors();

        assert!(
            BLOCK_BEHAVIORS
                .get_behavior(&vanilla_blocks::LEAF_LITTER)
                .as_bonemealable()
                .is_none()
        );
        assert!(
            BLOCK_BEHAVIORS
                .get_behavior(&vanilla_blocks::PINK_PETALS)
                .as_bonemealable()
                .is_some()
        );
    }
}
