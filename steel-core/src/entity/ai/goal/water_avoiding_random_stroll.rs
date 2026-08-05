use glam::DVec3;

use super::random_pos::{default_random_pos, land_random_pos};
use super::random_stroll::RandomStrollGoal;
use super::selector::{Goal, GoalControls};
use crate::entity::PathfinderMob;

const WATER_AVOIDING_RANDOM_STROLL_PROBABILITY: f32 = 0.001;

pub struct WaterAvoidingRandomStrollGoal {
    stroll: RandomStrollGoal,
    probability: f32,
}

impl WaterAvoidingRandomStrollGoal {
    #[must_use]
    pub const fn new(speed_modifier: f64) -> Self {
        Self::with_probability(speed_modifier, WATER_AVOIDING_RANDOM_STROLL_PROBABILITY)
    }

    #[must_use]
    pub const fn with_probability(speed_modifier: f64, probability: f32) -> Self {
        Self {
            stroll: RandomStrollGoal::new(speed_modifier),
            probability,
        }
    }
}

impl Goal for WaterAvoidingRandomStrollGoal {
    fn controls(&self) -> GoalControls {
        self.stroll.controls()
    }

    fn can_use(&mut self, mob: &dyn PathfinderMob) -> bool {
        let probability = self.probability;
        self.stroll
            .can_use_with_position(mob, |mob| random_stroll_pos(mob, probability))
    }

    fn can_continue_to_use(&mut self, mob: &dyn PathfinderMob) -> bool {
        self.stroll.can_continue_to_use(mob)
    }

    fn start(&mut self, mob: &dyn PathfinderMob) {
        self.stroll.start(mob);
    }

    fn stop(&mut self, mob: &dyn PathfinderMob) {
        self.stroll.stop(mob);
    }
}

fn random_stroll_pos(mob: &dyn PathfinderMob, probability: f32) -> Option<DVec3> {
    if mob.is_in_water() {
        return land_random_pos(mob, 15, 7).or_else(|| default_random_pos(mob, 10, 7));
    }

    let use_land_random_pos = rand::random::<f32>() >= probability;
    if use_land_random_pos {
        land_random_pos(mob, 10, 7)
    } else {
        default_random_pos(mob, 10, 7)
    }
}
