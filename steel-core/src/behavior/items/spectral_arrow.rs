use std::sync::Arc;

use steel_macros::item_behavior;
use steel_registry::item_stack::ItemStack;

use crate::behavior::item::{ArrowItemBehavior, ItemBehavior};
use crate::entity::entities::SpectralArrowEntity;
use crate::entity::{LivingEntityRef, SharedEntity, next_entity_id};
use crate::world::World;

/// Fires a [`SpectralArrowEntity`] instead of a plain arrow.
#[item_behavior]
pub struct SpectralArrowItem;

impl ItemBehavior for SpectralArrowItem {
    fn as_arrow_item(&self) -> Option<&dyn ArrowItemBehavior> {
        Some(self)
    }
}

impl ArrowItemBehavior for SpectralArrowItem {
    fn create_arrow(
        &self,
        world: &Arc<World>,
        pickup_item_stack: &mut ItemStack,
        shooter: LivingEntityRef<'_>,
        weapon: Option<&ItemStack>,
        is_crit: bool,
    ) -> Option<SharedEntity> {
        Some(Arc::new(SpectralArrowEntity::from_owner(
            next_entity_id(),
            Arc::downgrade(world),
            shooter,
            pickup_item_stack,
            weapon,
            is_crit,
        )))
    }
}
