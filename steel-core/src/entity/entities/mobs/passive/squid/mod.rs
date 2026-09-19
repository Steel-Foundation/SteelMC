use std::{
    f32::consts::{PI, TAU},
    sync::{Arc, Weak},
};

use glam::DVec3;
use steel_macros::entity_behavior;
use steel_registry::{
    entity_data::{EntityPose, ParticleData},
    entity_type::{EntityAttachments, EntityDimensions, EntityTypeRef},
    sound_event::SoundEventRef,
    sound_events, vanilla_attributes,
    vanilla_entity_data::SquidEntityData,
    vanilla_mob_effects::LEVITATION,
    vanilla_particle_types,
};
use steel_utils::{
    DowncastType, DowncastTypeKey, entity_events,
    locks::SyncMutex,
    random::{Random, legacy_random::LegacyRandom},
};

use crate::entity::{
    EntityMovementEmission, EntitySpawnReason, SpawnGroupData, ai::goal::SquidFleeGoal,
    damage::DamageSource,
};
use crate::{
    entity::{
        AgeableMob, AgeableMobBase, Entity, EntityBase, EntityBaseLoad, EntitySyncedData,
        LivingEntity, LivingEntityBase, Mob, MobBase, PathfinderMob,
        ai::goal::SquidRandomMovementGoal,
    },
    physics::{MoveResult, MoverType},
    world::World,
};

const SQUID_BABY_WIDTH: f32 = 0.5;
const SQUID_BABY_HEIGHT: f32 = 0.5;
const SQUID_BABY_EYE_HEIGHT: f32 = 0.37;

const SQUID_BABY_DIMENSIONS: EntityDimensions = EntityDimensions::new_with_attachments(
    SQUID_BABY_WIDTH,
    SQUID_BABY_HEIGHT,
    SQUID_BABY_EYE_HEIGHT,
    EntityAttachments::fallback(),
);

const SQUID_AIR_DRAG: f64 = 0.98;
const SQUID_GRAVITY: f64 = 0.08;
const SQUID_SOUND_VOLUME: f32 = 0.4;

#[entity_behavior(class = "Squid")]
/// Vanilla Squid entity
pub struct SquidEntity {
    base: EntityBase,
    entity_type: EntityTypeRef,
    living_base: LivingEntityBase,
    mob_base: MobBase,
    ageable_base: AgeableMobBase,
    entity_data: SyncMutex<SquidEntityData>,

    state: SyncMutex<SquidState>,
}

pub struct SquidState {
    x_body_rot: f32,
    // Required for inking on fleeing.
    x_body_rot_old: f32,
    movement_vector: DVec3,
    // I don't know if we want random here, or if its okay to just re-create it in SquidRandomMovementGoal
    random: LegacyRandom,
    tentacle_speed: f32,
    tentacle_movement: f32,
}

// SAFETY: This key is owned by Steel and uniquely identifies `SquidEntity`.
unsafe impl DowncastType for SquidEntity {
    const TYPE_KEY: DowncastTypeKey = DowncastTypeKey::new("steel:entity/squid");
}

impl SquidEntity {
    /// Creates a new Squid entity
    #[must_use]
    pub fn new(entity_type: EntityTypeRef, id: i32, position: DVec3, world: Weak<World>) -> Self {
        Self::new_with_base(
            EntityBase::new(id, position, entity_type.dimensions, world),
            entity_type,
        )
    }

    /// Checks if the squid has a movement vector.
    ///
    /// Equivalent to `Squid.hasMovementVector()` in vanilla.
    pub fn has_movement_vector(&self) -> bool {
        self.state.lock().movement_vector.length_squared() > 1.0e-5
    }

    /// Sets the squid's movement vector
    pub fn set_movement_vector(&self, new_vec: DVec3) {
        self.state.lock().movement_vector = new_vec;
    }

    /// Returns the next random f32
    pub fn random_next_f32(&self) -> f32 {
        self.state.lock().random.next_f32()
    }

    /// Returns the next random f64
    pub fn random_next_f64(&self) -> f64 {
        self.state.lock().random.next_f64()
    }

    /// Returns the next random i32 within the upper bound
    pub fn random_next_i32_bounded(&self, bound: i32) -> i32 {
        self.state.lock().random.next_i32_bounded(bound)
    }

    /// Returns the squid's movement vector
    pub fn movement_vector(&self) -> DVec3 {
        self.state.lock().movement_vector
    }

    /// Recreates a squid from saved entity state
    #[must_use]
    pub fn from_saved(entity_type: EntityTypeRef, load: EntityBaseLoad) -> Self {
        Self::new_with_base(
            EntityBase::from_load(load, entity_type.dimensions),
            entity_type,
        )
    }

