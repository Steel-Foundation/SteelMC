use crate::behavior::{InteractionResult, ItemBehavior};
use crate::entity::LivingEntity;
use crate::player::Player;
use steel_macros::item_behavior;
use steel_registry::data_components::vanilla_components::CUSTOM_NAME;
use steel_registry::item_stack::ItemStack;
use steel_utils::types::InteractionHand;
use text_components::TextComponent;

/// Vanilla name tag behavior.
#[item_behavior]
pub struct NameTagItem;

impl ItemBehavior for NameTagItem {
    fn interact_living_entity(
        &self,
        _stack: &mut ItemStack,
        _player: &Player,
        _target: &dyn LivingEntity,
        _hand: InteractionHand,
    ) -> InteractionResult {
        let Some(component) = _stack.get(CUSTOM_NAME) else {
            return InteractionResult::Pass;
        };
        InteractionResult::Pass
    }
}
