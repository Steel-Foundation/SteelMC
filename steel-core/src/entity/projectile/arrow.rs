//! Vanilla `AbstractArrow` shared projectile behavior.
//!
//! Concrete arrow-like projectiles embed [`ArrowBase`] and implement
//! [`AbstractArrow`]. The layers chain explicitly through
//! [`AbstractArrow::arrow_tick`] and [`AbstractArrow::arrow_save`] /
//! [`AbstractArrow::arrow_load`], which stand in for vanilla `super.tick()` and
//! `super.addAdditionalSaveData()`.

use std::cmp::Ordering;
use std::sync::Arc;

use glam::DVec3;
use parking_lot::MappedMutexGuard;
use simdnbt::borrow::NbtCompound as BorrowedNbtCompoundView;
use simdnbt::owned::NbtCompound;
use simdnbt::{FromNbtTag as _, ToNbtTag as _};
use steel_protocol::packets::game::{CGameEvent, GameEventType};
use steel_registry::blocks::block_state_ext::BlockStateExt as _;
use steel_registry::item_stack::ItemStack;
use steel_registry::sound_event::{SoundEventHolder, SoundEventRef};
use steel_registry::vanilla_entity_data::AbstractArrowEntityData;
use steel_registry::vanilla_entity_type_tags::EntityTypeTag;
use steel_registry::{REGISTRY, TaggedRegistryExt as _, sound_events, vanilla_damage_types};
use steel_utils::locks::SyncMutex;
use steel_utils::{BlockPos, BlockStateId, WorldAabb};

use crate::behavior::{BLOCK_BEHAVIORS, BlockCollisionContext};
use crate::block_entity::block_state_nbt;
use crate::enchantment_helper::{self, EnchantmentDamageContext, EnchantmentPostAttackContext};
use crate::entity::damage::DamageSource;
use crate::entity::{
    Entity, EntityHitResult, EntityReference, LivingEntity, RemovalReason, SharedEntity,
};
use crate::inventory::container::Container as _;
use crate::physics::collision::CollisionWorld as _;
use crate::physics::{MoveResult, MoverType, WorldCollisionProvider};
use crate::player::Player;
use crate::world::{ClipBlockShape, ClipFluid, ClipHitResult, World};

use super::{
    Projectile, ProjectileDeflection, ProjectileHit, compute_margin, get_many_entity_hits,
    lerp_rotation,
};

/// Vanilla `AbstractArrow.getAirDrag`.
pub const ARROW_AIR_DRAG: f64 = 0.99;

/// Vanilla `AbstractArrow.getDefaultGravity`.
pub const ARROW_GRAVITY: f64 = 0.05;

/// Vanilla `AbstractArrow.getWaterInertia` default.
const DEFAULT_WATER_INERTIA: f32 = 0.6;

/// Vanilla `AbstractArrow.tickDespawn` lifetime in ticks.
const ARROW_LIFETIME_TICKS: i32 = 1200;

/// Vanilla `AbstractArrow` shake duration after landing.
const HIT_SHAKE_TICKS: i32 = 7;

/// Vanilla `AbstractArrow` ground offset applied against the hit direction.
const GROUND_HIT_OFFSET: f64 = 0.05;

/// Vanilla `AbstractArrow.shouldFall` probe inflation.
const FALL_PROBE_INFLATION: f64 = 0.06;

/// Vanilla `AbstractArrow.baseDamage` default.
const DEFAULT_BASE_DAMAGE: f64 = 2.0;

/// Vanilla arrow damage is clamped to the `int` range before rounding.
const MAX_ARROW_DAMAGE: f64 = 2_147_483_647.0;

const CRIT_FLAG: i8 = 1 << 0;
const NO_PHYSICS_FLAG: i8 = 1 << 1;

/// Vanilla `AbstractArrow.Pickup`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArrowPickup {
    /// Cannot be picked up.
    Disallowed,
    /// Can be picked up by any player.
    Allowed,
    /// Can only be picked up in creative mode.
    CreativeOnly,
}

impl ArrowPickup {
    /// Decodes vanilla `Pickup.LEGACY_CODEC`.
    #[must_use]
    pub const fn from_legacy_byte(value: i8) -> Self {
        match value {
            1 => Self::Allowed,
            2 => Self::CreativeOnly,
            _ => Self::Disallowed,
        }
    }

    /// Encodes vanilla `Pickup.LEGACY_CODEC`.
    #[must_use]
    pub const fn legacy_byte(self) -> i8 {
        match self {
            Self::Disallowed => 0,
            Self::Allowed => 1,
            Self::CreativeOnly => 2,
        }
    }
}

struct ArrowState {
    last_state: Option<BlockStateId>,
    in_ground_time: i32,
    pickup: ArrowPickup,
    shake_time: i32,
    life: i32,
    base_damage: f64,
    sound_event: SoundEventRef,
    default_sound_event: SoundEventRef,
    default_pickup_item: ItemStack,
    ignored_entity_ids: Vec<i32>,
    pickup_item_stack: ItemStack,
    weapon: Option<ItemStack>,
}

