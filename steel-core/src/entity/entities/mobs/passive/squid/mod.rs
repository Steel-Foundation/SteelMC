//! Vanilla `Squid`, an `AgeableWaterCreature`.

mod goals;

use std::f32::consts::{PI, TAU};
use std::sync::{Arc, LazyLock, Weak};

use glam::DVec3;
use steel_macros::entity_behavior;
use steel_math::{DEGREE_90, vector};
use steel_registry::entity_data::ParticleData;
use steel_registry::entity_type::{EntityAttachments, EntityDimensions, EntityTypeRef};
use steel_registry::vanilla_entity_data::SquidEntityData;
use steel_registry::vanilla_mob_effects::LEVITATION;
use steel_registry::{
    sound_events, vanilla_damage_types, vanilla_entities, vanilla_particle_types,
};
use steel_utils::entity_events::EntityStatus;
use steel_utils::locks::CheckedMutex;
use steel_utils::{DowncastType, DowncastTypeKey};

use crate::entity::ageable::{self, Age, AgeableProps};
use crate::entity::ai::path::PathType;
use crate::entity::component::definition::MobDefinition;
use crate::entity::component::pipeline::{Pipeline, Stage, StageId};
use crate::entity::damage::DamageSource;
use crate::entity::mob::{self};
use crate::entity::prelude::*;
use crate::entity::{
    DefaultEntityPush, EntityBase, EntityBaseLoad, EntityMovementEmission, EntityProps,
    LivingEntityBase, LivingProps, MobBase, MobProps, SharedEntity, next_entity_id,
};
use crate::physics::MoverType;
use crate::world::World;
use goals::{SquidFleeGoal, SquidRandomMovementGoal};

const BABY_DIMENSIONS: EntityDimensions =
    EntityDimensions::new_with_attachments(0.5, 0.5, 0.37, EntityAttachments::fallback());
const INK_PARTICLE_COUNT: u32 = 30;
/// Squids above this squared speed are already moving somewhere.
const MOVEMENT_THRESHOLD: f64 = 1.0e-5;
/// The share of a stroke after which the tentacles push the squid.
const STROKE_PUSH_START: f32 = 0.75;
const AIR_SUPPLY: i32 = 300;

/// The squid's swimming state.
struct Swimming {
    x_body_rot: f32,
    x_body_rot_o: f32,
    movement_vector: DVec3,
    tentacle_speed: f32,
    tentacle_movement: f32,
}

/// A squid.
// TODO: `AgeableWaterCreature.checkSpawnObstruction` (`isUnobstructed`)
// once `Mob.checkSpawnObstruction` is a mob hook.
#[entity_behavior(class = "Squid")]
#[derive(Entity)]
#[entity(hooks = Self::ENTITY)]
pub struct SquidEntity {
    #[entity(base)]
    base: EntityBase,
    #[entity(entity_type)]
    entity_type: EntityTypeRef,
    #[entity(component)]
    living_base: LivingEntityBase,
    #[entity(component)]
    mob_base: MobBase,
    #[entity(component)]
    age: Age,
    swimming: CheckedMutex<Swimming>,
    #[entity(synced)]
    entity_data: CheckedMutex<SquidEntityData>,
}

// SAFETY: This key is owned by Steel and uniquely identifies `SquidEntity`.
unsafe impl DowncastType for SquidEntity {
    const TYPE_KEY: DowncastTypeKey = DowncastTypeKey::new("steel:entity/squid");
}

impl SquidEntity {
    const AGEABLE: AgeableProps = AgeableProps {
        breed_offspring: |parent, world, _| {
            let squid: SharedEntity = Arc::new(SquidEntity::new(
                &vanilla_entities::SQUID,
                next_entity_id(),
                parent.entity().position(),
                Arc::downgrade(world),
            ));
            Some(squid)
        },
    };
    const LIVING: LivingProps<Self> = LivingProps {
        default_dimensions: |mob, _| {
            if mob.living().is_baby() {
                BABY_DIMENSIONS
            } else {
                mob.entity_type.dimensions
            }
        },
        sound_volume: |_| 0.4,
        hurt_sound: |_, _| Some(&sound_events::ENTITY_SQUID_HURT),
        death_sound: |_| Some(&sound_events::ENTITY_SQUID_DEATH),
        // `AgeableWaterCreature.getBaseExperienceReward`.
        base_experience_reward: |_| 1 + rand::random_range(0..3),
        travel: |squid, entity, _| {
            Arc::clone(entity).move_entity(MoverType::SelfMovement, squid.velocity())
        },
        ..mob::living_props()
    };
    const MOB: MobProps<Self> = MobProps {
        ambient_sound: |_| Some(&sound_events::ENTITY_SQUID_AMBIENT),
        // `AgeableWaterCreature.getAmbientSoundInterval`.
        ambient_sound_interval: |_| 120,
        // Squids, unlike other water creatures, can be leashed.
        mob_can_be_leashed: |_| true,
        is_pathfinder: |_| true,
        ..MobProps::new()
    };
    const ENTITY: EntityProps<Self> = EntityProps {
        base_tick: Self::entity_base_tick,
        hurt: Self::entity_hurt,
        default_gravity: |_| 0.08,
        movement_emission: |_| EntityMovementEmission::Events,
        // `AgeableWaterCreature.isPushedByFluid`.
        is_pushed_by_fluid: |_| false,
        ..mob::entity_props()
    };
    const GOALS: StageId = StageId("squid.goals");
    const WATER_MALUS: StageId = StageId("squid.water_malus");
    const SWIM: StageId = StageId("squid.swim");

