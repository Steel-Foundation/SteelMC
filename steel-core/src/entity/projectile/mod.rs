//! Vanilla `Projectile`: entities with an owner that resolve hits along
//! their movement.
//!
//! State lives in the [`ProjectileBase`] component and behavior on the
//! [`Projectile`] view; per-type hooks are [`ProjectileProps`]. The
//! `ThrowableProjectile`, `ThrowableItemProjectile` and
//! `AbstractThrownPotion` layers are the [`throwable`], [`throwable_item`] and
//! [`thrown_potion`] modules.
//!
//! The block + entity move-vector raycast mirrors `ProjectileUtil`.

pub mod throwable;
pub mod throwable_item;
pub mod thrown_potion;

use crate::entity::entity;
use crate::entity::prelude::*;
use std::ops::Deref;
use std::sync::{Arc, Weak};
use steel_utils::nbt::NbtValueInput as _;

use glam::DVec3;
use simdnbt::borrow::NbtCompound as BorrowedNbtCompoundView;
use simdnbt::owned::NbtCompound;
use steel_math::{DEG_TO_RAD, DEGREE_180, DEGREE_360, trig};
use steel_registry::blocks::block_state_ext::BlockStateExt as _;
use steel_registry::item_stack::ItemStack;
use steel_registry::vanilla_entity_type_tags::EntityTypeTag;
use steel_registry::vanilla_game_rules::{MOB_GRIEFING, PROJECTILES_CAN_BREAK_BLOCKS};
use steel_registry::{REGISTRY, TaggedRegistryExt as _, vanilla_damage_types, vanilla_game_events};
use steel_utils::axis::Axis;
use steel_utils::locks::{CheckedMutex, CheckedMutexGuard};
use steel_utils::{BlockPos, DowncastType, DowncastTypeKey, WorldAabb};
use uuid::Uuid;

use crate::behavior::BLOCK_BEHAVIORS;
use crate::enchantment_helper;
use crate::entity::component::props::entity_props;
use crate::entity::component::{Component, Components as _, Has};
use crate::entity::damage::DamageSource;
use crate::entity::{Entity, EntityProps, EntityReference, Living, SharedEntity};
use crate::player::Player;
use crate::world::game_event::GameEventContext;
use crate::world::{ClipBlockShape, ClipFluid, ClipHitResult, World};

use super::{
    BUBBLE_COLUMN_ABOVE_UP_ACCELERATION, BUBBLE_COLUMN_DOWN_ACCELERATION,
    BUBBLE_COLUMN_INSIDE_UP_ACCELERATION,
};

pub use thrown_potion::{SPLASH_HORIZONTAL_REACH, SPLASH_RANGE_SQ, SPLASH_VERTICAL_REACH};

/// Returns a triangle-distributed random value.
#[must_use]
pub fn triangle_random(mode: f64, deviation: f64) -> f64 {
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

/// Vanilla projectile deflection behaviors (`ProjectileDeflection`).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProjectileDeflection {
    /// Continue with normal hit handling.
    None,
    /// Reverse the projectile with Vanilla's randomized yaw adjustment.
    Reverse,
    /// Redirect the projectile along the deflecting entity's look direction.
    AimDeflect,
    /// Redirect the projectile along the deflecting entity's movement direction.
    MomentumDeflect,
}