/// Runtime fields shared by vanilla `AbstractArrow` subclasses.
pub struct ArrowBase {
    state: SyncMutex<ArrowState>,
}

impl ArrowBase {
    /// Creates arrow state with vanilla defaults for the owning arrow type.
    #[must_use]
    pub fn new(default_sound_event: SoundEventRef, default_pickup_item: ItemStack) -> Self {
        Self {
            state: SyncMutex::new(ArrowState {
                last_state: None,
                in_ground_time: 0,
                pickup: ArrowPickup::Disallowed,
                shake_time: 0,
                life: 0,
                base_damage: DEFAULT_BASE_DAMAGE,
                sound_event: default_sound_event,
                default_sound_event,
                ignored_entity_ids: Vec::new(),
                pickup_item_stack: default_pickup_item.clone(),
                default_pickup_item,
                weapon: None,
            }),
        }
    }

    /// Sets the block state the arrow is embedded in.
    pub fn set_last_state(&self, last_state: Option<BlockStateId>) {
        self.state.lock().last_state = last_state;
    }

    /// Returns the block state the arrow is embedded in, if it has landed.
    #[must_use]
    pub fn last_state(&self) -> Option<BlockStateId> {
        self.state.lock().last_state
    }

    /// Returns how many ticks the arrow has been stuck in a block.
    #[must_use]
    pub fn in_ground_time(&self) -> i32 {
        self.state.lock().in_ground_time
    }

    /// Sets how many ticks the arrow has been stuck in a block.
    pub fn set_in_ground_time(&self, in_ground_time: i32) {
        self.state.lock().in_ground_time = in_ground_time;
    }

    /// Advances the in-ground tick counter.
    pub fn increment_in_ground_time(&self) {
        self.state.lock().in_ground_time += 1;
    }

    /// Returns the pickup rule that decides who may collect this arrow.
    #[must_use]
    pub fn pickup(&self) -> ArrowPickup {
        self.state.lock().pickup
    }

    /// Sets the pickup rule that decides who may collect this arrow.
    pub fn set_pickup(&self, pickup: ArrowPickup) {
        self.state.lock().pickup = pickup;
    }

    /// Returns the remaining landing shake time.
    #[must_use]
    pub fn shake_time(&self) -> i32 {
        self.state.lock().shake_time
    }

    /// Sets the landing shake time.
    pub fn set_shake_time(&self, shake_time: i32) {
        self.state.lock().shake_time = shake_time;
    }

    /// Decrements the landing shake time while it is positive.
    pub fn decrement_shake_time(&self) {
        let mut state = self.state.lock();
        if state.shake_time > 0 {
            state.shake_time -= 1;
        }
    }

    /// Returns how many ticks the arrow has been alive.
    #[must_use]
    pub fn life(&self) -> i32 {
        self.state.lock().life
    }

    /// Advances and returns the arrow lifetime counter.
    pub fn increment_life(&self) -> i32 {
        let mut state = self.state.lock();
        state.life += 1;
        state.life
    }

    /// Sets the arrow lifetime counter.
    pub fn set_life(&self, life: i32) {
        self.state.lock().life = life;
    }

    /// Returns the base damage scaled by impact speed on a hit.
    #[must_use]
    pub fn base_damage(&self) -> f64 {
        self.state.lock().base_damage
    }

    /// Sets the base damage scaled by impact speed on a hit.
    pub fn set_base_damage(&self, base_damage: f64) {
        self.state.lock().base_damage = base_damage;
    }

    /// Returns the sound played when this arrow hits a block.
    #[must_use]
    pub fn sound_event(&self) -> SoundEventRef {
        self.state.lock().sound_event
    }

    /// Overrides the sound played when this arrow hits a block.
    pub fn set_sound_event(&self, sound_event: SoundEventRef) {
        self.state.lock().sound_event = sound_event;
    }

    /// Returns the sound this arrow falls back to when no override is set.
    #[must_use]
    pub fn default_sound_event(&self) -> SoundEventRef {
        self.state.lock().default_sound_event
    }

    /// Returns the item this arrow falls back to when its pickup stack is empty.
    #[must_use]
    pub fn default_pickup_item(&self) -> ItemStack {
        self.state.lock().default_pickup_item.clone()
    }

    /// Adds `entity_id` to the piercing ignore set, keeping it duplicate-free.
    pub fn ignore_entity(&self, entity_id: i32) {
        let mut state = self.state.lock();
        if !state.ignored_entity_ids.contains(&entity_id) {
            state.ignored_entity_ids.push(entity_id);
        }
    }

    /// Returns whether `entity_id` has already been pierced.
    #[must_use]
    pub fn is_ignored(&self, entity_id: i32) -> bool {
        self.state.lock().ignored_entity_ids.contains(&entity_id)
    }

