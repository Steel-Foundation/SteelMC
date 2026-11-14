use std::borrow::Cow;
use std::sync::Arc;

use steel_macros::item_behavior;
use steel_registry::data_components::PotionContents;
use steel_registry::item_stack::ItemStack;
use steel_registry::items::ItemRef;
use steel_registry::vanilla_potions;
use text_components::TextComponent;

use crate::behavior::item::{ArrowItemBehavior, ItemBehavior};
use crate::entity::{LivingEntityRef, SharedEntity};
use crate::world::World;

use super::arrow::create_arrow_entity;
use super::dynamic_name::potion_name;

/// Provides the potion-content-dependent name and the poison default instance.
#[item_behavior]
pub struct TippedArrowItem;

impl ItemBehavior for TippedArrowItem {
    fn get_name<'a>(&self, stack: &'a ItemStack) -> Cow<'a, TextComponent> {
        potion_name(stack)
    }

    fn get_default_instance(&self, item: ItemRef) -> ItemStack {
        PotionContents::create_item_stack(item, &vanilla_potions::POISON)
    }

    fn as_arrow_item(&self) -> Option<&dyn ArrowItemBehavior> {
        Some(self)
    }
}

impl ArrowItemBehavior for TippedArrowItem {
    fn create_arrow(
        &self,
        world: &Arc<World>,
        pickup_item_stack: &mut ItemStack,
        shooter: LivingEntityRef<'_>,
        weapon: Option<&ItemStack>,
        is_crit: bool,
    ) -> Option<SharedEntity> {
        Some(create_arrow_entity(
            world,
            pickup_item_stack,
            shooter,
            weapon,
            is_crit,
        ))
    }
}