    /// Rotates a vector
    ///
    /// Equivalent to `Squid.rotateVector()` in vanilla.
    fn rotate_vector(&self, vector: DVec3) -> DVec3 {
        let x_rot = f64::from(self.state.lock().x_body_rot_old).to_radians();
        let y_rot = f64::from(-self.living_rotation_state().y_body_rot_o().to_radians());

        vector.rotate_x(x_rot).rotate_y(y_rot)
    }

    fn spawn_ink(&self) {
        let Some(world) = self.level() else {
            return;
        };

        self.make_sound(Some(&sound_events::ENTITY_SQUID_SQUIRT));

        let position = self.position() + self.rotate_vector(DVec3::new(0.0, -1.0, 0.0));
        let particle_position = position + DVec3::new(0.0, 0.5, 0.0);
        let particle = ParticleData::simple(&vanilla_particle_types::SQUID_INK);
        let position_scale = if AgeableMob::is_baby(self) { 0.1 } else { 0.3 };

        for _ in 0..30 {
            let direction = self.rotate_vector(DVec3::new(
                self.random_next_f64() * 0.6 - 0.3,
                -1.0,
                self.random_next_f64() * 0.6 - 0.3,
            ));
            let offset = direction * (position_scale + self.random_next_f64() * 2.0);
            world.send_particles(particle.clone(), particle_position, 0, offset, 0.1);
        }
    }

    fn tick_squid_movement(&self) {
        let (tentacle_movement, movement_vector, animation_sync) = {
            let mut state = self.state.lock();

            state.tentacle_movement += state.tentacle_speed;

            let animation_sync = state.tentacle_movement > TAU;

            if animation_sync {
                state.tentacle_movement -= TAU;

                if state.random.next_i32_bounded(10) == 0 {
                    state.tentacle_speed = 1.0 / (state.random.next_f32() + 1.0) * 0.2;
                }
            }

            (
                state.tentacle_movement,
                state.movement_vector,
                animation_sync,
            )
        };

        if animation_sync {
            self.broadcast_entity_event(entity_events::EntityStatus::SquidAnimSynch);
        }

        if self.is_in_water() {
            if tentacle_movement < PI {
                let tentacle_scale = tentacle_movement / PI;

                if tentacle_scale > 0.75 {
                    self.set_velocity(DVec3::new(
                        movement_vector.x,
                        movement_vector.y,
                        movement_vector.z,
                    ));
                }
            } else {
                self.set_velocity(self.velocity() * 0.9);
            }
            return;
        }

        let velocity = self.velocity();
        let y = self
            .mob_effect(LEVITATION)
            .map_or(velocity.y - self.get_gravity(), |effect| {
                0.05 * f64::from(effect.amplifier() + 1)
            });

        self.set_velocity(DVec3::new(0.0, y * SQUID_AIR_DRAG, 0.0));
    }

    fn update_squid_rotation(&self) {
        let mut state = self.state.lock();
        state.x_body_rot_old = state.x_body_rot;

        if self.is_in_water() {
            let movement = self.velocity();
            let horizontal = movement.x.hypot(movement.z);

            let y_body_rot = self.living_rotation_state().y_body_rot();
            let y_body_rot = y_body_rot
                + (-((movement.x.atan2(movement.z)).to_degrees() as f32) - y_body_rot) * 0.1;

            self.set_y_body_rot(y_body_rot);
            state.x_body_rot +=
                (-((horizontal.atan2(movement.y)).to_degrees() as f32) - state.x_body_rot) * 0.1;
        } else {
            state.x_body_rot += (-90.0 - state.x_body_rot) * 0.02;
        }
    }

    fn new_with_base(base: EntityBase, entity_type: EntityTypeRef) -> Self {
        let living_base = LivingEntityBase::new(entity_type);
        living_base
            .attributes()
            .lock()
            .set_base_value(vanilla_attributes::MAX_HEALTH, 10.0);

        let mob_base = MobBase::new();
        let ageable_base = AgeableMobBase::new();

        let mut entity_data = SquidEntityData::new();
        living_base.initialize_synced_data(&mut entity_data);

        let mut random = LegacyRandom::from_seed(rand::random());
        let tentacle_random = random.next_f32();

        {
            let mut goal_selector = mob_base.goal_selector().lock();
            // Fleeing must outrank the always-available random movement goal.
            goal_selector.add_goal(0, SquidFleeGoal::new());
            goal_selector.add_goal(1, SquidRandomMovementGoal::new());
        }

        Self {
            base,
            entity_type,
            living_base,
            mob_base,
            ageable_base,
            entity_data: SyncMutex::new(entity_data),
            state: SyncMutex::new(SquidState {
                x_body_rot: 0.0,
                x_body_rot_old: 0.0,
                movement_vector: DVec3::ZERO,
                random,
                tentacle_speed: 1.0 / (tentacle_random + 1.0) * 0.2,
                tentacle_movement: 0.0,
            }),
        }
    }
}

