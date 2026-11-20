//! Gravity-free projectiles that accelerate along their heading.

use glam::DVec3;
use simdnbt::borrow::NbtCompound as BorrowedNbtCompoundView;
use simdnbt::owned::NbtCompound;
use steel_math::trig;
use steel_math::{DEGREE_90, DEGREE_180, DEGREE_360, RAD_TO_DEG_F64};
use steel_utils::locks::CheckedMutex;
use steel_utils::nbt::NbtValueInput as _;
use steel_utils::{DowncastType, DowncastTypeKey, TICKS_PER_SECOND};

use crate::entity::component::{Component, Components as _, Has};
use crate::entity::prelude::*;
use crate::entity::projectile::{self, Projectile, ProjectileProps};
use crate::entity::{EntityProps, RemovalReason};

/// Acceleration restored when an attack deflects the projectile.
pub const INITIAL_ACCELERATION_POWER: f64 = 0.1;

const ACCELERATION_POWER_TAG: &str = "acceleration_power";
const DEFLECTION_SCALE: f64 = 0.5;
const DEFAULT_INERTIA: f32 = 0.95;
const DEFAULT_LIQUID_INERTIA: f32 = 0.8;
const ROTATION_SPEED: f32 = 0.2;
const NORMALIZE_EPSILON: f64 = 1.0e-5_f32 as f64;

/// Runtime state of an accelerating projectile.
pub struct HurtingProjectileBase {
    acceleration_power: CheckedMutex<f64>,
}

impl HurtingProjectileBase {
    /// Creates hurting-projectile state with the given acceleration power.
    #[must_use]
    pub const fn new(acceleration_power: f64) -> Self {
        Self {
            acceleration_power: CheckedMutex::new(acceleration_power),
        }
    }

    /// Returns acceleration along the heading per tick.
    #[must_use]
    pub fn acceleration_power(&self) -> f64 {
        *self.acceleration_power.lock()
    }
}

impl Default for HurtingProjectileBase {
    fn default() -> Self {
        Self::new(INITIAL_ACCELERATION_POWER)
    }
}

// SAFETY: This key identifies Steel's hurting-projectile component.
unsafe impl DowncastType for HurtingProjectileBase {
    const TYPE_KEY: DowncastTypeKey =
        DowncastTypeKey::new("steel:entity_component/hurting_projectile");
}

impl Component for HurtingProjectileBase {
    fn save(&self, _: &dyn Entity, nbt: &mut NbtCompound) {
        nbt.insert(ACCELERATION_POWER_TAG, self.acceleration_power());
    }

    fn load(&self, _: &dyn Entity, nbt: BorrowedNbtCompoundView<'_, '_>) {
        *self.acceleration_power.lock() = nbt
            .double_value(ACCELERATION_POWER_TAG)
            .unwrap_or(INITIAL_ACCELERATION_POWER);
    }
}

/// A concrete type in the `AbstractHurtingProjectile` hierarchy.
pub trait Parts: projectile::Parts + Has<HurtingProjectileBase> {
    /// Whether the projectile ignites itself while flying.
    const SHOULD_BURN: bool = true;
    /// Velocity multiplier applied outside water.
    const INERTIA: f32 = DEFAULT_INERTIA;
    /// Velocity multiplier applied in water.
    const LIQUID_INERTIA: f32 = DEFAULT_LIQUID_INERTIA;
}

/// Property overrides shared by hurting projectiles.
#[must_use]
pub const fn entity_props<E: Parts>() -> EntityProps<E> {
    EntityProps {
        tick: tick::<E>,
        hurt: |_, _, _, _| false,
        ..projectile::entity_props()
    }
}

/// Projectile property overrides shared by hurting projectiles.
#[must_use]
pub const fn projectile_props<E: Parts>() -> ProjectileProps<E> {
    ProjectileProps {
        can_hit_entity: can_hit_entity::<E>,
        on_deflection: on_deflection::<E>,
        ..ProjectileProps::new()
    }
}

/// Runs inertia, movement, block effects and the projectile base tick before processing the hit.
pub fn tick<E: Parts>(entity: &E) {
    let projectile = entity.projectile();
    let owner = projectile.owner();
    let movement = projectile.velocity();
    let inertia = if projectile.is_in_water() {
        E::LIQUID_INERTIA
    } else {
        E::INERTIA
    };
    let length = movement.length();
    let heading = if length < NORMALIZE_EPSILON {
        DVec3::ZERO
    } else {
        movement / length
    };
    let acceleration_power = entity.get::<HurtingProjectileBase>().acceleration_power();
    projectile.set_velocity((movement + heading * acceleration_power) * f64::from(inertia));

    let Some(world) = projectile.level() else {
        return;
    };
    if owner.is_some_and(|owner| owner.is_removed())
        || !world.is_full_chunk_loaded_at(projectile.block_position())
    {
        projectile.set_removed(RemovalReason::Discarded);
        return;
    }

    let hit = projectile.hit_result_on_move_vector();
    let position = hit.as_ref().map_or_else(
        || projectile.position() + projectile.velocity(),
        projectile::ProjectileHit::location,
    );
    rotate_towards_movement(&projectile);
    if let Err(error) = projectile.try_set_position(position) {
        log::debug!("failed to advance projectile {}: {error}", projectile.id());
        projectile.set_removed(RemovalReason::Discarded);
        return;
    }
    projectile.apply_effects_from_blocks();
    projectile::tick(entity);
    if E::SHOULD_BURN {
        projectile.ignite_for_ticks(TICKS_PER_SECOND);
    }

    if let Some(hit) = hit
        && projectile.is_alive()
        && !projectile.is_world_change_pending()
    {
        projectile.hit_target_or_deflect_self(&hit);
    }
}

/// Applies the base eligibility check and excludes no-physics targets.
pub fn can_hit_entity<E: Parts>(entity: &E, target: &dyn Entity) -> bool {
    projectile::can_hit_entity(entity, target) && !target.no_physics()
}

fn on_deflection<E: Parts>(entity: &E, by_attack: bool) {
    projectile::on_deflection(entity, by_attack);
    let mut power = entity
        .get::<HurtingProjectileBase>()
        .acceleration_power
        .lock();
    if by_attack {
        *power = INITIAL_ACCELERATION_POWER;
    } else {
        *power *= DEFLECTION_SCALE;
    }
}

fn rotate_towards_movement(projectile: &Projectile<'_>) {
    let movement = projectile.velocity();
    if movement.length_squared() == 0.0 {
        return;
    }
    let horizontal = (movement.x * movement.x + movement.z * movement.z).sqrt();
    let yaw = (trig::atan2(movement.z, movement.x) * RAD_TO_DEG_F64) as f32 + DEGREE_90;
    let pitch = (trig::atan2(horizontal, movement.y) * RAD_TO_DEG_F64) as f32 - DEGREE_90;
    let (mut yaw_old, mut pitch_old) = projectile.base().old_rotation();
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
    projectile.base().set_old_rotation((yaw_old, pitch_old));
    projectile.set_rotation((
        yaw_old + ROTATION_SPEED * (yaw - yaw_old),
        pitch_old + ROTATION_SPEED * (pitch - pitch_old),
    ));
}