    /// Returns how many entities this arrow has pierced.
    #[must_use]
    pub fn ignored_entity_count(&self) -> usize {
        self.state.lock().ignored_entity_ids.len()
    }

    /// Clears the piercing ignore set.
    pub fn clear_ignored(&self) {
        self.state.lock().ignored_entity_ids.clear();
    }

    /// Returns the item stack this arrow drops when picked up.
    #[must_use]
    pub fn pickup_item_stack(&self) -> ItemStack {
        self.state.lock().pickup_item_stack.clone()
    }

    /// Sets the pickup item stack, falling back to the vanilla default when empty.
    pub fn set_pickup_item_stack(&self, item_stack: ItemStack) {
        let mut state = self.state.lock();
        state.pickup_item_stack = if item_stack.is_empty() {
            state.default_pickup_item.clone()
        } else {
            item_stack
        };
    }

    /// Returns the weapon that fired this arrow, if any.
    #[must_use]
    pub fn weapon(&self) -> Option<ItemStack> {
        self.state.lock().weapon.clone()
    }

    /// Sets the weapon that fired this arrow.
    pub fn set_weapon(&self, weapon: Option<ItemStack>) {
        self.state.lock().weapon = weapon;
    }
}

/// Vanilla `AbstractArrow` behavior layered on top of [`Projectile`].
///
/// Rust traits cannot call `super`, so each overridable vanilla hook delegates
/// to a shared body that overrides call to keep the base behavior.
pub trait AbstractArrow: Projectile + Sized {
    /// Returns shared arrow runtime state.
    fn arrow_base(&self) -> &ArrowBase;

    /// Returns this projectile's `AbstractArrow` synchronized data layer.
    fn abstract_arrow_data(&self) -> MappedMutexGuard<'_, AbstractArrowEntityData>;

    /// Vanilla `AbstractArrow.tick`.
    fn arrow_tick(self: Arc<Self>) {
        let physics_enabled = !self.no_physics();
        let movement = self.velocity();
        let block_pos = self.block_position();
        let Some(world) = self.level() else {
            self.projectile_base_tick();
            return;
        };

        let block_state = world.get_block_state(block_pos);
        if !block_state.is_air() && physics_enabled {
            let position = self.position();
            let boxes = BLOCK_BEHAVIORS
                .get_behavior(block_state.get_block())
                .get_collision_boxes(
                    block_state,
                    world.as_ref(),
                    block_pos,
                    BlockCollisionContext::empty(),
                );
            if boxes
                .iter()
                .any(|aabb| aabb.at_block(block_pos).contains(position))
            {
                self.set_velocity(DVec3::ZERO);
                self.set_in_ground(true);
            }
        }

        self.arrow_base().decrement_shake_time();

        if self.is_in_water_or_rain() {
            self.clear_fire();
        }

        if self.in_ground() && physics_enabled {
            if self.arrow_base().last_state() != Some(block_state) && self.arrow_should_fall() {
                self.arrow_start_falling();
            } else {
                self.tick_despawn();
            }

            self.arrow_base().increment_in_ground_time();
            if self.is_alive() {
                self.apply_effects_from_blocks();
            }
            if let Some(synced_data) = self.synced_data() {
                synced_data.set_base_on_fire_flag(self.remaining_fire_ticks() > 0);
            }
            return;
        }

        self.arrow_base().set_in_ground_time(0);
        let original_position = self.position();

        if self.is_in_water() {
            self.arrow_apply_inertia(self.arrow_water_inertia());
            // VANILLA CLIENT-LOCAL: `AbstractArrow.addBubbleParticles` only
            // spawns client-side bubble particles.
        }

        if self.is_crit_arrow() {
            // VANILLA CLIENT-LOCAL: `AbstractArrow.tick` only spawns client-side
            // crit particles.
        }

        let horizontal = (movement.x * movement.x + movement.z * movement.z).sqrt();
        let y_rot = if physics_enabled {
            movement.x.atan2(movement.z)
        } else {
            (-movement.x).atan2(-movement.z)
        }
        .to_degrees() as f32;
        let x_rot = movement.y.atan2(horizontal).to_degrees() as f32;
        let (yaw, pitch) = self.rotation();
        self.set_rotation((lerp_rotation(yaw, y_rot), lerp_rotation(pitch, x_rot)));

        self.check_left_owner();
        if physics_enabled {
            let block_hit = world.clip_including_border(
                original_position,
                original_position + movement,
                ClipBlockShape::Collider,
                ClipFluid::None,
            );
            Arc::clone(&self).step_move_and_hit(block_hit);
        } else {
            if let Err(error) = self.try_set_position(original_position + movement) {
                log::debug!(
                    "Rejected no-physics arrow movement for entity {}: {error}",
                    self.id()
                );
            }
            self.apply_effects_from_blocks();
        }

        if !self.is_in_water() {
            self.arrow_apply_inertia(ARROW_AIR_DRAG as f32);
        }

        if physics_enabled && !self.in_ground() {
            self.apply_gravity();
        }

        self.projectile_base_tick();
    }

