use crate::world::game_event::GameEventContext;
use std::sync::Arc;
use steel_registry::blocks::block_state_ext::BlockStateExt;
use steel_registry::blocks::properties::BlockStateProperties;
use steel_registry::item_stack::ItemStack;
use steel_registry::level_events;
use steel_registry::vanilla_game_events;
use steel_utils::BlockPos;
use steel_utils::BlockStateId;
use steel_utils::types::UpdateFlags;

use super::DispenseItemBehavior;
use crate::behavior::blocks::FireBlock;
use crate::behavior::blocks::container::dispenser_block::FACING;
use crate::world::World;

fn can_light(state: BlockStateId) -> bool {
    let Some(lit) = state.try_get_value(&BlockStateProperties::LIT) else {
        return false;
    };
    if lit {
        return false;
    }
    true
}

pub struct FlintAndSteelDispenseBehavior;

impl DispenseItemBehavior for FlintAndSteelDispenseBehavior {
    fn dispense(
        &self,
        world: &Arc<World>,
        pos: BlockPos,
        state: BlockStateId,
        mut item: ItemStack,
    ) -> ItemStack {
        let facing = state.get_value(FACING);
        let target_pos = pos.relative(facing);
        let target_state = world.get_block_state(target_pos);
        let mut success = false;

        if can_light(target_state) {
            world.set_block(
                target_pos,
                target_state.set_value(&BlockStateProperties::LIT, true),
                UpdateFlags::UPDATE_ALL_IMMEDIATE,
            );
            world.game_event(
                &vanilla_game_events::BLOCK_CHANGE,
                target_pos,
                &GameEventContext::new(None, None),
            );
            success = true;
        } else if FireBlock::can_be_placed_at(world, target_pos, facing) {
            world.set_block(
                target_pos,
                FireBlock::get_state(world.as_ref(), target_pos),
                UpdateFlags::UPDATE_ALL,
            );
            world.game_event(
                &vanilla_game_events::BLOCK_PLACE,
                target_pos,
                &GameEventContext::new(None, None),
            );
            success = true;
        }

        if success {
            item.hurt_and_break(1, false);
        } else {
            world.level_event(level_events::SOUND_DISPENSER_FAIL, pos, 0, None);
        }

        item
    }
}
