//! Arrow projectile entity (`net.minecraft.world.entity.projectile.arrow.Arrow`,
//! backed by `AbstractArrow`).
//!
//! Server-authoritative implementation: flight physics, block/entity collision,
//! damage, pickup rules, and network sync. Client handles rendering, particles,
//! and sound playback from synced state and server-broadcast events.
//!
//! Not implemented yet: enchantment effects (Power/Punch/Flame/Piercing) and
//! tipped-arrow potion contents/color sync.

use std::sync::{Arc, Weak};

use glam::DVec3;
use simdnbt::borrow::NbtCompound as BorrowedNbtCompoundView;
use simdnbt::owned::NbtCompound;
use steel_macros::entity_behavior;
use steel_protocol::packets::game::{CTakeItemEntity, SoundSource};
use steel_registry::blocks::block_state_ext::BlockStateExt as _;
use steel_registry::entity_type::EntityTypeRef;
use steel_registry::item_stack::ItemStack;
use steel_registry::vanilla_entity_data::ArrowEntityData;
use steel_registry::{sound_events, vanilla_damage_types, vanilla_entities, vanilla_items};
use steel_utils::locks::SyncMutex;
use steel_utils::{BlockStateId, DowncastType, DowncastTypeKey};

use crate::entity::damage::DamageSource;
use crate::entity::entities::objects::projectiles::abstract_arrow::{AbstractArrow, Pickup};
use crate::entity::{
    Entity, EntityBase, EntityBaseLoad, EntitySyncedData, Projectile, ProjectileBase,
    ProjectileDeflection, RemovalReason, SharedEntity,
};
use crate::inventory::container::Container;
use crate::player::Player;
use crate::world::{ClipHitResult, LevelReader as _, World};

/// Vanilla `AbstractArrow.onHitEntity` crit damage minimum added to half the
/// base roll: `random.nextInt(damage / 2 + CRIT_DAMAGE_FLOOR)`.
const CRIT_DAMAGE_FLOOR: i32 = 2;
/// Vanilla `AbstractArrow.getHitGroundSoundEvent` / `onHitEntity` pitch
/// formula: `NUMERATOR / (random * RANDOM_RANGE + BASE)`.
const HIT_SOUND_PITCH_NUMERATOR: f32 = 1.2;
const HIT_SOUND_PITCH_RANDOM_RANGE: f32 = 0.2;
const HIT_SOUND_PITCH_BASE: f32 = 0.9;
/// Vanilla `AbstractArrow.onHitEntity` post-deflection velocity scale.
/// Applied after `deflect(REVERSE)` (which uses ×−0.5), yielding ×−0.1.
const DEFLECTION_POST_SCALE: f64 = 0.2;
/// Push distance to move the deflected arrow outside the entity's collision
/// volume (vanilla inflated bounding-box margin can be up to 0.3).
const DEFLECTION_COLLISION_ESCAPE: f64 = 0.35;
/// Velocity threshold below which a deflected arrow is considered stopped
/// and discarded as a dropped item.
const DEFLECTION_STOP_THRESHOLD: f64 = 1.0e-7;
/// SteelMC floating-point comparison threshold for near-zero velocity checks
/// (stuck detection, push direction fallback). Not a vanilla constant.
const VELOCITY_EPSILON: f64 = 1.0e-9;
/// Vanilla `AbstractArrow.shouldFall`: AABB inflate distance used to check
/// whether the arrow has free space around its resting position.
const SHOULD_FALL_INFLATE: f64 = 0.06;

/// Per-tick mutable runtime state mirroring vanilla `AbstractArrow` fields.
struct ArrowRuntime {
    /// Vanilla `life` — despawn counter while in ground.
    life: i32,
    /// Vanilla `inGroundTime` — consecutive ticks stuck in a block.
    in_ground_time: i32,
    /// Vanilla `shakeTime` — wobble window right after landing.
    shake_time: i32,
    /// Vanilla `baseDamage` (default 2.0).
    base_damage: f64,
    pickup: Pickup,
    /// Vanilla `lastState` — block that held the arrow when it landed.
    last_state: Option<BlockStateId>,
    /// Entity ID that deflected this arrow last tick, used to suppress
    /// repeated collision while the arrow exits the entity's bounding box.
    last_deflected_from: Option<i32>,
}