    /// Vanilla `AbstractArrow.shoot` resets the lifetime counter after the
    /// base launch.
    fn arrow_shoot(&self, direction: DVec3, power: f32, uncertainty: f32) {
        self.default_shoot(direction, power, uncertainty);
        self.arrow_base().set_life(0);
    }

    /// Applies vanilla `Projectile.applyOnProjectileSpawned` for arrow
    /// subclasses: the pickup item's effects, then the firing weapon's
    /// effects when the weapon is a different item (a bow firing an arrow).
    /// Arrow overrides call this from [`Projectile::apply_on_projectile_spawned`].
    fn arrow_apply_on_projectile_spawned(&self, world: &World, pickup_item_stack: &mut ItemStack) {
        self.projectile_apply_on_projectile_spawned(world, pickup_item_stack);

        let Some(mut weapon) = self.get_weapon_item() else {
            return;
        };
        if weapon.is_empty() || pickup_item_stack.item == weapon.item {
            return;
        }
        if enchantment_helper::on_projectile_spawned(
            world,
            &mut weapon,
            self.as_entity_event_source(),
            self.get_owner().as_ref(),
        ) {
            self.on_item_break(&weapon);
        }
    }

    /// Vanilla `AbstractArrow.stepMoveAndHit`.
    fn step_move_and_hit(self: Arc<Self>, block_hit: ClipHitResult) {
        while self.is_alive() {
            let initial_position = self.position();
            let mut entities_hit = self.find_hit_entities(initial_position, block_hit.location);
            entities_hit.sort_by(|a, b| {
                let a_distance = initial_position.distance_squared(a.entity.position());
                let b_distance = initial_position.distance_squared(b.entity.position());
                a_distance
                    .partial_cmp(&b_distance)
                    .unwrap_or(Ordering::Equal)
            });

            let next_location = entities_hit
                .first()
                .map_or(block_hit.location, |hit| hit.location);
            if let Err(error) = self.try_set_position(next_location) {
                log::debug!(
                    "Rejected arrow step movement for entity {}: {error}",
                    self.id()
                );
                return;
            }
            self.apply_effects_from_blocks_between(initial_position, next_location);
            // TODO: vanilla handles projectile portal entry here; projectile
            // portal processing is not implemented yet.

            if entities_hit.is_empty() {
                if self.is_alive() && !block_hit.miss {
                    Arc::clone(&self).hit_target_or_deflect_self(&ProjectileHit::Block {
                        location: block_hit.location,
                        hit: block_hit,
                    });
                }
                return;
            }

            if self.is_alive() && !self.no_physics() {
                let deflection = Arc::clone(&self).hit_targets_or_deflect_self(entities_hit);
                if self.pierce_level() > 0 && deflection == ProjectileDeflection::None {
                    continue;
                }
            }
            return;
        }
    }

    /// Vanilla `AbstractArrow.hitTargetsOrDeflectSelf`.
    fn hit_targets_or_deflect_self(
        self: Arc<Self>,
        entities_hit: Vec<EntityHitResult>,
    ) -> ProjectileDeflection {
        for hit in entities_hit {
            let deflection =
                Arc::clone(&self).hit_target_or_deflect_self(&ProjectileHit::Entity(hit));
            if !self.is_alive() || deflection != ProjectileDeflection::None {
                return deflection;
            }
        }

        ProjectileDeflection::None
    }

    /// Vanilla `AbstractArrow.applyInertia`.
    fn arrow_apply_inertia(&self, inertia: f32) {
        self.set_velocity(self.velocity() * f64::from(inertia));
    }

    /// Vanilla `AbstractArrow.shouldFall`.
    fn arrow_should_fall(&self) -> bool {
        if !self.in_ground() {
            return false;
        }
        let Some(world) = self.level() else {
            return false;
        };
        let position = self.position();
        let probe = WorldAabb::from_min_max(position, position).inflate(FALL_PROBE_INFLATION);
        !WorldCollisionProvider::new(&world)
            .has_collision_with_context(&probe, BlockCollisionContext::empty())
    }

    /// Vanilla `AbstractArrow.startFalling`.
    fn arrow_start_falling(&self) {
        self.set_in_ground(false);
        let movement = self.velocity();
        let scale = DVec3::new(
            f64::from(rand::random::<f32>() * 0.2),
            f64::from(rand::random::<f32>() * 0.2),
            f64::from(rand::random::<f32>() * 0.2),
        );
        self.set_velocity(movement * scale);
        self.arrow_base().set_life(0);
    }

    /// Vanilla `AbstractArrow.move`: an external mover that leaves the arrow
    /// unsupported makes it start falling.
    fn arrow_move(self: Arc<Self>, mover_type: MoverType, delta: DVec3) -> Option<MoveResult> {
        let result = Arc::clone(&self).default_move_entity(mover_type, delta);
        if mover_type != MoverType::SelfMovement && self.arrow_should_fall() {
            self.arrow_start_falling();
        }
        result
    }

