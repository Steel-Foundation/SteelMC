use std::sync::Arc;
use steel_registry::blocks::block_state_ext::BlockStateExt;
use steel_registry::fluid::FluidStateExt;
use steel_registry::item_stack::ItemStack;
use steel_registry::vanilla_game_events;
use steel_utils::BlockPos;
use steel_utils::BlockStateId;

use super::DefaultDispenseBehavior;
use super::DispenseItemBehavior;
use crate::behavior::blocks::container::dispenser_block::FACING;
use crate::behavior::items::water_potion_stack;
use crate::world::World;
use crate::world::game_event::GameEventContext;

pub struct GlassBottleDispenseBehavior;

impl DispenseItemBehavior for GlassBottleDispenseBehavior {
    fn dispense(
        &self,
        world: &Arc<World>,
        pos: BlockPos,
        state: BlockStateId,
        item: ItemStack,
    ) -> ItemStack {
        let target_pos = pos.relative(state.get_value(FACING));
        // TODO: honey bottle from a beehive with honey_level >= 5 — needs
        // beehive honey-level tracking, which isn't ported yet.
        if world
            .get_block_state(target_pos)
            .get_fluid_state()
            .is_water()
        {
            world.game_event(
                &vanilla_game_events::FLUID_PICKUP,
                pos,
                &GameEventContext::new(None, None),
            );
            return water_potion_stack();
        }

        DefaultDispenseBehavior.dispense(world, pos, state, item)
    }
}

// TODO: plain Potion dispensing — a water bottle in front of the dispenser
// converts CONVERTABLE_TO_MUD blocks to mud. Needs verifying that tag exists.
