//! Vanilla `Projectile` class hierarchy, mirrored as Rust traits + base structs.
//!
//! Mirrors `net.minecraft.world.entity.projectile`:
//! `Entity → Projectile → ThrowableProjectile → ThrowableItemProjectile`.
//! Concrete projectiles embed [`ProjectileBase`] (owner / left-owner / shot state)
//! and implement the trait stack; the per-layer tick logic chains explicitly via
//! [`Projectile::projectile_base_tick`] and
//! [`ThrowableProjectile::throwable_projectile_tick`] (vanilla `super.tick()`).
//!
//! The block + entity move-vector raycast mirrors `ProjectileUtil`.

mod throwable;
mod throwable_item;

use std::mem;
use std::sync::{Arc, Weak};

use glam::DVec3;
use simdnbt::borrow::NbtCompound as BorrowedNbtCompoundView;
use simdnbt::owned::{NbtCompound, NbtTag};
use steel_utils::axis::Axis;
use steel_utils::locks::SyncMutex;
use steel_utils::{UuidExt, WorldAabb};
use uuid::Uuid;

use crate::entity::{Entity, SharedEntity};
use crate::world::{ClipBlockShape, ClipFluid, ClipHitResult, World};

pub use throwable::ThrowableProjectile;
pub use throwable_item::ThrowableItemProjectile;

/// Vanilla `Projectile.shoot` per-axis spread scale (`0.0172275 * uncertainty`).
const SHOOT_INACCURACY_SCALE: f64 = 0.0172_275;

/// Vanilla `ProjectileUtil.DEFAULT_ENTITY_HIT_RESULT_MARGIN`.
const MAX_ENTITY_HIT_MARGIN: f64 = 0.3;

/// Mirrors vanilla `RandomSource.triangle(mode, deviation)`.
fn triangle_random(mode: f64, deviation: f64) -> f64 {
    mode + deviation * (rand::random::<f64>() - rand::random::<f64>())
}

/// Result of a projectile move-vector raycast (vanilla `HitResult`).
pub enum ProjectileHit {
    /// The pearl's path entered a block collider.
    Block {
        /// Exact entry location.
        location: DVec3,
        /// The underlying block clip result.
        hit: ClipHitResult,
    },
    /// The pearl's path intersected an entity.
    Entity(EntityHitResult),
}

impl ProjectileHit {
    /// Returns the world-space hit location.
    #[must_use]
    pub const fn location(&self) -> DVec3 {
        match self {
            Self::Block { location, .. } => *location,
            Self::Entity(hit) => hit.location,
        }
    }
}

/// A projectile-versus-entity raycast hit (vanilla `EntityHitResult`).
pub struct EntityHitResult {
    /// The entity that was hit.
    pub entity: SharedEntity,
    /// The world-space location of the hit.
    pub location: DVec3,
}

struct ProjectileState {
    owner: Option<Uuid>,
    owner_entity: Option<Weak<dyn Entity>>,
    left_owner: bool,
    left_owner_checked: bool,
    has_been_shot: bool,
}

/// Runtime fields shared by vanilla projectiles (vanilla `Projectile` fields).
pub struct ProjectileBase {
    state: SyncMutex<ProjectileState>,
}

impl ProjectileBase {
    /// Creates default projectile runtime state.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            state: SyncMutex::new(ProjectileState {
                owner: None,
                owner_entity: None,
                left_owner: false,
                left_owner_checked: false,
                has_been_shot: false,
            }),
        }
    }
}

impl Default for ProjectileBase {
    fn default() -> Self {
        Self::new()
    }
}

/// Vanilla-shaped behavior shared by entities that extend `Projectile`.
pub trait Projectile: Entity {
    /// Returns shared projectile runtime state.
    fn projectile_base(&self) -> &ProjectileBase;

    /// Sets the owner UUID. Vanilla stores an `EntityReference`; Steel stores the
    /// UUID and resolves lazily.
    // TODO: introduce an `EntityReference` type to cache the resolved owner.
    fn set_owner_uuid(&self, owner: Option<Uuid>) {
        let mut state = self.projectile_base().state.lock();
        state.owner = owner;
        state.owner_entity = None;
    }

    /// Sets the owning entity and caches its live reference.
    fn set_owner_entity(&self, owner: Option<&SharedEntity>) {
        let mut state = self.projectile_base().state.lock();
        state.owner = owner.map(|owner| owner.uuid());
        state.owner_entity = owner.map(Arc::downgrade);
    }

