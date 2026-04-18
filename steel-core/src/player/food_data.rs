//! Player food/hunger system.
//!
//! Manages food level, saturation, and exhaustion — the three values that
//! control natural health regeneration and starvation damage.

use steel_utils::types::Difficulty;

/// All food-system constants bundled in one place.
pub mod food_constants {
    /// Maximum food level of a player
    pub const MAX_FOOD_LEVEL: i32 = 20;

    /// Maximum saturation level
    pub const MAX_SATURATION: f32 = 20.0;

    /// Default saturation for a freshly spawned player
    pub const DEFAULT_SATURATION: f32 = 5.0;

    /// Saturation floor used by some food items
    pub const SATURATION_FLOOR: f32 = 2.5;

    /// Exhaustion threshold
    pub const EXHAUSTION_DROP: f32 = 4.0;

    /// Slow regeneration interval ticks
    pub const HEALTH_TICK_COUNT: i32 = 80;

    /// Fast regeneration interval ticks
    pub const HEALTH_TICK_COUNT_SATURATED: i32 = 10;

    /// Minimum food level required for slow natural regeneration
    pub const HEAL_LEVEL: i32 = 18;

    /// Sprint threshold — player can sprint when food level is above this value
    pub const SPRINT_LEVEL: i32 = 6;

    /// Poor saturation modifier
    pub const FOOD_SATURATION_POOR: f32 = 0.1;

    /// Low saturation modifier
    pub const FOOD_SATURATION_LOW: f32 = 0.3;

    /// Normal saturation modifier
    pub const FOOD_SATURATION_NORMAL: f32 = 0.6;

    /// Good saturation modifier
    pub const FOOD_SATURATION_GOOD: f32 = 0.8;

    /// Max saturation modifier
    pub const FOOD_SATURATION_MAX: f32 = 1.0;

    /// Supernatural saturation modifier
    pub const FOOD_SATURATION_SUPERNATURAL: f32 = 1.2;

    /// Exhaustion cost of regenerating health
    pub const EXHAUSTION_HEAL: f32 = 6.0;

    /// Exhaustion cost per jump
    pub const EXHAUSTION_JUMP: f32 = 0.05;

    /// Exhaustion cost per sprint-jump
    pub const EXHAUSTION_SPRINT_JUMP: f32 = 0.2;

    /// Exhaustion cost per block mined
    pub const EXHAUSTION_MINE: f32 = 0.005;

    /// Exhaustion cost per attack
    pub const EXHAUSTION_ATTACK: f32 = 0.1;

    /// Exhaustion cost per meter walked
    pub const EXHAUSTION_WALK: f32 = 0.0;

    /// Exhaustion cost per meter crouched
    pub const EXHAUSTION_CROUCH: f32 = 0.0;

    /// Exhaustion cost per meter sprinted
    pub const EXHAUSTION_SPRINT: f32 = 0.1;

    /// Exhaustion cost per meter swum
    pub const EXHAUSTION_SWIM: f32 = 0.01;

    /// Default food level for a freshly spawned player.
    pub const DEFAULT_FOOD_LEVEL: i32 = 20;

    /// Hard cap on accumulated exhaustion
    pub const MAX_EXHAUSTION: f32 = 40.0;
}

use food_constants::{
    DEFAULT_FOOD_LEVEL, DEFAULT_SATURATION, EXHAUSTION_DROP, EXHAUSTION_HEAL, HEAL_LEVEL,
    HEALTH_TICK_COUNT, HEALTH_TICK_COUNT_SATURATED, MAX_EXHAUSTION, MAX_FOOD_LEVEL, SPRINT_LEVEL,
};

/// Computes the absolute saturation value from a nutrition count and a modifier.
#[must_use]
pub fn saturation_by_modifier(nutrition: i32, modifier: f32) -> f32 {
    nutrition as f32 * modifier * 2.0
}

/// Tracks a player's hunger, saturation, and exhaustion state.
/// One instance is stored per player behind a `SyncMutex`.
#[derive(Debug, Clone)]
pub struct FoodData {
    /// Current food level (0–20). Displayed as the hunger bar on the client.
    pub food_level: i32,
    /// Saturation buffer — consumed before the food level drops.
    pub saturation_level: f32,
    /// Accumulated exhaustion from actions (sprinting, jumping, damage, …).
    pub exhaustion_level: f32,
    /// Internal tick counter shared between regeneration and starvation logic.
    pub tick_timer: i32,
}

