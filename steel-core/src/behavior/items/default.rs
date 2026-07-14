//! Default item behavior implementation.

use crate::behavior::{InteractionResult, ItemBehavior, UseItemContext};
use crate::entity::Entity;
use crate::player::player_inventory::EquipmentSwapResult;

/// Default item behavior - does nothing special.
pub struct DefaultItemBehavior;

impl ItemBehavior for DefaultItemBehavior {
    fn use_item(&self, context: &mut UseItemContext) -> InteractionResult {
        let Some(equippable) = context.inv.with_item(|item| item.get_equippable().cloned()) else {
            return InteractionResult::Pass;
        };

        if !equippable.swappable || !equippable.can_be_equipped_by(context.player.entity_type()) {
            return InteractionResult::Pass;
        }

        let slot = equippable.slot;
        let result = context.inv.with_inventory(|inventory| {
            inventory.try_swap_with_equipment_slot(
                context.hand,
                slot,
                context.player.has_infinite_materials(),
            )
        });

        match result {
            EquipmentSwapResult::Success(overflow) => {
                if !overflow.is_empty() {
                    let _ = context.player.drop_item(overflow, false, false);
                }
                InteractionResult::Success
            }
            EquipmentSwapResult::Fail => InteractionResult::Fail,
        }
    }
}
