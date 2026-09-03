// Arrow projectile entity implementation
use glam::DVec3;
use simdnbt::borrow::NbtCompound as BorrowedNbtCompoundView;
use simdnbt::owned::NbtCompound;
use std::sync::Weak;
use steel_macros::entity_behavior;
use steel_registry::entity_type::{EntityDimensions, EntityTypeRef};
use steel_registry::vanilla_entity_data::ArrowEntityData;
use steel_registry::{sound_events, vanilla_damage_types};
use steel_utils::locks::SyncMutex;
use steel_utils::{DowncastType, DowncastTypeKey};

use crate::entity::damage::DamageSource;
use crate::entity::projectile::{Projectile, ProjectileBase, ProjectileHit};
use crate::entity::{Entity, EntityBase, EntityBaseLoad, EntityPose, RemovalReason, SharedEntity};
use crate::world::World;

/// Arrow projectile entity
#[entity_behavior(class = "Arrow")]
pub struct ArrowEntity {
    base: EntityBase,
    entity_type: EntityTypeRef,
    projectile_base: ProjectileBase,
    entity_data: SyncMutex<ArrowEntityData>,
    in_ground: SyncMutex<bool>,
    damage: SyncMutex<f64>,
    life_ticks: SyncMutex<i32>,
}

unsafe impl DowncastType for ArrowEntity {
    const TYPE_KEY: DowncastTypeKey = DowncastTypeKey::new("steel:entity/arrow");
}

impl ArrowEntity {
    /// Creates a new arrow entity.
    #[must_use]
    pub fn new(entity_type: EntityTypeRef, id: i32, position: DVec3, world: Weak<World>) -> Self {
        Self::new_with_base(
            EntityBase::new(id, position, entity_type.dimensions, world),
            entity_type,
        )
    }

    /// Creates an arrow entity from saved base data.
    #[must_use]
    pub fn from_saved(entity_type: EntityTypeRef, load: EntityBaseLoad) -> Self {
        Self::new_with_base(
            EntityBase::from_load(load, entity_type.dimensions),
            entity_type,
        )
    }

    /// Vanilla `AbstractArrow.baseDamage` default.
    pub const DEFAULT_DAMAGE: f64 = 2.0;
    /// Vanilla `AbstractArrow` in-air gravity.
    const GRAVITY: f64 = 0.05;
    /// Vanilla `AbstractArrow` in-air drag.
    const AIR_DRAG: f64 = 0.99;
    /// Vanilla `AbstractArrow` in-water drag.
    const WATER_DRAG: f64 = 0.6;

    fn new_with_base(base: EntityBase, entity_type: EntityTypeRef) -> Self {
        Self {
            base,
            entity_type,
            projectile_base: ProjectileBase::new(),
            entity_data: SyncMutex::new(ArrowEntityData::new()),
            in_ground: SyncMutex::new(false),
            damage: SyncMutex::new(Self::DEFAULT_DAMAGE),
            life_ticks: SyncMutex::new(0),
        }
    }

    /// Sets the arrow's base damage before flight-speed scaling.
    pub fn set_base_damage(&self, damage: f64) {
        *self.damage.lock() = damage;
    }

    fn apply_flight_physics(&self) {
        let drag = if self.is_in_water() {
            Self::WATER_DRAG
        } else {
            Self::AIR_DRAG
        };
        let mut vel = self.velocity() * drag;
        vel.y -= Self::GRAVITY;
        self.set_velocity(vel);
    }
}

impl Entity for ArrowEntity {
    fn base(&self) -> &EntityBase {
        &self.base
    }

    fn entity_type(&self) -> EntityTypeRef {
        self.entity_type
    }

    fn tick(&self) {
        if *self.in_ground.lock() {
            let mut life = self.life_ticks.lock();
            *life += 1;
            if *life >= 1200 {
                self.set_removed(RemovalReason::Discarded);
            }
            return;
        }

        self.set_old_position_to_current();
        self.base().set_old_rotation_to_current();
        self.apply_flight_physics();

        if let Some(hit) = self.get_hit_result_on_move_vector() {
            self.hit_target_or_deflect_self(&hit);
            if *self.in_ground.lock() {
                self.projectile_base_tick();
                return;
            }
        }

        let new_pos = self.position() + self.velocity();
        if let Err(error) = self.try_set_position(new_pos) {
            log::debug!("failed to advance arrow {}: {error}", self.id());
            self.set_removed(RemovalReason::Discarded);
            return;
        }
        self.update_rotation();
        self.projectile_base_tick();
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

    fn dimensions_for_pose(&self, _pose: EntityPose) -> EntityDimensions {
        self.entity_type.dimensions
    }

    fn synced_data(&self) -> Option<&dyn crate::entity::EntitySyncedData> {
        Some(&self.entity_data)
    }

    fn save_additional(&self, nbt: &mut NbtCompound) {
        self.save_projectile(nbt);
        nbt.insert("inGround", i8::from(*self.in_ground.lock()));
        nbt.insert("damage", *self.damage.lock());
        nbt.insert("life", *self.life_ticks.lock());
    }

    fn load_additional(&self, nbt: BorrowedNbtCompoundView<'_, '_>) {
        self.load_projectile(nbt);
        *self.in_ground.lock() = nbt.byte("inGround").unwrap_or(0) != 0;
        *self.damage.lock() = nbt.double("damage").unwrap_or(2.0);
        *self.life_ticks.lock() = nbt.int("life").unwrap_or(0);
    }
}

impl Projectile for ArrowEntity {
    fn projectile_base(&self) -> &ProjectileBase {
        &self.projectile_base
    }

    fn on_hit(&self, hit: &ProjectileHit) {
        match hit {
            ProjectileHit::Block { .. } => {
                *self.in_ground.lock() = true;
                self.set_velocity(DVec3::ZERO);
                if let Some(world) = self.level() {
                    world.play_sound_at(
                        &sound_events::ENTITY_ARROW_HIT,
                        steel_protocol::packets::game::SoundSource::Neutral,
                        self.position(),
                        1.0,
                        1.2,
                        None,
                    );
                }
            }
            ProjectileHit::Entity(entity_hit) => {
                if let Some(world) = self.level() {
                    let speed = self.velocity().length();
                    let damage_amount = (*self.damage.lock() * speed).ceil() as f32;
                    let mut source = DamageSource::environment(&vanilla_damage_types::ARROW)
                        .with_direct_entity(self.id())
                        .with_source_position(self.position());
                    if let Some(owner) = self.get_owner() {
                        source = source.with_causing_entity(owner.id());
                    }
                    entity_hit.entity.hurt(&world, &source, damage_amount);
                    world.play_sound_at(
                        &sound_events::ENTITY_ARROW_HIT,
                        steel_protocol::packets::game::SoundSource::Neutral,
                        self.position(),
                        1.0,
                        1.2,
                        None,
                    );
                }
                self.set_removed(RemovalReason::Discarded);
            }
        }
        self.projectile_on_hit(hit);
    }
}
