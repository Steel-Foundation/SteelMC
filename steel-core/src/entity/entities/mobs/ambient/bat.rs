//! Vanilla Bat entity: rests on cave ceilings and flutters through the dark.

use std::f32::consts::PI;
use std::sync::{Arc, Weak};

use glam::DVec3;
use simdnbt::borrow::NbtCompound as BorrowedNbtCompoundView;
use simdnbt::owned::NbtCompound;
use steel_macros::entity_behavior;
use steel_math::wrap_degrees;
use steel_registry::blocks::block_state_ext::BlockStateExt as _;
use steel_registry::entity_type::{EntityDimensions, EntityTypeRef};
use steel_registry::sound_event::SoundEventRef;
use steel_registry::vanilla_entity_data::BatEntityData;
use steel_registry::{level_events, sound_events, vanilla_attributes};
use steel_utils::locks::SyncMutex;
use steel_utils::{BlockPos, BlockStateId, DowncastType, DowncastTypeKey};

use crate::entity::ai::targeting::TargetingConditions;
use crate::entity::damage::DamageSource;
use crate::entity::{
    Entity, EntityBase, EntityBaseLoad, EntityMovementEmission, EntityPose, EntitySyncedData,
    LivingEntity, LivingEntityBase, LivingTravelInput, Mob, MobBase,
};
use crate::physics::MoveResult;
use crate::world::{World, is_redstone_conductor};

const FLAG_RESTING: i8 = 1;
const BAT_RESTING_TARGETING_RANGE: f64 = 4.0;
const DEFAULT_STEP_HEIGHT: f32 = 0.6;

/// Vanilla `Bat.customServerAiStep` chance divisors (`nextInt` bounds).
const RESTING_YAW_ROLL_CHANCE: i32 = 200;
const RESTING_YAW_RANGE: i32 = 360;
const RESTING_RETARGET_CHANCE: i32 = 30;
const RESTING_SETTLE_CHANCE: i32 = 100;

/// Vanilla `Bat.customServerAiStep` target-selection bounds.
const TARGET_HORIZONTAL_RANGE: i32 = 7;
const TARGET_VERTICAL_RANGE: i32 = 6;
const TARGET_VERTICAL_OFFSET: f64 = 2.0;
const TARGET_RECENTER_DISTANCE: f64 = 2.0;

/// Vanilla `Bat.customServerAiStep` steering constants.
const STEER_TARGET_XZ: f64 = 0.5;
const STEER_TARGET_Y: f32 = 0.7;
const STEER_STEP: f32 = 0.1;
/// Forward flight input vanilla assigns to `zza`.
const FLIGHT_FORWARD_INPUT: f32 = 0.5;

const AIRBORNE_Y_DRAG: f64 = 0.6;

/// Vanilla bat entity.
#[entity_behavior(class = "Bat")]
pub struct BatEntity {
    base: EntityBase,
    entity_type: EntityTypeRef,
    living_base: LivingEntityBase,
    mob_base: MobBase,
    entity_data: SyncMutex<BatEntityData>,
    target_position: SyncMutex<Option<BlockPos>>,
}

// SAFETY: This key is owned by Steel and uniquely identifies `BatEntity`.
unsafe impl DowncastType for BatEntity {
    const TYPE_KEY: DowncastTypeKey = DowncastTypeKey::new("steel:entity/bat");
}

impl BatEntity {
    /// Creates a new bat at runtime.
    #[must_use]
    pub fn new(entity_type: EntityTypeRef, id: i32, position: DVec3, world: Weak<World>) -> Self {
        Self::new_with_base(
            EntityBase::new(id, position, entity_type.dimensions, world),
            entity_type,
        )
    }

    /// Reconstructs a bat from persisted base entity state.
    #[must_use]
    pub fn from_saved(entity_type: EntityTypeRef, load: EntityBaseLoad) -> Self {
        Self::new_with_base(
            EntityBase::from_load(load, entity_type.dimensions),
            entity_type,
        )
    }

    fn new_with_base(base: EntityBase, entity_type: EntityTypeRef) -> Self {
        let living_base = LivingEntityBase::new(entity_type);
        let mob_base = MobBase::new();
        let mut entity_data = BatEntityData::new();
        living_base.initialize_synced_data(&mut entity_data);
        // Vanilla `Bat` starts resting on the server; save data may override it.
        entity_data.id_flags.set(FLAG_RESTING);

        Self {
            base,
            entity_type,
            living_base,
            mob_base,
            entity_data: SyncMutex::new(entity_data),
            target_position: SyncMutex::new(None),
        }
    }