    /// Vanilla `AbstractArrow.push`: arrows stuck in a block ignore impulses.
    fn arrow_push_impulse(&self, impulse: DVec3) {
        if !self.in_ground() {
            self.default_push_impulse(impulse);
        }
    }

    /// Vanilla `AbstractArrow.onAboveBubbleColumn`: stuck arrows ignore columns.
    fn arrow_on_above_bubble_column(&self, drag_down: bool, pos: BlockPos) {
        if !self.in_ground() {
            self.on_above_bubble_column_projectile(drag_down, pos);
        }
    }

    /// Vanilla `AbstractArrow.onInsideBubbleColumn`: stuck arrows ignore columns.
    fn arrow_on_inside_bubble_column(&self, drag_down: bool) {
        if !self.in_ground() {
            self.on_inside_bubble_column_projectile(drag_down);
        }
    }

    /// Vanilla `AbstractArrow.tickDespawn`.
    fn tick_despawn(&self) {
        self.arrow_tick_despawn();
    }

    /// Shared `AbstractArrow.tickDespawn` body.
    fn arrow_tick_despawn(&self) {
        if self.arrow_base().increment_life() >= ARROW_LIFETIME_TICKS {
            self.set_removed(RemovalReason::Discarded);
        }
    }

    /// Vanilla `AbstractArrow.resetPiercedEntities`.
    ///
    /// Vanilla also clears `piercedAndKilledEntities`, which only feeds the
    /// `killed_by_arrow` advancement trigger; that trigger is not implemented.
    fn reset_pierced_entities(&self) {
        self.arrow_base().clear_ignored();
    }