impl ArrowRuntime {
    const fn new() -> Self {
        Self {
            life: 0,
            in_ground_time: 0,
            shake_time: 0,
            base_damage: <ArrowEntity as AbstractArrow>::BASE_DAMAGE,
            pickup: Pickup::Disallowed,
            last_state: None,
            last_deflected_from: None,
        }
    }
}

/// An arrow shot from a bow (or dispenser).
#[entity_behavior(class = "Arrow")]
pub struct ArrowEntity {
    /// Common entity fields (id, uuid, position, etc.).
    base: EntityBase,
    /// Vanilla entity type registered for this implementation.
    entity_type: EntityTypeRef,
    /// Synced data (`ID_FLAGS`, `PIERCE_LEVEL`, `IN_GROUND`, effect color).
    entity_data: SyncMutex<ArrowEntityData>,
    /// Shared `Projectile` state (owner / left-owner / has-been-shot).
    projectile_base: ProjectileBase,
    /// Vanilla scalar fields not part of synced data.
    runtime: SyncMutex<ArrowRuntime>,
}

// SAFETY: This key is owned by Steel and uniquely identifies `ArrowEntity`.
unsafe impl DowncastType for ArrowEntity {
    const TYPE_KEY: DowncastTypeKey = DowncastTypeKey::new("steel:entity/arrow");
}

impl AbstractArrow for ArrowEntity {
    fn is_in_ground(&self) -> bool {
        *self.entity_data.lock().abstract_arrow.in_ground.get()
    }

    fn set_in_ground(&self, value: bool) {
        self.entity_data.lock().abstract_arrow.in_ground.set(value);
    }

    fn is_crit_arrow(&self) -> bool {
        *self.entity_data.lock().abstract_arrow.id_flags.get() & 1 != 0
    }

    fn set_crit_arrow(&self, value: bool) {
        let mut data = self.entity_data.lock();
        let flags = *data.abstract_arrow.id_flags.get();
        data.abstract_arrow
            .id_flags
            .set(if value { flags | 1 } else { flags & !1 });
    }

    fn pickup(&self) -> Pickup {
        self.runtime.lock().pickup
    }

    fn set_pickup(&self, pickup: Pickup) {
        self.runtime.lock().pickup = pickup;
    }
}

impl ArrowEntity {
    /// Creates an arrow at `position` with no owner.
    #[must_use]
    pub fn new(entity_type: EntityTypeRef, id: i32, position: DVec3, world: Weak<World>) -> Self {
        Self {
            base: EntityBase::new(id, position, entity_type.dimensions, world),
            entity_type,
            entity_data: SyncMutex::new(ArrowEntityData::new()),
            projectile_base: ProjectileBase::new(),
            runtime: SyncMutex::new(ArrowRuntime::new()),
        }
    }

    /// Creates an arrow from saved base data.
    #[must_use]
    pub fn from_saved(entity_type: EntityTypeRef, load: EntityBaseLoad) -> Self {
        Self {
            base: EntityBase::from_load(load, entity_type.dimensions),
            entity_type,
            entity_data: SyncMutex::new(ArrowEntityData::new()),
            projectile_base: ProjectileBase::new(),
            runtime: SyncMutex::new(ArrowRuntime::new()),
        }
    }

    fn is_in_ground(&self) -> bool {
        <Self as AbstractArrow>::is_in_ground(self)
    }

    fn set_in_ground(&self, value: bool) {
        <Self as AbstractArrow>::set_in_ground(self, value);
    }

    fn is_crit_arrow(&self) -> bool {
        <Self as AbstractArrow>::is_crit_arrow(self)
    }

    /// Vanilla `AbstractArrow.setCritArrow` (synced flag bit 0).
    pub fn set_crit_arrow(&self, value: bool) {
        <Self as AbstractArrow>::set_crit_arrow(self, value);
    }

    /// Sets the vanilla `Pickup` rule used by [`Entity::player_touch`].
    pub fn set_pickup(&self, pickup: Pickup) {
        <Self as AbstractArrow>::set_pickup(self, pickup);
    }

