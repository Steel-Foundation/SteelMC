//! Vanilla `ThrownEgg`.

use std::sync::{Arc, Weak};

use glam::DVec3;
use steel_macros::entity_behavior;
use steel_registry::data_components::vanilla_components::CHICKEN_VARIANT;
use steel_registry::entity_type::{EntityDimensions, EntityTypeRef};
use steel_registry::vanilla_entity_data::EggEntityData;
use steel_registry::{vanilla_entities, vanilla_items};
use steel_utils::entity_events::EntityStatus;
use steel_utils::locks::CheckedMutex;
use steel_utils::{DowncastType, DowncastTypeKey};

use crate::entity::ageable::BABY_START_AGE;
use crate::entity::animal::Parts as _;
use crate::entity::entities::ChickenEntity;
use crate::entity::prelude::*;
use crate::entity::projectile::throwable;
use crate::entity::projectile::throwable_item::{Parts as _, ThrownItem};
use crate::entity::projectile::{Parts as _, ProjectileBase, ProjectileHit, ProjectileProps};
use crate::entity::variant::Variants as _;
use crate::entity::{
    Entity, EntityBase, EntityBaseLoad, EntityProps, RemovalReason, SharedEntity, next_entity_id,
};
use crate::world::World;

/// The size a hatched chick grows from when it is fitted into the world.
const ZERO_SIZED_DIMENSIONS: EntityDimensions = EntityDimensions::new(0.0, 0.0, 0.0);

/// A thrown egg, which may hatch chicks where it lands.
#[entity_behavior(class = "ThrownEgg")]
#[derive(Entity)]
#[entity(hooks = Self::ENTITY)]
pub struct ThrownEggEntity {
    #[entity(base)]
    base: EntityBase,
    #[entity(entity_type)]
    entity_type: EntityTypeRef,
    #[entity(component)]
    projectile: ProjectileBase,
    #[entity(component)]
    thrown_item: ThrownItem,
    #[entity(synced)]
    entity_data: CheckedMutex<EggEntityData>,
}

// SAFETY: This key is owned by Steel and uniquely identifies `ThrownEggEntity`.
unsafe impl DowncastType for ThrownEggEntity {
    const TYPE_KEY: DowncastTypeKey = DowncastTypeKey::new("steel:entity/thrown_egg");
}

impl ThrownEggEntity {
    const ENTITY: EntityProps<Self> = throwable::entity_props();
    const PROJECTILE: ProjectileProps<Self> = ProjectileProps {
        on_hit: Self::projectile_on_hit,
        on_hit_entity: |egg, entity, target, _| {
            if let Some(world) = target.level() {
                target.hurt(&world, &egg.projectile().thrown_damage(entity), 0.0);
            }
        },
        ..ProjectileProps::new()
    };

    /// Creates an egg with no owner.
    #[must_use]
    pub fn new(entity_type: EntityTypeRef, id: i32, position: DVec3, world: Weak<World>) -> Self {
        Self::from_base(
            EntityBase::new(id, position, entity_type.dimensions, world),
            entity_type,
        )
    }

    /// Reconstructs an egg from saved base data.
    #[must_use]
    pub fn from_saved(entity_type: EntityTypeRef, load: EntityBaseLoad) -> Self {
        Self::from_base(
            EntityBase::from_load(load, entity_type.dimensions),
            entity_type,
        )
    }

    fn from_base(base: EntityBase, entity_type: EntityTypeRef) -> Self {
        let mut entity_data = EggEntityData::new();
        let thrown_item = ThrownItem::new(&vanilla_items::EGG, &mut entity_data);
        Self {
            base,
            entity_type,
            projectile: ProjectileBase::new(&Self::PROJECTILE),
            thrown_item,
            entity_data: CheckedMutex::new(entity_data),
        }
    }

    /// `ThrownEgg.onHit`: one in eight eggs hatch a chick, one in 32 of those
    /// hatch four; then the egg breaks.
    fn projectile_on_hit(&self, entity: &SharedEntity, hit: &ProjectileHit) {
        self.projectile().on_hit_default(entity, hit);
        let Some(world) = self.level() else {
            return;
        };
        if rand::random_range(0..8) == 0 {
            let count = if rand::random_range(0..32) == 0 { 4 } else { 1 };
            for _ in 0..count {
                if !self.hatch_chick(&world) {
                    break;
                }
            }
        }
        // VANILLA CLIENT-LOCAL: entity event 3 shows the break particles.
        self.broadcast_entity_event(EntityStatus::Death);
        self.set_removed(RemovalReason::Discarded);
    }

