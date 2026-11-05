//! Shared behavior for arrow-like projectiles.
//!
//! Concrete arrows embed [`ArrowBase`] and implement [`AbstractArrow`]. The layers
//! chain through `arrow_tick` and `arrow_save` / `arrow_load`, which stand in for
//! `super.tick()` and `super.addAdditionalSaveData()`.

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
use crate::entity::{Entity, EntityHitResult, LivingEntity, RemovalReason, SharedEntity};
use crate::inventory::container::Container as _;
use crate::physics::collision::CollisionWorld as _;
use crate::physics::{MoveResult, MoverType, WorldCollisionProvider};
use crate::player::Player;
use crate::world::{ClipBlockShape, ClipFluid, ClipHitResult, World};

use super::{
    Projectile, ProjectileDeflection, ProjectileHit, compute_margin, get_many_entity_hits,
    lerp_rotation,
};

pub(crate) const ARROW_AIR_DRAG: f64 = 0.99;

pub(crate) const ARROW_GRAVITY: f64 = 0.05;

/// Inertia applied while the arrow is in water.
const DEFAULT_WATER_INERTIA: f32 = 0.6;

/// Ticks before an arrow despawns.
const ARROW_LIFETIME_TICKS: i32 = 1200;

/// Shake duration after landing.
const HIT_SHAKE_TICKS: i32 = 7;

/// Ground offset applied against the hit direction.
const GROUND_HIT_OFFSET: f64 = 0.05;

/// Probe inflation used to check whether a landed arrow lost its support.
const FALL_PROBE_INFLATION: f64 = 0.06;

const DEFAULT_BASE_DAMAGE: f64 = 2.0;

/// Arrow damage is clamped to the `int` range before rounding.
const MAX_ARROW_DAMAGE: f64 = 2_147_483_647.0;

/// Ticks an entity burns when hit by a flaming arrow.
const FIRE_TICKS_ON_HIT: i32 = 100;

/// Horizontal knockback scale applied by the knockback enchantment.
const KNOCKBACK_VELOCITY_SCALE: f64 = 0.6;

/// Upward lift added to knockback.
const KNOCKBACK_LIFT: f64 = 0.1;

/// Velocity below this is treated as stopped.
const STOPPED_VELOCITY_SQUARED: f64 = 1.0e-7;

/// Deflection power applied when a hit does not hurt.
const HIT_DEFLECTION_POWER: f64 = 0.2;

/// Spread applied per axis when an unsupported arrow starts falling.
const FALL_SPREAD: f32 = 0.2;

/// Offset applied when dropping a stopped arrow as an item.
const PICKUP_SPAWN_OFFSET: DVec3 = DVec3::new(0.0, 0.1, 0.0);

/// Entity search box inflation.
const ENTITY_SEARCH_INFLATE: f64 = 1.0;

const CRIT_DAMAGE_DIVISOR: i32 = 2;
const CRIT_DAMAGE_BASE: i32 = 2;

const HIT_SOUND_PITCH_BASE: f32 = 1.2;
const HIT_SOUND_PITCH_RANDOM_RANGE: f32 = 0.2;
const HIT_SOUND_PITCH_OFFSET: f32 = 0.9;

fn hit_sound_pitch() -> f32 {
    HIT_SOUND_PITCH_BASE
        / (rand::random::<f32>() * HIT_SOUND_PITCH_RANDOM_RANGE + HIT_SOUND_PITCH_OFFSET)
}

const CRIT_FLAG: i8 = 1 << 0;
const NO_PHYSICS_FLAG: i8 = 1 << 1;

/// How this arrow can be picked up.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ArrowPickup {
    /// Cannot be picked up.
    Disallowed,
    /// Can be picked up by any player.
    Allowed,
    /// Can only be picked up in creative mode.
    CreativeOnly,
}

impl ArrowPickup {
    #[must_use]
    pub const fn from_legacy_byte(value: i8) -> Self {
        match value {
            1 => Self::Allowed,
            2 => Self::CreativeOnly,
            _ => Self::Disallowed,
        }
    }

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

/// Runtime state shared by arrow-like projectiles.
pub(crate) struct ArrowBase {
    state: SyncMutex<ArrowState>,
}

impl ArrowBase {
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

    pub fn set_last_state(&self, last_state: Option<BlockStateId>) {
        self.state.lock().last_state = last_state;
    }

    #[must_use]
    pub fn last_state(&self) -> Option<BlockStateId> {
        self.state.lock().last_state
    }

    #[must_use]
    pub fn in_ground_time(&self) -> i32 {
        self.state.lock().in_ground_time
    }

