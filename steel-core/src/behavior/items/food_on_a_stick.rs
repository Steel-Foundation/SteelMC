//! Food-on-a-stick item behavior implementation.

use steel_macros::item_behavior;
use steel_registry::entity_type::EntityTypeRef;
use steel_registry::item_stack::ItemStack;
use steel_registry::stat::vanilla_stat_types;
use steel_registry::vanilla_items;

use crate::behavior::{InteractionResult, ItemBehavior, UseItemContext};
use crate::entity::Entity as _;
use crate::player::Player;

/// Behavior for vanilla `FoodOnAStickItem`.
#[item_behavior]
pub struct FoodOnAStickItem {
    #[json_arg(vanilla_entities)]
    can_interact_with: EntityTypeRef,
    #[json_arg(value)]
    consume_item_damage: i32,
}

impl FoodOnAStickItem {
    /// Creates a food-on-a-stick behavior for one controlled vehicle type.
    #[must_use]
    pub const fn new(can_interact_with: EntityTypeRef, consume_item_damage: i32) -> Self {
        Self {
            can_interact_with,
            consume_item_damage,
        }
    }
    fn pass_without_boost(player: &Player, item: ItemStack) -> InteractionResult {
        player.award_stat(&vanilla_stat_types::ITEM_USED, item.item());
        InteractionResult::Pass
    }
}

impl ItemBehavior for FoodOnAStickItem {
    fn use_item(&self, context: &mut UseItemContext) -> InteractionResult {
        let item = context.inv.with_item(|item| item.clone());
        let Some(vehicle) = context.player.controlled_vehicle() else {
            return Self::pass_without_boost(context.player, item);
        };
        if vehicle.entity_type() != self.can_interact_with {
            return Self::pass_without_boost(context.player, item);
        }
        let Some(steerable) = vehicle.as_item_steerable() else {
            return Self::pass_without_boost(context.player, item);
        };
        if !steerable.boost() {
            return Self::pass_without_boost(context.player, item);
        }

        let has_infinite_materials = context.player.has_infinite_materials();
        context.inv.with_inventory(|inventory| {
            inventory.hurt_and_convert_item_in_hand_on_break(
                context.hand,
                self.consume_item_damage,
                &vanilla_items::FISHING_ROD,
                has_infinite_materials,
            );
        });

        InteractionResult::SuccessServer
    }
}