    /// Returns vanilla `Bat.isResting`.
    #[must_use]
    pub fn is_resting(&self) -> bool {
        (self.entity_data.lock().id_flags.get() & FLAG_RESTING) != 0
    }

    /// Sets vanilla `Bat.setResting`.
    pub fn set_resting(&self, resting: bool) {
        let mut data = self.entity_data.lock();
        let current = *data.id_flags.get();
        let updated = if resting {
            current | FLAG_RESTING
        } else {
            current & !FLAG_RESTING // equal to -2
        };
        data.id_flags.set(updated);
    }

    //just default implementation
    fn update_dirty_mob_effect_entity_data(&self) {
        if !self.living_base.take_effects_dirty() {
            return;
        }

        let display = self.living_base.mob_effect_display_state();

        {
            let mut entity_data = self.entity_data.lock();
            let living = entity_data.living_entity_mut();
            living.effect_particles.set(display.particles);
            living.effect_ambience.set(display.ambient);
        }

        self.entity_data.set_base_invisible_flag(display.invisible);
    }

    /// Runs the resting branch of vanilla `Bat.customServerAiStep`.
    fn tick_resting_ai(&self, world: &World, pos: BlockPos, above: BlockPos) {
        let silent = self.is_silent();
        if is_redstone_conductor(world, world.get_block_state(above), pos) {
            if rand::random_range(0..RESTING_YAW_ROLL_CHANCE) == 0 {
                self.set_y_head_rot(rand::random_range(0..RESTING_YAW_RANGE) as f32);
            }

            let conditions =
                TargetingConditions::for_non_combat().range(BAT_RESTING_TARGETING_RANGE);
            let targeter: &dyn LivingEntity = self;
            let player_near = world
                .nearest_player(self.position(), BAT_RESTING_TARGETING_RANGE, |player| {
                    conditions.test(world, Some(targeter), player)
                })
                .is_some();
            if !player_near {
                return;
            }
        }

        self.set_resting(false);
        if !silent {
            world.level_event(level_events::SOUND_BAT_LIFTOFF, pos, 0, None);
        }
    }

    /// Runs the flying branch of vanilla `Bat.customServerAiStep`.
    fn tick_flying_ai(&self, world: &World, above: BlockPos) {
        let mut target_position = *self.target_position.lock();
        if target_position.is_some_and(|target| {
            !world.get_block_state(target).is_air() || target.y() <= world.get_min_y()
        }) {
            target_position = None;
        }

        let needs_new_target = target_position.is_none()
            || rand::random_range(0..RESTING_RETARGET_CHANCE) == 0
            || target_position.is_some_and(|target| {
                closer_to_center_than(target, self.position(), TARGET_RECENTER_DISTANCE)
            });
        if needs_new_target {
            target_position = Some(self.random_target_position());
        }
        *self.target_position.lock() = target_position;

        let Some(target) = target_position else {
            return;
        };

        let position = self.position();
        let dx = f64::from(target.x()) + 0.5 - position.x;
        let dy = f64::from(target.y()) + 0.1 - position.y;
        let dz = f64::from(target.z()) + 0.5 - position.z;

        let movement = self.velocity();
        let steer_step = f64::from(STEER_STEP);
        let new_movement = movement
            + DVec3::new(
                (dx.signum() * STEER_TARGET_XZ - movement.x) * steer_step,
                (dy.signum() * f64::from(STEER_TARGET_Y) - movement.y) * steer_step,
                (dz.signum() * STEER_TARGET_XZ - movement.z) * steer_step,
            );
        self.set_velocity(new_movement);

        let y_rot_d = (new_movement.z.atan2(new_movement.x) * 180.0 / f64::from(PI)) as f32 - 90.0;
        let (yaw, pitch) = self.rotation();
        let rot_diff = wrap_degrees(y_rot_d - yaw);

        // Vanilla assigns `zza` before applying the yaw delta; the mob move control
        // resets it later in the same server AI step, so keep the assignment for
        // parity with the vanilla order.
        let input = self.travel_input();
        self.set_travel_input(LivingTravelInput::new(
            input.sideways(),
            input.vertical(),
            FLIGHT_FORWARD_INPUT,
        ));

        self.set_rotation((yaw + rot_diff, pitch));

        if rand::random_range(0..RESTING_SETTLE_CHANCE) == 0
            && is_redstone_conductor(world, world.get_block_state(above), above)
        {
            self.set_resting(true);
        }
    }

