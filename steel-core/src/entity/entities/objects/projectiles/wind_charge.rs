//! Player-thrown wind charge projectile entity (`WindCharge`).
//!
//! Flies straight until it hits a block or entity (or climbs 30 blocks above
//! the build limit), then bursts into a small wind explosion that knocks
//! entities back and triggers blocks without breaking them.

use std::sync::Arc;
use std::sync::Weak;

use glam::DVec3;
use simdnbt::borrow::NbtCompound as BorrowedNbtCompoundView;
use simdnbt::owned::NbtCompound;
use steel_macros::entity_behavior;
use steel_protocol::packets::game::ExplosionParticlePalette;
use steel_registry::entity_type::EntityTypeRef;
use steel_registry::particle_type::ParticleData;
use steel_registry::sound_event::SoundEventHolder;
use steel_registry::vanilla_block_tags::BlockTag;
use steel_registry::vanilla_entity_type_tags::EntityTypeTag;
use steel_registry::{REGISTRY, TaggedRegistryExt as _, sound_events, vanilla_particle_types};
use steel_utils::locks::SyncMutex;
use steel_utils::{DowncastType, DowncastTypeKey, WorldAabb};

use crate::entity::damage::DamageSource;
use crate::entity::{
    AbstractHurtingProjectile, AbstractWindCharge, Entity, EntityBase, EntityBaseLoad,
    EntityMoveError, EntityReference, HurtingProjectileBase, Projectile, ProjectileBase,
    ProjectileDeflection, ProjectileHit, SharedEntity,
};
use crate::world::{
    ClipHitResult, ExplosionInteraction, ExplosionOptions, SimpleExplosionDamageCalculator, World,
};

const EXPLOSION_RADIUS: f32 = 1.2;
const EXPLOSION_KNOCKBACK_MULTIPLIER: f32 = 1.22;
/// Ticks after spawning during which the charge cannot be deflected.
const NO_DEFLECT_TICKS: i32 = 5;
const INERTIA: f32 = 1.0;

static EXPLOSION_DAMAGE_CALCULATOR: SimpleExplosionDamageCalculator =
    SimpleExplosionDamageCalculator::new(
        true,
        false,
        Some(EXPLOSION_KNOCKBACK_MULTIPLIER),
        Some(BlockTag::BLOCKS_WIND_CHARGE_EXPLOSIONS),
    );

/// A wind charge thrown by a player or shot from a dispenser.
#[entity_behavior(class = "WindCharge")]
pub struct WindChargeEntity {
    /// Common entity fields (id, uuid, position, etc.).
    base: EntityBase,
    /// Vanilla entity type registered for this implementation.
    entity_type: EntityTypeRef,
    /// Shared `Projectile` state (owner / left-owner / has-been-shot).
    projectile_base: ProjectileBase,
    /// Shared `AbstractHurtingProjectile` state (acceleration power).
    hurting_projectile_base: HurtingProjectileBase,
    /// Remaining ticks during which deflection is refused.
    no_deflect_ticks: SyncMutex<i32>,
}

// SAFETY: This key is owned by Steel and uniquely identifies `WindChargeEntity`.
unsafe impl DowncastType for WindChargeEntity {
    const TYPE_KEY: DowncastTypeKey = DowncastTypeKey::new("steel:entity/wind_charge");
}

impl WindChargeEntity {
    /// Creates a new wind charge with no owner.
    #[must_use]
    pub fn new(entity_type: EntityTypeRef, id: i32, position: DVec3, world: Weak<World>) -> Self {
        Self::with_base(
            entity_type,
            EntityBase::new(id, position, entity_type.dimensions, world),
        )
    }

    /// Creates a wind charge from saved base data.
    #[must_use]
    pub fn from_saved(entity_type: EntityTypeRef, load: EntityBaseLoad) -> Self {
        Self::with_base(
            entity_type,
            EntityBase::from_load(load, entity_type.dimensions),
        )
    }

