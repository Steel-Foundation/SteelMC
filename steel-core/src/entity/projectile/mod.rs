//! Projectile trait hierarchy and the shared move-vector raycasts.
//!
//! Rust traits cannot call `super`, so the layers chain explicitly through
//! [`Projectile::projectile_base_tick`] and the `default_*` bodies that overrides
//! delegate to. The block and entity raycasts follow `ProjectileUtil`.

mod arrow;
mod throwable;
mod throwable_item;
mod thrown_potion;

#[cfg(test)]
mod owner_tests;

use std::sync::{Arc, Weak};

use glam::DVec3;
use simdnbt::borrow::NbtCompound as BorrowedNbtCompoundView;
use simdnbt::owned::NbtCompound;
use steel_math::{DEG_TO_RAD, DEGREE_180, DEGREE_360, trig};
use steel_registry::blocks::block_state_ext::BlockStateExt as _;
use steel_registry::item_stack::ItemStack;
use steel_registry::vanilla_entity_type_tags::EntityTypeTag;
use steel_registry::vanilla_game_rules::{MOB_GRIEFING, PROJECTILES_CAN_BREAK_BLOCKS};
use steel_registry::{REGISTRY, TaggedRegistryExt as _, vanilla_game_events};
use steel_utils::axis::Axis;
use steel_utils::locks::SyncMutex;
use steel_utils::nbt::NbtValueInput as _;
use steel_utils::{BlockPos, WorldAabb};
use uuid::Uuid;

use crate::behavior::BLOCK_BEHAVIORS;
use crate::enchantment_helper;
use crate::entity::damage::DamageSource;
use crate::entity::{Entity, EntityReference, LivingEntity, SharedEntity};
use crate::player::Player;
use crate::world::game_event::GameEventContext;
use crate::world::{ClipBlockShape, ClipFluid, ClipHitResult, World};

use super::{
    BUBBLE_COLUMN_ABOVE_UP_ACCELERATION, BUBBLE_COLUMN_DOWN_ACCELERATION,
    BUBBLE_COLUMN_INSIDE_UP_ACCELERATION,
};

pub(crate) use arrow::{ARROW_GRAVITY, AbstractArrow, ArrowBase, ArrowPickup};
pub use throwable::ThrowableProjectile;
pub use throwable_item::ThrowableItemProjectile;
pub use thrown_potion::{
    AbstractThrownPotion, SPLASH_HORIZONTAL_REACH, SPLASH_RANGE_SQ, SPLASH_VERTICAL_REACH,
};

/// Per-axis spread scale of `Projectile.shoot` (`0.0172275 * uncertainty`).
const SHOOT_INACCURACY_SCALE: f64 = 0.0172_275;

const MAX_ENTITY_HIT_MARGIN: f32 = 0.3;
const ENTITY_HIT_MARGIN_START_TICKS: i32 = 2;
const ENTITY_HIT_MARGIN_RAMP_TICKS: f32 = 20.0;
const ENTITY_SEARCH_INFLATE: f64 = 1.0;
const OWNER_COLLISION_SEARCH_INFLATE: f64 = 1.0;

/// Movement shorter than this is treated as stopped.
const STOPPED_MOVEMENT_EPSILON: f64 = 1.0e-12;

/// Default deflection power applied when an entity deflects a projectile.
const DEFLECTION_POWER: f64 = 1.0;

/// Deflection power applied when a projectile bounces off its hit target.
const HIT_DEFLECTION_POWER: f64 = 0.2;

const REVERSE_ROTATION_MIN_DEGREES: f32 = 170.0;
const REVERSE_ROTATION_MAX_DEGREES: f32 = 190.0;
const REVERSE_VELOCITY_SCALE: f64 = 0.5;

const ROTATION_LERP_FACTOR: f32 = 0.2;

/// Spawn offset below the shooter's eye for thrown item projectiles.
const THROWN_ITEM_SPAWN_EYE_OFFSET: f64 = 0.1;

/// Returns a triangle-distributed random value.
#[must_use]
pub fn triangle_random(mode: f64, deviation: f64) -> f64 {
    mode + deviation * (rand::random::<f64>() - rand::random::<f64>())
}

/// A projectile move-vector raycast hit.
pub enum ProjectileHit {
    /// The projectile's path entered a block collider.
    Block {
        /// Exact entry location.
        location: DVec3,
        /// The underlying block clip result.
        hit: ClipHitResult,
    },
    /// The projectile's path intersected an entity.
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

/// A projectile-versus-entity raycast hit.
pub struct EntityHitResult {
    /// The entity that was hit.
    pub entity: SharedEntity,
    /// The world-space location of the hit.
    pub location: DVec3,
}

/// `Vec3.normalize` treats vectors shorter than this as zero.
const VEC3_NORMALIZE_EPSILON: f64 = 1.0e-5;

/// Deflection behavior applied when a projectile hits an entity.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProjectileDeflection {
    /// Continue with normal hit handling.
    None,
    /// Reverse the projectile with a randomized yaw adjustment.
    Reverse,
    /// Redirect the projectile along the deflecting entity's look direction.
    AimDeflect,
    /// Redirect the projectile along the deflecting entity's movement direction.
    MomentumDeflect,
}

impl ProjectileDeflection {
    fn apply(
        self,
        projectile: &dyn Projectile,
        deflecting_entity: Option<&dyn Entity>,
        power: DVec3,
    ) {
        match self {
            Self::None => {}
            Self::Reverse => {
                let rotation =
                    rand::random_range(REVERSE_ROTATION_MIN_DEGREES..REVERSE_ROTATION_MAX_DEGREES);
                let velocity = projectile.velocity();
                projectile.set_velocity(DVec3::new(
                    velocity.x * -power.x * REVERSE_VELOCITY_SCALE,
                    velocity.y * -power.y * REVERSE_VELOCITY_SCALE,
                    velocity.z * -power.z * REVERSE_VELOCITY_SCALE,
                ));
                let (yaw, pitch) = projectile.rotation();
                projectile.set_rotation((yaw + rotation, pitch));
                let (old_yaw, old_pitch) = projectile.base().old_rotation();
                projectile
                    .base()
                    .set_old_rotation((old_yaw + rotation, old_pitch));
                projectile.mark_velocity_sync();
            }
            Self::AimDeflect => {
                let Some(entity) = deflecting_entity else {
                    return;
                };
                let look_angle = entity.look_angle();
                projectile.set_velocity(DVec3::new(
                    look_angle.x * power.x,
                    look_angle.y * power.y,
                    look_angle.z * power.z,
                ));
                projectile.mark_velocity_sync();
            }
            Self::MomentumDeflect => {
                let Some(entity) = deflecting_entity else {
                    return;
                };
                let movement = entity.known_speed();
                let length = movement.length();
                let normalized = if length < VEC3_NORMALIZE_EPSILON {
                    DVec3::ZERO
                } else {
                    movement / length
                };
                projectile.set_velocity(DVec3::new(
                    normalized.x * power.x,
                    normalized.y * power.y,
                    normalized.z * power.z,
                ));
                projectile.mark_velocity_sync();
            }
        }
    }
}

