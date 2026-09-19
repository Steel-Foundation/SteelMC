use glam::DVec3;
use steel_registry::{
    blocks::block_state_ext::BlockStateExt, entity_data::ParticleData, fluid::FluidStateExt,
    vanilla_particle_types,
};
use steel_utils::{BlockPos, Downcast};

use crate::{
    entity::{
        PathfinderMob,
        ai::goal::selector::{Goal, GoalControls},
        entities::SquidEntity,
    },
    world::LevelReader,
};

const SQUID_FLEE_SPEED: f64 = 3.0;
const SQUID_FLEE_MIN_DISTANCE: f64 = 5.0;
const SQUID_FLEE_MAX_DISTANCE: f64 = 10.0; // Minecraft doesn't use this?

pub struct SquidFleeGoal {
    flee_ticks: i32,
}

impl SquidFleeGoal {
    pub const fn new() -> Self {
        Self { flee_ticks: 0 }
    }
}

impl Goal for SquidFleeGoal {
    fn controls(&self) -> GoalControls {
        GoalControls::MOVE
    }

    fn can_use(&mut self, mob: &dyn PathfinderMob) -> bool {
        mob.is_in_water()
            && mob
                .last_hurt_by_mob()
                .is_some_and(|attacker| mob.distance_to_sqr(attacker.position()) < 100.0)
    }

    fn requires_update_every_tick(&self) -> bool {
        true
    }

    fn start(&mut self, _mob: &dyn PathfinderMob) {
        self.flee_ticks = 0;
    }

    fn tick(&mut self, mob: &dyn PathfinderMob) {
        self.flee_ticks += 1;

        let Some(attacker) = mob.last_hurt_by_mob() else {
            return;
        };

        let Some(squid) = mob.downcast_ref::<SquidEntity>() else {
            tracing::warn!("SquidFleeGoal assigned to non-squid entity");
            return;
        };

        let Some(world) = mob.level() else {
            return;
        };

        let squid_pos = mob.position();
        let attacker_pos = attacker.position();
        let mut flee_to = squid_pos - attacker_pos;

        let block_pos = BlockPos::from(squid_pos + flee_to);
        let block_state = world.get_block_state(block_pos);
        let fluid_state = block_state.get_fluid_state();

        if !fluid_state.is_water() && !block_state.is_air() {
            return;
        }

        let distance = flee_to.length();

        if distance > 0.0 {
            flee_to = flee_to.normalize();

            let avoid_speed = if distance > SQUID_FLEE_MIN_DISTANCE {
                SQUID_FLEE_SPEED - (distance - SQUID_FLEE_MIN_DISTANCE) / SQUID_FLEE_MIN_DISTANCE
            } else {
                SQUID_FLEE_SPEED
            };

            if avoid_speed > 0.0 {
                flee_to *= avoid_speed;
            }
        }

        if block_state.is_air() {
            flee_to.y = 0.0;
        }

        squid.set_movement_vector(flee_to / 20.0);

        if self.flee_ticks % 10 == 5 {
            world.send_particles(
                ParticleData::simple(&vanilla_particle_types::BUBBLE),
                DVec3::new(squid_pos.x, squid_pos.y, squid_pos.z),
                0,
                DVec3::ZERO,
                0.0,
            );
        }
    }
}