    pub fn set_in_ground_time(&self, in_ground_time: i32) {
        self.state.lock().in_ground_time = in_ground_time;
    }

    pub fn increment_in_ground_time(&self) {
        self.state.lock().in_ground_time += 1;
    }

    #[must_use]
    pub fn pickup(&self) -> ArrowPickup {
        self.state.lock().pickup
    }

    pub fn set_pickup(&self, pickup: ArrowPickup) {
        self.state.lock().pickup = pickup;
    }

    #[must_use]
    pub fn shake_time(&self) -> i32 {
        self.state.lock().shake_time
    }

    pub fn set_shake_time(&self, shake_time: i32) {
        self.state.lock().shake_time = shake_time;
    }

    pub fn decrement_shake_time(&self) {
        let mut state = self.state.lock();
        if state.shake_time > 0 {
            state.shake_time -= 1;
        }
    }

    #[must_use]
    pub fn life(&self) -> i32 {
        self.state.lock().life
    }

    pub fn increment_life(&self) -> i32 {
        let mut state = self.state.lock();
        state.life += 1;
        state.life
    }

    pub fn set_life(&self, life: i32) {
        self.state.lock().life = life;
    }

    #[must_use]
    pub fn base_damage(&self) -> f64 {
        self.state.lock().base_damage
    }

    pub fn set_base_damage(&self, base_damage: f64) {
        self.state.lock().base_damage = base_damage;
    }

    #[must_use]
    pub fn sound_event(&self) -> SoundEventRef {
        self.state.lock().sound_event
    }

    pub fn set_sound_event(&self, sound_event: SoundEventRef) {
        self.state.lock().sound_event = sound_event;
    }

    #[must_use]
    pub fn default_sound_event(&self) -> SoundEventRef {
        self.state.lock().default_sound_event
    }

    #[must_use]
    pub fn default_pickup_item(&self) -> ItemStack {
        self.state.lock().default_pickup_item.clone()
    }

    pub fn ignore_entity(&self, entity_id: i32) {
        let mut state = self.state.lock();
        if !state.ignored_entity_ids.contains(&entity_id) {
            state.ignored_entity_ids.push(entity_id);
        }
    }

    #[must_use]
    pub fn is_ignored(&self, entity_id: i32) -> bool {
        self.state.lock().ignored_entity_ids.contains(&entity_id)
    }

    #[must_use]
    pub fn ignored_entity_count(&self) -> usize {
        self.state.lock().ignored_entity_ids.len()
    }

    pub fn clear_ignored(&self) {
        self.state.lock().ignored_entity_ids.clear();
    }

    #[must_use]
    pub fn pickup_item_stack(&self) -> ItemStack {
        self.state.lock().pickup_item_stack.clone()
    }

    pub fn set_pickup_item_stack(&self, item_stack: ItemStack) {
        let mut state = self.state.lock();
        state.pickup_item_stack = if item_stack.is_empty() {
            state.default_pickup_item.clone()
        } else {
            item_stack
        };
    }

    #[must_use]
    pub fn weapon(&self) -> Option<ItemStack> {
        self.state.lock().weapon.clone()
    }

    pub fn set_weapon(&self, weapon: Option<ItemStack>) {
        self.state.lock().weapon = weapon;
    }
}

/// Behavior layered on top of [`Projectile`].
///
/// Rust traits cannot call `super`, so each overridable hook delegates to a shared
/// body that overrides call to keep the base behavior.
pub(crate) trait AbstractArrow: Projectile + Sized {
    fn arrow_base(&self) -> &ArrowBase;

    fn abstract_arrow_data(&self) -> MappedMutexGuard<'_, AbstractArrowEntityData>;

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

    fn arrow_shoot(&self, direction: DVec3, power: f32, uncertainty: f32) {
        self.default_shoot(direction, power, uncertainty);
        self.arrow_base().set_life(0);
    }

    /// Applies the pickup item's effects, then the firing weapon's effects when the
    /// weapon is a different item (for example a bow firing an arrow).
    ///
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

    fn arrow_apply_inertia(&self, inertia: f32) {
        self.set_velocity(self.velocity() * f64::from(inertia));
    }

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

    fn arrow_start_falling(&self) {
        self.set_in_ground(false);
        let movement = self.velocity();
        let scale = DVec3::new(
            f64::from(rand::random::<f32>() * FALL_SPREAD),
            f64::from(rand::random::<f32>() * FALL_SPREAD),
            f64::from(rand::random::<f32>() * FALL_SPREAD),
        );
        self.set_velocity(movement * scale);
        self.arrow_base().set_life(0);
    }

