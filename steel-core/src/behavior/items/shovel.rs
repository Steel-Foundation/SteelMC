use std::sync::Arc;

use steel_macros::item_behavior;
use steel_registry::{
    blocks::{
        Block,
        block_state_ext::BlockStateExt,
        properties::{BlockStateProperties, BoolProperty},
    },
    items::item::BlockHitResult,
    vanilla_block_tags::BlockTag,
    vanilla_blocks, vanilla_game_events,
};
use steel_utils::types::UpdateFlags;
use steel_utils::{Direction, types::InteractionHand};

use crate::{
    behavior::{InteractionResult, InventoryAccess, ItemBehavior},
    player::Player,
    world::{World, game_event_context::GameEventContext},
};

const FLATTENABLES: [&Block; 6] = [
    &vanilla_blocks::GRASS_BLOCK,
    &vanilla_blocks::DIRT,
    &vanilla_blocks::PODZOL,
    &vanilla_blocks::COARSE_DIRT,
    &vanilla_blocks::MYCELIUM,
    &vanilla_blocks::ROOTED_DIRT,
];

const LIT_PROPERTY: BoolProperty = BlockStateProperties::LIT;

/// Behavior for Shovels, extinguishes campfires and turns grass blocks into paths
#[item_behavior]
pub struct ShovelItem;

impl ItemBehavior for ShovelItem {
    fn use_on(
        &self,
        player: &Player,
        _hand: InteractionHand,
        hit_result: BlockHitResult,
        world: &Arc<World>,
        inv: &mut InventoryAccess,
    ) -> InteractionResult {
        if hit_result.direction == Direction::Down {
            return InteractionResult::Pass;
        }

        let block_state = world.get_block_state(hit_result.block_pos);
        let block = block_state.get_block();

        // Flattenables — vanilla checks these first
        if FLATTENABLES.contains(&block) {
            if !world.get_block_state(hit_result.block_pos.above()).is_air() {
                return InteractionResult::Pass;
            }
            // TODO: Play SoundEvents.SHOVEL_FLATTEN
            let infinite_materials = player.has_infinite_materials();
            inv.with_item(|item| item.hurt_and_break(1, infinite_materials));
            let updated_state = vanilla_blocks::DIRT_PATH.default_state();
            world.set_block(
                hit_result.block_pos,
                updated_state,
                UpdateFlags::UPDATE_ALL_IMMEDIATE,
            );
            world.game_event(
                &vanilla_game_events::BLOCK_CHANGE,
                hit_result.block_pos,
                &GameEventContext::new(Some(player), Some(updated_state)),
            );
            return InteractionResult::Success;
        }

        // Campfire extinguishing
        if block.has_tag(&BlockTag::CAMPFIRES) {
            if !block_state.get_value(&LIT_PROPERTY) {
                return InteractionResult::Pass;
            }
            // TODO: level_event(1009, pos, 0) — extinguish particle/sound
            // TODO: CampfireBlock::dowse() — eject cooking items
            let updated_state = block_state.set_value(&LIT_PROPERTY, false);
            world.set_block(
                hit_result.block_pos,
                updated_state,
                UpdateFlags::UPDATE_ALL_IMMEDIATE,
            );
            // TODO: hurt_and_break(1, ...) — shovels take durability damage
            world.game_event(
                &vanilla_game_events::BLOCK_CHANGE,
                hit_result.block_pos,
                &GameEventContext::new(Some(player), Some(updated_state)),
            );
            return InteractionResult::Success;
        }

        InteractionResult::Pass
    }
}
