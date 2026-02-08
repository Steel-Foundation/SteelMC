use std::ptr;

use steel_registry::{
    REGISTRY,
    blocks::{
        Block,
        block_state_ext::BlockStateExt,
        properties::{BlockStateProperties, BoolProperty},
    },
    entity_data::Direction,
    vanilla_block_tags, vanilla_blocks,
};
use steel_utils::types::UpdateFlags;

use crate::behavior::{InteractionResult, ItemBehavior, UseOnContext};

const FLATTENABLES: [&Block; 6] = [
    vanilla_blocks::GRASS_BLOCK,
    vanilla_blocks::DIRT,
    vanilla_blocks::PODZOL,
    vanilla_blocks::COARSE_DIRT,
    vanilla_blocks::MYCELIUM,
    vanilla_blocks::ROOTED_DIRT,
];

const LIT_PROPERTY: BoolProperty = BlockStateProperties::LIT;

/// Behaviour for Shovels, extinguises campfires and turns grass blocks into paths
pub struct ShovelBehaviour;

impl ItemBehavior for ShovelBehaviour {
    fn use_on(&self, context: &mut UseOnContext) -> InteractionResult {
        if context.hit_result.direction == Direction::Down {
            return InteractionResult::Pass;
        }

        let block_state = context.world.get_block_state(&context.hit_result.block_pos);
        let block = block_state.get_block();

        if REGISTRY
            .blocks
            .is_in_tag(block, &vanilla_block_tags::CAMPFIRES_TAG)
        {
            if !block_state.get_value(&LIT_PROPERTY) {
                return InteractionResult::Pass;
            }
            context.world.set_block(
                context.hit_result.block_pos,
                block_state.set_value(&LIT_PROPERTY, false),
                UpdateFlags::UPDATE_ALL_IMMEDIATE,
            );
            return InteractionResult::Success;
        }

        if !context
            .world
            .get_block_state(&context.hit_result.block_pos.above())
            .is_air()
        {
            return InteractionResult::Pass;
        }

        if FLATTENABLES.iter().any(|it| ptr::eq(*it, block)) {
            context
                .item_stack
                .hurt_and_break(1, context.player.has_infinite_materials());
            context.world.set_block(
                context.hit_result.block_pos,
                vanilla_blocks::DIRT_PATH.default_state(),
                UpdateFlags::UPDATE_ALL_IMMEDIATE,
            );
            return InteractionResult::Success;
        }
        InteractionResult::Pass
    }
}