    /// Caches a live owner reference when it matches the saved owner UUID.
    fn cache_owner_entity(&self, owner: &SharedEntity) {
        let mut state = self.projectile_base().state.lock();
        if state.owner == Some(owner.uuid()) {
            state.owner_entity = Some(Arc::downgrade(owner));
        }
    }

    /// Returns the owner UUID, if any.
    fn owner_uuid(&self) -> Option<Uuid> {
        self.projectile_base().state.lock().owner
    }

    /// Resolves the owning entity in the current world (vanilla `Projectile.getOwner`).
    fn get_owner(&self) -> Option<SharedEntity> {
        let uuid = self.owner_uuid()?;
        if let Some(owner) = self
            .projectile_base()
            .state
            .lock()
            .owner_entity
            .as_ref()
            .and_then(Weak::upgrade)
            && !owner.is_removed()
            && owner.uuid() == uuid
        {
            return Some(owner);
        }

        let owner = self.level()?.get_entity_by_uuid(&uuid)?;
        self.cache_owner_entity(&owner);
        Some(owner)
    }

    /// Returns vanilla `Projectile.ownedBy`.
    fn owned_by(&self, entity: &dyn Entity) -> bool {
        self.owner_uuid() == Some(entity.uuid())
    }

    /// Returns vanilla `Projectile.hasBeenShot`.
    fn has_been_shot(&self) -> bool {
        self.projectile_base().state.lock().has_been_shot
    }

    /// Sets vanilla `Projectile.hasBeenShot`.
    fn set_has_been_shot(&self, value: bool) {
        self.projectile_base().state.lock().has_been_shot = value;
    }

    /// Returns vanilla `Projectile.leftOwner`.
    fn left_owner(&self) -> bool {
        self.projectile_base().state.lock().left_owner
    }

    /// Runs vanilla `Projectile.checkLeftOwner`.
    fn check_left_owner(&self) {
        let mut state = self.projectile_base().state.lock();
        if state.left_owner || state.left_owner_checked {
            return;
        }
        state.left_owner_checked = true;
        drop(state);

        let left = self.is_outside_owner_collision_range();
        self.projectile_base().state.lock().left_owner = left;
    }

    /// Resets the per-tick left-owner check flag (vanilla clears it after `tick`).
    fn reset_left_owner_checked(&self) {
        self.projectile_base().state.lock().left_owner_checked = false;
    }

    /// Returns vanilla `Projectile.isOutsideOwnerCollisionRange`.
    fn is_outside_owner_collision_range(&self) -> bool {
        let Some(owner) = self.get_owner() else {
            return true;
        };
        let aabb = self
            .bounding_box()
            .expand_towards(self.velocity())
            .inflate(1.0);
        let root_vehicle = owner.root_vehicle().unwrap_or_else(|| owner.clone());
        let mut to_check = vec![root_vehicle];
        let mut visited = Vec::new();

        while let Some(entity) = to_check.pop() {
            let entity_id = entity.id();
            if visited.contains(&entity_id) {
                continue;
            }
            visited.push(entity_id);

            if entity.is_pickable() && aabb.intersects(entity.bounding_box()) {
                return false;
            }
            to_check.extend(entity.passengers());
        }

        true
    }

    /// Returns vanilla `Projectile.canHitEntity`.
    fn can_hit_entity(&self, entity: &dyn Entity) -> bool {
        if !entity.can_be_hit_by_projectile() {
            return false;
        }
        let Some(owner) = self.get_owner() else {
            return true;
        };
        self.left_owner() || !owner.is_passenger_of_same_vehicle(entity)
    }

    /// Returns vanilla `Projectile.getMovementToShoot`.
    fn get_movement_to_shoot(&self, direction: DVec3, power: f32, uncertainty: f32) -> DVec3 {
        let deviation = SHOOT_INACCURACY_SCALE * f64::from(uncertainty);
        let jitter = DVec3::new(
            triangle_random(0.0, deviation),
            triangle_random(0.0, deviation),
            triangle_random(0.0, deviation),
        );
        (direction.normalize_or_zero() + jitter) * f64::from(power)
    }

    /// Runs vanilla `Projectile.shoot`.
    fn shoot(&self, direction: DVec3, power: f32, uncertainty: f32) {
        let movement = self.get_movement_to_shoot(direction, power, uncertainty);
        self.set_velocity(movement);
        self.mark_velocity_sync();

        let horizontal = (movement.x * movement.x + movement.z * movement.z).sqrt();
        let yaw = movement.x.atan2(movement.z).to_degrees() as f32;
        let pitch = movement.y.atan2(horizontal).to_degrees() as f32;
        self.set_rotation((yaw, pitch));
        self.base().set_old_rotation_to_current();
    }

