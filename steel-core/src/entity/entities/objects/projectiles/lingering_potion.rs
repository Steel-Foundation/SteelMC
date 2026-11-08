//! Vanilla `ThrownLingeringPotion`.

use crate::entity::prelude::*;
use std::sync::Arc;

use steel_macros::entity_behavior;
use steel_registry::entity_type::EntityTypeRef;
use steel_registry::item_stack::ItemStack;
use steel_registry::vanilla_entity_data::LingeringPotionEntityData;
use steel_registry::{vanilla_entities, vanilla_items};
use steel_utils::locks::CheckedMutex;

use crate::entity::entities::AreaEffectCloudEntity;
use crate::entity::projectile::throwable_item::ThrownItem;
use crate::entity::projectile::{ProjectileBase, ProjectileHit, ProjectileProps, thrown_potion};
use crate::entity::{
    Entity, EntityBase, EntityProps, SharedEntity, apply_implicit_item_stack_components,
    next_entity_id,
};
use crate::world::World;

/// A thrown lingering potion, which leaves an effect cloud where it lands.
#[entity_behavior(class = "ThrownLingeringPotion")]
#[derive(Entity)]
#[entity(hooks = Self::ENTITY)]
#[entity(key = "steel:entity/lingering_potion", constructors)]
pub struct LingeringPotionEntity {
    #[entity(base)]
    base: EntityBase,
    #[entity(entity_type)]
    entity_type: EntityTypeRef,
    #[entity(component)]
    projectile: ProjectileBase,
    #[entity(component)]
    thrown_item: ThrownItem,
    #[entity(synced)]
    entity_data: CheckedMutex<LingeringPotionEntityData>,
}

impl LingeringPotionEntity {
    const ENTITY: EntityProps<Self> = thrown_potion::entity_props();
    const PROJECTILE: ProjectileProps<Self> = thrown_potion::projectile_props();

    fn from_base(base: EntityBase, entity_type: EntityTypeRef) -> Self {
        let mut entity_data = LingeringPotionEntityData::new();
        let thrown_item = ThrownItem::new(&vanilla_items::LINGERING_POTION, &mut entity_data);
        Self {
            base,
            entity_type,
            projectile: ProjectileBase::new(&Self::PROJECTILE),
            thrown_item,
            entity_data: CheckedMutex::new(entity_data),
        }
    }
}

impl thrown_potion::Parts for LingeringPotionEntity {
    /// `ThrownLingeringPotion.onHitAsPotion`: leaves an effect cloud at the
    /// hit entity or where the potion is.
    fn on_hit_as_potion(
        &self,
        _entity: &SharedEntity,
        world: &Arc<World>,
        potion_item: &ItemStack,
        hit: &ProjectileHit,
    ) {
        let position = match hit {
            ProjectileHit::Entity(entity_hit) => entity_hit.entity.position(),
            ProjectileHit::Block { .. } => self.position(),
        };
        let cloud = Arc::new(AreaEffectCloudEntity::new(
            &vanilla_entities::AREA_EFFECT_CLOUD,
            next_entity_id(),
            position,
            Arc::downgrade(world),
        ));
        cloud.set_owner(self.projectile().owner().as_ref());
        cloud.set_radius(3.0);
        cloud.set_radius_on_use(-0.5);
        cloud.set_duration(600);
        cloud.set_wait_time(10);
        cloud.set_radius_per_tick(-cloud.radius() / cloud.duration() as f32);

        let cloud: SharedEntity = cloud;
        apply_implicit_item_stack_components(&cloud, potion_item);
        if let Err(error) = world.try_add_entity(cloud) {
            log::debug!("world rejected a lingering potion cloud: {error}");
        }
    }
}
