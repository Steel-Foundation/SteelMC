use crate::behavior::BLOCK_BEHAVIORS;
use crate::behavior::BlockStateBehaviorExt;
use crate::inventory::click::Click::Pickup;
use std::sync::Arc;
use steel_registry::blocks::block_state_ext::BlockStateExt;
use steel_registry::item_stack::ItemStack;
use steel_registry::{vanilla_blocks, vanilla_game_events, vanilla_items};
use steel_utils::BlockPos;
use steel_utils::BlockStateId;
use steel_utils::types::UpdateFlags;

use super::DefaultDispenseBehavior;
use super::DispenseItemBehavior;
use crate::behavior::blocks::container::dispenser_block::FACING;
use crate::world::{World, game_event::GameEventContext};

pub struct BucketDispenseBehavior;

impl DispenseItemBehavior for BucketDispenseBehavior {
    fn dispense(
        &self,
        world: &Arc<World>,
        pos: BlockPos,
        state: BlockStateId,
        item: ItemStack,
    ) -> ItemStack {
        let facing = state.get_value(FACING);
        let target_pos = pos.relative(facing);
        let target_state = world.get_block_state(target_pos);

        if item.item() == &*vanilla_items::BUCKET {
            let block_behavior = BLOCK_BEHAVIORS.get_behavior(target_state.get_block());
            if let Some(result) = block_behavior.pickup_block(world, target_pos, target_state, None)
            {
                world.game_event(
                    &vanilla_game_events::FLUID_PICKUP,
                    target_pos,
                    &GameEventContext::new(None, None),
                );
                return result.filled_bucket;
            } else {
                return DefaultDispenseBehavior.dispense(world, pos, state, item);
            }
        }

        let fluid_block = if item.item() == &*vanilla_items::WATER_BUCKET {
            &vanilla_blocks::WATER
        } else if item.item() == &*vanilla_items::LAVA_BUCKET {
            &vanilla_blocks::LAVA
        } else {
            return DefaultDispenseBehavior.dispense(world, pos, state, item);
        };

        let can_replace = target_state.can_be_replaced_by_fluid(fluid_block);
        if can_replace {
            if !target_state.get_block().config.liquid && !target_state.get_block().config.is_air {
                world.destroy_block(target_pos, true);
            }
            if world.set_block(
                target_pos,
                fluid_block.default_state(),
                UpdateFlags::UPDATE_ALL,
            ) {
                return ItemStack::new(&vanilla_items::BUCKET);
            }
        }

        DefaultDispenseBehavior.dispense(world, pos, state, item)
    }
}