    /// Runs vanilla `Projectile.shootFromRotation`.
    fn shoot_from_rotation(
        &self,
        source: &dyn Entity,
        x_rot: f32,
        y_rot: f32,
        y_offset: f32,
        power: f32,
        uncertainty: f32,
    ) {
        let yaw = y_rot.to_radians();
        let pitch = x_rot.to_radians();
        let pitch_offset = (x_rot + y_offset).to_radians();
        let direction = DVec3::new(
            f64::from(-yaw.sin() * pitch.cos()),
            f64::from(-pitch_offset.sin()),
            f64::from(yaw.cos() * pitch.cos()),
        );
        self.shoot(direction, power, uncertainty);

        let source_movement = source.known_movement();
        let added_y = if source.on_ground() {
            0.0
        } else {
            source_movement.y
        };
        self.set_velocity(
            self.velocity() + DVec3::new(source_movement.x, added_y, source_movement.z),
        );
    }

    /// Runs vanilla `Projectile.updateRotation` (lerped toward the movement vector).
    fn update_rotation(&self) {
        let movement = self.velocity();
        let horizontal = (movement.x * movement.x + movement.z * movement.z).sqrt();
        let (yaw_old, pitch_old) = self.base().old_rotation();
        let yaw = lerp_rotation(yaw_old, movement.x.atan2(movement.z).to_degrees() as f32);
        let pitch = lerp_rotation(pitch_old, movement.y.atan2(horizontal).to_degrees() as f32);
        self.set_rotation((yaw, pitch));
    }

    /// Casts the move vector and returns the nearest block/entity hit (vanilla
    /// `ProjectileUtil.getHitResultOnMoveVector` with `this::canHitEntity`).
    fn get_hit_result_on_move_vector(&self) -> Option<ProjectileHit> {
        let world = self.level()?;
        let from = self.position();
        let delta = self.velocity();
        let to = from + delta;

        let block_hit = world.clip(from, to, ClipBlockShape::Collider, ClipFluid::None);
        let entity_end = if block_hit.is_miss() {
            to
        } else {
            block_hit.location
        };

        let search_box = self.bounding_box().expand_towards(delta).inflate(1.0);
        let margin = compute_margin(self.tick_count());
        let self_id = self.id();
        let entity_hit = get_entity_hit_result(&world, from, entity_end, search_box, margin, |e| {
            e.id() != self_id && self.can_hit_entity(e)
        });

        if let Some(hit) = entity_hit {
            return Some(ProjectileHit::Entity(hit));
        }
        if !block_hit.is_miss() {
            return Some(ProjectileHit::Block {
                location: block_hit.location,
                hit: block_hit,
            });
        }
        None
    }

    /// Vanilla `Projectile.hitTargetOrDeflectSelf`.
    fn hit_target_or_deflect_self(&self, hit: &ProjectileHit) {
        // TODO: projectile deflection (REDIRECTABLE_PROJECTILE / world-border bounce).
        // Ender pearls neither deflect nor are redirectable, so we always hit.
        self.on_hit(hit);
    }

    /// Vanilla `Projectile.onHit`. Subclasses override this and call
    /// [`Projectile::projectile_on_hit`] for the base dispatch (`super.onHit()`).
    fn on_hit(&self, hit: &ProjectileHit) {
        self.projectile_on_hit(hit);
    }

    /// The base `Projectile.onHit` dispatch to block/entity handlers. Not meant to
    /// be overridden — override [`Projectile::on_hit`] and delegate here instead.
    fn projectile_on_hit(&self, hit: &ProjectileHit) {
        // TODO: fire the PROJECTILE_LAND game event (sculk sensors) once an
        // `&dyn Entity` source cast is available here.
        match hit {
            ProjectileHit::Entity(entity_hit) => {
                self.on_hit_entity(&entity_hit.entity, entity_hit.location);
            }
            ProjectileHit::Block { hit, .. } => self.on_hit_block(hit),
        }
    }

    /// Vanilla `Projectile.onHitEntity` (no-op by default).
    fn on_hit_entity(&self, _entity: &SharedEntity, _location: DVec3) {}

    /// Vanilla `Projectile.onHitBlock`.
    fn on_hit_block(&self, _hit: &ClipHitResult) {
        // TODO: call BlockState.onProjectileHit once block behaviors expose it.
    }

    /// Vanilla `Projectile.tick` (the `super.tick()` reached from subclasses).
    fn projectile_base_tick(&self) {
        if !self.has_been_shot() {
            // TODO: fire the PROJECTILE_SHOOT game event for the owner.
            self.set_has_been_shot(true);
        }
        self.check_left_owner();
        self.default_tick();
        self.reset_left_owner_checked();
    }

