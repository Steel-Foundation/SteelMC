use std::sync::Arc;
use steel_registry::REGISTRY;
use steel_registry::blocks::block_state_ext::BlockStateExt;
use steel_registry::item_stack::ItemStack;
use steel_registry::level_events;
use steel_registry::vanilla_blocks;
use steel_utils::BlockPos;
use steel_utils::BlockStateId;
use steel_utils::types::UpdateFlags;

use super::DefaultDispenseBehavior;
use super::DispenseItemBehavior;
use crate::behavior::blocks::RespawnAnchorBlock;
use crate::behavior::blocks::container::dispenser_block::FACING;
use crate::behavior::items::BoneMealItem;
use crate::behavior::waxables::get_waxed_from_normal_variant;
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
        let target_pos = pos.relative(state.get_value(FACING));

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
            world.level_event(
                level_events::PARTICLES_AND_SOUND_WAX_ON,
                target_pos,
                0,
                None,
            );
            ItemStack::empty()
        } else {
            DefaultDispenseBehavior.dispense(world, pos, state, item)
        }
    }
}

pub struct GlowstoneDispenseBehavior;

impl DispenseItemBehavior for GlowstoneDispenseBehavior {
    fn dispense(
        &self,
        world: &Arc<World>,
        pos: BlockPos,
        state: BlockStateId,
        mut item: ItemStack,
    ) -> ItemStack {
        let target_pos = pos.relative(state.get_value(FACING));
        let target_state = world.get_block_state(target_pos);

        if target_state.get_block() == &vanilla_blocks::RESPAWN_ANCHOR {
            if RespawnAnchorBlock::can_be_charged(target_state) {
                RespawnAnchorBlock::charge(None, world, target_pos, target_state);
                item.shrink(1);
            } else {
                world.level_event(level_events::SOUND_DISPENSER_FAIL, pos, 0, None);
            }
            return item;
        }

        DefaultDispenseBehavior.dispense(world, pos, state, item)
    }
}

// TODO: CarvedPumpkinDispenseBehavior — vanilla's primary path needs a
// golem-spawn check that isn't ported (carved_pumpkin_block.rs has none).
// The fallback (equip via ArmorDispenseBehavior-style logic) exists, but
// shipping only that half would silently break dispenser golem farms.
