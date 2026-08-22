//! Arrow projectile entity (`net.minecraft.world.entity.projectile.arrow.Arrow`,
//! backed by `AbstractArrow`).
//!
//! Mirrors the vanilla flight model: stick into blocks (`IN_GROUND`), gravity
//! 0.05, air drag 0.99 (water 0.6), entity-hit damage from impact speed
//! (`ceil(speed * baseDamage)` with a crit roll at full draw), despawn after
//! 1200 in-ground ticks, and ground pickup via [`Entity::player_touch`].
//!
//! Not implemented yet (missing foundations): enchantment effects
//! (Power/Punch/Flame/Piercing) and tipped-arrow potion contents/color sync.
//! Crit/bubble particles are client-local in vanilla, so there is no server work.

use std::sync::{Arc, Weak};

use glam::DVec3;
use simdnbt::borrow::NbtCompound as BorrowedNbtCompoundView;
use simdnbt::owned::NbtCompound;
use steel_macros::entity_behavior;
use steel_protocol::packets::game::SoundSource;
use steel_registry::blocks::block_state_ext::BlockStateExt as _;
use steel_registry::entity_type::EntityTypeRef;
use steel_registry::item_stack::ItemStack;
use steel_registry::vanilla_entity_data::ArrowEntityData;
use steel_registry::{sound_events, vanilla_damage_types, vanilla_entities, vanilla_items};
use steel_utils::locks::SyncMutex;
use steel_utils::{BlockStateId, DowncastType, DowncastTypeKey};

use crate::entity::damage::DamageSource;
use crate::entity::{
    Entity, EntityBase, EntityBaseLoad, EntitySyncedData, Projectile, ProjectileBase,
    RemovalReason, SharedEntity,
};
use crate::player::Player;
use crate::world::{ClipHitResult, LevelReader as _, World};

/// Vanilla `AbstractArrow.ARROW_BASE_DAMAGE`.
const ARROW_BASE_DAMAGE: f64 = 2.0;
/// Vanilla `AbstractArrow.SHAKE_TIME`.
const SHAKE_TIME: i32 = 7;
/// Vanilla `AbstractArrow.WATER_INERTIA`.
const WATER_INERTIA: f64 = 0.6;
/// Vanilla `AbstractArrow.INERTIA` (air drag).
const INERTIA: f64 = 0.99;
/// Vanilla `AbstractArrow.tickDespawn`: 1200 ticks (~60s) before discard.
const DESPAWN_LIFE: i32 = 1200;
/// Vanilla `AbstractArrow.getDefaultGravity`.
const GRAVITY: f64 = 0.05;
/// Vanilla `AbstractArrow.startFalling`: per-axis random velocity multiplier
/// upper bound applied when a stuck arrow pops free.
const START_FALLING_JITTER_SCALE: f32 = 0.2;
/// Vanilla `AbstractArrow.onHitBlock`: distance backed off along the impact
/// sign direction before the arrow sticks into the block.
const HIT_BLOCK_BACKOFF: f64 = 0.05;

/// Vanilla `AbstractArrow.Pickup`. Ordinals match the vanilla enum for NBT.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Pickup {
    /// Vanilla `DISALLOWED` — nobody can pick the arrow up.
    #[default]
    Disallowed = 0,
    /// Vanilla `ALLOWED` — players can pick the arrow up.
    Allowed = 1,
    /// Vanilla `CREATIVE_ONLY` — only players with infinite materials.
    CreativeOnly = 2,
}

impl From<i8> for Pickup {
    fn from(value: i8) -> Self {
        match value {
            1 => Self::Allowed,
            2 => Self::CreativeOnly,
            _ => Self::Disallowed,
        }
    }
}

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
}