    /// Vanilla `AbstractArrow.onHitEntity`.
    #[expect(
        clippy::too_many_lines,
        reason = "mirrors vanilla AbstractArrow.onHitEntity"
    )]
    fn arrow_on_hit_entity(self: Arc<Self>, entity: &SharedEntity, _location: DVec3) {
        let Some(world) = entity.level() else {
            return;
        };

        let power = self.velocity().length() as f32;
        let mut arrow_damage = self.arrow_base().base_damage();
        let owner = self.get_owner();
        let direct_entity: SharedEntity = Arc::<Self>::clone(&self);
        let causing_entity = owner.clone().unwrap_or_else(|| Arc::clone(&direct_entity));
        let damage_source = DamageSource::environment(&vanilla_damage_types::ARROW)
            .with_direct_entity(direct_entity)
            .with_causing_entity(causing_entity);

        if let Some(weapon_item) = self.get_weapon_item() {
            let context = EnchantmentDamageContext::new(
                entity.entity_type(),
                owner.as_ref().map(|owner| owner.entity_type()),
                Some(self.entity_type()),
                &damage_source,
            );
            arrow_damage = f64::from(enchantment_helper::modify_damage(
                &weapon_item,
                &context,
                arrow_damage as f32,
            ));
        }

        let mut damage = (f64::from(power) * arrow_damage)
            .clamp(0.0, MAX_ARROW_DAMAGE)
            .ceil() as i32;

        let pierce_level = self.pierce_level();
        if pierce_level > 0 {
            if self.arrow_base().ignored_entity_count() > usize::from(pierce_level as u8) {
                self.set_removed(RemovalReason::Discarded);
                return;
            }
            self.arrow_base().ignore_entity(entity.id());
        }

        if self.is_crit_arrow() {
            let bonus = rand::random_range(0..(damage / 2 + 2));
            damage = (i64::from(damage) + i64::from(bonus)).min(i64::from(i32::MAX)) as i32;
        }

        if let Some(owner) = owner.as_ref()
            && let Some(living_owner) = owner.as_living_entity()
        {
            living_owner.set_last_hurt_mob(Some(entity));
        }

        let remaining_fire_ticks = entity.remaining_fire_ticks();
        if self.is_on_fire()
            && let Some(living) = entity.as_living_entity()
        {
            living.ignite_for_ticks(100);
        }

        if entity.hurt(&world, &damage_source, damage as f32) {
            if let Some(mob) = entity.as_living_entity() {
                if self.pierce_level() <= 0 {
                    mob.set_arrow_count(mob.arrow_count() + 1);
                }

                self.arrow_do_knockback(mob, &damage_source);

                if let Some(weapon_item) = self.get_weapon_item() {
                    let context = EnchantmentPostAttackContext::new(entity, &damage_source);
                    enchantment_helper::do_post_attack_effects_with_item_source(
                        &world,
                        entity.as_ref(),
                        &weapon_item,
                        &context,
                    );
                }

                Arc::clone(&self).arrow_do_post_hurt_effects(mob);
                if entity.as_player().is_some()
                    && !self.is_silent()
                    && let Some(owner) = owner.as_ref()
                    && let Some(owner_player) = owner.as_player()
                    && !Arc::ptr_eq(entity, owner)
                {
                    owner_player.send_packet(CGameEvent {
                        event: GameEventType::PlayArrowHitSound,
                        data: 0.0,
                    });
                }
            }

            // TODO: vanilla tracks killed entities and triggers the
            // `killed_by_arrow` advancement for player owners.

            self.play_sound(
                self.arrow_base().sound_event(),
                1.0,
                1.2 / (rand::random::<f32>() * 0.2 + 0.9),
            );
            if self.pierce_level() <= 0 {
                self.set_removed(RemovalReason::Discarded);
            }
        } else if entity.projectile_receives_side_effects_on_hit(false) {
            entity.set_remaining_fire_ticks(remaining_fire_ticks);
            let owner_reference = owner.as_ref().map(EntityReference::from_entity);
            self.deflect(
                ProjectileDeflection::Reverse,
                Some(entity.as_ref()),
                owner_reference,
                false,
                DVec3::splat(0.2),
            );
            if self.velocity().length_squared() < 1.0e-7 {
                if self.arrow_base().pickup() == ArrowPickup::Allowed {
                    let _ = self.spawn_at_location_with_offset(
                        self.arrow_pickup_item(),
                        DVec3::new(0.0, 0.1, 0.0),
                    );
                }
                self.set_removed(RemovalReason::Discarded);
            }
        }
    }

    /// Vanilla `AbstractArrow.doKnockback`.
    fn arrow_do_knockback(&self, mob: &dyn LivingEntity, damage_source: &DamageSource) {
        let Some(weapon_item) = self.arrow_base().weapon() else {
            return;
        };
        let owner = self.get_owner();
        let context = EnchantmentDamageContext::new(
            mob.entity_type(),
            owner.as_ref().map(|owner| owner.entity_type()),
            Some(self.entity_type()),
            damage_source,
        );
        let knockback = enchantment_helper::modify_knockback(&weapon_item, &context, 0.0);
        if knockback <= 0.0 {
            return;
        }

        let knockback_resistance = (1.0 - mob.knockback_resistance()).max(0.0);
        let scale = f64::from(knockback) * 0.6 * knockback_resistance;
        let velocity = self.velocity();
        let movement = DVec3::new(velocity.x, 0.0, velocity.z).normalize_or_zero() * scale;
        if movement.length_squared() > 0.0 {
            mob.push_impulse(DVec3::new(movement.x, 0.1, movement.z));
        }
    }

    /// Vanilla `AbstractArrow.doPostHurtEffects` (no-op by default).
    fn arrow_do_post_hurt_effects(self: Arc<Self>, _mob: &dyn LivingEntity) {}

    /// Vanilla `Projectile.getEffectSource`: the owner when present, otherwise
    /// the projectile itself. The owner need not be a living entity.
    fn arrow_get_effect_source(self: Arc<Self>) -> SharedEntity {
        if let Some(owner) = self.get_owner() {
            return owner;
        }
        self
    }

    /// Vanilla `AbstractArrow.onHitBlock`.
    fn arrow_on_hit_block(self: Arc<Self>, hit: &ClipHitResult) {
        if let Some(world) = self.level() {
            self.arrow_base()
                .set_last_state(Some(world.get_block_state(hit.block_pos)));
        }
        Arc::clone(&self).projectile_on_hit_block(hit);
        // TODO: vanilla applies weapon block-hit enchantment effects here once
        // `EnchantmentHelper.onHitBlock` is ported.

        let movement = self.velocity();
        let offset = DVec3::new(
            movement.x.signum(),
            movement.y.signum(),
            movement.z.signum(),
        ) * GROUND_HIT_OFFSET;
        if let Err(error) = self.try_set_position(self.position() - offset) {
            log::debug!(
                "Rejected arrow ground placement for entity {}: {error}",
                self.id()
            );
        }
        self.set_velocity(DVec3::ZERO);
        self.play_sound(
            self.arrow_base().sound_event(),
            1.0,
            1.2 / (rand::random::<f32>() * 0.2 + 0.9),
        );
        self.set_in_ground(true);
        self.arrow_base().set_shake_time(HIT_SHAKE_TICKS);
        self.set_crit_arrow(false);
        self.set_pierce_level(0);
        self.arrow_base()
            .set_sound_event(&sound_events::ENTITY_ARROW_HIT);
        self.reset_pierced_entities();
    }

    /// Vanilla `AbstractArrow.getDefaultHitGroundSoundEvent`.
    fn arrow_default_hit_ground_sound_event(&self) -> SoundEventRef {
        self.arrow_base().default_sound_event()
    }

    /// Vanilla `AbstractArrow.getWeaponItem`.
    fn get_weapon_item(&self) -> Option<ItemStack> {
        self.arrow_base().weapon()
    }

    /// Vanilla `AbstractArrow.findHitEntity`.
    fn find_hit_entity(&self, from: DVec3, to: DVec3) -> Option<EntityHitResult> {
        self.arrow_find_hit_entity(from, to)
    }

    /// Shared `AbstractArrow.findHitEntity` body.
    fn arrow_find_hit_entity(&self, from: DVec3, to: DVec3) -> Option<EntityHitResult> {
        self.arrow_find_hit_entities(from, to)
            .into_iter()
            .min_by(|a, b| {
                let a_distance = from.distance_squared(a.location);
                let b_distance = from.distance_squared(b.location);
                a_distance
                    .partial_cmp(&b_distance)
                    .unwrap_or(Ordering::Equal)
            })
    }

    /// Vanilla `AbstractArrow.findHitEntities`.
    fn find_hit_entities(&self, from: DVec3, to: DVec3) -> Vec<EntityHitResult> {
        self.arrow_find_hit_entities(from, to)
    }

    /// Shared `AbstractArrow.findHitEntities` body.
    fn arrow_find_hit_entities(&self, from: DVec3, to: DVec3) -> Vec<EntityHitResult> {
        let Some(world) = self.level() else {
            return Vec::new();
        };
        let search_box = self
            .bounding_box()
            .expand_towards(self.velocity())
            .inflate(1.0);
        get_many_entity_hits(
            world.as_ref(),
            from,
            to,
            search_box,
            compute_margin(self.tick_count()),
            |entity| self.can_hit_entity(entity),
            false,
            false,
        )
    }

    /// Vanilla `AbstractArrow.canHitEntity`, layered on the base projectile check.
    fn arrow_can_hit_entity(&self, entity: &dyn Entity) -> bool {
        // TODO: vanilla also rejects player-vs-player hits when the owner is a
        // player; `Player::can_harm_player` is not implemented yet.
        self.base_can_hit_entity(entity) && !self.arrow_base().is_ignored(entity.id())
    }

    /// Vanilla `AbstractArrow.addAdditionalSaveData`.
    fn arrow_save(&self, nbt: &mut NbtCompound) {
        let arrow_base = self.arrow_base();
        nbt.insert("life", arrow_base.life() as i16);
        if let Some(last_state) = arrow_base.last_state() {
            nbt.insert("inBlockState", block_state_nbt::save(last_state));
        }
        nbt.insert("shake", arrow_base.shake_time() as i8);
        nbt.insert("inGround", i8::from(self.in_ground()));
        nbt.insert("pickup", arrow_base.pickup().legacy_byte());
        nbt.insert("damage", arrow_base.base_damage());
        nbt.insert("crit", i8::from(self.is_crit_arrow()));
        nbt.insert("PierceLevel", self.pierce_level());
        nbt.insert(
            "SoundEvent",
            SoundEventHolder::Registry(arrow_base.sound_event()).to_nbt_tag(),
        );
        nbt.insert("item", arrow_base.pickup_item_stack().to_nbt_tag_ref());
        if let Some(weapon) = arrow_base.weapon() {
            nbt.insert("weapon", weapon.to_nbt_tag_ref());
        }
    }

    /// Vanilla `AbstractArrow.readAdditionalSaveData`.
    fn arrow_load(&self, nbt: BorrowedNbtCompoundView<'_, '_>) {
        let arrow_base = self.arrow_base();
        arrow_base.set_last_state(nbt.compound("inBlockState").and_then(block_state_nbt::load));
        arrow_base.set_shake_time(i32::from(nbt.byte("shake").unwrap_or(0)) & 255);
        arrow_base.set_base_damage(nbt.double("damage").unwrap_or(DEFAULT_BASE_DAMAGE));
        arrow_base.set_pickup(
            nbt.byte("pickup")
                .map_or(ArrowPickup::Disallowed, ArrowPickup::from_legacy_byte),
        );
        arrow_base.set_sound_event(
            nbt.get("SoundEvent")
                .and_then(SoundEventHolder::from_nbt_tag)
                .and_then(|holder| holder.registry_ref())
                .unwrap_or_else(|| arrow_base.default_sound_event()),
        );
        self.arrow_set_pickup_item_stack(
            nbt.compound("item")
                .and_then(|compound| ItemStack::from_borrowed_compound(&compound))
                .unwrap_or_else(|| arrow_base.default_pickup_item()),
        );
        arrow_base.set_weapon(
            nbt.compound("weapon")
                .and_then(|compound| ItemStack::from_borrowed_compound(&compound)),
        );

        self.set_in_ground(nbt.byte("inGround").is_some_and(|value| value != 0));
        self.set_crit_arrow(nbt.byte("crit").is_some_and(|value| value != 0));
        self.set_pierce_level(nbt.byte("PierceLevel").unwrap_or(0));
        // Vanilla reads `life` first and stores it as a short.
        arrow_base.set_life(i32::from(nbt.short("life").unwrap_or(0)));
    }

    /// Vanilla `AbstractArrow.playerTouch`.
    fn arrow_player_touch(self: Arc<Self>, player: &Arc<Player>) {
        if (self.in_ground() || self.no_physics())
            && self.arrow_base().shake_time() <= 0
            && self.try_pickup(player)
        {
            player.take(self.as_ref(), 1);
            self.set_removed(RemovalReason::Discarded);
        }
    }

    /// Vanilla `AbstractArrow.tryPickup`.
    fn try_pickup(&self, player: &Arc<Player>) -> bool {
        self.arrow_try_pickup(player)
    }

    /// Shared `AbstractArrow.tryPickup` body.
    fn arrow_try_pickup(&self, player: &Arc<Player>) -> bool {
        match self.arrow_base().pickup() {
            ArrowPickup::Disallowed => false,
            ArrowPickup::Allowed => {
                let mut item = self.arrow_pickup_item();
                player.inventory.lock().add(&mut item)
            }
            ArrowPickup::CreativeOnly => player.has_infinite_materials(),
        }
    }

    /// Vanilla `AbstractArrow.getPickupItem`.
    fn arrow_pickup_item(&self) -> ItemStack {
        self.arrow_base().pickup_item_stack()
    }

    /// Vanilla `AbstractArrow.getPickupItemStackOrigin`.
    fn arrow_pickup_item_stack_origin(&self) -> ItemStack {
        self.arrow_base().pickup_item_stack()
    }

    /// Vanilla `AbstractArrow.setPickupItemStack`.
    fn arrow_set_pickup_item_stack(&self, item_stack: ItemStack) {
        self.arrow_base().set_pickup_item_stack(item_stack);
    }

    /// Vanilla `AbstractArrow.setBaseDamage`.
    fn set_base_damage(&self, base_damage: f64) {
        self.arrow_base().set_base_damage(base_damage);
    }

    /// Vanilla `AbstractArrow.setCritArrow`.
    fn set_crit_arrow(&self, crit_arrow: bool) {
        self.arrow_set_flag(CRIT_FLAG, crit_arrow);
    }

    /// Vanilla `AbstractArrow.isCritArrow`.
    fn is_crit_arrow(&self) -> bool {
        *self.abstract_arrow_data().id_flags.get() & CRIT_FLAG != 0
    }

    /// Vanilla `AbstractArrow.setPierceLevel`.
    fn set_pierce_level(&self, pierce_level: i8) {
        self.abstract_arrow_data().pierce_level.set(pierce_level);
    }

    /// Vanilla `AbstractArrow.getPierceLevel`.
    fn pierce_level(&self) -> i8 {
        *self.abstract_arrow_data().pierce_level.get()
    }

    /// Vanilla `AbstractArrow.setInGround`.
    fn set_in_ground(&self, in_ground: bool) {
        self.abstract_arrow_data().in_ground.set(in_ground);
    }

    /// Vanilla `AbstractArrow.isInGround`.
    fn in_ground(&self) -> bool {
        *self.abstract_arrow_data().in_ground.get()
    }

    /// Vanilla `AbstractArrow.isPushedByFluid`.
    fn arrow_is_pushed_by_fluid(&self) -> bool {
        !self.in_ground()
    }

    /// Vanilla `AbstractArrow.setNoPhysics`: mirrors the flag into synced data.
    fn arrow_set_no_physics(&self, no_physics: bool) {
        self.base().set_no_physics(no_physics);
        self.arrow_set_flag(NO_PHYSICS_FLAG, no_physics);
    }

    /// Vanilla `AbstractArrow.setOwner`: a player owner makes an otherwise
    /// disallowed arrow pickable.
    fn arrow_set_owner(&self, owner: Option<&SharedEntity>) {
        self.default_set_owner_entity(owner);
        if self.arrow_base().pickup() == ArrowPickup::Disallowed
            && owner.is_some_and(|owner| owner.as_player().is_some())
        {
            self.arrow_base().set_pickup(ArrowPickup::Allowed);
        }
    }

    /// Vanilla `AbstractArrow.isPickable`: inherits `Projectile`'s
    /// redirectable-only rule and additionally refuses while in ground.
    fn arrow_is_pickable(&self) -> bool {
        self.arrow_is_attackable() && !self.in_ground()
    }

    /// Vanilla `AbstractArrow.getWaterInertia`.
    fn arrow_water_inertia(&self) -> f32 {
        DEFAULT_WATER_INERTIA
    }

    /// Vanilla `AbstractArrow.isAttackable`.
    fn arrow_is_attackable(&self) -> bool {
        REGISTRY
            .entity_types
            .is_in_tag(self.entity_type(), &EntityTypeTag::REDIRECTABLE_PROJECTILE)
    }

    /// Vanilla `AbstractArrow` flags are stored in the synced `ID_FLAGS` byte.
    fn arrow_set_flag(&self, flag: i8, value: bool) {
        let mut data = self.abstract_arrow_data();
        let flags = *data.id_flags.get();
        data.id_flags
            .set(if value { flags | flag } else { flags & !flag });
    }

    /// Returns how long this arrow has been stuck in a block.
    fn arrow_in_ground_time(&self) -> i32 {
        self.arrow_base().in_ground_time()
    }
}