    /// Saves vanilla `Projectile` fields (`Owner`, `LeftOwner`, `HasBeenShot`).
    fn save_projectile(&self, nbt: &mut NbtCompound) {
        let state = self.projectile_base().state.lock();
        if let Some(owner) = state.owner {
            nbt.insert("Owner", NbtTag::IntArray(owner.to_int_array().to_vec()));
        }
        if state.left_owner {
            nbt.insert("LeftOwner", 1i8);
        }
        nbt.insert("HasBeenShot", i8::from(state.has_been_shot));
    }

    /// Loads vanilla `Projectile` fields.
    fn load_projectile(&self, nbt: BorrowedNbtCompoundView<'_, '_>) {
        let mut state = self.projectile_base().state.lock();
        if let Some(owner_arr) = nbt.int_array("Owner")
            && let Some(uuid) = Uuid::from_int_array(&owner_arr)
        {
            state.owner = Some(uuid);
        }
        state.left_owner = nbt.byte("LeftOwner").is_some_and(|value| value != 0);
        state.has_been_shot = nbt.byte("HasBeenShot").is_some_and(|value| value != 0);
    }
}

/// Vanilla `ProjectileUtil.computeMargin`: ramps the entity hit margin from 0 to
/// 0.3 over the first ticks of flight.
#[must_use]
pub fn compute_margin(tick_count: i32) -> f64 {
    (f64::from(tick_count - 2) / 20.0).clamp(0.0, MAX_ENTITY_HIT_MARGIN)
}

/// Vanilla `ProjectileUtil.getEntityHitResult` (entity-margin overload): returns
/// the nearest entity whose inflated box the segment `from -> to` enters.
fn get_entity_hit_result(
    world: &Arc<World>,
    from: DVec3,
    to: DVec3,
    search_box: WorldAabb,
    margin: f64,
    predicate: impl Fn(&dyn Entity) -> bool,
) -> Option<EntityHitResult> {
    let mut nearest: Option<EntityHitResult> = None;
    let mut nearest_dist_sq = f64::MAX;

    for entity in world.get_entities_in_aabb(&search_box) {
        if !predicate(entity.as_ref()) {
            continue;
        }
        let target_box = entity.bounding_box().inflate(margin);
        let Some(location) = clip_segment(target_box, from, to) else {
            continue;
        };
        let dist_sq = from.distance_squared(location);
        if dist_sq < nearest_dist_sq {
            nearest_dist_sq = dist_sq;
            nearest = Some(EntityHitResult { entity, location });
        }
    }

    nearest
}

/// Clips the segment `from -> to` against `aabb`, returning the entry point.
///
/// Returns `from` when the segment starts inside the box and `None` when it never
/// intersects. Mirrors vanilla `AABB.clip` using the slab method.
fn clip_segment(aabb: WorldAabb, from: DVec3, to: DVec3) -> Option<DVec3> {
    const EPSILON: f64 = 1.0e-7;

    let direction = to - from;
    let mut t_min = 0.0_f64;
    let mut t_max = 1.0_f64;

    for axis in [Axis::X, Axis::Y, Axis::Z] {
        let start = axis_component(from, axis);
        let delta = axis_component(direction, axis);
        let axis_min = aabb.min(axis);
        let axis_max = aabb.max(axis);

        if delta.abs() < EPSILON {
            if start < axis_min || start > axis_max {
                return None;
            }
            continue;
        }

        let inv_delta = 1.0 / delta;
        let mut low = (axis_min - start) * inv_delta;
        let mut high = (axis_max - start) * inv_delta;
        if low > high {
            mem::swap(&mut low, &mut high);
        }

        t_min = t_min.max(low);
        t_max = t_max.min(high);
        if t_min > t_max {
            return None;
        }
    }

    Some(from + direction * t_min)
}

const fn axis_component(vec: DVec3, axis: Axis) -> f64 {
    match axis {
        Axis::X => vec.x,
        Axis::Y => vec.y,
        Axis::Z => vec.z,
    }
}

/// Vanilla `Mth.lerp(0.2, rotO, rot)` after wrapping the old angle into range.
fn lerp_rotation(mut rot_old: f32, rot: f32) -> f32 {
    while rot - rot_old < -180.0 {
        rot_old -= 360.0;
    }
    while rot - rot_old >= 180.0 {
        rot_old += 360.0;
    }
    rot_old + 0.2 * (rot - rot_old)
}

#[cfg(test)]
mod tests {
    use super::*;
    use steel_registry::{
        entity_type::EntityTypeRef, test_support::init_test_registry, vanilla_entities,
    };

