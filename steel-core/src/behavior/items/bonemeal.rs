use steel_registry::item_stack::ItemStack;
use steel_utils::BlockPos;

use crate::{
    behavior::{BLOCK_BEHAVIORS, InteractionResult, ItemBehavior, UseOnContext},
    world::World,
};

/// Behavior for the Bonemeal item.
pub struct BonemealBehavior;

impl BonemealBehavior {
    fn grow(item_stack: &mut ItemStack, world: &World, pos: BlockPos) -> InteractionResult {
        let state = world.get_block_state(&pos);
        let Some(behavior) = BLOCK_BEHAVIORS.get_behavior_for_state(state) else {
            log::info!("couldnt get behavior");
            return InteractionResult::Pass;
        };
        if let Some(bonemealable) = behavior.as_bonemealable() {
            if !bonemealable.is_bonemealable(state, world, pos) {
                log::info!("wasnt bonemealable");
                return InteractionResult::Pass;
            }

            bonemealable.apply_bonemeal(state, world, pos);
            item_stack.shrink(1);

            // TODO: spawn particles
            return InteractionResult::Success;
        }
        log::info!("Bonemealable wasnt implemented");
        InteractionResult::Pass
    }
}

impl ItemBehavior for BonemealBehavior {
    fn use_on(&self, context: &mut UseOnContext) -> InteractionResult {
        log::info!("BonemealBehavior::use_on");
        Self::grow(
            context.item_stack,
            context.world,
            context.hit_result.block_pos,
        )
    }
}
