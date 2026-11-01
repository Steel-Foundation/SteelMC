use super::super::*;

use crate::content::{BlockProperties, HorizontalPlacement};
use steel_registry::{
    blocks::shapes::{BooleanOp, join_is_not_empty},
    vanilla_blocks,
};
use steel_utils::BlockStateId;

#[test]
fn horizontal_geometry_matches_extracted_stairs_in_every_shape_channel() -> Result<(), String> {
    let mut registry = Registry::new_vanilla();
    let state = register_block(
        &mut registry,
        BlockDefinition::new("facing:chair")
            .properties(BlockProperties::of_full_copy("minecraft:oak_planks"))
            .vanilla_geometry("minecraft:oak_stairs".into())
            .horizontal_facing(HorizontalPlacement::Player),
    )?;
    let block = registry
        .blocks
        .by_state_id(BlockStateId(state.id))
        .ok_or("missing chair")?;
    let model = registry
        .client_content
        .block(block)
        .ok_or("missing projection")?;
    let family = model.horizontal.as_ref().ok_or("missing facing family")?;
    for offset in 0..block.state_count() {
        let state = BlockStateId(registry.blocks.get_base_state_id(block).0 + offset);
        let facing = registry
            .blocks
            .get_property_str(state, "facing")
            .ok_or("missing facing")?;
        let expected = registry
            .blocks
            .state_id_from_block_defaulted_properties(
                &vanilla_blocks::OAK_STAIRS,
                [("facing", facing)],
            )
            .ok_or("missing stair state")?;
        let expected_offset = expected.0
            - registry
                .blocks
                .get_base_state_id(&vanilla_blocks::OAK_STAIRS)
                .0;
        for (actual, expected) in [
            (
                block.get_collision_shape(offset),
                vanilla_blocks::OAK_STAIRS.get_collision_shape(expected_offset),
            ),
            (
                block.get_support_shape(offset),
                vanilla_blocks::OAK_STAIRS.get_support_shape(expected_offset),
            ),
            (
                block.get_outline_shape(offset),
                vanilla_blocks::OAK_STAIRS.get_outline_shape(expected_offset),
            ),
            (
                block.get_occlusion_shape(offset),
                vanilla_blocks::OAK_STAIRS.get_occlusion_shape(expected_offset),
            ),
            (
                block.get_interaction_shape(offset),
                vanilla_blocks::OAK_STAIRS.get_interaction_shape(expected_offset),
            ),
            (
                block.get_visual_shape(offset),
                vanilla_blocks::OAK_STAIRS.get_visual_shape(expected_offset),
            ),
        ] {
            assert!(
                !join_is_not_empty(actual, expected, BooleanOp::NotSame),
                "{facing} geometry must match native stairs"
            );
        }
        assert_eq!(family.states[usize::from(offset)].carrier, expected);
    }
    Ok(())
}