    use crate::entity::EntityBase;

    struct OwnerCollisionProjectile {
        base: EntityBase,
        projectile_base: ProjectileBase,
    }

    impl OwnerCollisionProjectile {
        fn new(id: i32, position: DVec3) -> Self {
            Self {
                base: EntityBase::new(
                    id,
                    position,
                    vanilla_entities::ENDER_PEARL.dimensions,
                    Weak::new(),
                ),
                projectile_base: ProjectileBase::new(),
            }
        }
    }

    crate::entity::impl_test_downcast_type!(OwnerCollisionProjectile);

    impl Entity for OwnerCollisionProjectile {
        fn base(&self) -> &EntityBase {
            &self.base
        }

        fn entity_type(&self) -> EntityTypeRef {
            &vanilla_entities::ENDER_PEARL
        }
    }

    impl Projectile for OwnerCollisionProjectile {
        fn projectile_base(&self) -> &ProjectileBase {
            &self.projectile_base
        }
    }

    struct OwnerCollisionTestEntity {
        base: EntityBase,
        pickable: bool,
    }

    impl OwnerCollisionTestEntity {
        fn shared(id: i32, position: DVec3, pickable: bool) -> SharedEntity {
            Arc::new(Self {
                base: EntityBase::new(id, position, vanilla_entities::PIG.dimensions, Weak::new()),
                pickable,
            })
        }
    }

    crate::entity::impl_test_downcast_type!(OwnerCollisionTestEntity);

    impl Entity for OwnerCollisionTestEntity {
        fn base(&self) -> &EntityBase {
            &self.base
        }

        fn entity_type(&self) -> EntityTypeRef {
            &vanilla_entities::PIG
        }

        fn is_pickable(&self) -> bool {
            self.pickable && !self.is_removed()
        }
    }

    #[test]
    fn compute_margin_ramps_from_zero_to_cap() {
        assert!((compute_margin(2) - 0.0).abs() < 1.0e-9);
        assert!((compute_margin(7) - 0.25).abs() < 1.0e-9);
        assert!((compute_margin(100) - 0.3).abs() < 1.0e-9);
    }

    #[test]
    fn clip_segment_hits_box_in_path() {
        let aabb = WorldAabb::new(4.0, -0.5, -0.5, 5.0, 0.5, 0.5);
        let hit = clip_segment(aabb, DVec3::ZERO, DVec3::new(10.0, 0.0, 0.0))
            .expect("ray along +x should enter the box");
        assert!((hit.x - 4.0).abs() < 1.0e-6);
    }

    #[test]
    fn clip_segment_returns_start_when_inside() {
        let aabb = WorldAabb::new(-1.0, -1.0, -1.0, 1.0, 1.0, 1.0);
        let hit = clip_segment(aabb, DVec3::ZERO, DVec3::new(0.0, 5.0, 0.0))
            .expect("a ray starting inside the box hits at its origin");
        assert_eq!(hit, DVec3::ZERO);
    }

    #[test]
    fn owner_collision_range_checks_root_vehicle_passengers() {
        init_test_registry();

        let projectile = OwnerCollisionProjectile::new(1, DVec3::ZERO);
        let owner = OwnerCollisionTestEntity::shared(2, DVec3::new(10.0, 0.0, 0.0), true);
        let vehicle = OwnerCollisionTestEntity::shared(3, DVec3::new(10.0, 0.0, 0.0), true);
        let passenger = OwnerCollisionTestEntity::shared(4, DVec3::ZERO, true);
        EntityBase::restore_passenger_relationship(&vehicle, &owner);
        EntityBase::restore_passenger_relationship(&vehicle, &passenger);

        projectile.set_owner_entity(Some(&owner));

        assert!(!projectile.is_outside_owner_collision_range());
    }

    #[test]
    fn owner_collision_range_ignores_non_pickable_root_vehicle_passengers() {
        init_test_registry();

        let projectile = OwnerCollisionProjectile::new(1, DVec3::ZERO);
        let owner = OwnerCollisionTestEntity::shared(2, DVec3::new(10.0, 0.0, 0.0), true);
        let vehicle = OwnerCollisionTestEntity::shared(3, DVec3::new(10.0, 0.0, 0.0), true);
        let passenger = OwnerCollisionTestEntity::shared(4, DVec3::ZERO, false);
        EntityBase::restore_passenger_relationship(&vehicle, &owner);
        EntityBase::restore_passenger_relationship(&vehicle, &passenger);

        projectile.set_owner_entity(Some(&owner));

        assert!(projectile.is_outside_owner_collision_range());
    }
}
