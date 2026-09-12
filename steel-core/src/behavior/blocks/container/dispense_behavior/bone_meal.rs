use std::sync::Arc;
use steel_registry::blocks::block_state_ext::BlockStateExt;
use steel_registry::item_stack::ItemStack;
use steel_registry::level_events;
use steel_utils::BlockPos;
use steel_utils::BlockStateId;

use super::DispenseItemBehavior;
use crate::behavior::blocks::container::dispenser_block::FACING;
use crate::behavior::items::BoneMealItem;
use crate::world::World;

pub struct BoneMealDispenseBehavior;

impl DispenseItemBehavior for BoneMealDispenseBehavior {
    fn dispense(
        &self,
        world: &Arc<World>,
        pos: BlockPos,
        state: BlockStateId,
        mut item: ItemStack,
    ) -> ItemStack {
        let facing = state.get_value(FACING);
        let target_pos = pos.relative(facing);

        if BoneMealItem::grow(world, target_pos) {
            world.level_event(
                level_events::PARTICLES_AND_SOUND_PLANT_GROWTH,
                target_pos,
                15,
                None,
            );
            world.level_event(level_events::SOUND_DISPENSER_DISPENSE, pos, 0, None);
            item.shrink(1);
        } else {
            world.level_event(level_events::SOUND_DISPENSER_FAIL, pos, 0, None);
        }

        item
    }
}