    /// Vanilla `AbstractArrow.shouldFall`: free space around the resting point.
    fn should_fall(&self) -> bool {
        let Some(world) = self.level() else {
            return false;
        };
        let pos = self.position();
        for dx in [-SHOULD_FALL_INFLATE, SHOULD_FALL_INFLATE] {
            for dy in [-SHOULD_FALL_INFLATE, SHOULD_FALL_INFLATE] {
                for dz in [-SHOULD_FALL_INFLATE, SHOULD_FALL_INFLATE] {
                    let p = DVec3::new(pos.x + dx, pos.y + dy, pos.z + dz);
                    let bpos = steel_utils::BlockPos::new(
                        p.x.floor() as i32,
                        p.y.floor() as i32,
                        p.z.floor() as i32,
                    );
                    if !world.get_block_state(bpos).is_air() {
                        return false;
                    }
                }
            }
        }
        true
    }

    /// Vanilla `AbstractArrow.startFalling`.
    fn start_falling(&self) {
        self.set_in_ground(false);
        let jitter = DVec3::new(
            f64::from(
                rand::random::<f32>() * <ArrowEntity as AbstractArrow>::START_FALLING_JITTER_SCALE,
            ),
            f64::from(
                rand::random::<f32>() * <ArrowEntity as AbstractArrow>::START_FALLING_JITTER_SCALE,
            ),
            f64::from(
                rand::random::<f32>() * <ArrowEntity as AbstractArrow>::START_FALLING_JITTER_SCALE,
            ),
        );
        self.set_velocity(self.velocity() * jitter);
        self.runtime.lock().life = 0;
    }

    /// Vanilla `AbstractArrow.tickDespawn`.
    fn tick_despawn(&self) {
        let mut runtime = self.runtime.lock();
        runtime.life += 1;
        if runtime.life >= <ArrowEntity as AbstractArrow>::DESPAWN_LIFE {
            drop(runtime);
            self.set_removed(RemovalReason::Discarded);
        }
    }

    /// Vanilla `AbstractArrow.onHitBlock` body: back off along the impact
    /// direction, stop, play the hit sound, and stick (`IN_GROUND` + shake).
    fn stick_in_ground(&self, world: &Arc<World>) {
        if self.is_in_ground() {
            return;
        }
        let movement = self.velocity();
        let offset = DVec3::new(
            movement.x.signum() * <ArrowEntity as AbstractArrow>::HIT_BLOCK_BACKOFF,
            movement.y.signum() * <ArrowEntity as AbstractArrow>::HIT_BLOCK_BACKOFF,
            movement.z.signum() * <ArrowEntity as AbstractArrow>::HIT_BLOCK_BACKOFF,
        );
        let _ = self.try_set_position(self.position() - offset);
        self.set_velocity(DVec3::ZERO);

            let pitch = HIT_SOUND_PITCH_NUMERATOR
                / (rand::random::<f32>() * HIT_SOUND_PITCH_RANDOM_RANGE + HIT_SOUND_PITCH_BASE);
        world.play_sound_at(
            &sound_events::ENTITY_ARROW_HIT,
            SoundSource::Neutral,
            self.position(),
            1.0,
            pitch,
            None,
        );

        self.set_in_ground(true);
        let mut runtime = self.runtime.lock();
        runtime.shake_time = <ArrowEntity as AbstractArrow>::SHAKE_TIME;
        // After the position offset, update last_state to match the block at
        // the new resting position so the in-ground changed-block check does
        // not incorrectly detect a block change and restart the flight loop.
        runtime.last_state = Some(world.get_block_state(self.block_position()));
        drop(runtime);
        self.set_crit_arrow(false);
        self.entity_data.lock().abstract_arrow.pierce_level.set(0);
    }
}

impl Entity for ArrowEntity {
    fn base(&self) -> &EntityBase {
        &self.base
    }

    fn entity_type(&self) -> EntityTypeRef {
        self.entity_type
    }