    /// An external mover that leaves the arrow unsupported makes it start falling.
    fn arrow_move(self: Arc<Self>, mover_type: MoverType, delta: DVec3) -> Option<MoveResult> {
        let result = Arc::clone(&self).default_move_entity(mover_type, delta);
        if mover_type != MoverType::SelfMovement && self.arrow_should_fall() {
            self.arrow_start_falling();
        }
        result
    }

    /// Arrows stuck in a block ignore impulses.
    fn arrow_push_impulse(&self, impulse: DVec3) {
        if !self.in_ground() {
            self.default_push_impulse(impulse);
        }
    }

    /// Stuck arrows ignore bubble columns.
    fn arrow_on_above_bubble_column(&self, drag_down: bool, pos: BlockPos) {
        if !self.in_ground() {
            self.on_above_bubble_column_projectile(drag_down, pos);
        }
    }

    /// Stuck arrows ignore bubble columns.
    fn arrow_on_inside_bubble_column(&self, drag_down: bool) {
        if !self.in_ground() {
            self.on_inside_bubble_column_projectile(drag_down);
        }
    }

    fn tick_despawn(&self) {
        self.arrow_tick_despawn();
    }

    fn arrow_tick_despawn(&self) {
        if self.arrow_base().increment_life() >= ARROW_LIFETIME_TICKS {
            self.set_removed(RemovalReason::Discarded);
        }
    }

    /// `piercedAndKilledEntities` is not tracked because it only feeds the
    /// `killed_by_arrow` advancement trigger, which is not implemented.
    fn reset_pierced_entities(&self) {
        self.arrow_base().clear_ignored();
    }

    #[expect(clippy::too_many_lines, reason = "keeps the hit sequence in order")]
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
            let bonus = rand::random_range(0..(damage / CRIT_DAMAGE_DIVISOR + CRIT_DAMAGE_BASE));
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
            living.ignite_for_ticks(FIRE_TICKS_ON_HIT);
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

