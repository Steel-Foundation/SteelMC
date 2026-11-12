//! Vanilla `AbstractHurtingProjectile` — gravity-free projectiles that accelerate
//! along their heading (fireballs, wither skulls, wind charges).

use std::f32::consts::PI;
use std::sync::Arc;

use glam::DVec3;
use simdnbt::borrow::NbtCompound as BorrowedNbtCompoundView;
use simdnbt::owned::NbtCompound;
use steel_math::{DEGREE_180, DEGREE_360};
use steel_utils::locks::SyncMutex;
use steel_utils::nbt::NbtValueInput as _;

use crate::entity::projectile::{Projectile, ProjectileHit};
use crate::entity::{Entity, RemovalReason};

/// Default acceleration power, restored when an attack deflects the projectile.
pub const INITIAL_ACCELERATION_POWER: f64 = 0.1;

const ACCELERATION_POWER_TAG: &str = "acceleration_power";
const DEFAULT_INERTIA: f32 = 0.95;
const DEFAULT_LIQUID_INERTIA: f32 = 0.8;
const ROTATION_SPEED: f32 = 0.2;
/// Vanilla `igniteForSeconds(1.0F)`.
const BURN_TICKS: i32 = 20;
/// Vanilla `Vec3.normalize` returns zero below this length.
const NORMALIZE_EPSILON: f64 = 1.0e-5_f32 as f64;

/// Runtime fields shared by vanilla hurting projectiles.
pub struct HurtingProjectileBase {
    acceleration_power: SyncMutex<f64>,
}

impl HurtingProjectileBase {
    /// Creates hurting-projectile state with the given acceleration power.
    #[must_use]
    pub const fn new(acceleration_power: f64) -> Self {
        Self {
            acceleration_power: SyncMutex::new(acceleration_power),
        }
    }
}

impl Default for HurtingProjectileBase {
    fn default() -> Self {
        Self::new(INITIAL_ACCELERATION_POWER)
    }
}

/// Vanilla-shaped behavior shared by entities that extend `AbstractHurtingProjectile`.
///
/// Implementors also override [`Projectile::hurt`] to return `false` without
/// marking hurt, matching vanilla `AbstractHurtingProjectile.hurtServer`.
pub trait AbstractHurtingProjectile: Projectile {
    /// Returns shared hurting-projectile runtime state.
    fn hurting_projectile_base(&self) -> &HurtingProjectileBase;

    /// Returns the acceleration added along the heading each tick.
    fn acceleration_power(&self) -> f64 {
        *self.hurting_projectile_base().acceleration_power.lock()
    }

    /// Sets the acceleration added along the heading each tick.
    fn set_acceleration_power(&self, acceleration_power: f64) {
        *self.hurting_projectile_base().acceleration_power.lock() = acceleration_power;
    }

    /// Whether this projectile keeps itself on fire while flying.
    fn should_burn(&self) -> bool {
        true
    }

    /// Velocity multiplier applied each tick outside water.
    fn get_inertia(&self) -> f32 {
        DEFAULT_INERTIA
    }

    /// Velocity multiplier applied each tick in water.
    fn get_liquid_inertia(&self) -> f32 {
        DEFAULT_LIQUID_INERTIA
    }

    /// Vanilla `AbstractHurtingProjectile.canHitEntity`, for overrides of
    /// [`Projectile::can_hit_entity`].
    fn hurting_can_hit_entity(&self, entity: &dyn Entity) -> bool {
        self.base_can_hit_entity(entity) && !entity.no_physics()
    }

    /// Vanilla `AbstractHurtingProjectile.onDeflection`, for overrides of
    /// [`Projectile::on_deflection`].
    fn hurting_on_deflection(&self, by_attack: bool) {
        if by_attack {
            self.set_acceleration_power(INITIAL_ACCELERATION_POWER);
        } else {
            self.set_acceleration_power(self.acceleration_power() * 0.5);
        }
    }