impl ArrowRuntime {
    const fn new() -> Self {
        Self {
            life: 0,
            in_ground_time: 0,
            shake_time: 0,
            base_damage: ARROW_BASE_DAMAGE,
            pickup: Pickup::Disallowed,
            last_state: None,
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
        *self.entity_data.lock().abstract_arrow.in_ground.get()
    }

    fn set_in_ground(&self, value: bool) {
        self.entity_data.lock().abstract_arrow.in_ground.set(value);
    }

    fn is_crit_arrow(&self) -> bool {
        *self.entity_data.lock().abstract_arrow.id_flags.get() & 1 != 0
    }

    /// Vanilla `AbstractArrow.setCritArrow` (synced flag bit 0).
    pub fn set_crit_arrow(&self, value: bool) {
        let mut data = self.entity_data.lock();
        let flags = *data.abstract_arrow.id_flags.get();
        data.abstract_arrow
            .id_flags
            .set(if value { flags | 1 } else { flags & !1 });
    }

    /// Sets the vanilla `Pickup` rule used by [`Entity::player_touch`].
    pub fn set_pickup(&self, pickup: Pickup) {
        self.runtime.lock().pickup = pickup;
    }

    /// Vanilla `AbstractArrow.shouldFall`: free space around the resting point.
    fn should_fall(&self) -> bool {
        self.is_free(DVec3::ZERO)
    }

    /// Vanilla `AbstractArrow.startFalling`.
    fn start_falling(&self) {
        self.set_in_ground(false);
        let jitter = DVec3::new(
            f64::from(rand::random::<f32>() * START_FALLING_JITTER_SCALE),
            f64::from(rand::random::<f32>() * START_FALLING_JITTER_SCALE),
            f64::from(rand::random::<f32>() * START_FALLING_JITTER_SCALE),
        );
        self.set_velocity(self.velocity() * jitter);
        self.runtime.lock().life = 0;
    }

    /// Vanilla `AbstractArrow.tickDespawn`.
    fn tick_despawn(&self) {
        let mut runtime = self.runtime.lock();
        runtime.life += 1;
        if runtime.life >= DESPAWN_LIFE {
            drop(runtime);
            self.set_removed(RemovalReason::Discarded);
        }
    }

    /// Vanilla `AbstractArrow.onHitBlock` body: back off along the impact
    /// direction, stop, play the hit sound, and stick (`IN_GROUND` + shake).
    fn stick_in_ground(&self, world: &Arc<World>) {
        let movement = self.velocity();
        let offset = DVec3::new(
            movement.x.signum() * HIT_BLOCK_BACKOFF,
            movement.y.signum() * HIT_BLOCK_BACKOFF,
            movement.z.signum() * HIT_BLOCK_BACKOFF,
        );
        let _ = self.try_set_position(self.position() - offset);
        self.set_velocity(DVec3::ZERO);

        let pitch = 1.2 / (rand::random::<f32>() * 0.2 + 0.9);
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
        runtime.shake_time = SHAKE_TIME;
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

        if self.runtime.lock().shake_time > 0 {
            self.runtime.lock().shake_time -= 1;
        }

        // Resting inside a non-air cell counts as stuck (approximation of the
        // vanilla point-in-collision-shape test; arrows stop inside the shape).
        if !world.get_block_state(self.block_position()).is_air() && self.velocity() == DVec3::ZERO
        {
            self.set_in_ground(true);
        }

        if self.is_in_ground() {
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
                return;
            }
        } else {
            self.runtime.lock().in_ground_time = 0;
        }

        // Flight branch. Water drag applies pre-move; bubble/crit particles are
        // VANILLA CLIENT-LOCAL so there is no server work for them.
        if self.is_in_water() {
            self.set_velocity(self.velocity() * WATER_INERTIA);
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

        if !self.is_in_water() {
            self.set_velocity(self.velocity() * INERTIA);
        }
        // Vanilla skips gravity once this tick's hit has grounded the arrow.
        if !self.is_in_ground() {
            self.apply_gravity();
        }

        if let Some(result) = hit
            && self.is_alive()
        {
            self.hit_target_or_deflect_self(&result);
        }

        // Vanilla runs `super.tick()` at the very end of the flight branch
        // (grounded arrows return above without reaching it).
        self.projectile_base_tick();
    }

    fn get_default_gravity(&self) -> f64 {
        GRAVITY
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
            runtime.base_damage = nbt.double("damage").unwrap_or(ARROW_BASE_DAMAGE);
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
        let collected = match self.runtime.lock().pickup {
            Pickup::Disallowed => false,
            Pickup::CreativeOnly => player.has_infinite_materials(),
            Pickup::Allowed => true,
        };
        if !collected {
            return;
        }
        // TODO: leave the arrow in-world when the inventory cannot fit it
        // (vanilla only picks up when `inventory.add` succeeds fully).
        player.add_item_or_drop(ItemStack::new(&vanilla_items::ARROW));
        self.set_removed(RemovalReason::Discarded);
    }
}

impl Projectile for ArrowEntity {
    fn projectile_base(&self) -> &ProjectileBase {
        &self.projectile_base
    }

    /// Vanilla `AbstractArrow.onHitEntity`.
    fn on_hit_entity(&self, entity: &SharedEntity, _location: DVec3) {
        let Some(world) = entity.level() else {
            return;
        };

        let speed = self.velocity().length() as f32;
        let raw = speed * self.runtime.lock().base_damage as f32;
        let mut damage_amount = raw.ceil().clamp(0.0, i32::MAX as f32) as i32;
        if self.is_crit_arrow() {
            damage_amount += (rand::random::<u32>() % (damage_amount / 2 + 2) as u32) as i32;
        }

        // Vanilla `damageSources().arrow(this, owner != null ? owner : this)`.
        let damage = DamageSource::environment(&vanilla_damage_types::ARROW)
            .with_direct_entity(self.id())
            .with_causing_entity(self.get_owner().map_or(self.id(), |owner| owner.id()));

        if entity.hurt(&world, &damage, damage_amount as f32) {
            // Vanilla lets arrows pass through endermen without sound or discard.
            if entity.entity_type() == &vanilla_entities::ENDERMAN {
                return;
            }
            let pitch = 1.2 / (rand::random::<f32>() * 0.2 + 0.9);
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
            // Vanilla deflect path: `REVERSE` deflection plus a `scale(0.2)`
            // shrink; a near-stopped allowed arrow drops as an item.
            self.set_velocity(self.velocity() * -0.2);
            if self.velocity().length_squared() < 1.0e-7 {
                if self.runtime.lock().pickup == Pickup::Allowed {
                    world.spawn_item(self.position(), ItemStack::new(&vanilla_items::ARROW));
                }
                self.set_removed(RemovalReason::Discarded);
            }
        }
    }

    /// Vanilla `AbstractArrow.onHitBlock`: record `lastState` at the hit block,
    /// run the base dispatch so target blocks etc. can react, then stick.
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

    use super::{ArrowEntity, Pickup};
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
