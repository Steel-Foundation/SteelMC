//! Vanilla `DyeItem` behavior: dyes an alive, unsheared sheep.

use steel_macros::item_behavior;
use steel_registry::data_components::vanilla_components::DYE;
use steel_registry::item_stack::ItemStack;
use steel_registry::sound_events::ITEM_DYE_USE;
use steel_utils::Downcast as _;
use steel_utils::types::InteractionHand;

use crate::behavior::{InteractionResult, ItemBehavior};
use crate::entity::entities::SheepEntity;
use crate::entity::{Entity, LivingEntity};
use crate::player::Player;

/// Behavior for the sixteen dye items (`DyeItem`).
///
/// Ports vanilla `DyeItem.interactLivingEntity`: dying a sheep plays the `DYE_USE`
/// sound, sets the sheep's wool color, and consumes one dye from the stack.
#[item_behavior(class = "DyeItem")]
pub struct DyeItem;

impl ItemBehavior for DyeItem {
    fn interact_living_entity(
        &self,
        stack: &mut ItemStack,
        _player: &Player,
        target: &dyn LivingEntity,
        _hand: InteractionHand,
    ) -> InteractionResult {
        let Some(dye_color) = stack.get(DYE).copied() else {
            return InteractionResult::Pass;
        };
        let Some(sheep) = target.downcast_ref::<SheepEntity>() else {
            return InteractionResult::Pass;
        };

        if !Entity::is_alive(sheep) || sheep.is_sheared() || sheep.color() == dye_color {
            return InteractionResult::Pass;
        }

        sheep.play_sound(&ITEM_DYE_USE, 1.0, 1.0);
        sheep.set_color(dye_color);
        stack.shrink(1);

        InteractionResult::Success
    }
}