            self.play_sound(self.arrow_base().sound_event(), 1.0, hit_sound_pitch());
            if self.pierce_level() <= 0 {
                self.set_removed(RemovalReason::Discarded);
            }
        } else if entity.projectile_receives_side_effects_on_hit(false) {
            entity.set_remaining_fire_ticks(remaining_fire_ticks);
            let owner_reference = self.owner_reference();
            self.deflect(
                ProjectileDeflection::Reverse,
                Some(entity.as_ref()),
                owner_reference,
                false,
                DVec3::splat(HIT_DEFLECTION_POWER),
            );
            if self.velocity().length_squared() < STOPPED_VELOCITY_SQUARED {
                if self.arrow_base().pickup() == ArrowPickup::Allowed {
                    let _ = self.spawn_at_location_with_offset(
                        self.arrow_pickup_item(),
                        PICKUP_SPAWN_OFFSET,
                    );
                }
                self.set_removed(RemovalReason::Discarded);
            }
        }
    }

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
        let scale = f64::from(knockback) * KNOCKBACK_VELOCITY_SCALE * knockback_resistance;
        let velocity = self.velocity();
        let movement = DVec3::new(velocity.x, 0.0, velocity.z).normalize_or_zero() * scale;
        if movement.length_squared() > 0.0 {
            mob.push_impulse(DVec3::new(movement.x, KNOCKBACK_LIFT, movement.z));
        }
    }

    fn arrow_do_post_hurt_effects(self: Arc<Self>, _mob: &dyn LivingEntity) {}

    /// Returns the owner when present, otherwise the projectile itself. The owner
    /// need not be a living entity.
    fn arrow_get_effect_source(self: Arc<Self>) -> SharedEntity {
        if let Some(owner) = self.get_owner() {
            return owner;
        }
        self
    }

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
        self.play_sound(self.arrow_base().sound_event(), 1.0, hit_sound_pitch());
        self.set_in_ground(true);
        self.arrow_base().set_shake_time(HIT_SHAKE_TICKS);
        self.set_crit_arrow(false);
        self.set_pierce_level(0);
        self.arrow_base()
            .set_sound_event(&sound_events::ENTITY_ARROW_HIT);
        self.reset_pierced_entities();
    }

    fn get_weapon_item(&self) -> Option<ItemStack> {
        self.arrow_base().weapon()
    }

    fn find_hit_entity(&self, from: DVec3, to: DVec3) -> Option<EntityHitResult> {
        self.arrow_find_hit_entity(from, to)
    }

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

    fn find_hit_entities(&self, from: DVec3, to: DVec3) -> Vec<EntityHitResult> {
        self.arrow_find_hit_entities(from, to)
    }

    fn arrow_find_hit_entities(&self, from: DVec3, to: DVec3) -> Vec<EntityHitResult> {
        let Some(world) = self.level() else {
            return Vec::new();
        };
        let search_box = self
            .bounding_box()
            .expand_towards(self.velocity())
            .inflate(ENTITY_SEARCH_INFLATE);
        get_many_entity_hits(
            world.as_ref(),
            self.id(),
            from,
            to,
            search_box,
            compute_margin(self.tick_count()),
            |entity| self.can_hit_entity(entity),
            false,
            false,
        )
    }

    fn arrow_can_hit_entity(&self, entity: &dyn Entity) -> bool {
        // TODO: vanilla also rejects player-vs-player hits when the owner is a
        // player; `Player::can_harm_player` is not implemented yet.
        self.base_can_hit_entity(entity) && !self.arrow_base().is_ignored(entity.id())
    }

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
        arrow_base.set_life(i32::from(nbt.short("life").unwrap_or(0)));
    }

    fn arrow_player_touch(self: Arc<Self>, player: &Arc<Player>) {
        if (self.in_ground() || self.no_physics())
            && self.arrow_base().shake_time() <= 0
            && self.try_pickup(player)
        {
            player.take(self.as_ref(), 1);
            self.set_removed(RemovalReason::Discarded);
        }
    }

    fn try_pickup(&self, player: &Arc<Player>) -> bool {
        self.arrow_try_pickup(player)
    }

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

    fn arrow_pickup_item(&self) -> ItemStack {
        self.arrow_base().pickup_item_stack()
    }

    fn arrow_pickup_item_stack_origin(&self) -> ItemStack {
        self.arrow_base().pickup_item_stack()
    }

    fn arrow_set_pickup_item_stack(&self, item_stack: ItemStack) {
        self.arrow_base().set_pickup_item_stack(item_stack);
    }

    fn set_crit_arrow(&self, crit_arrow: bool) {
        self.arrow_set_flag(CRIT_FLAG, crit_arrow);
    }

    fn is_crit_arrow(&self) -> bool {
        *self.abstract_arrow_data().id_flags.get() & CRIT_FLAG != 0
    }

    fn set_pierce_level(&self, pierce_level: i8) {
        self.abstract_arrow_data().pierce_level.set(pierce_level);
    }

    fn pierce_level(&self) -> i8 {
        *self.abstract_arrow_data().pierce_level.get()
    }

    fn set_in_ground(&self, in_ground: bool) {
        self.abstract_arrow_data().in_ground.set(in_ground);
    }

    fn in_ground(&self) -> bool {
        *self.abstract_arrow_data().in_ground.get()
    }

    fn arrow_is_pushed_by_fluid(&self) -> bool {
        !self.in_ground()
    }

    /// Mirrors the flag into synced data.
    fn arrow_set_no_physics(&self, no_physics: bool) {
        self.base().set_no_physics(no_physics);
        self.arrow_set_flag(NO_PHYSICS_FLAG, no_physics);
    }

    /// A player owner makes an otherwise disallowed arrow pickable.
    fn arrow_set_owner(&self, owner: Option<&SharedEntity>) {
        self.default_set_owner_entity(owner);
        if self.arrow_base().pickup() == ArrowPickup::Disallowed
            && owner.is_some_and(|owner| owner.as_player().is_some())
        {
            self.arrow_base().set_pickup(ArrowPickup::Allowed);
        }
    }

    /// Inherits the redirectable-only rule and additionally refuses while in
    /// ground.
    fn arrow_is_pickable(&self) -> bool {
        self.arrow_is_attackable() && !self.in_ground()
    }

    fn arrow_water_inertia(&self) -> f32 {
        DEFAULT_WATER_INERTIA
    }

    fn arrow_is_attackable(&self) -> bool {
        REGISTRY
            .entity_types
            .is_in_tag(self.entity_type(), &EntityTypeTag::REDIRECTABLE_PROJECTILE)
    }

    /// Flags live in the synced `ID_FLAGS` byte.
    fn arrow_set_flag(&self, flag: i8, value: bool) {
        let mut data = self.abstract_arrow_data();
        let flags = *data.id_flags.get();
        data.id_flags
            .set(if value { flags | flag } else { flags & !flag });
    }

    fn arrow_in_ground_time(&self) -> i32 {
        self.arrow_base().in_ground_time()
    }
}