impl ProjectileDeflection {
    fn apply(self, projectile: &Projectile<'_>, deflecting_entity: Option<&dyn Entity>) {
        match self {
            Self::None => {}
            Self::Reverse => {
                let rotation = rand::random_range(170.0..190.0);
                projectile.set_velocity(projectile.velocity() * -0.5);
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
                projectile.set_velocity(entity.look_angle());
                projectile.mark_velocity_sync();
            }
            Self::MomentumDeflect => {
                let Some(entity) = deflecting_entity else {
                    return;
                };
                let movement = entity.known_speed();
                let length = movement.length();
                let normalized = if length < 1.0e-5 {
                    DVec3::ZERO
                } else {
                    movement / length
                };
                projectile.set_velocity(normalized);
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

/// State of a vanilla `Projectile`: its owner and whether it has been shot
/// and has left its owner.
pub struct ProjectileBase {
    state: CheckedMutex<ProjectileState>,
}

impl ProjectileBase {
    /// A projectile with no owner that has not been shot.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            state: CheckedMutex::new(ProjectileState {
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

// SAFETY: This key is owned by Steel and uniquely identifies `ProjectileBase`.
unsafe impl DowncastType for ProjectileBase {
    const TYPE_KEY: DowncastTypeKey = DowncastTypeKey::new("steel:entity_component/projectile");
}

impl Component for ProjectileBase {
    fn save(&self, _owner: &dyn Entity, nbt: &mut NbtCompound) {
        let state = self.state.lock();
        if let Some(owner) = &state.owner {
            owner.store(nbt, "Owner");
        }
        if state.left_owner {
            nbt.insert("LeftOwner", 1i8);
        }
        nbt.insert("HasBeenShot", i8::from(state.has_been_shot));
    }

    fn load(&self, _owner: &dyn Entity, nbt: BorrowedNbtCompoundView<'_, '_>) {
        let mut state = self.state.lock();
        state.owner = EntityReference::read(&nbt, "Owner");
        state.left_owner = nbt.byte_value("LeftOwner").is_some_and(|value| value != 0);
        state.has_been_shot = nbt
            .byte_value("HasBeenShot")
            .is_some_and(|value| value != 0);
    }
}

entity_props! {
    /// Per-type properties of a vanilla `Projectile`.
    pub struct ProjectileProps<E>: ProjectileHooks for Projectile
    where E: Parts,
    table ProjectileTable::PROJECTILE,
    component ProjectileBase {
        /// `Projectile.onDeflection`.
        fn on_deflection(by_attack: bool);
        /// `Projectile.shouldBounceOnWorldBorder`.
        fn should_bounce_on_world_border() -> bool;
        /// `Projectile.calculateHorizontalHurtKnockbackDirection`.
        fn horizontal_hurt_knockback_direction(hurt_entity: &Living<'_>, source: &DamageSource) -> (f64, f64);
        /// `Projectile.onHit`: dispatches to the entity or block hit.
        fn on_hit(hit: &ProjectileHit);
        /// `Projectile.onHitEntity`.
        fn on_hit_entity(target: &SharedEntity, location: DVec3);
        /// `Projectile.onHitBlock`: the block's projectile reaction.
        fn on_hit_block(hit: &ClipHitResult);
        /// `Projectile.canHitEntity`.
        fn can_hit_entity(target: &dyn Entity) -> bool;
    }
}

/// An entity seen as a vanilla `Projectile`.
#[derive(Clone, Copy)]
pub struct Projectile<'a> {
    entity: &'a (dyn Entity + 'static),
    base: &'a ProjectileBase,
    hooks: &'a dyn ProjectileHooks,
}

impl<'a> Projectile<'a> {
    /// Pairs a concrete projectile with its state.
    #[must_use]
    pub fn new<E: ProjectileTable>(entity: &'a E) -> Self {
        Self {
            entity,
            base: entity.get::<ProjectileBase>(),
            hooks: entity,
        }
    }

    /// Returns `entity` as a projectile, if it is one.
    #[must_use]
    pub fn of(entity: &'a (dyn Entity + 'static)) -> Option<Self> {
        let hooks = entity.as_projectile_hooks()?;
        Some(Self {
            entity,
            base: hooks.layer_component(),
            hooks,
        })
    }

    /// Returns the entity this view belongs to.
    #[must_use]
    pub const fn entity(&self) -> &'a (dyn Entity + 'static) {
        self.entity
    }
}

impl Deref for Projectile<'_> {
    type Target = dyn Entity;

    fn deref(&self) -> &Self::Target {
        self.entity
    }
}

/// A concrete projectile type: its component and its hook table.
pub trait ProjectileTable: Entity + Has<ProjectileBase> + Sized {
    /// The `Projectile` hooks this type runs, built on its layer's preset.
    const PROJECTILE: ProjectileProps<Self>;
}

/// Every projectile type; keeps generic bounds short.
pub trait Parts: ProjectileTable {
    /// Returns this entity as a projectile.
    fn projectile(&self) -> Projectile<'_>;
}

impl<E: ProjectileTable> Parts for E {
    fn projectile(&self) -> Projectile<'_> {
        Projectile::new(self)
    }
}