    /// Picks a random target block around the bat, mirroring vanilla's
    /// `BlockPos.containing` bounds.
    fn random_target_position(&self) -> BlockPos {
        let position = self.position();
        BlockPos::containing(
            position.x
                + f64::from(
                    rand::random_range(0..TARGET_HORIZONTAL_RANGE)
                        - rand::random_range(0..TARGET_HORIZONTAL_RANGE),
                ),
            position.y + f64::from(rand::random_range(0..TARGET_VERTICAL_RANGE))
                - TARGET_VERTICAL_OFFSET,
            position.z
                + f64::from(
                    rand::random_range(0..TARGET_HORIZONTAL_RANGE)
                        - rand::random_range(0..TARGET_HORIZONTAL_RANGE),
                ),
        )
    }
}

/// Mirrors vanilla `Vec3i.closerToCenterThan`.
fn closer_to_center_than(pos: BlockPos, position: DVec3, distance: f64) -> bool {
    let (center_x, center_y, center_z) = pos.get_center();
    let dx = center_x - position.x;
    let dy = center_y - position.y;
    let dz = center_z - position.z;
    dx * dx + dy * dy + dz * dz < distance * distance
}

impl Entity for BatEntity {
    fn base(&self) -> &EntityBase {
        &self.base
    }

    fn entity_type(&self) -> EntityTypeRef {
        self.entity_type
    }

    fn base_tick(&self) {
        Mob::base_tick_mob(self);
    }

    fn tick(&self) {
        LivingEntity::tick_living_entity(self);

        // Vanilla `Bat.setupAnimationStates` only drives client-side `AnimationState`s.

        if self.is_resting() {
            self.set_velocity(DVec3::ZERO);
            let height = f64::from(self.base().dimensions().height);
            let current = self.position();
            let snapped = DVec3::new(current.x, current.y.floor() + 1.0 - height, current.z);
            if snapped != current
                && let Err(error) = self.try_set_position(snapped)
            {
                tracing::debug!(
                    entity_id = self.id(),
                    "bat resting position update failed: {error}"
                );
            }
        } else {
            let velocity = self.velocity();
            self.set_velocity(DVec3::new(
                velocity.x,
                velocity.y * AIRBORNE_Y_DRAG,
                velocity.z,
            ));
        }
    }

    fn dimensions_for_pose(&self, _pose: EntityPose) -> EntityDimensions {
        let scale = LivingEntity::get_scale(self);
        if self.entity_type.fixed {
            self.entity_type.dimensions
        } else {
            self.entity_type.dimensions.scale(scale)
        }
    }

    fn synced_data(&self) -> Option<&dyn EntitySyncedData> {
        Some(&self.entity_data)
    }

    fn update_data_before_sync(&self) {
        self.update_dirty_mob_effect_entity_data();
    }

    fn max_up_step(&self) -> f32 {
        self.attributes()
            .lock()
            .get_value(vanilla_attributes::STEP_HEIGHT)
            .unwrap_or(f64::from(DEFAULT_STEP_HEIGHT)) as f32
    }

    fn is_pushable(&self) -> bool {
        false
    }

    fn is_ignoring_block_triggers(&self) -> bool {
        true
    }

    fn movement_emission(&self) -> EntityMovementEmission {
        EntityMovementEmission::Events
    }

    fn check_fall_damage(
        &self,
        _vertical_movement: f64,
        _on_ground: bool,
        _on_state: BlockStateId,
        _pos: BlockPos,
        _world: &Arc<World>,
    ) {
        // Vanilla `Bat.checkFallDamage` is a no-op: bats never take fall damage.
    }

    fn is_flapping(&self) -> bool {
        !self.is_resting() && self.tick_count() % 10 == 0
    }

    fn hurt(&self, world: &World, source: &DamageSource, amount: f32) -> bool {
        // Vanilla `Bat.hurtServer` wakes a resting bat before applying damage.
        if !self.is_invulnerable_to(world, source) && self.is_resting() {
            self.set_resting(false);
        }
        LivingEntity::hurt_server(self, world, source, amount)
    }

    fn save_additional(&self, nbt: &mut NbtCompound) {
        self.save_mob(nbt);
        nbt.insert("BatFlags", *self.entity_data.lock().id_flags.get());
    }

    fn load_additional(&self, nbt: BorrowedNbtCompoundView<'_, '_>) {
        self.load_mob(nbt);
        self.entity_data
            .lock()
            .id_flags
            .set(nbt.byte("BatFlags").unwrap_or(0));
    }
}