    /// Vanilla `AbstractHurtingProjectile.tick`: accelerates, raycasts the move
    /// vector, moves, runs the `Projectile` base tick, then resolves the hit.
    /// Discards itself once its owner is removed or its chunk is unloaded.
    fn hurting_projectile_tick(self: Arc<Self>) {
        // Vanilla `Entity.setOldPosAndRot()` runs before ticking; capture it so
        // `rotate_towards_movement` lerps from the pre-tick rotation.
        self.set_old_position_to_current();
        self.base().set_old_rotation_to_current();

        let owner = self.get_owner();
        self.apply_hurting_inertia();

        let Some(world) = self.level() else {
            return;
        };
        if owner.is_some_and(|owner| owner.is_removed())
            || !world.is_full_chunk_loaded_at(self.block_position())
        {
            self.set_removed(RemovalReason::Discarded);
            return;
        }

        let hit = self.get_hit_result_on_move_vector();
        let new_position = hit.as_ref().map_or_else(
            || self.position() + self.velocity(),
            ProjectileHit::location,
        );

        self.rotate_towards_movement(ROTATION_SPEED);
        if let Err(error) = self.try_set_position(new_position) {
            log::debug!("failed to advance projectile {}: {error}", self.id());
            self.set_removed(RemovalReason::Discarded);
            return;
        }
        self.apply_effects_from_blocks();
        self.projectile_base_tick();
        if self.should_burn() {
            self.ignite_for_ticks(BURN_TICKS);
        }

        if let Some(hit) = hit
            && self.is_alive()
            && !self.is_world_change_pending()
        {
            self.hit_target_or_deflect_self(&hit);
        }
        // VANILLA CLIENT-LOCAL: `createParticleTrail` adds the trail particle.
    }

    /// Adds the acceleration along the heading, then applies air or water inertia.
    fn apply_hurting_inertia(&self) {
        let movement = self.velocity();
        // VANILLA CLIENT-LOCAL: the in-water branch also adds bubble particles.
        let inertia = if self.is_in_water() {
            self.get_liquid_inertia()
        } else {
            self.get_inertia()
        };
        let length = movement.length();
        let heading = if length < NORMALIZE_EPSILON {
            DVec3::ZERO
        } else {
            movement / length
        };
        self.set_velocity((movement + heading * self.acceleration_power()) * f64::from(inertia));
    }

    /// Vanilla `ProjectileUtil.rotateTowardsMovement`.
    fn rotate_towards_movement(&self, rotation_speed: f32) {
        let movement = self.velocity();
        if movement.length_squared() == 0.0 {
            return;
        }
        let horizontal = (movement.x * movement.x + movement.z * movement.z).sqrt();
        let yaw = (movement.z.atan2(movement.x) * 180.0 / f64::from(PI)) as f32 + 90.0;
        let pitch = (horizontal.atan2(movement.y) * 180.0 / f64::from(PI)) as f32 - 90.0;

        let (mut yaw_old, mut pitch_old) = self.base().old_rotation();
        while pitch - pitch_old < -DEGREE_180 {
            pitch_old -= DEGREE_360;
        }
        while pitch - pitch_old >= DEGREE_180 {
            pitch_old += DEGREE_360;
        }
        while yaw - yaw_old < -DEGREE_180 {
            yaw_old -= DEGREE_360;
        }
        while yaw - yaw_old >= DEGREE_180 {
            yaw_old += DEGREE_360;
        }
        self.base().set_old_rotation((yaw_old, pitch_old));
        self.set_rotation((
            yaw_old + rotation_speed * (yaw - yaw_old),
            pitch_old + rotation_speed * (pitch - pitch_old),
        ));
    }

    /// Saves vanilla `Projectile` and `AbstractHurtingProjectile` fields.
    fn save_hurting_projectile(&self, nbt: &mut NbtCompound) {
        self.save_projectile(nbt);
        nbt.insert(ACCELERATION_POWER_TAG, self.acceleration_power());
    }

    /// Loads vanilla `Projectile` and `AbstractHurtingProjectile` fields.
    fn load_hurting_projectile(&self, nbt: BorrowedNbtCompoundView<'_, '_>) {
        self.load_projectile(nbt);
        self.set_acceleration_power(
            nbt.double_value(ACCELERATION_POWER_TAG)
                .unwrap_or(INITIAL_ACCELERATION_POWER),
        );
    }
}
