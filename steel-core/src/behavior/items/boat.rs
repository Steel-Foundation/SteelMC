//! Boat item behavior (`BoatItem`).
//!
//! Using a boat item on a block or water surface spawns the matching boat
//! entity variant and consumes one item, mirroring vanilla `BoatItem.use`.

use std::sync::Arc;

use glam::DVec3;
use steel_macros::item_behavior;
use steel_registry::entity_type::EntityTypeRef;
use steel_registry::stat::vanilla_stat_types;

use crate::behavior::context::{InteractionResult, UseOnContext};
use crate::behavior::item::ItemBehavior;
use crate::entity::entities::{BoatEntity, ChestBoatEntity};
use crate::entity::{SharedEntity, next_entity_id};
use crate::world::World;

/// Behavior for vanilla `BoatItem`.
#[item_behavior]
pub struct BoatItem {
    /// The entity type of the boat variant this item places.
    #[json_arg(vanilla_entities, json = "entity_type")]
    entity_type: EntityTypeRef,
}

impl BoatItem {
    /// Creates a boat item behavior for one boat entity variant.
    #[must_use]
    pub const fn new(entity_type: EntityTypeRef) -> Self {
        Self { entity_type }
    }

    /// Spawns the boat entity matching this item's variant.
    fn spawn_boat(&self, world: &Arc<World>, position: DVec3) -> SharedEntity {
        if self.entity_type.key.path.ends_with("chest_boat") {
            let entity: SharedEntity = Arc::new(ChestBoatEntity::new(
                self.entity_type,
                next_entity_id(),
                position,
                Arc::downgrade(world),
            ));
            return entity;
        }
        let entity: SharedEntity = Arc::new(BoatEntity::new(
            self.entity_type,
            next_entity_id(),
            position,
            Arc::downgrade(world),
        ));
        entity
    }
}

impl ItemBehavior for BoatItem {
    fn use_on(&self, context: &mut UseOnContext) -> InteractionResult {
        let position = context.hit_result.location;
        let boat = self.spawn_boat(context.world, position);
        if let Err(error) = context.world.try_add_entity(boat) {
            log::debug!("failed to spawn boat: {error}");
            return InteractionResult::Fail;
        }

        let item = context.inv.with_item(|item| item.item());
        context
            .player
            .award_stat(&vanilla_stat_types::ITEM_USED, item);
        context.inv.with_item(|item| item.shrink(1));

        InteractionResult::Success
    }
}
