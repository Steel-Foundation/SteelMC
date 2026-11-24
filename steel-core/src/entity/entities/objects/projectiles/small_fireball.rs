//! Blaze fireball projectile entity (`SmallFireball`).
//!
//! Deals 5 fire damage and ignites its target, places fire on block impact
//! (respecting `mobGriefing`), then discards itself.

use std::sync::Arc;
use std::sync::Weak;

use glam::DVec3;
use simdnbt::borrow::NbtCompound as BorrowedNbtCompoundView;
use simdnbt::owned::NbtCompound;
use steel_macros::entity_behavior;
use steel_registry::blocks::block_state_ext::BlockStateExt as _;
use steel_registry::entity_type::EntityTypeRef;
use steel_registry::item_stack::ItemStack;
use steel_registry::vanilla_damage_types;
use steel_registry::vanilla_entity_data::SmallFireballEntityData;
use steel_registry::vanilla_game_rules::MOB_GRIEFING;
use steel_utils::locks::SyncMutex;
use steel_utils::types::UpdateFlags;
use steel_utils::{DowncastType, DowncastTypeKey, TICKS_PER_SECOND};

use crate::behavior::blocks::FireBlock;
use crate::enchantment_helper::{self, EnchantmentPostAttackContext};
use crate::entity::damage::DamageSource;
use crate::entity::entity::EntityEventSource as _;
use crate::entity::{
    AbstractHurtingProjectile, Entity, EntityBase, EntityBaseLoad, EntitySyncedData, Fireball,
    HurtingProjectileBase, Projectile, ProjectileBase, ProjectileHit, RemovalReason, SharedEntity,
};
use crate::world::{ClipHitResult, World};

const FIREBALL_DAMAGE: f32 = 5.0;
const FIREBALL_IGNITE_TICKS: i32 = 5 * TICKS_PER_SECOND;

/// A small fireball fired by a blaze.
#[entity_behavior(class = "SmallFireball")]
pub struct SmallFireballEntity {
    /// Common entity fields (id, uuid, position, etc.).
    base: EntityBase,
    /// Vanilla entity type registered for this implementation.
    entity_type: EntityTypeRef,
    /// Synced data carrying the rendered item stack.
    entity_data: SyncMutex<SmallFireballEntityData>,
    /// Shared `Projectile` state (owner / left-owner / has-been-shot).
    projectile_base: ProjectileBase,
    /// Shared `AbstractHurtingProjectile` state (acceleration power).
    hurting_projectile_base: HurtingProjectileBase,
}

// SAFETY: This key is owned by Steel and uniquely identifies `SmallFireballEntity`.
unsafe impl DowncastType for SmallFireballEntity {
    const TYPE_KEY: DowncastTypeKey = DowncastTypeKey::new("steel:entity/small_fireball");
}

impl SmallFireballEntity {
    /// Creates a new small fireball with no owner.
    #[must_use]
    pub fn new(entity_type: EntityTypeRef, id: i32, position: DVec3, world: Weak<World>) -> Self {
        Self::with_base(
            entity_type,
            EntityBase::new(id, position, entity_type.dimensions, world),
        )
    }

    /// Creates a small fireball from saved base data.
    #[must_use]
    pub fn from_saved(entity_type: EntityTypeRef, load: EntityBaseLoad) -> Self {
        Self::with_base(
            entity_type,
            EntityBase::from_load(load, entity_type.dimensions),
        )
    }

    fn with_base(entity_type: EntityTypeRef, base: EntityBase) -> Self {
        Self {
            base,
            entity_type,
            entity_data: SyncMutex::new(SmallFireballEntityData::new()),
            projectile_base: ProjectileBase::new(),
            hurting_projectile_base: HurtingProjectileBase::default(),
        }
    }
}

impl Entity for SmallFireballEntity {
    fn base(&self) -> &EntityBase {
        &self.base
    }

    fn entity_type(&self) -> EntityTypeRef {
        self.entity_type
    }

    fn tick(self: Arc<Self>) {
        self.hurting_projectile_tick();
    }

    fn spawn_data(&self) -> i32 {
        self.get_owner().map_or(0, |owner| owner.id())
    }

    fn restore_owner_reference(&self, owner: &SharedEntity) {
        self.cache_owner_entity(owner);
    }

    fn projectile_owner_uuid(&self) -> Option<uuid::Uuid> {
        self.owner_uuid()
    }

    fn projectile_owner(&self) -> Option<SharedEntity> {
        self.get_owner()
    }

    fn synced_data(&self) -> Option<&dyn EntitySyncedData> {
        Some(&self.entity_data)
    }

    fn play_entity_on_fire_extinguished_sound(&self) {}

    fn save_additional(&self, nbt: &mut NbtCompound) {
        self.save_fireball(nbt);
    }

    fn load_additional(&self, nbt: BorrowedNbtCompoundView<'_, '_>) {
        self.load_fireball(nbt);
    }
}

impl Projectile for SmallFireballEntity {
    fn projectile_base(&self) -> &ProjectileBase {
        &self.projectile_base
    }

    fn can_hit_entity(&self, entity: &dyn Entity) -> bool {
        entity.is_alive() && self.hurting_can_hit_entity(entity)
    }

    fn on_deflection(&self, by_attack: bool) {
        self.hurting_on_deflection(by_attack);
    }

    fn on_hit(self: Arc<Self>, hit: &ProjectileHit) {
        Arc::clone(&self).projectile_on_hit(hit);
        self.set_removed(RemovalReason::Discarded);
    }

