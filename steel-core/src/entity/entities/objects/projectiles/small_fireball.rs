//! Blaze fireball projectile entity (`SmallFireball`).
//!
//! Deals 5 fire damage and ignites its target, places fire on block impact
//! (respecting `mobGriefing`), then discards itself.

use glam::DVec3;
use steel_macros::entity_behavior;
use steel_registry::blocks::block_state_ext::BlockStateExt as _;
use steel_registry::entity_type::EntityTypeRef;
use steel_registry::vanilla_damage_types;
use steel_registry::vanilla_entity_data::SmallFireballEntityData;
use steel_registry::vanilla_game_rules::MOB_GRIEFING;
use steel_utils::TICKS_PER_SECOND;
use steel_utils::locks::SyncMutex;
use steel_utils::types::UpdateFlags;

use crate::behavior::blocks::FireBlock;
use crate::enchantment_helper::{self, EnchantmentPostAttackContext};
use crate::entity::damage::DamageSource;
use crate::entity::prelude::*;
use crate::entity::projectile::fireball::{self, FireballItem};
use crate::entity::projectile::{self, ProjectileTable, hurting};
use crate::entity::{
    EntityBase, EntityProps, EntityTable, HurtingProjectileBase, ProjectileBase, ProjectileHit,
    ProjectileProps, RemovalReason, SharedEntity,
};
use crate::world::ClipHitResult;

const FIREBALL_DAMAGE: f32 = 5.0;
const FIREBALL_IGNITE_TICKS: i32 = 5 * TICKS_PER_SECOND;

/// A small fireball fired by a blaze.
#[entity_behavior(class = "SmallFireball")]
#[derive(Entity)]
#[entity(key = "steel:entity/small_fireball", constructors)]
pub struct SmallFireballEntity {
    #[entity(base)]
    base: EntityBase,
    #[entity(entity_type)]
    entity_type: EntityTypeRef,
    #[entity(component)]
    projectile: ProjectileBase,
    #[entity(component)]
    hurting_projectile: HurtingProjectileBase,
    #[entity(component)]
    fireball_item: FireballItem,
    #[entity(synced)]
    entity_data: SyncMutex<SmallFireballEntityData>,
}

impl SmallFireballEntity {
    fn from_base(base: EntityBase, entity_type: EntityTypeRef) -> Self {
        Self {
            base,
            entity_type,
            projectile: ProjectileBase::new(),
            hurting_projectile: HurtingProjectileBase::default(),
            fireball_item: FireballItem,
            entity_data: SyncMutex::new(SmallFireballEntityData::new()),
        }
    }

    fn projectile_on_hit(&self, hit: &ProjectileHit) {
        projectile::on_hit(self, hit);
        self.set_removed(RemovalReason::Discarded);
    }

    fn projectile_on_hit_entity(&self, target: &SharedEntity, location: DVec3) {
        projectile::on_hit_entity(self, target, location);
        let Some(world) = self.level() else {
            return;
        };
        let Some(fireball) = self.shared() else {
            return;
        };

        let remaining_fire_ticks = target.remaining_fire_ticks();
        target.ignite_for_ticks(FIREBALL_IGNITE_TICKS);

        let source = match self.projectile().owner() {
            Some(owner) => DamageSource::environment(&vanilla_damage_types::FIREBALL)
                .with_direct_entity(fireball)
                .with_causing_entity(owner),
            None => DamageSource::direct(&vanilla_damage_types::UNATTRIBUTED_FIREBALL, fireball),
        };

        if target.hurt(&world, &source, FIREBALL_DAMAGE) {
            let context = EnchantmentPostAttackContext::new(target, &source);
            enchantment_helper::do_post_attack_effects(&world, target.as_ref(), &context);
        } else {
            target.set_remaining_fire_ticks(remaining_fire_ticks);
        }
    }

    fn projectile_on_hit_block(&self, hit: &ClipHitResult) {
        projectile::on_hit_block(self, hit);
        let Some(world) = self.level() else {
            return;
        };

        // Only mob-owned fireballs need `mobGriefing` to place fire.
        let mob_owned = self
            .projectile()
            .owner()
            .is_some_and(|owner| owner.as_mob().is_some());
        if mob_owned && !world.get_game_rule(&MOB_GRIEFING) {
            return;
        }

        let pos = hit.block_pos.relative(hit.direction);
        if !world.get_block_state(pos).is_air() {
            return;
        }
        world.set_block(
            pos,
            FireBlock::get_state(world.as_ref(), pos),
            UpdateFlags::UPDATE_ALL,
        );
    }
}

impl EntityTable for SmallFireballEntity {
    const ENTITY: EntityProps<Self> = fireball::entity_props();
}