impl Entity for SquidEntity {
    fn base(&self) -> &EntityBase {
        &self.base
    }

    fn hurt(&self, world: &World, source: &DamageSource, amount: f32) -> bool {
        let hurt = LivingEntity::hurt_server(self, world, source, amount);

        if hurt && self.last_hurt_by_mob().is_some() {
            self.spawn_ink();
        }

        hurt
    }

    fn movement_emission(&self) -> EntityMovementEmission {
        EntityMovementEmission::Events
    }

    fn entity_type(&self) -> EntityTypeRef {
        self.entity_type
    }

    fn base_tick(&self) {
        Mob::base_tick_mob(self);
    }

    fn get_default_gravity(&self) -> f64 {
        SQUID_GRAVITY
    }

    fn dimensions_for_pose(&self, _pose: EntityPose) -> EntityDimensions {
        let scale = LivingEntity::get_scale(self);
        if AgeableMob::is_baby(self) {
            SQUID_BABY_DIMENSIONS.scale(scale)
        } else if self.entity_type.fixed {
            self.entity_type.dimensions
        } else {
            self.entity_type.dimensions.scale(scale)
        }
    }

    fn synced_data(&self) -> Option<&dyn EntitySyncedData> {
        Some(&self.entity_data)
    }
}

impl LivingEntity for SquidEntity {
    fn living_base(&self) -> &LivingEntityBase {
        &self.living_base
    }

    fn sound_volume(&self) -> f32 {
        SQUID_SOUND_VOLUME
    }

    fn can_breathe_underwater(&self) -> bool {
        true
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

    fn travel(&self, _input: DVec3) -> Option<MoveResult> {
        self.move_entity(MoverType::SelfMovement, self.velocity())
    }

    fn server_ai_step(&self) {
        Mob::mob_server_ai_step(self);
    }

    fn ai_step(&self) -> Option<MoveResult> {
        let result = Mob::mob_ai_step(self);

        self.update_squid_rotation();
        self.tick_squid_movement();

        AgeableMob::tick_ageable_mob(self);
        result
    }

    fn hurt_sound(&self, _source: &DamageSource) -> Option<SoundEventRef> {
        Some(&sound_events::ENTITY_SQUID_HURT)
    }

    fn death_sound(&self) -> Option<SoundEventRef> {
        Some(&sound_events::ENTITY_SQUID_DEATH)
    }
}

impl AgeableMob for SquidEntity {
    fn ageable_base(&self) -> &AgeableMobBase {
        &self.ageable_base
    }

    fn is_age_locked(&self) -> bool {
        *self.entity_data.lock().ageable_mob().age_locked.get()
    }

    fn set_age_locked(&self, age_locked: bool) {
        self.entity_data
            .lock()
            .ageable_mob_mut()
            .age_locked
            .set(age_locked);
    }

    fn set_synced_baby(&self, baby: bool) {
        self.entity_data.lock().ageable_mob_mut().baby.set(baby);
    }
}

impl Mob for SquidEntity {
    fn mob_base(&self) -> &MobBase {
        &self.mob_base
    }

    fn tick_goal_selectors(&self) {
        PathfinderMob::tick_pathfinder_goal_selectors(self);
    }

    fn tick_path_navigation(&self) {
        PathfinderMob::tick_pathfinder_path_navigation(self);
    }

    fn mob_flags(&self) -> i8 {
        *self.entity_data.lock().mob().mob_flags.get()
    }

    fn set_mob_flags(&self, flags: i8) {
        self.entity_data.lock().mob_mut().mob_flags.set(flags);
    }

    fn ambient_sound(&self) -> Option<SoundEventRef> {
        Some(&sound_events::ENTITY_SQUID_AMBIENT)
    }

    fn finalize_spawn(
        &self,
        world: &Arc<World>,
        spawn_reason: EntitySpawnReason,
        group_data: Option<SpawnGroupData>,
    ) -> Option<SpawnGroupData> {
        self.finalize_spawn_ageable_mob(world, spawn_reason, group_data)
    }
}

impl PathfinderMob for SquidEntity {}