    fn with_base(entity_type: EntityTypeRef, base: EntityBase) -> Self {
        let entity = Self {
            base,
            entity_type,
            projectile_base: ProjectileBase::new(),
            hurting_projectile_base: HurtingProjectileBase::new(0.0),
            no_deflect_ticks: SyncMutex::new(NO_DEFLECT_TICKS),
        };
        entity
            .base
            .set_bounding_box(entity.wind_charge_bounding_box(entity.position()));
        entity
    }
}

impl Entity for WindChargeEntity {
    fn base(&self) -> &EntityBase {
        &self.base
    }

    fn entity_type(&self) -> EntityTypeRef {
        self.entity_type
    }

    fn tick(self: Arc<Self>) {
        Arc::clone(&self).wind_charge_tick();
        let mut no_deflect_ticks = self.no_deflect_ticks.lock();
        if *no_deflect_ticks > 0 {
            *no_deflect_ticks -= 1;
        }
    }

    fn is_pickable(&self) -> bool {
        REGISTRY
            .entity_types
            .is_in_tag(self.entity_type, &EntityTypeTag::REDIRECTABLE_PROJECTILE)
    }

    fn can_collide_with(&self, other: &dyn Entity) -> bool {
        self.wind_charge_can_collide_with(other)
    }

    /// Wind charges ignore pushes, including explosion knockback.
    fn push_impulse(&self, _impulse: DVec3) {}

    fn make_bounding_box_at(&self, position: DVec3) -> WorldAabb {
        self.wind_charge_bounding_box(position)
    }

    fn try_set_position(&self, pos: DVec3) -> Result<(), EntityMoveError> {
        self.try_set_position_base(pos)?;
        self.base
            .set_bounding_box(self.wind_charge_bounding_box(pos));
        Ok(())
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

    fn save_additional(&self, nbt: &mut NbtCompound) {
        self.save_hurting_projectile(nbt);
    }

    fn load_additional(&self, nbt: BorrowedNbtCompoundView<'_, '_>) {
        self.load_hurting_projectile(nbt);
    }
}

impl Projectile for WindChargeEntity {
    fn projectile_base(&self) -> &ProjectileBase {
        &self.projectile_base
    }

    fn can_hit_entity(&self, entity: &dyn Entity) -> bool {
        self.wind_charge_can_hit_entity(entity)
    }

    fn deflect(
        &self,
        deflection: ProjectileDeflection,
        deflecting_entity: Option<&dyn Entity>,
        new_owner: Option<EntityReference>,
        by_attack: bool,
    ) -> bool {
        if *self.no_deflect_ticks.lock() > 0 {
            return false;
        }
        self.base_deflect(deflection, deflecting_entity, new_owner, by_attack)
    }

    fn on_deflection(&self, by_attack: bool) {
        self.hurting_on_deflection(by_attack);
    }

    fn on_hit(self: Arc<Self>, hit: &ProjectileHit) {
        self.wind_charge_on_hit(hit);
    }

    fn on_hit_entity(self: Arc<Self>, entity: &SharedEntity, _location: DVec3) {
        self.wind_charge_on_hit_entity(entity);
    }

    fn on_hit_block(self: Arc<Self>, hit: &ClipHitResult) {
        self.wind_charge_on_hit_block(hit);
    }

    fn hurt(&self, _world: &World, _source: &DamageSource, _amount: f32) -> bool {
        false
    }
}

impl AbstractHurtingProjectile for WindChargeEntity {
    fn hurting_projectile_base(&self) -> &HurtingProjectileBase {
        &self.hurting_projectile_base
    }

    fn should_burn(&self) -> bool {
        false
    }

    fn get_inertia(&self) -> f32 {
        INERTIA
    }