    /// Vanilla `AbstractArrow.tick`.
    fn tick(&self) {
        let Some(world) = self.level() else {
            return;
        };

        self.set_old_position_to_current();
        self.base().set_old_rotation_to_current();

        {
            let mut runtime = self.runtime.lock();
            if runtime.shake_time > 0 {
                runtime.shake_time -= 1;
            }
        }

        // Resting inside a non-air cell counts as stuck (approximation of the
        // vanilla point-in-collision-shape test; arrows stop inside the shape).
        if !world.get_block_state(self.block_position()).is_air()
            && self.velocity().length_squared() < VELOCITY_EPSILON
        {
            self.set_in_ground(true);
        }

        if self.is_in_ground() && !self.no_physics() {
            let current = world.get_block_state(self.block_position());
            // A missing `last_state` (fresh load without persisted block data)
            // counts as changed, like vanilla's null `lastState` check.
            let changed = self.runtime.lock().last_state != Some(current);
            if changed && self.should_fall() {
                self.start_falling();
                // Falls through into the flight branch like vanilla.
            } else {
                self.tick_despawn();
                self.runtime.lock().in_ground_time += 1;
                if self.is_alive() {
                    self.apply_effects_from_blocks();
                }
                return;
            }
        } else {
            self.runtime.lock().in_ground_time = 0;
        }

        // Flight branch: water drag, movement, hit detection, air drag, gravity.
        if self.is_in_water() {
            self.set_velocity(self.velocity() * <ArrowEntity as AbstractArrow>::WATER_INERTIA);
        }

        self.update_rotation();
        self.check_left_owner();

        let hit = self.get_hit_result_on_move_vector();
        let new_position = match &hit {
            Some(result) => result.location(),
            None => self.position() + self.velocity(),
        };
        if let Err(error) = self.try_set_position(new_position) {
            log::debug!("failed to advance arrow {}: {error}", self.id());
            self.set_removed(RemovalReason::Discarded);
            return;
        }
        self.runtime.lock().last_state = Some(world.get_block_state(self.block_position()));

        // Vanilla ordering: hit detection runs BEFORE air drag and gravity.
        // Deflection changes velocity, so drag/gravity must apply to the
        // post-deflection velocity, not the pre-hit velocity.
        if let Some(result) = hit
            && self.is_alive()
        {
            self.hit_target_or_deflect_self(&result);
        }

        if !self.is_in_water() {
            self.set_velocity(self.velocity() * <ArrowEntity as AbstractArrow>::INERTIA);
        }
        // Vanilla skips gravity once this tick's hit has grounded the arrow.
        if !self.is_in_ground() {
            self.apply_gravity();
        }

        // Vanilla runs `super.tick()` at the very end of the flight branch
        // (grounded arrows return above without reaching it).
        self.projectile_base_tick();
    }

    fn get_default_gravity(&self) -> f64 {
        <ArrowEntity as AbstractArrow>::GRAVITY
    }

