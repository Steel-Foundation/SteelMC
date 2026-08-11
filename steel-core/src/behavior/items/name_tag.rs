use crate::behavior::{InteractionResult, ItemBehavior};
use crate::entity::LivingEntity;
use crate::player::Player;
use steel_macros::item_behavior;
use steel_registry::data_components::vanilla_components::CUSTOM_NAME;
use steel_registry::item_stack::ItemStack;
use steel_utils::types::InteractionHand;

/// Vanilla name tag behavior.
#[item_behavior]
pub struct NameTagItem;

impl ItemBehavior for NameTagItem {
    fn interact_living_entity(
        &self,
        stack: &mut ItemStack,
        _player: &Player,
        target: &dyn LivingEntity,
        _hand: InteractionHand,
    ) -> InteractionResult {
        if target.entity_type().can_serialize
            && let Some(component) = stack.get(CUSTOM_NAME)
        {
            if LivingEntity::is_alive(target) {
                target.set_custom_name(Some(component.clone()));
                if let Some(mob) = target.as_mob() {
                    mob.set_persistence_required();
                }
                stack.shrink(1);
            }
            InteractionResult::Success
        } else {
            InteractionResult::Pass
        }
    }
}