impl LivingEntity for BatEntity {
    fn living_base(&self) -> &LivingEntityBase {
        &self.living_base
    }

    fn get_health(&self) -> f32 {
        *self.entity_data.lock().living_entity().health.get()
    }

    fn set_health(&self, health: f32) {
        let max_health = self.get_max_health();
        let clamped = health.clamp(0.0, max_health);
        self.entity_data
            .lock()
            .living_entity_mut()
            .health
            .set(clamped);
    }

    fn sound_volume(&self) -> f32 {
        0.1
    }

    fn voice_pitch(&self) -> f32 {
        self.default_voice_pitch() * 0.95
    }

    fn hurt_sound(&self, _source: &DamageSource) -> Option<SoundEventRef> {
        Some(&sound_events::ENTITY_BAT_HURT)
    }

    fn death_sound(&self) -> Option<SoundEventRef> {
        Some(&sound_events::ENTITY_BAT_DEATH)
    }

    fn push_entities(&self) {
        // Vanilla `Bat.pushEntities` is a no-op.
    }

    fn server_ai_step(&self) {
        Mob::mob_server_ai_step(self);
    }

    fn ai_step(&self) -> Option<MoveResult> {
        Mob::mob_ai_step(self)
    }
}

impl Mob for BatEntity {
    fn mob_base(&self) -> &MobBase {
        &self.mob_base
    }

    fn mob_can_be_leashed(&self) -> bool {
        false
    }

    fn custom_server_ai_step(&self) {
        let Some(world) = self.level() else {
            return;
        };
        let pos = self.block_position();
        let above = pos.above();
        if self.is_resting() {
            self.tick_resting_ai(&world, pos, above);
        } else {
            self.tick_flying_ai(&world, above);
        }
    }

    fn ambient_sound(&self) -> Option<SoundEventRef> {
        if self.is_resting() && rand::random_range(0..4) != 0 {
            None
        } else {
            Some(&sound_events::ENTITY_BAT_AMBIENT)
        }
    }

    fn mob_flags(&self) -> i8 {
        *self.entity_data.lock().mob().mob_flags.get()
    }

    fn set_mob_flags(&self, flags: i8) {
        self.entity_data.lock().mob_mut().mob_flags.set(flags);
    }
}

#[cfg(test)]
mod tests {
    use super::{BatEntity, closer_to_center_than};
    use crate::entity::{Entity, leash::Leashable};
    use glam::DVec3;
    use simdnbt::borrow::read_compound;
    use simdnbt::owned::NbtCompound;
    use std::io::Cursor;
    use std::sync::Weak;
    use steel_registry::{init_vanilla_registry, vanilla_entities};
    use steel_utils::BlockPos;

    #[test]
    fn bat_starts_resting_and_round_trips_flags() {
        init_vanilla_registry();

        let bat = BatEntity::new(&vanilla_entities::BAT, 1, DVec3::ZERO, Weak::new());
        assert!(bat.is_resting());
        assert!(!bat.can_be_leashed());

        bat.set_resting(false);
        assert!(!bat.is_resting());

        let mut nbt = NbtCompound::new();
        bat.save_additional(&mut nbt);
        assert_eq!(nbt.byte("BatFlags"), Some(0));

        let loaded = BatEntity::new(&vanilla_entities::BAT, 2, DVec3::ZERO, Weak::new());
        let mut bytes = Vec::new();
        nbt.write(&mut bytes);
        let borrowed = read_compound(&mut Cursor::new(&bytes))
            .unwrap_or_else(|error| panic!("reborrow failed: {error}"));
        loaded.load_additional((&borrowed).into());
        assert!(!loaded.is_resting());
    }

    #[test]
    fn bat_is_flapping_on_the_vanilla_tick_cadence() {
        init_vanilla_registry();

        let bat = BatEntity::new(&vanilla_entities::BAT, 1, DVec3::ZERO, Weak::new());
        assert!(!bat.is_flapping(), "resting bats never flap");

        bat.set_resting(false);
        assert!(bat.is_flapping(), "tick count 0 is a multiple of 10");

        bat.advance_tick_count();
        assert!(!bat.is_flapping());
    }

    #[test]
    fn closer_to_center_uses_block_center() {
        let pos = BlockPos::new(0, 0, 0);
        assert!(closer_to_center_than(pos, DVec3::new(0.5, 0.5, 0.5), 1.0));
        assert!(!closer_to_center_than(pos, DVec3::new(1.5, 0.5, 0.5), 1.0));
    }
}
