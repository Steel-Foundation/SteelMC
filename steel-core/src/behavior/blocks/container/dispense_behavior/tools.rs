use crate::world::game_event::GameEventContext;
use std::sync::Arc;
use steel_registry::blocks::block_state_ext::BlockStateExt;
use steel_registry::blocks::properties::BlockStateProperties;
use steel_registry::item_stack::ItemStack;
use steel_registry::vanilla_game_events;
use steel_utils::BlockPos;
use steel_utils::BlockStateId;
use steel_utils::types::UpdateFlags;

use super::{DispenseItemBehavior, DispenseResult};
use crate::behavior::blocks::container::dispenser_block::FACING;
use crate::behavior::blocks::{FireBlock, TntBlock};
use crate::behavior::items::can_light;
use crate::world::World;
use steel_registry::vanilla_blocks;

pub struct FlintAndSteelDispenseBehavior;

impl DispenseItemBehavior for FlintAndSteelDispenseBehavior {
    fn dispense(
        &self,
        world: &Arc<World>,
        pos: BlockPos,
        state: BlockStateId,
        item: &mut ItemStack,
    ) -> DispenseResult {
        let facing = state.get_value(FACING);
        let target_pos = pos.relative(facing);
        let target_state = world.get_block_state(target_pos);

        if FireBlock::can_be_placed_at(world, target_pos, facing) {
            world.set_block(
                target_pos,
                FireBlock::get_state(world.as_ref(), target_pos),
                UpdateFlags::UPDATE_ALL,
            );
            world.game_event(
                &vanilla_game_events::BLOCK_PLACE,
                target_pos,
                &GameEventContext::default(),
            );
        } else if can_light(target_state) {
            world.set_block(
                target_pos,
                target_state.set_value(&BlockStateProperties::LIT, true),
                UpdateFlags::UPDATE_ALL,
            );

            world.game_event(
                &vanilla_game_events::BLOCK_CHANGE,
                target_pos,
                &GameEventContext::default(),
            );
        } else if target_state.get_block() == &vanilla_blocks::TNT {
            if !TntBlock::prime(world, target_pos) {
                return DispenseResult::Failure;
            }

            world.remove_block(target_pos, false);
        }

        item.hurt_and_break(1, false);
        DispenseResult::Success
    }
}

// TODO: Shears need beehive harvesting with bee release and honey-level reset,
// plus entity targeting for leash cutting and shearable mobs.

// TODO: BrushDispenseBehavior — needs the Armadillo entity, which doesn't exist.