/// `Projectile`'s [`EntityProps::tick`].
///
/// `Projectile.tick` around its `super.tick()`: announces the shot once
/// and tracks leaving the owner.
pub fn tick<E: Parts>(entity: &E) {
    let projectile = &entity.projectile();
    if !projectile.has_been_shot() {
        let owner = projectile.owner();
        projectile.game_event_with_source_entity(
            &vanilla_game_events::PROJECTILE_SHOOT,
            owner.as_deref(),
        );
        projectile.state().has_been_shot = true;
    }
    projectile.check_left_owner();
    entity::tick(entity);
    projectile.state().left_owner_checked = false;
}

/// `Projectile`'s [`EntityProps::hurt`].
///
/// `Projectile.hurtServer`: marks hurt unless invulnerable to the base
/// checks, but never takes damage.
pub fn hurt<E: Parts>(entity: &E, _: &World, source: &DamageSource, _: f32) -> bool {
    let projectile = &entity.projectile();
    if !projectile.is_invulnerable_to_base(source) {
        projectile.mark_hurt();
    }
    false
}

/// `Projectile`'s [`EntityProps::is_pickable`].
pub fn is_pickable<E: Parts>(projectile: &E) -> bool {
    REGISTRY.entity_types.is_in_tag(
        projectile.entity_type(),
        &EntityTypeTag::REDIRECTABLE_PROJECTILE,
    )
}

/// `Projectile`'s [`EntityProps::dimension_changing_delay`].
#[must_use]
pub const fn dimension_changing_delay<E: Parts>(_: &E) -> i32 {
    2
}

/// `Projectile`'s [`EntityProps::may_interact`].
pub fn may_interact<E: Parts>(projectile: &E, world: &World, pos: BlockPos) -> bool {
    projectile.projectile().may_interact(world, pos)
}

/// `Projectile`'s [`EntityProps::on_above_bubble_column`].
pub fn on_above_bubble_column<E: Parts>(projectile: &E, drag_down: bool, pos: BlockPos) {
    projectile
        .projectile()
        .on_above_bubble_column(drag_down, pos);
}

/// `Projectile`'s [`EntityProps::on_inside_bubble_column`].
pub fn on_inside_bubble_column<E: Parts>(projectile: &E, drag_down: bool) {
    projectile.projectile().on_inside_bubble_column(drag_down);
}

/// `Projectile`'s [`EntityProps::spawn_data`].
pub fn spawn_data<E: Parts>(projectile: &E) -> i32 {
    projectile
        .projectile()
        .owner()
        .map_or(0, |owner| owner.id())
}

/// `Projectile`'s [`EntityProps::restore_owner_reference`].
pub fn restore_owner_reference<E: Parts>(projectile: &E, owner: &SharedEntity) {
    projectile.projectile().cache_owner(owner);
}

/// `Projectile`'s overrides of the entity hooks.
#[must_use]
pub const fn entity_props<E: Parts>() -> EntityProps<E> {
    EntityProps {
        tick: tick::<E>,
        hurt: hurt::<E>,
        is_pickable: is_pickable::<E>,
        dimension_changing_delay: dimension_changing_delay::<E>,
        may_interact: may_interact::<E>,
        on_above_bubble_column: on_above_bubble_column::<E>,
        on_inside_bubble_column: on_inside_bubble_column::<E>,
        spawn_data: spawn_data::<E>,
        restore_owner_reference: restore_owner_reference::<E>,
        ..EntityProps::new()
    }
}

