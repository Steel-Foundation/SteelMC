use std::sync::Arc;
use steel_registry::blocks::block_state_ext::BlockStateExt;
use steel_registry::item_stack::ItemStack;
use steel_registry::level_events;
use steel_utils::BlockPos;
use steel_utils::BlockStateId;

use super::DispenseItemBehavior;
use crate::behavior::blocks::container::dispenser_block::FACING;
use crate::world::World;

pub struct DefaultDispenseBehavior;

impl DispenseItemBehavior for DefaultDispenseBehavior {
    fn dispense(
        &self,
        world: &Arc<World>,
        pos: BlockPos,
        state: BlockStateId,
        item: ItemStack,
    ) -> ItemStack {
        let facing = state.get_value(FACING);
        let target_pos = pos.relative(facing);

        world.drop_item_stack(target_pos, item);
        world.level_event(level_events::SOUND_DISPENSER_DISPENSE, pos, 0, None);
        ItemStack::empty()
    }
}