    fn sound_source(&self) -> SoundSource {
        SoundSource::Neutral
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

    fn attackable(&self) -> bool {
        // TODO: vanilla returns the REDIRECTABLE_PROJECTILE tag check, which
        // lets players deflect arrows in melee. Revisit once combat deflection
        // exists on the Steel side.
        false
    }

    fn synced_data(&self) -> Option<&dyn EntitySyncedData> {
        Some(&self.entity_data)
    }

    fn hurt(&self, _world: &World, _source: &DamageSource, _amount: f32) -> bool {
        // Vanilla `Projectile.hurtServer` marks hurt but never takes damage.
        false
    }

    fn save_additional(&self, nbt: &mut NbtCompound) {
        self.save_projectile(nbt);
        let runtime = self.runtime.lock();
        nbt.insert("life", i16::try_from(runtime.life).unwrap_or(i16::MAX));
        nbt.insert("shake", i8::try_from(runtime.shake_time).unwrap_or(i8::MAX));
        nbt.insert("damage", runtime.base_damage);
        nbt.insert("pickup", runtime.pickup as i8);
        nbt.insert("crit", i8::from(self.is_crit_arrow()));
        nbt.insert("inGround", i8::from(self.is_in_ground()));
    }

    fn load_additional(&self, nbt: BorrowedNbtCompoundView<'_, '_>) {
        self.load_projectile(nbt);
        {
            let mut runtime = self.runtime.lock();
            runtime.life = i32::from(nbt.short("life").unwrap_or(0));
            runtime.shake_time = i32::from(nbt.byte("shake").unwrap_or(0));
            runtime.base_damage = nbt
                .double("damage")
                .unwrap_or(<ArrowEntity as AbstractArrow>::BASE_DAMAGE);
            runtime.pickup = Pickup::from(nbt.byte("pickup").unwrap_or(0));
        }
        self.set_crit_arrow(nbt.byte("crit").is_some_and(|v| v != 0));
        self.set_in_ground(nbt.byte("inGround").is_some_and(|v| v != 0));
    }

    /// Vanilla `AbstractArrow.playerTouch`: pick up only once settled.
    fn player_touch(self: Arc<Self>, player: &Arc<Player>) {
        if self.is_removed() || !self.is_in_ground() || self.runtime.lock().shake_time > 0 {
            return;
        }

        // Vanilla tryPickup: check pickup permission and try inventory add.
        let picked = match self.runtime.lock().pickup {
            Pickup::Disallowed => false,
            Pickup::Allowed => {
                let mut item = ItemStack::new(&vanilla_items::ARROW);
                let added = player.inventory.lock().add(&mut item);
                // Only succeed if the entire stack was consumed (inventory had space).
                added && item.is_empty()
            }
            Pickup::CreativeOnly => player.has_infinite_materials(),
        };

        if !picked {
            return;
        }

        // Vanilla player.take: send pickup animation to tracking clients.
        if let Some(world) = self.level() {
            let take_packet = CTakeItemEntity::new(self.id(), player.id(), 1);
            world.broadcast_to_nearby(
                steel_utils::ChunkPos::from_entity_pos(self.position()),
                take_packet,
                None,
            );
        }

        self.set_removed(RemovalReason::Discarded);
    }
}

impl Projectile for ArrowEntity {
    fn projectile_base(&self) -> &ProjectileBase {
        &self.projectile_base
    }

    /// Vanilla `AbstractArrow.onHitEntity`.
    fn on_hit_entity(&self, entity: &SharedEntity, _location: DVec3) {
        // Skip if this entity deflected us last tick — gives the arrow one
        // tick to exit the bounding box before processing another collision.
        if self.runtime.lock().last_deflected_from == Some(entity.id()) {
            self.runtime.lock().last_deflected_from = None;
            return;
        }

        let Some(world) = entity.level() else {
            return;
        };

        let speed = self.velocity().length() as f32;
        let raw = speed * self.runtime.lock().base_damage as f32;
        let mut damage_amount = raw.ceil().clamp(0.0, i32::MAX as f32) as i32;
        if self.is_crit_arrow() {
            damage_amount += (rand::random::<u32>()
                % (damage_amount / 2 + CRIT_DAMAGE_FLOOR) as u32)
                as i32;
        }

        // Vanilla `damageSources().arrow(this, owner != null ? owner : this)`.
        let damage = DamageSource::environment(&vanilla_damage_types::ARROW)
            .with_direct_entity(self.id())
            .with_causing_entity(self.get_owner().map_or(self.id(), |owner| owner.id()));

        // Save fire ticks before damage check — restored on deflection so the
        // entity's fire state is unchanged when the arrow bounces off.
        let remaining_fire_ticks = entity.remaining_fire_ticks();

        if entity.hurt(&world, &damage, damage_amount as f32) {
            // Arrow dealt damage — clear any deflection cooldown since the
            // arrow is about to be discarded.
            self.runtime.lock().last_deflected_from = None;

            // Vanilla lets arrows pass through endermen without sound or discard.
            if entity.entity_type() == &vanilla_entities::ENDERMAN {
                return;
            }
        let pitch = HIT_SOUND_PITCH_NUMERATOR
            / (rand::random::<f32>() * HIT_SOUND_PITCH_RANDOM_RANGE + HIT_SOUND_PITCH_BASE);
            world.play_sound_at(
                &sound_events::ENTITY_ARROW_HIT,
                SoundSource::Neutral,
                self.position(),
                1.0,
                pitch,
                None,
            );
            self.set_removed(RemovalReason::Discarded);
        } else {
            // Vanilla: restore fire ticks on deflection so the entity's fire
            // state is unchanged when the arrow bounces off.
            entity.set_remaining_fire_ticks(remaining_fire_ticks);
            // Vanilla deflect path: `REVERSE` + `scale(0.2)`.
            // The arrow was blocked (e.g. shield). Reflect the velocity and
            // push the arrow outside the entity's collision volume so the
            // next tick's raycast won't re-detect the same entity.
            self.deflect(
                ProjectileDeflection::Reverse,
                Some(entity.as_ref() as &dyn Entity),
                self.projectile_owner_uuid(),
                self.projectile_owner().as_ref(),
                false,
            );
            self.set_velocity(self.velocity() * DEFLECTION_POST_SCALE);

            // Push the arrow along the reflected direction so it exits the
            // entity's inflated bounding box (margin can be up to 0.3).
            let velocity = self.velocity();
            let push_direction = if velocity.length_squared() > VELOCITY_EPSILON {
                velocity.normalize()
            } else {
                let diff = self.position() - entity.position();
                if diff.length_squared() > VELOCITY_EPSILON {
                    diff.normalize()
                } else {
                    DVec3::Y
                }
            };
            let _ = self.try_set_position(self.position() + push_direction * DEFLECTION_COLLISION_ESCAPE);

            // Suppress re-collision with this entity on the next tick.
            self.runtime.lock().last_deflected_from = Some(entity.id());

            if self.velocity().length_squared() < DEFLECTION_STOP_THRESHOLD {
                if self.runtime.lock().pickup == Pickup::Allowed {
                    world.spawn_item(self.position(), ItemStack::new(&vanilla_items::ARROW));
                }
                self.set_removed(RemovalReason::Discarded);
            }
        }
    }

