//! Ender eye item behavior implementation.

use std::sync::Arc;

use steel_macros::item_behavior;
use steel_registry::REGISTRY;
use steel_registry::blocks::block_state_ext::BlockStateExt;
use steel_registry::blocks::properties::BlockStateProperties;
use steel_registry::items::item::BlockHitResult;
use steel_registry::level_events;
use steel_registry::vanilla_blocks;
use steel_utils::types::InteractionHand;
use steel_utils::types::UpdateFlags;

use crate::behavior::InventoryAccess;
use crate::behavior::ItemBehavior;
use crate::behavior::context::InteractionResult;
use crate::player::Player;
use crate::world::World;

/// Behavior for the ender eye item.
///
/// When used on an end portal frame without an eye, places the eye
/// and checks for portal completion.
#[item_behavior]
pub struct EnderEyeItem;

impl ItemBehavior for EnderEyeItem {
    fn use_on(
        &self,
        _player: &Player,
        _hand: InteractionHand,
        hit_result: BlockHitResult,
        world: &Arc<World>,
        inv: &mut InventoryAccess,
    ) -> InteractionResult {
        // TODO: updateNeighborForOutputSignal, portal completion check

        let clicked_pos = hit_result.block_pos;
        let clicked_state = world.get_block_state(clicked_pos);

        let Some(clicked_block) = REGISTRY.blocks.by_state_id(clicked_state) else {
            return InteractionResult::Pass;
        };

        if clicked_block.key != vanilla_blocks::END_PORTAL_FRAME.key {
            return InteractionResult::Pass;
        }

        let has_eye: bool = clicked_state.get_value(&BlockStateProperties::EYE);
        if has_eye {
            return InteractionResult::Pass;
        }

        let new_state = clicked_state.set_value(&BlockStateProperties::EYE, true);

        if !world.set_block(clicked_pos, new_state, UpdateFlags::UPDATE_ALL_IMMEDIATE) {
            return InteractionResult::Pass;
        }

        // Play the end portal frame fill sound effect (no exclusion, all players hear it)
        world.level_event(level_events::END_PORTAL_FRAME_FILL, clicked_pos, 0, None);

        inv.with_item(|item| item.shrink(1));

        InteractionResult::Success
    }
}