impl Default for FoodData {
    fn default() -> Self {
        Self::new()
    }
}

impl FoodData {
    /// Creates a new `FoodData` with default values (full hunger bar).
    #[must_use]
    pub const fn new() -> Self {
        Self {
            food_level: DEFAULT_FOOD_LEVEL,
            saturation_level: DEFAULT_SATURATION,
            exhaustion_level: 0.0,
            tick_timer: 0,
        }
    }

    /// Returns `true` if the player's food level is below maximum and they
    /// could benefit from eating.
    #[must_use]
    pub const fn needs_food(&self) -> bool {
        self.food_level < MAX_FOOD_LEVEL
    }

    /// Returns `true` if the player has enough food to perform exhaustive
    /// maneuvers (for example sprinting).
    #[must_use]
    pub const fn has_enough_food(&self) -> bool {
        self.food_level > SPRINT_LEVEL
    }

    /// Adds exhaustion from an action (sprinting, jumping, taking damage, …).
    /// The value is clamped so `exhaustion_level` never exceeds 40.
    pub fn add_exhaustion(&mut self, amount: f32) {
        self.exhaustion_level = (self.exhaustion_level + amount).min(MAX_EXHAUSTION);
    }

    /// Applies food and saturation
    fn add(&mut self, food: i32, saturation: f32) {
        self.food_level = (self.food_level + food).clamp(0, MAX_FOOD_LEVEL);
        self.saturation_level =
            (self.saturation_level + saturation).clamp(0.0, self.food_level as f32);
    }

    /// Applies the nutrition from eating food, given a **saturation modifier**.
    pub fn eat(&mut self, nutrition: i32, saturation_modifier: f32) {
        self.add(
            nutrition,
            saturation_by_modifier(nutrition, saturation_modifier),
        );
    }

    /// Runs one tick of the hunger system.
    /// Returns a [`FoodTickResult`] describing what happened this tick so the
    /// caller (`Player::tick`) can apply healing or starvation damage
    #[must_use]
    pub fn tick(
        &mut self,
        difficulty: Difficulty,
        natural_regen: bool,
        current_health: f32,
        max_health: f32,
    ) -> FoodTickResult {
        if self.exhaustion_level > EXHAUSTION_DROP {
            self.exhaustion_level -= EXHAUSTION_DROP;

            if self.saturation_level > 0.0 {
                self.saturation_level = (self.saturation_level - 1.0).max(0.0);
            } else if difficulty != Difficulty::Peaceful {
                self.food_level = (self.food_level - 1).max(0);
            }
        }

        let food = self.food_level;
        let is_hurt = current_health > 0.0 && current_health < max_health;

        // Fast regen
        if natural_regen && self.saturation_level > 0.0 && is_hurt && food >= MAX_FOOD_LEVEL {
            self.tick_timer += 1;

            if self.tick_timer >= HEALTH_TICK_COUNT_SATURATED {
                let saturation_spent = self.saturation_level.min(EXHAUSTION_HEAL);
                let heal_amount = saturation_spent / EXHAUSTION_HEAL;
                self.tick_timer = 0;
                return FoodTickResult::Heal {
                    amount: heal_amount,
                    exhaustion: saturation_spent,
                };
            }
        }
        // Slow regen
        else if natural_regen && food >= HEAL_LEVEL && is_hurt {
            self.tick_timer += 1;

            if self.tick_timer >= HEALTH_TICK_COUNT {
                self.tick_timer = 0;
                return FoodTickResult::Heal {
                    amount: 1.0,
                    exhaustion: EXHAUSTION_HEAL,
                };
            }
        }
        // Starving
        else if food <= 0 {
            self.tick_timer += 1;

            if self.tick_timer >= HEALTH_TICK_COUNT {
                self.tick_timer = 0;

                if current_health > 10.0
                    || difficulty == Difficulty::Hard
                    || (current_health > 1.0 && difficulty == Difficulty::Normal)
                {
                    return FoodTickResult::Starve;
                }
            }
        } else {
            self.tick_timer = 0;
        }

        FoodTickResult::None
    }
}