    /// Vanilla `AbstractArrow.onHitBlock`: record `lastState`, dispatch to
    /// target blocks, then stick into the hit block.
    fn on_hit_block(&self, hit: &ClipHitResult) {
        if let Some(world) = self.level() {
            self.runtime.lock().last_state = Some(world.get_block_state(hit.block_pos));
            self.projectile_on_hit_block(hit);
            self.stick_in_ground(&world);
        } else {
            self.projectile_on_hit_block(hit);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Weak;

    use glam::DVec3;
    use simdnbt::owned::NbtCompound;
    use steel_registry::{init_vanilla_registry, vanilla_entities};

    use super::ArrowEntity;
    use crate::entity::entities::objects::projectiles::Pickup;
    use crate::entity::{Entity, Projectile};
    use crate::world::World;

    #[test]
    fn shoot_aligns_velocity_with_direction() {
        init_vanilla_registry();

        let arrow = ArrowEntity::new(
            &vanilla_entities::ARROW,
            1,
            DVec3::ZERO,
            Weak::<World>::new(),
        );
        arrow.shoot(DVec3::new(0.0, 0.0, 1.0), 3.0, 1.0);

        let velocity = arrow.velocity();
        assert!(velocity.z > 0.0);
        assert!((velocity.length() - 3.0).abs() < 0.1);
        assert!(velocity.x.abs() < 0.1 && velocity.y.abs() < 0.1);
    }

    #[test]
    fn pickup_defaults_disallowed_and_round_trips() {
        init_vanilla_registry();

        let arrow = ArrowEntity::new(
            &vanilla_entities::ARROW,
            1,
            DVec3::ZERO,
            Weak::<World>::new(),
        );
        assert_eq!(Pickup::from(0), Pickup::Disallowed);
        assert_eq!(Pickup::from(1), Pickup::Allowed);
        assert_eq!(Pickup::from(2), Pickup::CreativeOnly);
        assert_eq!(Pickup::from(99), Pickup::Disallowed);

        arrow.set_pickup(Pickup::CreativeOnly);
        let mut nbt = NbtCompound::new();
        arrow.save_additional(&mut nbt);
        assert_eq!(nbt.byte("pickup"), Some(2));
    }

    #[test]
    fn crit_flag_toggles_synced_value() {
        init_vanilla_registry();

        let arrow = ArrowEntity::new(
            &vanilla_entities::ARROW,
            1,
            DVec3::ZERO,
            Weak::<World>::new(),
        );
        assert!(!arrow.is_crit_arrow());
        arrow.set_crit_arrow(true);
        assert!(arrow.is_crit_arrow());
        arrow.set_crit_arrow(false);
        assert!(!arrow.is_crit_arrow());
    }
}