    fn on_hit_entity(self: Arc<Self>, entity: &SharedEntity, _location: DVec3) {
        let Some(world) = entity.level() else {
            return;
        };

        let remaining_fire_ticks = entity.remaining_fire_ticks();
        entity.ignite_for_ticks(FIREBALL_IGNITE_TICKS);

        let fireball = Arc::clone(&self).into_shared_entity();
        let source = if let Some(owner) = self.get_owner() {
            DamageSource::environment(&vanilla_damage_types::FIREBALL)
                .with_direct_entity(fireball)
                .with_causing_entity(owner)
        } else {
            DamageSource::direct(&vanilla_damage_types::UNATTRIBUTED_FIREBALL, fireball)
        };

        if entity.hurt(&world, &source, FIREBALL_DAMAGE) {
            let context = EnchantmentPostAttackContext::new(entity, &source);
            enchantment_helper::do_post_attack_effects(&world, entity.as_ref(), &context);
        } else {
            // Vanilla restores the fire ticks it set before the failed hurt.
            entity.set_remaining_fire_ticks(remaining_fire_ticks);
        }
    }

    fn on_hit_block(self: Arc<Self>, hit: &ClipHitResult) {
        self.projectile_on_hit_block(hit);
        let Some(world) = self.level() else {
            return;
        };

        // Vanilla only lets mob-owned fireballs place fire when `mobGriefing` is on.
        let owner = self.get_owner();
        let may_place = owner
            .as_deref()
            .is_none_or(|owner| owner.as_mob().is_none() || world.get_game_rule(&MOB_GRIEFING));
        if !may_place {
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

    fn hurt(&self, _world: &World, _source: &DamageSource, _amount: f32) -> bool {
        false
    }
}

impl AbstractHurtingProjectile for SmallFireballEntity {
    fn hurting_projectile_base(&self) -> &HurtingProjectileBase {
        &self.hurting_projectile_base
    }
}

impl Fireball for SmallFireballEntity {
    fn get_item(&self) -> ItemStack {
        self.entity_data.lock().fireball().item_stack.get().clone()
    }

    fn set_item(&self, item: ItemStack) {
        self.entity_data.lock().fireball_mut().item_stack.set(item);
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use glam::DVec3;
    use steel_registry::{init_vanilla_registry, vanilla_entities};
    use steel_utils::ChunkPos;

    use super::*;
    use crate::entity::entities::{BlazeEntity, PigEntity};
    use crate::entity::projectile::EntityHitResult;
    use crate::entity::{
        Entity, INITIAL_ACCELERATION_POWER, LivingEntity, Projectile, ProjectileHit, SharedEntity,
        next_entity_id,
    };
    use crate::test_support::{fresh_test_world, insert_ready_full_chunk};

    #[test]
    fn hit_entity_ignites_for_five_seconds_and_deals_five_damage() {
        init_vanilla_registry();
        let fixture = fresh_test_world("small_fireball_hit_entity");
        let world = &fixture.world;
        insert_ready_full_chunk(world, ChunkPos::new(0, 0));

        let blaze = Arc::new(BlazeEntity::new(
            &vanilla_entities::BLAZE,
            next_entity_id(),
            DVec3::new(4.0, 65.0, 8.0),
            Arc::downgrade(world),
        ));
        let pig = Arc::new(PigEntity::new(
            &vanilla_entities::PIG,
            next_entity_id(),
            DVec3::new(8.0, 65.0, 8.0),
            Arc::downgrade(world),
        ));
        let owner: SharedEntity = Arc::<BlazeEntity>::clone(&blaze);
        let target: SharedEntity = Arc::<PigEntity>::clone(&pig);
        world
            .try_add_entity(Arc::clone(&owner))
            .expect("owner should attach to the loaded chunk");
        world
            .try_add_entity(Arc::clone(&target))
            .expect("target should attach to the loaded chunk");

        let fireball = Arc::new(SmallFireballEntity::new(
            &vanilla_entities::SMALL_FIREBALL,
            next_entity_id(),
            DVec3::new(7.0, 65.5, 8.0),
            Arc::downgrade(world),
        ));
        fireball.set_owner_entity(Some(&owner));

        let hit = ProjectileHit::Entity(EntityHitResult {
            entity: target,
            location: DVec3::new(7.5, 65.5, 8.0),
        });
        Arc::clone(&fireball).on_hit(&hit);

        assert_eq!(pig.get_health(), 5.0);
        assert_eq!(pig.remaining_fire_ticks(), FIREBALL_IGNITE_TICKS);
        assert!(Entity::is_removed(fireball.as_ref()));
        let source = pig
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
        let dying = Arc::new(PigEntity::new(
            &vanilla_entities::PIG,
            next_entity_id(),
            DVec3::new(7.75, 65.0, 8.0),
            Arc::downgrade(world),
        ));
        let target = Arc::new(PigEntity::new(
            &vanilla_entities::PIG,
            next_entity_id(),
            DVec3::new(8.9, 65.0, 8.0),
            Arc::downgrade(world),
        ));
        dying.set_health(0.0);
        world
            .try_add_entity(Arc::<PigEntity>::clone(&dying))
            .expect("dying mob stays in the world");
        world
            .try_add_entity(Arc::<PigEntity>::clone(&target))
            .expect("target should attach to the loaded chunk");

        let fireball = Arc::new(SmallFireballEntity::new(
            &vanilla_entities::SMALL_FIREBALL,
            next_entity_id(),
            DVec3::new(5.0, 65.5, 8.0),
            Arc::downgrade(world),
        ));
        fireball.set_velocity(DVec3::X * INITIAL_ACCELERATION_POWER);
        for _ in 0..12 {
            if fireball.is_removed() {
                break;
            }
            Arc::clone(&fireball).tick();
        }

        assert!(!Entity::is_removed(dying.as_ref()));
        assert!(fireball.is_removed());
        assert_eq!(target.get_health(), 5.0);
        let source = target
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