/// Describes what the hunger tick determined should happen this tick
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum FoodTickResult {
    /// Nothing happened this tick
    None,
    /// The player should be healed by `amount` HP and `exhaustion` should be added back
    Heal {
        /// Health points to restore
        amount: f32,
        /// Exhaustion to add as cost of this regeneration
        exhaustion: f32,
    },
    /// The player should take 1 point of starvation damage
    Starve,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exhaustion_drains_saturation_then_food() {
        let mut food = FoodData::new();
        food.saturation_level = 3.0;
        food.add_exhaustion(4.5);

        // First drain: saturation drops by 1
        let _ = food.tick(Difficulty::Normal, false, 20.0, 20.0);
        assert!((food.saturation_level - 2.0).abs() < f32::EPSILON);
        assert_eq!(food.food_level, MAX_FOOD_LEVEL);

        // Now empty saturation and trigger again — food level should drop
        food.saturation_level = 0.0;
        food.exhaustion_level = 5.0;
        let _ = food.tick(Difficulty::Normal, false, 20.0, 20.0);
        assert_eq!(food.food_level, MAX_FOOD_LEVEL - 1);
    }

    /// Fast regen: food=20, saturation>0, hurt → heal every 10 ticks.
    #[test]
    fn fast_regen() {
        let mut food = FoodData::new(); // food=20, sat=5.0

        let mut result = FoodTickResult::None;
        for _ in 0..HEALTH_TICK_COUNT_SATURATED {
            result = food.tick(Difficulty::Normal, true, 15.0, 20.0);
        }

        match result {
            FoodTickResult::Heal { amount, exhaustion } => {
                assert!((exhaustion - 5.0).abs() < f32::EPSILON);
                assert!((amount - 5.0 / 6.0).abs() < f32::EPSILON);
            }
            other => panic!("Expected Heal, got {other:?}"),
        }
    }

    /// Slow regen: food>=18, sat=0, hurt → heal 1 HP every 80 ticks.
    #[test]
    fn slow_regen() {
        let mut food = FoodData::new();
        food.food_level = 18;
        food.saturation_level = 0.0;

        let mut result = FoodTickResult::None;
        for _ in 0..HEALTH_TICK_COUNT {
            result = food.tick(Difficulty::Normal, true, 10.0, 20.0);
        }

        match result {
            FoodTickResult::Heal { amount, exhaustion } => {
                assert!((amount - 1.0).abs() < f32::EPSILON);
                assert!((exhaustion - EXHAUSTION_HEAL).abs() < f32::EPSILON);
            }
            other => panic!("Expected Heal, got {other:?}"),
        }
    }

    /// Starvation thresholds per difficulty:
    /// - Hard: always starves
    /// - Normal: stops at 1 HP
    /// - Easy/Peaceful: stops at 10 HP
    #[test]
    fn starvation_by_difficulty() {
        let run = |diff, health| {
            let mut food = FoodData::new();
            food.food_level = 0;
            food.saturation_level = 0.0;
            let mut result = FoodTickResult::None;
            for _ in 0..HEALTH_TICK_COUNT {
                result = food.tick(diff, false, health, 20.0);
            }
            result
        };

        assert_eq!(run(Difficulty::Hard, 1.0), FoodTickResult::Starve);
        assert_eq!(run(Difficulty::Normal, 1.0), FoodTickResult::None);
        assert_eq!(run(Difficulty::Normal, 2.0), FoodTickResult::Starve);
        assert_eq!(run(Difficulty::Easy, 10.0), FoodTickResult::None);
        assert_eq!(run(Difficulty::Easy, 11.0), FoodTickResult::Starve);
    }

    /// Vanilla `Player.isHurt()` returns false when health <= 0,
    /// so dead players must not regenerate.
    #[test]
    fn no_regen_when_dead() {
        let mut food = FoodData::new(); // food=20, sat=5.0

        let mut result = FoodTickResult::None;
        for _ in 0..HEALTH_TICK_COUNT_SATURATED {
            result = food.tick(Difficulty::Normal, true, 0.0, 20.0);
        }

        assert_eq!(result, FoodTickResult::None);
    }

    /// Peaceful never drains food from exhaustion (only saturation).
    #[test]
    fn peaceful_never_drains_food() {
        let mut food = FoodData::new();
        food.saturation_level = 0.0;
        food.exhaustion_level = 5.0;

        let _ = food.tick(Difficulty::Peaceful, false, 20.0, 20.0);

        assert_eq!(food.food_level, MAX_FOOD_LEVEL);
    }
}
