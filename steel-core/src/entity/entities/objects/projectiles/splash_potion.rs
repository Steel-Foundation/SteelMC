//! Vanilla `ThrownSplashPotion`.

use crate::entity::prelude::*;
use std::sync::{Arc, Weak};

use glam::DVec3;
use steel_macros::entity_behavior;
use steel_registry::MobEffectInstance as RegistryMobEffectInstance;
use steel_registry::data_components::{PotionContents, vanilla_components};
use steel_registry::entity_type::EntityTypeRef;
use steel_registry::item_stack::ItemStack;
use steel_registry::vanilla_entity_data::SplashPotionEntityData;
use steel_registry::vanilla_items;
use steel_utils::locks::CheckedMutex;
use steel_utils::{DowncastType, DowncastTypeKey};

use crate::behavior::MOB_EFFECT_BEHAVIORS;
use crate::entity::potion_contents::to_runtime_instance_icon_from_visibility;
use crate::entity::projectile::throwable_item::ThrownItem;
use crate::entity::projectile::{ProjectileBase, ProjectileHit, ProjectileProps, thrown_potion};
use crate::entity::{
    Entity, EntityBase, EntityBaseLoad, EntityProps, Living, SPLASH_HORIZONTAL_REACH,
    SPLASH_RANGE_SQ, SPLASH_VERTICAL_REACH, SharedEntity, compute_margin,
};
use crate::world::World;

/// Effects that would last at most this many ticks are not applied.
const MIN_REMAINING_TICKS: i32 = 20;

/// A thrown splash potion.
#[entity_behavior(class = "ThrownSplashPotion")]
#[derive(Entity)]
#[entity(hooks = Self::ENTITY)]
pub struct SplashPotionEntity {
    #[entity(base)]
    base: EntityBase,
    #[entity(entity_type)]
    entity_type: EntityTypeRef,
    #[entity(component)]
    projectile: ProjectileBase,
    #[entity(component)]
    thrown_item: ThrownItem,
    #[entity(synced)]
    entity_data: CheckedMutex<SplashPotionEntityData>,
}

// SAFETY: This key is owned by Steel and uniquely identifies `SplashPotionEntity`.
unsafe impl DowncastType for SplashPotionEntity {
    const TYPE_KEY: DowncastTypeKey = DowncastTypeKey::new("steel:entity/splash_potion");
}

impl SplashPotionEntity {
    const ENTITY: EntityProps<Self> = thrown_potion::entity_props();
    const PROJECTILE: ProjectileProps<Self> = thrown_potion::projectile_props();

    /// Creates a splash potion with no owner.
    #[must_use]
    pub fn new(entity_type: EntityTypeRef, id: i32, position: DVec3, world: Weak<World>) -> Self {
        Self::from_base(
            EntityBase::new(id, position, entity_type.dimensions, world),
            entity_type,
        )
    }

    /// Reconstructs a splash potion from saved base data.
    #[must_use]
    pub fn from_saved(entity_type: EntityTypeRef, load: EntityBaseLoad) -> Self {
        Self::from_base(
            EntityBase::from_load(load, entity_type.dimensions),
            entity_type,
        )
    }

    fn from_base(base: EntityBase, entity_type: EntityTypeRef) -> Self {
        let mut entity_data = SplashPotionEntityData::new();
        let thrown_item = ThrownItem::new(&vanilla_items::SPLASH_POTION, &mut entity_data);
        Self {
            base,
            entity_type,
            projectile: ProjectileBase::new(&Self::PROJECTILE),
            thrown_item,
            entity_data: CheckedMutex::new(entity_data),
        }
    }

    /// Applies one of the potion's effects to `living`, scaled by distance.
    fn apply_effect(
        entity: &SharedEntity,
        world: &World,
        living: &Living<'_>,
        effect: &RegistryMobEffectInstance,
        scale: f64,
        duration_scale: f32,
        owner: Option<&SharedEntity>,
    ) {
        let behavior = MOB_EFFECT_BEHAVIORS.get_behavior(effect.effect());
        if let Some(instantaneous) = behavior.as_instantaneous() {
            instantaneous.apply_instantaneous(
                world,
                living,
                effect.amplifier(),
                Some(entity),
                owner,
                scale,
            );
            return;
        }

        let duration = effect.map_duration(|base_duration| {
            (scale * f64::from(base_duration) * f64::from(duration_scale) + 0.5) as i32
        });
        let scaled_effect = to_runtime_instance_icon_from_visibility(effect, duration);
        if scaled_effect.ends_within(MIN_REMAINING_TICKS) {
            return;
        }
        let effect_source: &dyn Entity = owner.map_or(entity.as_ref(), AsRef::as_ref);
        living.add_mob_effect_with_source(scaled_effect, Some(effect_source));
    }
}

impl thrown_potion::Parts for SplashPotionEntity {
    /// `ThrownSplashPotion.onHitAsPotion`: applies the effects to living
    /// entities in reach, weaker further from the impact.
    fn on_hit_as_potion(
        &self,
        entity: &SharedEntity,
        world: &Arc<World>,
        potion_item: &ItemStack,
        hit: &ProjectileHit,
    ) {
        let contents = potion_item
            .get_or_default(vanilla_components::POTION_CONTENTS, PotionContents::empty());
        let duration_scale =
            potion_item.get_or_default(vanilla_components::POTION_DURATION_SCALE, 1.0);
        let mob_effects = contents.all_effects();

        let potion_aabb = self
            .bounding_box()
            .translate(hit.location() - self.position());
        let effect_aabb = potion_aabb.inflate_xyz(
            SPLASH_HORIZONTAL_REACH,
            SPLASH_VERTICAL_REACH,
            SPLASH_HORIZONTAL_REACH,
        );
        let targets =
            world.get_entities_in_aabb_matching(&effect_aabb, |target| !target.is_spectator());
        if targets.is_empty() {
            return;
        }

        let margin = compute_margin(self.tick_count());
        let owner = self.projectile().owner();
        for target in targets {
            let Some(living) = target.as_living_entity() else {
                continue;
            };
            if !living.is_affected_by_potions() {
                continue;
            }
            let dist = potion_aabb.distance_to_sqr_aabb(target.bounding_box().inflate(margin));
            if dist >= SPLASH_RANGE_SQ {
                continue;
            }
            let scale = 1.0 - dist.sqrt() / 4.0;
            for effect in &mob_effects {
                Self::apply_effect(
                    entity,
                    world,
                    &living,
                    effect,
                    scale,
                    duration_scale,
                    owner.as_ref(),
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use steel_registry::{init_vanilla_registry, vanilla_entities};
    use steel_utils::{BlockPos, Direction};

    use super::*;
    use crate::behavior::init_behaviors;
    use crate::test_support::test_world;
    use crate::world::ClipHitResult;

    #[test]
    fn hitting_a_block_discards_the_splash_potion() {
        init_vanilla_registry();
        init_behaviors();

        let potion = Arc::new(SplashPotionEntity::new(
            &vanilla_entities::SPLASH_POTION,
            1,
            DVec3::ZERO,
            Arc::downgrade(test_world()),
        ));
        let entity: SharedEntity = Arc::clone(&potion) as SharedEntity;
        let hit = ProjectileHit::Block {
            location: DVec3::ZERO,
            hit: ClipHitResult {
                location: DVec3::ZERO,
                direction: Direction::Up,
                block_pos: BlockPos::new(0, 0, 0),
                miss: false,
                inside: false,
                world_border_hit: false,
            },
        };

        potion.projectile().on_hit(&entity, &hit);
        assert!(potion.is_removed());
    }
}