    /// Hatches a chick where the egg is, with the egg's chicken variant.
    /// Returns `false` when the chick does not fit, which stops the hatching.
    fn hatch_chick(&self, world: &Arc<World>) -> bool {
        let chicken = Arc::new(ChickenEntity::new(
            &vanilla_entities::CHICKEN,
            next_entity_id(),
            self.position(),
            Arc::downgrade(world),
        ));
        chicken.animal().ageable().set_age(BABY_START_AGE);
        chicken.set_rotation((self.rotation().0, 0.0));
        if let Some(variant) = self.throwable_item().item().get(CHICKEN_VARIANT) {
            chicken.set_variant(variant.value());
        }
        if !chicken.fudge_position_after_size_change(ZERO_SIZED_DIMENSIONS) {
            return false;
        }
        if let Err(error) = world.try_add_entity(chicken) {
            // Vanilla `addFreshEntity` drops the chick silently.
            log::debug!("failed to add hatched chick: {error}");
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use steel_registry::item_stack::ItemStack;
    use steel_registry::{
        RegistryReference, init_vanilla_registry, vanilla_blocks, vanilla_chicken_variants,
    };
    use steel_utils::types::UpdateFlags;
    use steel_utils::{BlockPos, ChunkPos, Downcast as _, WorldAabb};

    use super::*;
    use crate::behavior::init_behaviors;
    use crate::test_support::{fresh_test_world, insert_ready_full_chunk};

    fn egg(world: &Arc<World>, pos: DVec3) -> ThrownEggEntity {
        ThrownEggEntity::new(
            &vanilla_entities::EGG,
            next_entity_id(),
            pos,
            Arc::downgrade(world),
        )
    }

    fn chicks_near(world: &World, pos: DVec3) -> Vec<SharedEntity> {
        world
            .get_entities_in_aabb(&WorldAabb::of_size(pos, 1.0, 2.0, 1.0))
            .into_iter()
            .filter(|entity| entity.entity_type() == &vanilla_entities::CHICKEN)
            .collect()
    }

    #[test]
    fn hatched_chick_is_a_baby_with_the_egg_variant() {
        init_vanilla_registry();
        let world_fixture = fresh_test_world("thrown_egg_hatchling");
        let world = &world_fixture.world;
        let pos = DVec3::new(0.5, 80.0, 0.5);
        insert_ready_full_chunk(world, ChunkPos::from_entity_pos(pos));
        let egg = egg(world, pos);
        let mut stack = ItemStack::new(&vanilla_items::EGG);
        stack.set(
            CHICKEN_VARIANT,
            RegistryReference::new(&vanilla_chicken_variants::COLD),
        );
        egg.throwable_item().set_item(&stack);

        assert!(egg.hatch_chick(world));

        let chicks = chicks_near(world, pos);
        let chick = chicks
            .first()
            .and_then(|chick| chick.downcast_ref::<ChickenEntity>())
            .expect("hatched chick should be in the world");
        assert!(chick.animal().ageable().is_baby());
        assert_eq!(chick.variant().key, vanilla_chicken_variants::COLD.key);
    }

    #[test]
    fn chick_enclosed_in_a_block_does_not_hatch() {
        init_vanilla_registry();
        init_behaviors();
        let world_fixture = fresh_test_world("thrown_egg_enclosed");
        let world = &world_fixture.world;
        insert_ready_full_chunk(world, ChunkPos::new(0, 0));
        assert!(world.set_block(
            BlockPos::new(0, 80, 0),
            vanilla_blocks::STONE.default_state(),
            UpdateFlags::UPDATE_NONE,
        ));

        let inside = DVec3::new(0.5, 80.5, 0.5);
        assert!(!egg(world, inside).hatch_chick(world));
        assert!(chicks_near(world, inside).is_empty());

        let on_top = DVec3::new(0.5, 81.0, 0.5);
        assert!(egg(world, on_top).hatch_chick(world));
        assert!(!chicks_near(world, on_top).is_empty());
    }
}