struct ProjectileState {
    owner: Option<EntityReference>,
    left_owner: bool,
    left_owner_checked: bool,
    has_been_shot: bool,
    last_deflected_by: Option<Weak<dyn Entity>>,
}

/// Runtime state shared by all projectiles.
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
                left_owner: false,
                left_owner_checked: false,
                has_been_shot: false,
                last_deflected_by: None,
            }),
        }
    }
}

impl Default for ProjectileBase {
    fn default() -> Self {
        Self::new()
    }
}

/// Object-safe access to a projectile trait object from default [`Projectile`] methods.
pub trait ProjectileEventSource {
    /// Returns this entity as a projectile.
    fn as_projectile_event_source(&self) -> &dyn Projectile;
}

impl<T: Projectile> ProjectileEventSource for T {
    fn as_projectile_event_source(&self) -> &dyn Projectile {
        self
    }
}

/// Behavior shared by entities that extend `Projectile`.
pub trait Projectile: Entity + ProjectileEventSource {
    /// Returns shared projectile runtime state.
    fn projectile_base(&self) -> &ProjectileBase;

    /// Returns the horizontal hurt-knockback vector used for this projectile.
    fn calculate_horizontal_hurt_knockback_direction(
        &self,
        _hurt_entity: &dyn LivingEntity,
        _damage_source: &DamageSource,
    ) -> (f64, f64) {
        let movement = self.velocity();
        (movement.x, movement.z)
    }

    /// Applies bubble-column surface acceleration without clamping velocity.
    fn on_above_bubble_column_projectile(&self, drag_down: bool, pos: BlockPos) {
        let acceleration = if drag_down {
            -BUBBLE_COLUMN_DOWN_ACCELERATION
        } else {
            BUBBLE_COLUMN_ABOVE_UP_ACCELERATION
        };
        self.set_velocity(self.velocity() + DVec3::new(0.0, acceleration, 0.0));

        if let Some(world) = self.level() {
            world.send_bubble_column_particles(pos);
        }
    }

    /// Applies inside-bubble-column acceleration without clamping velocity.
    fn on_inside_bubble_column_projectile(&self, drag_down: bool) {
        let acceleration = if drag_down {
            -BUBBLE_COLUMN_DOWN_ACCELERATION
        } else {
            BUBBLE_COLUMN_INSIDE_UP_ACCELERATION
        };
        self.set_velocity(self.velocity() + DVec3::new(0.0, acceleration, 0.0));
        self.reset_fall_distance();
    }

    /// Sets the persisted owner UUID and clears the live owner cache.
    fn set_owner_uuid(&self, owner: Option<Uuid>) {
        self.projectile_base().state.lock().owner = owner.map(EntityReference::from_uuid);
    }

    /// Sets the owning entity and caches its live reference.
    fn set_owner_entity(&self, owner: Option<&SharedEntity>) {
        self.default_set_owner_entity(owner);
    }

    /// Base `setOwner` body for overrides that need the stored owner without
    /// their own side effects.
    fn default_set_owner_entity(&self, owner: Option<&SharedEntity>) {
        self.projectile_base().state.lock().owner = owner.map(EntityReference::from_entity);
    }

    /// Caches a live owner reference when it matches the saved owner UUID.
    fn cache_owner_entity(&self, owner: &SharedEntity) {
        if let Some(reference) = &self.projectile_base().state.lock().owner {
            reference.cache_entity(owner);
        }
    }

    /// Returns the owner UUID, if any.
    fn owner_uuid(&self) -> Option<Uuid> {
        self.projectile_base()
            .state
            .lock()
            .owner
            .as_ref()
            .map(EntityReference::uuid)
    }

    /// Returns the stored owner reference without resolving it to a live entity.
    fn owner_reference(&self) -> Option<EntityReference> {
        self.projectile_base().state.lock().owner.clone()
    }

    /// Resolves the cached owner, then looks up its UUID in the current domain.
    fn get_owner(&self) -> Option<SharedEntity> {
        let world = self.level()?;
        let owner = self.projectile_base().state.lock().owner.clone()?;
        owner.get_entity(&world)
    }

    /// Returns whether this projectile may interact with the block at `pos`:
    /// defers to the owning player's build permission, or the mobGriefing
    /// rule for a non-player owner.
    fn projectile_may_interact(&self, world: &World, pos: steel_utils::BlockPos) -> bool {
        let Some(owner) = self.get_owner() else {
            return true;
        };
        if owner.as_player().is_some() {
            return owner.may_interact(world, pos);
        }
        world.get_game_rule(&MOB_GRIEFING)
    }

    /// Returns whether this projectile type is allowed to break the blocks
    /// it hits, per the mobGriefing-gated `projectilesCanBreakBlocks` game rule.
    fn may_break(&self, world: &World) -> bool {
        REGISTRY
            .entity_types
            .is_in_tag(self.entity_type(), &EntityTypeTag::IMPACT_PROJECTILES)
            && world.get_game_rule(&PROJECTILES_CAN_BREAK_BLOCKS)
    }

    /// Returns whether `entity` is this projectile's owner.
    fn owned_by(&self, entity: &dyn Entity) -> bool {
        self.owner_uuid() == Some(entity.uuid())
    }

    /// Returns whether this projectile has been shot, as opposed to just spawned.
    fn has_been_shot(&self) -> bool {
        self.projectile_base().state.lock().has_been_shot
    }

    /// Marks whether this projectile has been shot.
    fn set_has_been_shot(&self, value: bool) {
        self.projectile_base().state.lock().has_been_shot = value;
    }

    /// Returns whether this projectile has moved outside its owner's collision range.
    fn left_owner(&self) -> bool {
        self.projectile_base().state.lock().left_owner
    }

    /// Updates whether this projectile has left its owner's collision range,
    /// but only the first time it's called each tick.
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