    fn get_liquid_inertia(&self) -> f32 {
        INERTIA
    }
}

impl AbstractWindCharge for WindChargeEntity {
    fn explode(self: Arc<Self>, position: DVec3) {
        let Some(world) = self.level() else {
            return;
        };
        let source: SharedEntity = self;
        let mut options =
            ExplosionOptions::new(position, EXPLOSION_RADIUS, ExplosionInteraction::Trigger);
        options.source = Some(source);
        options.damage_calculator = Some(&EXPLOSION_DAMAGE_CALCULATOR);
        options.immutable_block_calculator = Some(&EXPLOSION_DAMAGE_CALCULATOR);
        options.small_explosion_particle =
            ParticleData::simple(&vanilla_particle_types::GUST_EMITTER_SMALL);
        options.large_explosion_particle =
            ParticleData::simple(&vanilla_particle_types::GUST_EMITTER_LARGE);
        options.block_particles = ExplosionParticlePalette::empty();
        options.explosion_sound =
            SoundEventHolder::registry(&sound_events::ENTITY_WIND_CHARGE_WIND_BURST);
        world.explode(options);
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Weak};

    use glam::DVec3;
    use steel_registry::{init_vanilla_registry, vanilla_blocks, vanilla_entities};
    use steel_utils::types::UpdateFlags;
    use steel_utils::{BlockPos, ChunkPos};

    use super::*;
    use crate::behavior::init_behaviors;
    use crate::entity::entities::PigEntity;
    use crate::entity::{INITIAL_ACCELERATION_POWER, LivingEntity as _, next_entity_id};
    use crate::test_support::{fresh_test_world, insert_ready_full_chunk};

    #[test]
    fn deflection_is_refused_while_fresh_and_attacks_restore_acceleration() {
        init_vanilla_registry();

        let charge = WindChargeEntity::new(
            &vanilla_entities::WIND_CHARGE,
            1,
            DVec3::ZERO,
            Weak::<World>::new(),
        );
        charge.set_velocity(DVec3::X);

        assert!(!charge.deflect(ProjectileDeflection::Reverse, None, None, false));
        assert_eq!(charge.velocity(), DVec3::X);

        *charge.no_deflect_ticks.lock() = 0;
        assert!(charge.deflect(ProjectileDeflection::None, None, None, true));
        assert_eq!(charge.acceleration_power(), INITIAL_ACCELERATION_POWER);
        assert!(charge.deflect(ProjectileDeflection::None, None, None, false));
        assert_eq!(
            charge.acceleration_power(),
            INITIAL_ACCELERATION_POWER * 0.5
        );
    }

    #[test]
    fn block_hit_bursts_and_knocks_back_nearby_entities_without_damage() {
        init_vanilla_registry();
        init_behaviors();

        let fixture = fresh_test_world("wind_charge_block_hit");
        let world = &fixture.world;
        insert_ready_full_chunk(world, ChunkPos::new(0, 0));
        assert!(world.set_block(
            BlockPos::new(8, 63, 8),
            vanilla_blocks::STONE.default_state(),
            UpdateFlags::UPDATE_NONE,
        ));

        let pig = Arc::new(PigEntity::new(
            &vanilla_entities::PIG,
            next_entity_id(),
            DVec3::new(9.25, 64.0, 8.5),
            Arc::downgrade(world),
        ));
        world
            .try_add_entity(Arc::clone(&pig) as SharedEntity)
            .expect("pig should attach to the loaded chunk");
        let health = pig.get_health();

        let charge = Arc::new(WindChargeEntity::new(
            &vanilla_entities::WIND_CHARGE,
            next_entity_id(),
            DVec3::new(8.5, 65.0, 8.5),
            Arc::downgrade(world),
        ));
        charge.set_velocity(DVec3::new(0.0, -1.5, 0.0));
        world
            .try_add_entity(Arc::clone(&charge) as SharedEntity)
            .expect("wind charge should attach to the loaded chunk");

        Arc::clone(&charge).tick();

        assert!(charge.is_removed());
        assert!(
            pig.velocity().x > 0.0,
            "the burst should push the pig away from the impact point"
        );
        assert_eq!(pig.get_health(), health);
    }
}
