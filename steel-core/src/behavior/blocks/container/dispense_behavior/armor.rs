use std::sync::Arc;

use steel_registry::blocks::block_state_ext::BlockStateExt as _;
use steel_registry::item_stack::ItemStack;
use steel_utils::{BlockLocalAabb, BlockPos, BlockStateId};

use super::{DefaultDispenseBehavior, DispenseItemBehavior, DispenseResult};
use crate::behavior::blocks::container::dispenser_block::FACING;
use crate::world::World;

pub struct ArmorDispenseBehavior;

impl DispenseItemBehavior for ArmorDispenseBehavior {
    fn dispense(
        &self,
        world: &Arc<World>,
        pos: BlockPos,
        state: BlockStateId,
        item: &mut ItemStack,
    ) -> DispenseResult {
        let target_pos = pos.relative(state.get_value(FACING));
        let target_aabb = BlockLocalAabb::FULL_BLOCK.at_block(target_pos);

        if let Some(slot) = item.get_equippable_slot() {
            for entity in world.get_entities_in_aabb(&target_aabb) {
                let Some(living) = entity.as_living_entity() else {
                    continue;
                };

                if !living.can_equip_with_dispenser(item) {
                    continue;
                }

                let equipped = item.split(1);
                living.on_equip_item(slot, &ItemStack::empty(), &equipped);
                living.living_base().equipment().lock().set(slot, equipped);

                if let Some(mob) = living.as_mob() {
                    mob.set_guaranteed_drop(slot);
                    mob.set_persistence_required();
                }

                return DispenseResult::Success;
            }
        }

        DefaultDispenseBehavior.dispense(world, pos, state, item)
    }
}
