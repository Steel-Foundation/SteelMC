use std::sync::Arc;
use steel_registry::REGISTRY;
use steel_registry::blocks::block_state_ext::BlockStateExt;
use steel_registry::item_stack::ItemStack;
use steel_utils::BlockPos;
use steel_utils::BlockStateId;
use steel_utils::types::UpdateFlags;

use super::DefaultDispenseBehavior;
use super::DispenseItemBehavior;
use crate::behavior::blocks::container::dispenser_block::FACING;
use crate::behavior::waxables::get_waxed_from_normal_variant;
use crate::world::World;

pub struct HoneycombDispenseBehavior;

impl DispenseItemBehavior for HoneycombDispenseBehavior {
    fn dispense(
        &self,
        world: &Arc<World>,
        pos: BlockPos,
        state: BlockStateId,
        item: ItemStack,
    ) -> ItemStack {
        let target_pos = pos.relative(state.get_value(FACING));
        let old_state = world.get_block_state(target_pos);
        let maybe_waxed = get_waxed_from_normal_variant(old_state.get_block()).map(|waxed_block| {
            REGISTRY
                .blocks
                .copy_matching_properties(old_state, waxed_block)
        });
        if let Some(waxed) = maybe_waxed {
            world.set_block(target_pos, waxed, UpdateFlags::UPDATE_ALL);
            world.level_event(3003, target_pos, 0, None);
            ItemStack::empty()
        } else {
            DefaultDispenseBehavior.dispense(world, pos, state, item)
        }
    }
}