impl ProjectileTable for SmallFireballEntity {
    const PROJECTILE: ProjectileProps<Self> = ProjectileProps {
        on_hit: Self::projectile_on_hit,
        on_hit_entity: Self::projectile_on_hit_entity,
        on_hit_block: Self::projectile_on_hit_block,
        ..hurting::projectile_props()
    };
}

impl hurting::Parts for SmallFireballEntity {}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use glam::DVec3;
    use steel_registry::{init_vanilla_registry, vanilla_entities};
    use steel_utils::ChunkPos;

    use super::*;
    use crate::entity::entities::{BlazeEntity, PigEntity};
    use crate::entity::projectile::EntityHitResult;
    use crate::entity::{INITIAL_ACCELERATION_POWER, next_entity_id};
    use crate::test_support::{fresh_test_world, insert_ready_full_chunk};

    #[test]
    fn hit_entity_ignites_for_five_seconds_and_deals_five_damage() {
        init_vanilla_registry();
        let fixture = fresh_test_world("small_fireball_hit_entity");
        let world = &fixture.world;
        insert_ready_full_chunk(world, ChunkPos::new(0, 0));

        let blaze = BlazeEntity::new(
            &vanilla_entities::BLAZE,
            next_entity_id(),
            DVec3::new(4.0, 65.0, 8.0),
            Arc::downgrade(world),
        );
        let pig = PigEntity::new(
            &vanilla_entities::PIG,
            next_entity_id(),
            DVec3::new(8.0, 65.0, 8.0),
            Arc::downgrade(world),
        );
        let owner: SharedEntity = Arc::<BlazeEntity>::clone(&blaze);
        let target: SharedEntity = Arc::<PigEntity>::clone(&pig);
        world
            .try_add_entity(Arc::clone(&owner))
            .expect("owner should attach to the loaded chunk");
        world
            .try_add_entity(Arc::clone(&target))
            .expect("target should attach to the loaded chunk");

        let fireball = SmallFireballEntity::new(
            &vanilla_entities::SMALL_FIREBALL,
            next_entity_id(),
            DVec3::new(7.0, 65.5, 8.0),
            Arc::downgrade(world),
        );
        fireball.projectile().set_owner(Some(&owner));

        let hit = ProjectileHit::Entity(EntityHitResult {
            entity: target,
            location: DVec3::new(7.5, 65.5, 8.0),
        });
        fireball.projectile().on_hit(&hit);

        assert_eq!(pig.living().health(), 5.0);
        assert_eq!(pig.remaining_fire_ticks(), FIREBALL_IGNITE_TICKS);
        assert!(fireball.is_removed());
        let source = pig
            .living()
            .last_damage_source()
            .expect("the pig was hit by the fireball");
        assert_eq!(source.damage_type.key, vanilla_damage_types::FIREBALL.key);
        assert_eq!(
            source.direct_entity().map(|direct| direct.uuid()),
            Some(fireball.uuid())
        );
        assert_eq!(
            source.causing_entity().map(|cause| cause.uuid()),
            Some(blaze.uuid())
        );
    }

    #[test]
    fn ownerless_fireball_flies_past_a_dying_mob_and_credits_itself() {
        init_vanilla_registry();
        let fixture = fresh_test_world("ownerless_fireball_flight");
        let world = &fixture.world;
        insert_ready_full_chunk(world, ChunkPos::new(0, 0));
        let dying = PigEntity::new(
            &vanilla_entities::PIG,
            next_entity_id(),
            DVec3::new(7.75, 65.0, 8.0),
            Arc::downgrade(world),
        );
        let target = PigEntity::new(
            &vanilla_entities::PIG,
            next_entity_id(),
            DVec3::new(8.9, 65.0, 8.0),
            Arc::downgrade(world),
        );
        dying.living().set_health(0.0);
        world
            .try_add_entity(Arc::<PigEntity>::clone(&dying))
            .expect("dying mob stays in the world");
        world
            .try_add_entity(Arc::<PigEntity>::clone(&target))
            .expect("target should attach to the loaded chunk");

        let fireball = SmallFireballEntity::new(
            &vanilla_entities::SMALL_FIREBALL,
            next_entity_id(),
            DVec3::new(5.0, 65.5, 8.0),
            Arc::downgrade(world),
        );
        fireball.set_velocity(DVec3::X * INITIAL_ACCELERATION_POWER);
        for _ in 0..12 {
            if fireball.is_removed() {
                break;
            }
            fireball.tick();
        }

        assert!(!dying.is_removed());
        assert!(fireball.is_removed());
        assert_eq!(target.living().health(), 5.0);
        let source = target
            .living()
            .last_damage_source()
            .expect("the target was hit by the fireball");
        assert_eq!(
            source.damage_type.key,
            vanilla_damage_types::UNATTRIBUTED_FIREBALL.key
        );
        assert!(source.is_direct());
        assert_eq!(
            source.causing_entity().map(|cause| cause.uuid()),
            Some(fireball.uuid())
        );
    }
}