impl<'a> Projectile<'a> {
    fn state(&self) -> CheckedMutexGuard<'a, ProjectileState> {
        self.base.state.lock()
    }

    /// Sets the owner from a saved UUID, clearing any cached live owner.
    pub fn set_owner_uuid(&self, owner: Option<Uuid>) {
        self.state().owner = owner.map(EntityReference::from_uuid);
    }

    /// Sets the owning entity.
    pub fn set_owner(&self, owner: Option<&SharedEntity>) {
        self.state().owner = owner.map(EntityReference::from_entity);
    }

    /// Caches a live owner when it matches the saved owner UUID.
    pub fn cache_owner(&self, owner: &SharedEntity) {
        if let Some(reference) = &self.state().owner {
            reference.cache_entity(owner);
        }
    }

    /// The owner's UUID, if any.
    #[must_use]
    pub fn owner_uuid(&self) -> Option<Uuid> {
        self.state().owner.as_ref().map(EntityReference::uuid)
    }

    /// The live owner: the cached entity, or the owner UUID looked up in the
    /// current domain.
    #[must_use]
    pub fn owner(&self) -> Option<SharedEntity> {
        let world = self.level()?;
        let owner = self.state().owner.clone()?;
        owner.get_entity(&world)
    }

    /// `DamageSources.thrown`: damage from this projectile, caused by its
    /// owner.
    #[must_use]
    pub fn thrown_damage(&self) -> Option<DamageSource> {
        let damage = DamageSource::environment(&vanilla_damage_types::THROWN)
            .with_direct_entity(self.shared()?);
        Some(match self.owner() {
            Some(owner) => damage.with_causing_entity(owner),
            None => damage,
        })
    }

    /// `Projectile.mayInteract`: the owning player's build permission, or the
    /// mobGriefing rule for a non-player owner.
    #[must_use]
    pub fn may_interact(&self, world: &World, pos: BlockPos) -> bool {
        let Some(owner) = self.owner() else {
            return true;
        };
        if owner.as_player().is_some() {
            return owner.may_interact(world, pos);
        }
        world.get_game_rule(&MOB_GRIEFING)
    }

    /// Whether this projectile may break the blocks it hits.
    // TODO: check the `can_break` component (`canBreakBlockInAdventureMode`).
    #[must_use]
    pub fn may_break(&self, world: &World) -> bool {
        REGISTRY
            .entity_types
            .is_in_tag(self.entity_type(), &EntityTypeTag::IMPACT_PROJECTILES)
            && world.get_game_rule(&PROJECTILES_CAN_BREAK_BLOCKS)
    }

    /// Returns whether `entity` is this projectile's owner.
    #[must_use]
    pub fn owned_by(&self, entity: &dyn Entity) -> bool {
        self.owner_uuid() == Some(entity.uuid())
    }

    /// Whether this projectile has been shot, as opposed to just spawned.
    #[must_use]
    pub fn has_been_shot(&self) -> bool {
        self.state().has_been_shot
    }

    /// Whether this projectile has moved out of its owner's collision range.
    #[must_use]
    pub fn left_owner(&self) -> bool {
        self.state().left_owner
    }

    /// Updates [`Self::left_owner`], at most once per tick.
    fn check_left_owner(&self) {
        {
            let mut state = self.state();
            if state.left_owner || state.left_owner_checked {
                return;
            }
            state.left_owner_checked = true;
        }
        let left = self.is_outside_owner_collision_range();
        self.state().left_owner = left;
    }

    /// Returns whether this projectile's expanded bounding box no longer
    /// overlaps its owner or any entity riding the same vehicle tree.
    #[must_use]
    pub fn is_outside_owner_collision_range(&self) -> bool {
        let Some(owner) = self.owner() else {
            return true;
        };
        let aabb = self
            .bounding_box()
            .expand_towards(self.velocity())
            .inflate(1.0);
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

    /// The velocity for shooting toward `direction` at `power`, with random
    /// spread scaled by `uncertainty`.
    #[must_use]
    pub fn movement_to_shoot(direction: DVec3, power: f32, uncertainty: f32) -> DVec3 {
        let deviation = 0.0172_275 * f64::from(uncertainty);
        let jitter = DVec3::new(
            triangle_random(0.0, deviation),
            triangle_random(0.0, deviation),
            triangle_random(0.0, deviation),
        );
        (direction.normalize_or_zero() + jitter) * f64::from(power)
    }

    /// Sets this projectile's velocity and facing toward `direction` at
    /// `power`, with random spread scaled by `uncertainty`.
    pub fn shoot(&self, direction: DVec3, power: f32, uncertainty: f32) {
        let movement = Self::movement_to_shoot(direction, power, uncertainty);
        self.set_velocity(movement);
        self.mark_velocity_sync();

        let horizontal = (movement.x * movement.x + movement.z * movement.z).sqrt();
        let yaw = movement.x.atan2(movement.z).to_degrees() as f32;
        let pitch = movement.y.atan2(horizontal).to_degrees() as f32;
        self.set_rotation((yaw, pitch));
        self.base().set_old_rotation_to_current();
    }

    /// Shoots this projectile from `source`'s rotation (offset by `y_offset`),
    /// adding the source's own movement on top.
    pub fn shoot_from_rotation(
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

    /// `Projectile.updateRotation`: lerps the rotation toward the movement.
    pub fn update_rotation(&self) {
        let movement = self.velocity();
        let horizontal = (movement.x * movement.x + movement.z * movement.z).sqrt();
        let (yaw_old, pitch_old) = self.base().old_rotation();
        let yaw = lerp_rotation(yaw_old, movement.x.atan2(movement.z).to_degrees() as f32);
        let pitch = lerp_rotation(pitch_old, movement.y.atan2(horizontal).to_degrees() as f32);
        self.set_rotation((yaw, pitch));
    }

    /// Deflects this projectile and hands it to `new_owner`.
    pub fn deflect(
        &self,
        deflection: ProjectileDeflection,
        deflecting_entity: Option<&dyn Entity>,
        new_owner: Option<EntityReference>,
        by_attack: bool,
    ) {
        deflection.apply(self, deflecting_entity);
        self.state().owner = new_owner;
        self.on_deflection(by_attack);
    }

    /// `ProjectileUtil.getHitResultOnMoveVector` with this projectile's
    /// `canHitEntity`: the nearest block or entity on the move vector.
    #[must_use]
    pub fn hit_result_on_move_vector(&self) -> Option<ProjectileHit> {
        const EPSILON: f64 = 1.0e-12;
        let world = self.level()?;
        let from = self.position();
        let delta = self.velocity();
        if delta.length_squared() < EPSILON {
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

        let search_box = self.bounding_box().expand_towards(delta).inflate(1.0);
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

    /// `Projectile.hitTargetOrDeflectSelf`: deflects this projectile off an
    /// entity or the world border, or resolves the hit.
    pub fn hit_target_or_deflect_self(&self, hit: &ProjectileHit) -> ProjectileDeflection {
        if let ProjectileHit::Entity(entity_hit) = hit {
            let deflection = entity_hit.entity.deflection(self);
            if deflection != ProjectileDeflection::None {
                let already_deflected = self
                    .state()
                    .last_deflected_by
                    .as_ref()
                    .and_then(Weak::upgrade)
                    .is_some_and(|last| Arc::ptr_eq(&last, &entity_hit.entity));
                let owner = self.state().owner.clone();
                if !already_deflected {
                    self.deflect(deflection, Some(entity_hit.entity.as_ref()), owner, false);
                    self.state().last_deflected_by = Some(Arc::downgrade(&entity_hit.entity));
                }
                return deflection;
            }
        } else if let ProjectileHit::Block { hit, .. } = hit
            && self.should_bounce_on_world_border()
            && hit.world_border_hit
        {
            let deflection = ProjectileDeflection::Reverse;
            let owner = self.state().owner.clone();
            self.deflect(deflection, None, owner, false);
            self.set_velocity(self.velocity() * 0.2);
            return deflection;
        }

        self.on_hit(hit);
        ProjectileDeflection::None
    }

    /// `Projectile.onAboveBubbleColumn`: unlike other entities, projectiles
    /// accelerate without a speed cap.
    fn on_above_bubble_column(&self, drag_down: bool, pos: BlockPos) {
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

    /// `Projectile.onInsideBubbleColumn`: accelerates without a speed cap.
    fn on_inside_bubble_column(&self, drag_down: bool) {
        let acceleration = if drag_down {
            -BUBBLE_COLUMN_DOWN_ACCELERATION
        } else {
            BUBBLE_COLUMN_INSIDE_UP_ACCELERATION
        };
        self.set_velocity(self.velocity() + DVec3::new(0.0, acceleration, 0.0));
        self.reset_fall_distance();
    }
}

/// `Projectile`'s [`ProjectileProps::horizontal_hurt_knockback_direction`].
pub fn horizontal_hurt_knockback_direction<E: Parts>(
    entity: &E,
    _hurt_entity: &Living<'_>,
    _source: &DamageSource,
) -> (f64, f64) {
    let projectile = &entity.projectile();
    let movement = projectile.velocity();
    (movement.x, movement.z)
}

/// `Projectile`'s [`ProjectileProps::on_hit_entity`]: vanilla's does nothing.
pub const fn on_hit_entity<E: Parts>(_entity: &E, _target: &SharedEntity, _location: DVec3) {}

/// `Projectile`'s [`ProjectileProps::can_hit_entity`].
pub fn can_hit_entity<E: Parts>(entity: &E, target: &dyn Entity) -> bool {
    let projectile = &entity.projectile();
    if !target.can_be_hit_by_projectile() {
        return false;
    }
    let Some(owner) = projectile.owner() else {
        return true;
    };
    projectile.left_owner() || !owner.is_passenger_of_same_vehicle(target)
}

/// `Projectile`'s [`ProjectileProps::on_deflection`].
pub const fn on_deflection<E: Parts>(_entity: &E, _by_attack: bool) {}

/// `Projectile`'s [`ProjectileProps::should_bounce_on_world_border`].
pub const fn should_bounce_on_world_border<E: Parts>(_entity: &E) -> bool {
    false
}

/// `Projectile`'s [`ProjectileProps::on_hit`].
pub fn on_hit<E: Parts>(this: &E, hit: &ProjectileHit) {
    let projectile = &this.projectile();
    let world = projectile.level();
    match hit {
        ProjectileHit::Entity(entity_hit) => {
            if REGISTRY.entity_types.is_in_tag(
                entity_hit.entity.entity_type(),
                &EntityTypeTag::REDIRECTABLE_PROJECTILE,
            ) && let Some(target) = Projectile::of(entity_hit.entity.as_ref())
            {
                let owner_entity = projectile.owner();
                let owner = projectile.state().owner.clone();
                target.deflect(
                    ProjectileDeflection::AimDeflect,
                    owner_entity.as_deref(),
                    owner,
                    true,
                );
            }
            projectile.on_hit_entity(&entity_hit.entity, entity_hit.location);
            if let Some(world) = world {
                world.game_event_at(
                    &vanilla_game_events::PROJECTILE_LAND,
                    entity_hit.location,
                    &GameEventContext::new(Some(projectile.entity), None),
                );
            }
        }
        ProjectileHit::Block { hit, .. } => {
            projectile.on_hit_block(hit);
            if let Some(world) = world {
                let state = world.get_block_state(hit.block_pos);
                world.game_event(
                    &vanilla_game_events::PROJECTILE_LAND,
                    hit.block_pos,
                    &GameEventContext::new(Some(projectile.entity), Some(state)),
                );
            }
        }
    }
}

/// `Projectile`'s [`ProjectileProps::on_hit_block`].
pub fn on_hit_block<E: Parts>(entity: &E, hit: &ClipHitResult) {
    let projectile = &entity.projectile();
    let Some(world) = projectile.level() else {
        return;
    };
    let state = world.get_block_state(hit.block_pos);
    BLOCK_BEHAVIORS
        .get_behavior(state.get_block())
        .on_projectile_hit(state, &world, hit, projectile);
}

/// Spawns a throwable-item projectile from the source's eye along its look
/// direction and registers it in the world.
///
/// Mirrors vanilla `Projectile.spawnProjectileFromRotation` + `spawnProjectile`:
/// create the projectile at the eye minus 0.1, set its owner and item, shoot it
/// with `shootFromRotation`, add it to the world, then run
/// `applyOnProjectileSpawned` (the `minecraft:projectile_spawned` enchantment
/// effects). Returns `None` when the world rejects the projectile so the caller
/// can fail the use.
#[must_use]
pub fn spawn_throwable_item_projectile<E: throwable_item::Parts>(
    world: &Arc<World>,
    player: &Arc<Player>,
    item_stack: &mut ItemStack,
    pitch_offset: f32,
    power: f32,
    uncertainty: f32,
    create: impl FnOnce(DVec3) -> Arc<E>,
) -> Option<SharedEntity> {
    let player_pos = player.position();
    let spawn_pos = DVec3::new(
        player_pos.x,
        player.eye_y() - f64::from(0.1_f32),
        player_pos.z,
    );

    let entity = create(spawn_pos);
    let owner: SharedEntity = Arc::<Player>::clone(player);
    let projectile = entity.projectile();
    projectile.set_owner(Some(&owner));
    entity.throwable_item().set_item(item_stack);

    let (yaw, player_pitch) = player.rotation();
    projectile.shoot_from_rotation(
        player.as_ref(),
        player_pitch,
        yaw,
        pitch_offset,
        power,
        uncertainty,
    );

    let entity: SharedEntity = entity;
    if let Err(error) = world.try_add_entity(Arc::clone(&entity)) {
        log::debug!("failed to spawn throwable item projectile: {error}");
        return None;
    }
    enchantment_helper::on_projectile_spawned(world, item_stack, entity.as_ref(), Some(&owner));

    Some(entity)
}

/// Vanilla `ProjectileUtil.computeMargin`: ramps the entity hit margin from 0 to
/// 0.3 over the first ticks of flight.
#[must_use]
pub fn compute_margin(tick_count: i32) -> f64 {
    f64::from(((tick_count - 2) as f32 / 20.0).clamp(0.0, 0.3))
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
    let from = DVec3::new(position.x, source.eye_y(), position.z);
    let delta = source.look_angle() * distance;
    let to = from + delta;

    let block_hit =
        world.clip_including_border(from, to, ClipBlockShape::Collider, ClipFluid::None);
    let entity_end = if block_hit.is_miss() {
        to
    } else {
        block_hit.location
    };

    let search_box = source.bounding_box().expand_towards(delta).inflate(1.0);
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

/// Vanilla `ProjectileUtil.getEntityHitResult` (entity-margin overload): returns
/// the nearest entity whose inflated box the segment `from -> to` enters.
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

/// Vanilla `AABB.clip`: where the segment `from -> to` enters `aabb` through a
/// face. A segment that starts inside the box enters no face and misses.
fn clip_segment(aabb: WorldAabb, from: DVec3, to: DVec3) -> Option<DVec3> {
    const EPSILON: f64 = 1.0e-7;

    let delta = to - from;
    let mut scale = 1.0;
    let mut hit = false;
    // `AABB.clipPoint` for the face at `point` on axis `a`, bounded on `b` and `c`.
    let mut clip_point = |[da, db, dc]: [f64; 3],
                          point: f64,
                          [min_b, max_b]: [f64; 2],
                          [min_c, max_c]: [f64; 2],
                          [from_a, from_b, from_c]: [f64; 3]| {
        let s = (point - from_a) / da;
        let pb = from_b + s * db;
        let pc = from_c + s * dc;
        if 0.0 < s
            && s < scale
            && min_b - EPSILON < pb
            && pb < max_b + EPSILON
            && min_c - EPSILON < pc
            && pc < max_c + EPSILON
        {
            scale = s;
            hit = true;
        }
    };

    let x = [aabb.min(Axis::X), aabb.max(Axis::X)];
    let y = [aabb.min(Axis::Y), aabb.max(Axis::Y)];
    let z = [aabb.min(Axis::Z), aabb.max(Axis::Z)];
    let (dx, dy, dz) = (delta.x, delta.y, delta.z);
    if dx.abs() > EPSILON {
        let plane = if dx > 0.0 { x[0] } else { x[1] };
        clip_point([dx, dy, dz], plane, y, z, [from.x, from.y, from.z]);
    }
    if dy.abs() > EPSILON {
        let plane = if dy > 0.0 { y[0] } else { y[1] };
        clip_point([dy, dz, dx], plane, z, x, [from.y, from.z, from.x]);
    }
    if dz.abs() > EPSILON {
        let plane = if dz > 0.0 { z[0] } else { z[1] };
        clip_point([dz, dx, dy], plane, x, y, [from.z, from.x, from.y]);
    }

    hit.then(|| from + DVec3::new(scale * dx, scale * dy, scale * dz))
}

/// Vanilla `Mth.lerp(0.2, rotO, rot)` after wrapping the old angle into range.
fn lerp_rotation(mut rot_old: f32, rot: f32) -> f32 {
    while rot - rot_old < -DEGREE_180 {
        rot_old -= DEGREE_360;
    }
    while rot - rot_old >= DEGREE_180 {
        rot_old += DEGREE_360;
    }
    rot_old + 0.2 * (rot - rot_old)
}

#[cfg(test)]
mod owner_tests;

#[cfg(test)]
mod tests;
