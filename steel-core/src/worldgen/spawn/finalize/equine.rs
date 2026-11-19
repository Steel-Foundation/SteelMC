use steel_registry::vanilla_attributes;
use steel_utils::random::{Random, RandomSource};

use crate::entity::Mob;

const BASE_HEALTH: f64 = 15.0;
const HEALTH_BONUS_FIRST_BOUND: i32 = 8;
const HEALTH_BONUS_SECOND_BOUND: i32 = 9;
const BASE_SPEED_ROLL: f32 = 0.45;
const SPEED_ROLL_SCALE: f64 = 0.3;
const SPEED_CONVERSION_SCALE: f64 = 0.25;
const BASE_JUMP_STRENGTH: f32 = 0.4;
const JUMP_STRENGTH_ROLL_SCALE: f64 = 0.2;
const LLAMA_HIGH_STRENGTH_CHANCE: f32 = 0.04;
const LLAMA_HIGH_MAX_STRENGTH: i32 = 5;
const LLAMA_DEFAULT_MAX_STRENGTH: i32 = 3;

#[derive(Debug, Clone, Copy)]
pub(super) struct EquineAttributes {
    health: f64,
    speed_and_jump: Option<(f64, f64)>,
}

impl EquineAttributes {
    pub(super) fn sample(random: &mut RandomSource, horse: bool) -> Self {
        let health = BASE_HEALTH
            + f64::from(random.next_i32_bounded(HEALTH_BONUS_FIRST_BOUND))
            + f64::from(random.next_i32_bounded(HEALTH_BONUS_SECOND_BOUND));

        let speed_and_jump = horse.then(|| {
            let speed = (f64::from(BASE_SPEED_ROLL)
                + random.next_f64() * SPEED_ROLL_SCALE
                + random.next_f64() * SPEED_ROLL_SCALE
                + random.next_f64() * SPEED_ROLL_SCALE)
                * SPEED_CONVERSION_SCALE;
            let jump = f64::from(BASE_JUMP_STRENGTH)
                + random.next_f64() * JUMP_STRENGTH_ROLL_SCALE
                + random.next_f64() * JUMP_STRENGTH_ROLL_SCALE
                + random.next_f64() * JUMP_STRENGTH_ROLL_SCALE;
            (speed, jump)
        });

        Self {
            health,
            speed_and_jump,
        }
    }

    pub(super) fn apply(self, mob: &dyn Mob) {
        let mut attributes = mob.attributes().lock();
        attributes.set_base_value(vanilla_attributes::MAX_HEALTH, self.health);

        if let Some((speed, jump)) = self.speed_and_jump {
            attributes.set_base_value(vanilla_attributes::MOVEMENT_SPEED, speed);
            attributes.set_base_value(vanilla_attributes::JUMP_STRENGTH, jump);
        }
    }
}

pub(super) fn llama_strength(random: &mut RandomSource) -> i32 {
    let bound = if random.next_f32() < LLAMA_HIGH_STRENGTH_CHANCE {
        LLAMA_HIGH_MAX_STRENGTH
    } else {
        LLAMA_DEFAULT_MAX_STRENGTH
    };

    1 + random.next_i32_bounded(bound)
}
