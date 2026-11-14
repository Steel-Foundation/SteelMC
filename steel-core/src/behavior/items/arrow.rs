use std::sync::Arc;

use steel_macros::item_behavior;
use steel_registry::item_stack::ItemStack;

use crate::behavior::item::{ArrowItemBehavior, ItemBehavior};
use crate::entity::entities::ArrowEntity;
use crate::entity::{LivingEntityRef, SharedEntity, next_entity_id};
use crate::world::World;

pub(crate) fn create_arrow_entity(
    world: &Arc<World>,
    pickup_item_stack: &mut ItemStack,
    shooter: LivingEntityRef<'_>,
    weapon: Option<&ItemStack>,
    is_crit: bool,
) -> SharedEntity {
    Arc::new(ArrowEntity::from_owner(
        next_entity_id(),
        Arc::downgrade(world),
        shooter,
        pickup_item_stack,
        weapon,
        is_crit,
    ))
}

/// Behavior for the plain arrow item.
#[item_behavior]
pub struct ArrowItem;

impl ItemBehavior for ArrowItem {
    fn as_arrow_item(&self) -> Option<&dyn ArrowItemBehavior> {
        Some(self)
    }
}

impl ArrowItemBehavior for ArrowItem {
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