    /// Returns whether this projectile's expanded bounding box no longer
    /// overlaps its owner or any entity riding the same vehicle tree.
    fn is_outside_owner_collision_range(&self) -> bool {
        let Some(owner) = self.get_owner() else {
            return true;
        };
        let aabb = self
            .bounding_box()
            .expand_towards(self.velocity())
            .inflate(OWNER_COLLISION_SEARCH_INFLATE);
        let root_vehicle = owner.root_vehicle().unwrap_or_else(|| Arc::clone(&owner));
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

    /// Returns whether this projectile can hit `entity`.
    /// Use this if you don't need to override.
    fn can_hit_entity(&self, entity: &dyn Entity) -> bool {
        self.base_can_hit_entity(entity)
    }

    /// Base `canHitEntity` body for overrides that need the default checks.
    ///
    /// Split from [`Projectile::can_hit_entity`] because Rust has no `super`
    /// call, so an override cannot reach the base body otherwise.
    fn base_can_hit_entity(&self, entity: &dyn Entity) -> bool {
        if !entity.can_be_hit_by_projectile() {
            return false;
        }
        let Some(owner) = self.get_owner() else {
            return true;
        };
        self.left_owner() || !owner.is_passenger_of_same_vehicle(entity)
    }

    /// Returns the velocity vector for shooting toward `direction` at `power`,
    /// with random spread scaled by `uncertainty`.
    fn get_movement_to_shoot(&self, direction: DVec3, power: f32, uncertainty: f32) -> DVec3 {
        let deviation = SHOOT_INACCURACY_SCALE * f64::from(uncertainty);
        let jitter = DVec3::new(
            triangle_random(0.0, deviation),
            triangle_random(0.0, deviation),
            triangle_random(0.0, deviation),
        );
        (direction.normalize_or_zero() + jitter) * f64::from(power)
    }

    /// Sets this projectile's velocity and facing rotation toward `direction`
    /// at `power`, with random spread scaled by `uncertainty`.
    fn shoot(&self, direction: DVec3, power: f32, uncertainty: f32) {
        self.default_shoot(direction, power, uncertainty);
    }

    /// Base `shoot` body for overrides that need the launch without their own side
    /// effects.
    fn default_shoot(&self, direction: DVec3, power: f32, uncertainty: f32) {
        let movement = self.get_movement_to_shoot(direction, power, uncertainty);
        self.set_velocity(movement);
        self.mark_velocity_sync();

        let horizontal = (movement.x * movement.x + movement.z * movement.z).sqrt();
        let yaw = movement.x.atan2(movement.z).to_degrees() as f32;
        let pitch = movement.y.atan2(horizontal).to_degrees() as f32;
        self.set_rotation((yaw, pitch));
        self.base().set_old_rotation_to_current();
    }

    /// Shoots this projectile at `(x, y, z)`.
    fn shoot_at(&self, x: f64, y: f64, z: f64, power: f32, uncertainty: f32) {
        self.shoot(DVec3::new(x, y, z), power, uncertainty);
    }

    /// Shoots this projectile from `source`'s rotation (offset by `y_offset`),
    /// adding the source's own movement on top.
    fn shoot_from_rotation(
        &self,
        source: &dyn Entity,
        x_rot: f32,
        y_rot: f32,
        y_offset: f32,
        power: f32,
        uncertainty: f32,
    ) {
        let yaw = y_rot * DEG_TO_RAD;
        let pitch = x_rot * DEG_TO_RAD;
        let pitch_offset = (x_rot + y_offset) * DEG_TO_RAD;
        let direction = DVec3::new(
            f64::from(-trig::sin(yaw) * trig::cos(pitch)),
            f64::from(-trig::sin(pitch_offset)),
            f64::from(trig::cos(yaw) * trig::cos(pitch)),
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

    /// Runs `updateRotation` (lerped toward the movement vector).
    fn update_rotation(&self) {
        let movement = self.velocity();
        let horizontal = (movement.x * movement.x + movement.z * movement.z).sqrt();
        let (yaw_old, pitch_old) = self.base().old_rotation();
        let yaw = lerp_rotation(yaw_old, movement.x.atan2(movement.z).to_degrees() as f32);
        let pitch = lerp_rotation(pitch_old, movement.y.atan2(horizontal).to_degrees() as f32);
        self.set_rotation((yaw, pitch));
    }

    /// Applies a deflection and replaces the stored owner.
    fn deflect(
        &self,
        deflection: ProjectileDeflection,
        deflecting_entity: Option<&dyn Entity>,
        new_owner: Option<EntityReference>,
        by_attack: bool,
        power: DVec3,
    ) -> bool {
        deflection.apply(self.as_projectile_event_source(), deflecting_entity, power);
        self.projectile_base().state.lock().owner = new_owner;
        self.on_deflection(by_attack);
        true
    }

    /// Called after a successful deflection.
    fn on_deflection(&self, _by_attack: bool) {}

    /// Returns whether this projectile bounces from the world border.
    fn should_bounce_on_world_border(&self) -> bool {
        false
    }

    /// Casts the move vector and returns the nearest block or entity hit.
    fn get_hit_result_on_move_vector(&self) -> Option<ProjectileHit> {
        let world = self.level()?;
        let from = self.position();
        let delta = self.velocity();
        if delta.length_squared() < STOPPED_MOVEMENT_EPSILON {
            return None;
        }
        let to = from + delta;

        let block_hit =
            world.clip_including_border(from, to, ClipBlockShape::Collider, ClipFluid::None);
        let entity_end = if block_hit.is_miss() {
            to
        } else {
            block_hit.location
        };

        let search_box = self
            .bounding_box()
            .expand_towards(delta)
            .inflate(ENTITY_SEARCH_INFLATE);
        let margin = compute_margin(self.tick_count());
        let self_id = self.id();
        let entity_hit =
            get_entity_hit_result(world.as_ref(), from, entity_end, search_box, margin, |e| {
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

    /// Handles a projectile hit, deflecting it off an entity or the world
    /// border instead of resolving the hit when deflection applies.
    fn hit_target_or_deflect_self(self: Arc<Self>, hit: &ProjectileHit) -> ProjectileDeflection {
        if let ProjectileHit::Entity(entity_hit) = hit {
            let deflection = entity_hit
                .entity
                .deflection(self.as_projectile_event_source());
            if deflection != ProjectileDeflection::None {
                let already_deflected = self
                    .projectile_base()
                    .state
                    .lock()
                    .last_deflected_by
                    .as_ref()
                    .and_then(Weak::upgrade)
                    .is_some_and(|last| Arc::ptr_eq(&last, &entity_hit.entity));
                let owner = self.owner_reference();
                if !already_deflected
                    && self.deflect(
                        deflection,
                        Some(entity_hit.entity.as_ref()),
                        owner,
                        false,
                        DVec3::splat(DEFLECTION_POWER),
                    )
                {
                    self.projectile_base().state.lock().last_deflected_by =
                        Some(Arc::downgrade(&entity_hit.entity));
                }
                return deflection;
            }
        } else if let ProjectileHit::Block { hit, .. } = hit {
            if !hit.world_border_hit {
                // Vanilla skips the hit entirely when the collider clip reports a
                // position whose block state is air.
                if self
                    .level()
                    .is_some_and(|world| world.get_block_state(hit.block_pos).is_air())
                {
                    return ProjectileDeflection::None;
                }
            } else if self.should_bounce_on_world_border() {
                let deflection = ProjectileDeflection::Reverse;
                let owner = self.owner_reference();
                if self.deflect(
                    deflection,
                    None,
                    owner,
                    false,
                    DVec3::splat(HIT_DEFLECTION_POWER),
                ) {
                    return deflection;
                }
            }
        }

        self.on_hit(hit);
        ProjectileDeflection::None
    }

    /// Subclasses override this and call [`Projectile::projectile_on_hit`] for the
    /// base dispatch.
    fn on_hit(self: Arc<Self>, hit: &ProjectileHit) {
        self.projectile_on_hit(hit);
    }

    /// Base `onHit` dispatch to the block and entity handlers.
    ///
    /// Override [`Projectile::on_hit`] and delegate here instead.
    fn projectile_on_hit(self: Arc<Self>, hit: &ProjectileHit) {
        let world = self.level();
        match hit {
            ProjectileHit::Entity(entity_hit) => {
                if REGISTRY.entity_types.is_in_tag(
                    entity_hit.entity.entity_type(),
                    &EntityTypeTag::REDIRECTABLE_PROJECTILE,
                ) && let Some(projectile) = entity_hit.entity.as_projectile()
                {
                    let owner_entity = self.get_owner();
                    let owner = self.owner_reference();
                    projectile.deflect(
                        ProjectileDeflection::AimDeflect,
                        owner_entity.as_deref(),
                        owner,
                        true,
                        DVec3::splat(DEFLECTION_POWER),
                    );
                }
                Arc::clone(&self).on_hit_entity(&entity_hit.entity, entity_hit.location);
                if let Some(world) = world {
                    world.game_event_at(
                        &vanilla_game_events::PROJECTILE_LAND,
                        entity_hit.location,
                        &GameEventContext::new(Some(self.as_entity_event_source()), None),
                    );
                }
            }
            ProjectileHit::Block { hit, .. } => {
                Arc::clone(&self).on_hit_block(hit);
                if let Some(world) = world {
                    let state = world.get_block_state(hit.block_pos);
                    world.game_event(
                        &vanilla_game_events::PROJECTILE_LAND,
                        hit.block_pos,
                        &GameEventContext::new(Some(self.as_entity_event_source()), Some(state)),
                    );
                }
            }
        }
    }

    /// No-op by default.
    fn on_hit_entity(self: Arc<Self>, _entity: &SharedEntity, _location: DVec3) {}

    /// Default block-hit hook; concrete projectiles override this and call
    /// [`Projectile::projectile_on_hit_block`] to preserve the base dispatch.
    fn on_hit_block(self: Arc<Self>, hit: &ClipHitResult) {
        self.projectile_on_hit_block(hit);
    }

    /// The base `Projectile.onHitBlock` implementation. Concrete projectiles
    /// that override the hook call this to preserve the Java `super` dispatch.
    fn projectile_on_hit_block(&self, hit: &ClipHitResult) {
        let Some(world) = self.level() else {
            return;
        };
        let Some(projectile) = self.as_projectile() else {
            return;
        };
        let state = world.get_block_state(hit.block_pos);
        BLOCK_BEHAVIORS
            .get_behavior(state.get_block())
            .on_projectile_hit(state, &world, hit, projectile);
    }

    /// Marks hurt unless invulnerable to the base checks, but never takes damage.
    fn hurt(&self, _world: &World, source: &DamageSource, _amount: f32) -> bool {
        if !self.is_invulnerable_to_base(source) {
            self.mark_hurt();
        }
        false
    }

    /// Base tick reached from subclasses (`super.tick()`).
    fn projectile_base_tick(&self) {
        if !self.has_been_shot() {
            let owner = self.get_owner();
            self.game_event_with_source_entity(
                &vanilla_game_events::PROJECTILE_SHOOT,
                owner.as_deref(),
            );
            self.set_has_been_shot(true);
        }
        self.check_left_owner();
        self.default_tick();
        self.reset_left_owner_checked();
    }

    /// Overrides call [`Projectile::projectile_apply_on_projectile_spawned`] to
    /// run the base behavior.
    fn apply_on_projectile_spawned(&self, world: &World, pickup_item_stack: &mut ItemStack) {
        self.projectile_apply_on_projectile_spawned(world, pickup_item_stack);
    }

    /// Shared `Projectile.applyOnProjectileSpawned` body.
    ///
    /// The pickup stack breaking is ignored, matching vanilla's no-op break
    /// handler for this call.
    fn projectile_apply_on_projectile_spawned(
        &self,
        world: &World,
        pickup_item_stack: &mut ItemStack,
    ) {
        let owner = self.get_owner();
        let _ = enchantment_helper::on_projectile_spawned(
            world,
            pickup_item_stack,
            self.as_entity_event_source(),
            owner.as_ref(),
        );
    }

    /// Called when an enchantment effect breaks the item that spawned this
    /// projectile.
    fn on_item_break(&self, _item: &ItemStack) {}

    /// Saves `Owner`, `LeftOwner`, and `HasBeenShot`.
    fn save_projectile(&self, nbt: &mut NbtCompound) {
        let state = self.projectile_base().state.lock();
        if let Some(owner) = &state.owner {
            owner.store(nbt, "Owner");
        }
        if state.left_owner {
            nbt.insert("LeftOwner", 1i8);
        }
        nbt.insert("HasBeenShot", i8::from(state.has_been_shot));
    }

    /// Loads `Owner`, `LeftOwner`, and `HasBeenShot`.
    fn load_projectile(&self, nbt: BorrowedNbtCompoundView<'_, '_>) {
        let mut state = self.projectile_base().state.lock();
        state.owner = EntityReference::read(&nbt, "Owner");
        state.left_owner = nbt.byte_value("LeftOwner").is_some_and(|value| value != 0);
        state.has_been_shot = nbt
            .byte_value("HasBeenShot")
            .is_some_and(|value| value != 0);
    }
}

/// Spawns a throwable-item projectile from the player's eye along its look
/// direction and registers it in the world.
///
/// `create` receives the spawn position (eye minus the spawn offset) and must
/// return the concrete projectile type. Returns `None` when the world rejects the
/// projectile so the caller can fail the use.
#[must_use]
pub fn spawn_throwable_item_projectile<E>(
    world: &Arc<World>,
    player: &Arc<Player>,
    item_stack: &mut ItemStack,
    pitch_offset: f32,
    power: f32,
    uncertainty: f32,
    create: impl FnOnce(DVec3) -> E,
) -> Option<SharedEntity>
where
    E: Projectile + ThrowableItemProjectile + Entity + 'static,
{
    let player_pos = player.position();
    let spawn_pos = DVec3::new(
        player_pos.x,
        player.get_eye_y() - THROWN_ITEM_SPAWN_EYE_OFFSET,
        player_pos.z,
    );

    let entity = create(spawn_pos);
    let owner: SharedEntity = Arc::<Player>::clone(player);
    entity.set_owner_entity(Some(&owner));
    entity.set_item_clamped(item_stack.clone());

    let (yaw, player_pitch) = player.rotation();
    entity.shoot_from_rotation(
        player.as_ref(),
        player_pitch,
        yaw,
        pitch_offset,
        power,
        uncertainty,
    );

    let entity: SharedEntity = Arc::new(entity);
    if let Err(error) = world.try_add_entity(Arc::clone(&entity)) {
        log::debug!("failed to spawn throwable item projectile: {error}");
        return None;
    }
    if let Some(projectile) = entity.as_projectile() {
        projectile.apply_on_projectile_spawned(world, item_stack);
    }

    Some(entity)
}

/// Shoots the projectile, adds it to the world, then applies the spawn
/// enchantment effects to `pickup_item_stack`.
///
/// Returns `None` when the world rejects the projectile so the caller can fail
/// the attack.
#[must_use]
pub fn spawn_projectile(
    world: &Arc<World>,
    projectile: SharedEntity,
    pickup_item_stack: &mut ItemStack,
    shoot: impl FnOnce(&dyn Projectile),
) -> Option<SharedEntity> {
    let Some(projectile_capability) = projectile.as_projectile() else {
        log::debug!("spawn_projectile called with a non-projectile entity");
        return None;
    };
    shoot(projectile_capability);

    if let Err(error) = world.try_add_entity(Arc::clone(&projectile)) {
        log::debug!("failed to spawn projectile: {error}");
        return None;
    }
    if let Some(projectile_capability) = projectile.as_projectile() {
        projectile_capability.apply_on_projectile_spawned(world, pickup_item_stack);
    }

    Some(projectile)
}

/// Shoots a new projectile at `target`, adds it to the world, then applies the
/// spawn enchantment effects.
///
/// Returns `None` when the world rejects the projectile so the caller can fail
/// the attack.
#[must_use]
pub fn spawn_projectile_using_shoot<E>(
    world: &Arc<World>,
    pickup_item_stack: &mut ItemStack,
    create: impl FnOnce() -> E,
    target: DVec3,
    power: f32,
    uncertainty: f32,
) -> Option<SharedEntity>
where
    E: Projectile + 'static,
{
    let projectile: SharedEntity = Arc::new(create());
    spawn_projectile(world, projectile, pickup_item_stack, |projectile| {
        projectile.shoot_at(target.x, target.y, target.z, power, uncertainty);
    })
}

/// Ramps the entity hit margin from zero to the cap over the first flight ticks.
#[must_use]
pub fn compute_margin(tick_count: i32) -> f64 {
    let elapsed = (tick_count - ENTITY_HIT_MARGIN_START_TICKS) as f32;
    f64::from((elapsed / ENTITY_HIT_MARGIN_RAMP_TICKS).clamp(0.0, MAX_ENTITY_HIT_MARGIN))
}

/// Outcome of casting a projectile-style ray along an entity's view vector.
pub enum ViewVectorHitResult {
    /// No block or matching entity was hit within range.
    Miss,
    /// Nearest hit was a block collider (or world border).
    Block(ClipHitResult),
    /// Nearest hit was a matching entity, closer than any block along the ray.
    Entity(EntityHitResult),
}

/// Casts from the source eye along the look vector for `distance` blocks using
/// collider shapes, then prefers a matching entity hit over the block hit.
#[must_use]
pub fn get_hit_result_on_view_vector(
    world: &World,
    source: &dyn Entity,
    distance: f64,
    matching: impl Fn(&dyn Entity) -> bool,
) -> ViewVectorHitResult {
    let position = source.position();
    let from = DVec3::new(position.x, source.get_eye_y(), position.z);
    let delta = source.look_angle() * distance;
    let to = from + delta;

    let block_hit =
        world.clip_including_border(from, to, ClipBlockShape::Collider, ClipFluid::None);
    let entity_end = if block_hit.is_miss() {
        to
    } else {
        block_hit.location
    };

    let search_box = source
        .bounding_box()
        .expand_towards(delta)
        .inflate(ENTITY_SEARCH_INFLATE);
    let source_id = source.id();
    let entity_hit = get_entity_hit_result(world, from, entity_end, search_box, 0.0, |entity| {
        entity.id() != source_id && matching(entity)
    });

    if let Some(hit) = entity_hit {
        return ViewVectorHitResult::Entity(hit);
    }
    if !block_hit.is_miss() {
        return ViewVectorHitResult::Block(block_hit);
    }
    ViewVectorHitResult::Miss
}

/// Returns the nearest entity whose inflated box the segment `from -> to`
/// enters.
fn get_entity_hit_result(
    world: &World,
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

/// Returns every entity whose inflated box the segment `from -> to` enters.
///
/// Unlike [`get_entity_hit_result`], this returns all matching entities and
/// re-clips against the entity's surface when only the margin box was hit.
#[expect(
    clippy::too_many_arguments,
    reason = "mirrors the getManyEntityHitResult parameters"
)]
pub(crate) fn get_many_entity_hits(
    world: &World,
    source_id: i32,
    from: DVec3,
    to: DVec3,
    search_box: WorldAabb,
    margin: f64,
    predicate: impl Fn(&dyn Entity) -> bool,
    include_from_entity: bool,
    project_surface_hit_location: bool,
) -> Vec<EntityHitResult> {
    let mut hits = Vec::new();

    for entity in world.get_entities_in_aabb(&search_box) {
        if entity.id() == source_id || !predicate(entity.as_ref()) {
            continue;
        }

        let entity_box = entity.bounding_box();
        if include_from_entity && entity_box.contains(from) {
            hits.push(EntityHitResult {
                entity,
                location: from,
            });
            continue;
        }
        if let Some(location) = clip_segment(entity_box, from, to) {
            hits.push(EntityHitResult { entity, location });
            continue;
        }
        if margin <= 0.0 {
            continue;
        }

        let Some(outside_hit_position) = clip_segment(entity_box.inflate(margin), from, to) else {
            continue;
        };
        let mut towards_target = entity_box.center();
        let block_hit = world.clip_including_border(
            outside_hit_position,
            towards_target,
            ClipBlockShape::Collider,
            ClipFluid::None,
        );
        if !block_hit.is_miss() {
            towards_target = block_hit.location;
        }

        if let Some(surface_hit) = clip_segment(entity_box, outside_hit_position, towards_target) {
            hits.push(EntityHitResult {
                entity,
                location: if project_surface_hit_location {
                    surface_hit
                } else {
                    outside_hit_position
                },
            });
        }
    }

    hits
}

/// Clips the segment `from -> to` against `aabb`, returning the entry point.
///
/// The entry parameter is strictly positive, so a segment that starts inside or
/// exactly on the box never hits at its origin. This keeps a deflected
/// projectile resting on an entity's surface from re-hitting it every tick.
#[expect(
    clippy::too_many_lines,
    reason = "keeps the per-axis ordering explicit"
)]
fn clip_segment(aabb: WorldAabb, from: DVec3, to: DVec3) -> Option<DVec3> {
    const EPSILON: f64 = 1.0e-7;

    let delta = to - from;
    // Entry parameter of the nearest face crossed so far; starts at 1.0 and
    // only shrinks.
    let mut scale = 1.0_f64;
    let mut hit = false;

    let mut clip_point = |axis_delta: f64,
                          other_b: f64,
                          other_c: f64,
                          plane: f64,
                          min_b: f64,
                          max_b: f64,
                          min_c: f64,
                          max_c: f64,
                          from_axis: f64,
                          from_b: f64,
                          from_c: f64| {
        let s = (plane - from_axis) / axis_delta;
        let point_b = from_b + s * other_b;
        let point_c = from_c + s * other_c;
        if 0.0 < s
            && s < scale
            && min_b - EPSILON < point_b
            && point_b < max_b + EPSILON
            && min_c - EPSILON < point_c
            && point_c < max_c + EPSILON
        {
            scale = s;
            hit = true;
        }
    };

    if delta.x > EPSILON {
        clip_point(
            delta.x,
            delta.y,
            delta.z,
            aabb.min(Axis::X),
            aabb.min(Axis::Y),
            aabb.max(Axis::Y),
            aabb.min(Axis::Z),
            aabb.max(Axis::Z),
            from.x,
            from.y,
            from.z,
        );
    } else if delta.x < -EPSILON {
        clip_point(
            delta.x,
            delta.y,
            delta.z,
            aabb.max(Axis::X),
            aabb.min(Axis::Y),
            aabb.max(Axis::Y),
            aabb.min(Axis::Z),
            aabb.max(Axis::Z),
            from.x,
            from.y,
            from.z,
        );
    }

    if delta.y > EPSILON {
        clip_point(
            delta.y,
            delta.z,
            delta.x,
            aabb.min(Axis::Y),
            aabb.min(Axis::Z),
            aabb.max(Axis::Z),
            aabb.min(Axis::X),
            aabb.max(Axis::X),
            from.y,
            from.z,
            from.x,
        );
    } else if delta.y < -EPSILON {
        clip_point(
            delta.y,
            delta.z,
            delta.x,
            aabb.max(Axis::Y),
            aabb.min(Axis::Z),
            aabb.max(Axis::Z),
            aabb.min(Axis::X),
            aabb.max(Axis::X),
            from.y,
            from.z,
            from.x,
        );
    }

    if delta.z > EPSILON {
        clip_point(
            delta.z,
            delta.x,
            delta.y,
            aabb.min(Axis::Z),
            aabb.min(Axis::X),
            aabb.max(Axis::X),
            aabb.min(Axis::Y),
            aabb.max(Axis::Y),
            from.z,
            from.x,
            from.y,
        );
    } else if delta.z < -EPSILON {
        clip_point(
            delta.z,
            delta.x,
            delta.y,
            aabb.max(Axis::Z),
            aabb.min(Axis::X),
            aabb.max(Axis::X),
            aabb.min(Axis::Y),
            aabb.max(Axis::Y),
            from.z,
            from.x,
            from.y,
        );
    }

    hit.then(|| from + delta * scale)
}

/// Interpolates toward `rot` after wrapping the old angle into range.
pub(crate) fn lerp_rotation(mut rot_old: f32, rot: f32) -> f32 {
    while rot - rot_old < -DEGREE_180 {
        rot_old -= DEGREE_360;
    }
    while rot - rot_old >= DEGREE_180 {
        rot_old += DEGREE_360;
    }
    rot_old + ROTATION_LERP_FACTOR * (rot - rot_old)
}

#[cfg(test)]
mod tests {
    use super::*;
    use steel_registry::{
        blocks::properties::{BlockStateProperties, Tilt},
        entity_type::EntityTypeRef,
        init_vanilla_registry, vanilla_blocks, vanilla_entities,
    };
    use steel_utils::{
        BlockPos, ChunkPos, Direction,
        types::{GameType, UpdateFlags},
    };

    use crate::{
        behavior::init_behaviors,
        block_entity::init_block_entities,
        entity::{
            EntityBase,
            entities::{ArrowEntity, FireworkRocketEntity},
        },
        test_support::{
            TestPlayerBuilder, fresh_test_world, insert_ready_full_chunk, test_world,
            world_border_projectile_test_world,
        },
    };

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
                    Arc::downgrade(test_world()),
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
        entity_type: EntityTypeRef,
    }

    impl OwnerCollisionTestEntity {
        fn shared(id: i32, position: DVec3, pickable: bool) -> SharedEntity {
            Self::shared_with_type(id, position, pickable, &vanilla_entities::PIG)
        }

        fn shared_with_type(
            id: i32,
            position: DVec3,
            pickable: bool,
            entity_type: EntityTypeRef,
        ) -> SharedEntity {
            Arc::new(Self {
                base: EntityBase::new(
                    id,
                    position,
                    entity_type.dimensions,
                    Arc::downgrade(test_world()),
                ),
                pickable,
                entity_type,
            })
        }
    }

    crate::entity::impl_test_downcast_type!(OwnerCollisionTestEntity);

    impl Entity for OwnerCollisionTestEntity {
        fn base(&self) -> &EntityBase {
            &self.base
        }

        fn entity_type(&self) -> EntityTypeRef {
            self.entity_type
        }

        fn is_pickable(&self) -> bool {
            self.pickable && !self.is_removed()
        }
    }

    #[test]
    fn compute_margin_ramps_from_zero_to_cap() {
        assert!((compute_margin(2) - 0.0).abs() < 1.0e-9);
        assert!((compute_margin(7) - 0.25).abs() < 1.0e-9);
        assert!((compute_margin(100) - 0.3).abs() < 1.0e-6);
    }

    #[test]
    fn compute_margin_matches_vanilla_float_widening() {
        assert_eq!(compute_margin(3), f64::from(0.05_f32));
        assert_eq!(compute_margin(100), f64::from(0.3_f32));
        assert_ne!(compute_margin(100), 0.3);
    }

    #[test]
    fn clip_segment_hits_box_in_path() {
        let aabb = WorldAabb::new(4.0, -0.5, -0.5, 5.0, 0.5, 0.5);
        let hit = clip_segment(aabb, DVec3::ZERO, DVec3::new(10.0, 0.0, 0.0))
            .expect("ray along +x should enter the box");
        assert!((hit.x - 4.0).abs() < 1.0e-6);
    }

    #[test]
    fn clip_segment_ignores_segment_starting_inside() {
        let aabb = WorldAabb::new(-1.0, -1.0, -1.0, 1.0, 1.0, 1.0);
        let hit = clip_segment(aabb, DVec3::ZERO, DVec3::new(0.0, 5.0, 0.0));
        assert_eq!(hit, None);
    }

    #[test]
    fn clip_segment_ignores_segment_starting_on_surface() {
        let aabb = WorldAabb::new(-1.0, -1.0, -1.0, 1.0, 1.0, 1.0);
        // Exactly on the +z face, moving away from the box.
        assert_eq!(
            clip_segment(aabb, DVec3::new(0.0, 0.0, 1.0), DVec3::new(0.0, 0.0, 2.0)),
            None
        );
        // Exactly on the +z face, moving into the box: vanilla only tests the
        // entry face for the direction of travel, which is behind the origin.
        assert_eq!(
            clip_segment(aabb, DVec3::new(0.0, 0.0, 1.0), DVec3::new(0.0, 0.0, 0.0)),
            None
        );
    }

    #[test]
    fn may_break_requires_impact_projectile_tag() {
        init_vanilla_registry();

        let firework = FireworkRocketEntity::new(
            &vanilla_entities::FIREWORK_ROCKET,
            1,
            DVec3::ZERO,
            Weak::new(),
        );
        let ender_pearl = OwnerCollisionProjectile::new(2, DVec3::ZERO);

        assert!(firework.may_break(test_world()));
        assert!(!ender_pearl.may_break(test_world()));
    }

    #[test]
    fn move_vector_synthesizes_world_border_block_hit() {
        init_vanilla_registry();
        init_behaviors();

        let world = Arc::clone(world_border_projectile_test_world());
        let firework = FireworkRocketEntity::new(
            &vanilla_entities::FIREWORK_ROCKET,
            3,
            DVec3::new(4.5, 64.0, 0.0),
            Arc::downgrade(&world),
        );
        firework.set_velocity(DVec3::X);

        let Some(ProjectileHit::Block { location, hit }) = firework.get_hit_result_on_move_vector()
        else {
            panic!("firework should hit the world border");
        };

        assert!(hit.world_border_hit);
        assert!(!hit.inside);
        assert_eq!(hit.direction, Direction::East);
        assert_eq!(hit.block_pos, BlockPos::new(4, 64, 0));
        assert_eq!(location, hit.location);
        assert!(location.x < 5.0 && location.x > 4.999);
    }

    #[test]
    fn firework_deflects_without_exploding_on_deflecting_entity() {
        init_vanilla_registry();

        let world = Arc::clone(test_world());
        let owner = OwnerCollisionTestEntity::shared(6, DVec3::ZERO, true);
        let replacement: SharedEntity =
            TestPlayerBuilder::new(Arc::clone(&world), "Replacement", 7)
                .uuid(owner.uuid())
                .build();
        for cached in [false, true] {
            let firework = Arc::new(FireworkRocketEntity::new(
                &vanilla_entities::FIREWORK_ROCKET,
                4,
                DVec3::ZERO,
                Arc::downgrade(&world),
            ));
            if cached {
                firework.set_owner_entity(Some(&owner));
            } else {
                firework.set_owner_uuid(Some(owner.uuid()));
            }
            let reference = firework
                .projectile_base()
                .state
                .lock()
                .owner
                .clone()
                .expect("stored owner reference");
            firework.set_velocity(DVec3::X);
            let deflector = OwnerCollisionTestEntity::shared_with_type(
                5,
                DVec3::X,
                true,
                &vanilla_entities::BREEZE,
            );

            let deflection = Arc::clone(&firework).hit_target_or_deflect_self(
                &ProjectileHit::Entity(EntityHitResult {
                    entity: deflector,
                    location: DVec3::X,
                }),
            );

            assert_eq!(deflection, ProjectileDeflection::Reverse);
            assert_eq!(firework.velocity(), DVec3::new(-0.5, 0.0, 0.0));
            assert!(firework.needs_velocity_sync());
            assert!(!firework.is_removed());
            assert_eq!(firework.owner_uuid(), Some(owner.uuid()));
            if cached {
                assert!(Arc::ptr_eq(
                    &firework.get_owner().expect("cached owner"),
                    &owner
                ));
            } else {
                assert!(firework.get_owner().is_none());
            }
            reference.cache_entity(&replacement);
            assert!(Arc::ptr_eq(
                &firework.get_owner().expect("shared owner resolution"),
                &replacement
            ));
        }
    }

    #[test]
    fn base_block_hit_dispatches_vanilla_block_callbacks() {
        init_vanilla_registry();
        init_block_entities();
        init_behaviors();

        let world = Arc::clone(test_world());
        let chunk_map = Arc::clone(&world.chunk_map);
        let pos = BlockPos::new(1_136, 64, 1_136);
        let block_callback_test =
            chunk_map.with_full_chunks_in_radius(ChunkPos::from_block_pos(pos), 0, || {
                let unlit = vanilla_blocks::CANDLE
                    .default_state()
                    .set_value(&BlockStateProperties::LIT, false);
                assert!(world.set_block(pos, unlit, UpdateFlags::UPDATE_ALL));

                let firework = FireworkRocketEntity::new(
                    &vanilla_entities::FIREWORK_ROCKET,
                    3,
                    DVec3::new(1_136.5, 64.5, 1_136.5),
                    Arc::downgrade(&world),
                );
                firework.set_remaining_fire_ticks(1);
                firework.projectile_on_hit_block(&ClipHitResult {
                    location: firework.position(),
                    direction: Direction::Up,
                    block_pos: pos,
                    miss: false,
                    inside: false,
                    world_border_hit: false,
                });

                assert!(
                    world
                        .get_block_state(pos)
                        .get_value(&BlockStateProperties::LIT)
                );

                let campfire_pos = pos.offset(1, 0, 0);
                let unlit_campfire = vanilla_blocks::CAMPFIRE
                    .default_state()
                    .set_value(&BlockStateProperties::LIT, false)
                    .set_value(&BlockStateProperties::WATERLOGGED, false);
                assert!(
                    world.set_block(campfire_pos, unlit_campfire, UpdateFlags::UPDATE_CLIENTS,)
                );
                firework.projectile_on_hit_block(&ClipHitResult {
                    location: firework.position(),
                    direction: Direction::Up,
                    block_pos: campfire_pos,
                    miss: false,
                    inside: false,
                    world_border_hit: false,
                });
                assert!(
                    world
                        .get_block_state(campfire_pos)
                        .get_value(&BlockStateProperties::LIT)
                );

                let dripleaf_pos = pos.offset(2, 0, 0);
                assert!(world.set_block(
                    dripleaf_pos,
                    vanilla_blocks::BIG_DRIPLEAF.default_state(),
                    UpdateFlags::UPDATE_CLIENTS,
                ));
                firework.projectile_on_hit_block(&ClipHitResult {
                    location: firework.position(),
                    direction: Direction::Up,
                    block_pos: dripleaf_pos,
                    miss: false,
                    inside: false,
                    world_border_hit: false,
                });
                assert_eq!(
                    world
                        .get_block_state(dripleaf_pos)
                        .get_value(&BlockStateProperties::TILT),
                    Tilt::Full
                );

                let chorus_pos = pos.offset(3, 0, 0);
                assert!(world.set_block(
                    chorus_pos,
                    vanilla_blocks::CHORUS_FLOWER.default_state(),
                    UpdateFlags::UPDATE_CLIENTS,
                ));
                firework.projectile_on_hit_block(&ClipHitResult {
                    location: firework.position(),
                    direction: Direction::Up,
                    block_pos: chorus_pos,
                    miss: false,
                    inside: false,
                    world_border_hit: false,
                });
                assert!(world.get_block_state(chorus_pos).is_air());
            });
        let result = chunk_map.chunk_runtime.block_on(block_callback_test);

        assert_eq!(result, Some(()));
    }

    #[test]
    fn owner_collision_range_checks_root_vehicle_passengers() {
        init_vanilla_registry();

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
        init_vanilla_registry();

        let projectile = OwnerCollisionProjectile::new(1, DVec3::ZERO);
        let owner = OwnerCollisionTestEntity::shared(2, DVec3::new(10.0, 0.0, 0.0), true);
        let vehicle = OwnerCollisionTestEntity::shared(3, DVec3::new(10.0, 0.0, 0.0), true);
        let passenger = OwnerCollisionTestEntity::shared(4, DVec3::ZERO, false);
        EntityBase::restore_passenger_relationship(&vehicle, &owner);
        EntityBase::restore_passenger_relationship(&vehicle, &passenger);

        projectile.set_owner_entity(Some(&owner));

        assert!(projectile.is_outside_owner_collision_range());
    }

    #[test]
    fn deflected_arrow_leaves_an_invulnerable_creative_player() {
        init_vanilla_registry();
        init_behaviors();

        let world_fixture = fresh_test_world("deflected_arrow_creative_player");
        let world = &world_fixture.world;
        insert_ready_full_chunk(world, ChunkPos::new(0, 0));

        let player = TestPlayerBuilder::new(Arc::clone(world), "Target", 1).build();
        player.base().set_position_local(DVec3::new(0.0, 64.0, 0.0));
        player.set_client_loaded(true);
        player.restore_game_modes(GameType::Creative, None);
        player
            .abilities
            .lock()
            .update_for_game_mode(GameType::Creative);
        world
            .try_add_entity(Arc::clone(&player) as SharedEntity)
            .expect("player should attach to the loaded chunk");

        // Flying along -z toward the player's +z face at z = 0.3.
        let arrow = Arc::new(ArrowEntity::new(
            &vanilla_entities::ARROW,
            2,
            DVec3::new(0.0, 64.9, 0.5),
            Arc::downgrade(world),
        ));
        arrow.set_velocity(DVec3::new(0.0, 0.0, -0.5));
        world
            .try_add_entity(Arc::clone(&arrow) as SharedEntity)
            .expect("arrow should attach to the loaded chunk");

        let health_before = player.get_health();

        Entity::tick(Arc::clone(&arrow));
        assert!(
            arrow.velocity().z > 0.0,
            "the creative player must deflect the arrow instead of taking the hit"
        );
        let surface_z = arrow.position().z;
        assert!(
            (0.29..=0.31).contains(&surface_z),
            "arrow should stop on the player's surface, got {surface_z}"
        );

        // A deflected arrow resting on the surface must keep moving away. If
        // clipping treats the surface as a new entry hit, it is snapped back
        // every tick and hovers on the player.
        for _ in 0..5 {
            Entity::tick(Arc::clone(&arrow));
        }

        assert!(
            arrow.position().z > surface_z + 0.05,
            "arrow stayed on the player's surface at z={}",
            arrow.position().z
        );
        assert!(!arrow.is_removed());
        assert_eq!(player.get_health(), health_before);
    }
}