    /// Vanilla `Squid`: the `AgeableMob` chains with the squid's goals, its
    /// water malus and its swimming.
    fn define() -> MobDefinition<Self> {
        MobDefinition {
            construct: Pipeline::new([
                Stage::effect(Self::GOALS, Self::register_goals),
                Stage::effect(Self::WATER_MALUS, |mob_base: &MobBase| {
                    mob_base
                        .pathfinding_malus()
                        .lock()
                        .set(PathType::Water, 0.0);
                }),
            ]),
            ai_step: Pipeline::new([
                mob::stages::ai_step::<Self>(),
                ageable::ai_step::<Self>(),
                Stage::effect(Self::SWIM, Self::swim),
            ]),
            custom_server_ai_step: Pipeline::new([]),
            mob_interact: Pipeline::new([ageable::mob_interact::<Self>()]),
            finalize_spawn: Pipeline::new([
                ageable::finalize_spawn::<Self>(),
                mob::stages::finalize_spawn::<Self>(),
            ]),
        }
    }

    fn definition() -> &'static MobDefinition<Self> {
        static DEFINITION: LazyLock<MobDefinition<SquidEntity>> =
            LazyLock::new(SquidEntity::define);
        &DEFINITION
    }

    /// Creates a squid.
    #[must_use]
    pub fn new(entity_type: EntityTypeRef, id: i32, position: DVec3, world: Weak<World>) -> Self {
        Self::from_base(
            EntityBase::new(id, position, entity_type.dimensions, world),
            entity_type,
        )
    }

    /// Reconstructs a squid from saved base data.
    #[must_use]
    pub fn from_saved(entity_type: EntityTypeRef, load: EntityBaseLoad) -> Self {
        Self::from_base(
            EntityBase::from_load(load, entity_type.dimensions),
            entity_type,
        )
    }

    fn from_base(base: EntityBase, entity_type: EntityTypeRef) -> Self {
        let mut entity_data = SquidEntityData::new();
        let living_base = LivingEntityBase::new(entity_type, &Self::LIVING, &mut entity_data);
        Self {
            base,
            entity_type,
            living_base,
            mob_base: MobBase::new(&Self::MOB, Self::definition()),
            age: Age::new(&Self::AGEABLE),
            swimming: CheckedMutex::new(Swimming {
                x_body_rot: 0.0,
                x_body_rot_o: 0.0,
                movement_vector: DVec3::ZERO,
                tentacle_speed: 1.0 / (rand::random::<f32>() + 1.0) * 0.2,
                tentacle_movement: 0.0,
            }),
            entity_data: CheckedMutex::new(entity_data),
        }
    }

    fn register_goals(mob_base: &MobBase) {
        let mut goal_selector = mob_base.goal_selector().lock();
        goal_selector.add_goal(0, SquidRandomMovementGoal);
        goal_selector.add_goal(1, SquidFleeGoal::new());
    }

    /// Whether the squid is set to move somewhere.
    fn has_movement_vector(&self) -> bool {
        self.swimming.lock().movement_vector.length_squared() > MOVEMENT_THRESHOLD
    }

    fn set_movement_vector(&self, movement_vector: DVec3) {
        self.swimming.lock().movement_vector = movement_vector;
    }

    /// `AgeableWaterCreature.baseTick`: squids drown on land.
    fn entity_base_tick(&self) {
        let air_supply = self.air_supply();
        self.mob().base_tick_mob();
        if self.is_alive() && !self.is_in_water() {
            self.set_air_supply(air_supply - 1);
            if self.living().should_take_drowning_damage() {
                self.set_air_supply(0);
                if let Some(world) = self.level() {
                    let drown = DamageSource::environment(&vanilla_damage_types::DROWN);
                    self.hurt(&world, &drown, 2.0);
                }
            }
        } else {
            self.set_air_supply(AIR_SUPPLY);
        }
    }

    /// `Squid.hurtServer`: a squid hurt by a mob squirts ink.
    fn entity_hurt(&self, world: &World, source: &DamageSource, amount: f32) -> bool {
        let living = self.living();
        if living.hurt_server(world, source, amount) && living.last_hurt_by_mob().is_some() {
            self.spawn_ink();
            return true;
        }
        false
    }

    /// `Squid.rotateVector`: from squid-local space into world space.
    fn rotate_vector(&self, local: DVec3) -> DVec3 {
        let x_rot = self.swimming.lock().x_body_rot_o.to_radians();
        let y_rot = -self
            .living()
            .living_rotation_state()
            .y_body_rot_o()
            .to_radians();
        vector::y_rot(vector::x_rot(local, x_rot), y_rot)
    }

    fn spawn_ink(&self) {
        let Some(world) = self.level() else {
            return;
        };
        self.living()
            .make_sound(Some(&sound_events::ENTITY_SQUID_SQUIRT));

        let position = self.position() + self.rotate_vector(DVec3::new(0.0, -1.0, 0.0));
        let particle_position = position + DVec3::new(0.0, 0.5, 0.0);
        let particle = ParticleData::simple(&vanilla_particle_types::SQUID_INK);
        let position_scale = if self.living().is_baby() { 0.1 } else { 0.3 };
        for _ in 0..INK_PARTICLE_COUNT {
            let direction = self.rotate_vector(DVec3::new(
                rand::random_range(-0.3..0.3),
                -1.0,
                rand::random_range(-0.3..0.3),
            ));
            let speed = direction * f64::from(position_scale + rand::random::<f32>() * 2.0);
            world.send_particles(particle.clone(), particle_position, 0, speed, 0.1);
        }
    }

    /// `Squid.aiStep` after `super`: strokes its tentacles, swims along its
    /// movement vector in water and sinks outside it, and turns to face its
    /// movement.
    fn swim(&self) {
        let (tentacle_movement, movement_vector, stroke_ended) = {
            let mut swimming = self.swimming.lock();
            swimming.x_body_rot_o = swimming.x_body_rot;
            swimming.tentacle_movement += swimming.tentacle_speed;
            let stroke_ended = swimming.tentacle_movement > TAU;
            if stroke_ended {
                swimming.tentacle_movement -= TAU;
                if rand::random_range(0..10) == 0 {
                    swimming.tentacle_speed = 1.0 / (rand::random::<f32>() + 1.0) * 0.2;
                }
            }
            (
                swimming.tentacle_movement,
                swimming.movement_vector,
                stroke_ended,
            )
        };
        if stroke_ended {
            self.broadcast_entity_event(EntityStatus::SquidAnimSynch);
        }

        if self.is_in_water() {
            // VANILLA CLIENT-LOCAL: the tentacle angle and rotate speed.
            if tentacle_movement < PI {
                if tentacle_movement / PI > STROKE_PUSH_START {
                    self.set_velocity(movement_vector);
                }
            } else {
                self.set_velocity(self.velocity() * 0.9);
            }

            let movement = self.velocity();
            let living = self.living();
            let y_body_rot = living.y_body_rot();
            let y_body_rot = y_body_rot
                + (-(movement.x.atan2(movement.z) as f32).to_degrees() - y_body_rot) * 0.1;
            living.set_y_body_rot(y_body_rot);
            self.set_rotation((y_body_rot, self.rotation().1));
            let horizontal = movement.x.hypot(movement.z);
            let mut swimming = self.swimming.lock();
            swimming.x_body_rot +=
                (-(horizontal.atan2(movement.y) as f32).to_degrees() - swimming.x_body_rot) * 0.1;
        } else {
            let y = match self.living().mob_effect(LEVITATION) {
                Some(levitation) => 0.05 * f64::from(levitation.amplifier() + 1),
                None => self.velocity().y - self.gravity(),
            };
            self.set_velocity(DVec3::new(0.0, y * self.air_drag(), 0.0));
            let mut swimming = self.swimming.lock();
            swimming.x_body_rot += (-DEGREE_90 - swimming.x_body_rot) * 0.02;
        }
    }
}

impl DefaultEntityPush for SquidEntity {}

#[cfg(test)]
mod tests;
