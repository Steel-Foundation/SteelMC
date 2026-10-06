//! Vanilla item-steered vehicle helpers.

use std::f32::consts::PI;

use steel_utils::locks::SyncMutex;

use crate::entity::Entity;

const MIN_BOOST_TIME: i32 = 140;
const BOOST_TIME_BOUND: i32 = 841;
const BOOST_FACTOR_SCALE: f32 = 1.15;

/// Runtime state for vanilla `ItemBasedSteering`.
#[derive(Debug, Default)]
pub struct ItemBasedSteering {
    boosting: bool,
    boost_time: i32,
}

impl ItemBasedSteering {
    /// Creates default item-based steering state.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            boosting: false,
            boost_time: 0,
        }
    }

    /// Mirrors vanilla `ItemBasedSteering.onSynced`.
    pub const fn on_synced(&mut self) {
        self.boosting = true;
        self.boost_time = 0;
    }

    /// Mirrors vanilla `ItemBasedSteering.boost`.
    pub fn boost(&mut self) -> Option<i32> {
        if self.boosting {
            return None;
        }

        self.boosting = true;
        self.boost_time = 0;
        Some(rand::random_range(0..BOOST_TIME_BOUND) + MIN_BOOST_TIME)
    }

    /// Mirrors vanilla `ItemBasedSteering.tickBoost`.
    pub const fn tick_boost(&mut self, boost_time_total: i32) {
        if !self.boosting {
            return;
        }

        let previous_boost_time = self.boost_time;
        self.boost_time += 1;
        if previous_boost_time > boost_time_total {
            self.boosting = false;
        }
    }

    /// Mirrors vanilla `ItemBasedSteering.boostFactor`.
    #[must_use]
    pub fn boost_factor(&self, boost_time_total: i32) -> f32 {
        if !self.boosting || boost_time_total <= 0 {
            return 1.0;
        }

        1.0 + BOOST_FACTOR_SCALE * ((self.boost_time as f32 / boost_time_total as f32) * PI).sin()
    }

    /// Returns whether a boost is currently active.
    #[must_use]
    pub const fn is_boosting(&self) -> bool {
        self.boosting
    }

    /// Returns vanilla `ItemBasedSteering.boostTime`.
    #[must_use]
    pub const fn boost_time(&self) -> i32 {
        self.boost_time
    }
}

/// Entity behavior for vanilla `ItemSteerable`.
pub trait ItemSteerable: Entity {
    /// Returns the shared runtime steering state.
    fn item_based_steering(&self) -> &SyncMutex<ItemBasedSteering>;

    /// Returns the synced vanilla `boostTimeTotal`.
    fn boost_time_total(&self) -> i32;

    /// Sets the synced vanilla `boostTimeTotal`.
    fn set_boost_time_total(&self, boost_time_total: i32);

    /// Attempts to start an item-steering boost.
    fn boost(&self) -> bool {
        let boost_time_total = {
            let mut steering = self.item_based_steering().lock();
            steering.boost()
        };
        let Some(boost_time_total) = boost_time_total else {
            return false;
        };

        self.set_boost_time_total(boost_time_total);
        true
    }

    /// Advances the active item-steering boost.
    fn tick_boost(&self) {
        let boost_time_total = self.boost_time_total();
        self.item_based_steering()
            .lock()
            .tick_boost(boost_time_total);
    }

    /// Returns vanilla `ItemBasedSteering.boostFactor`.
    fn boost_factor(&self) -> f32 {
        let boost_time_total = self.boost_time_total();
        self.item_based_steering()
            .lock()
            .boost_factor(boost_time_total)
    }
}

#[cfg(test)]
mod tests {
    use super::ItemBasedSteering;

    #[test]
    fn boost_starts_once_and_returns_vanilla_total_range() {
        let mut steering = ItemBasedSteering::new();

        let Some(total) = steering.boost() else {
            panic!("first boost should start");
        };

        assert!((140..=980).contains(&total));
        assert!(steering.is_boosting());
        assert_eq!(steering.boost_time(), 0);
        assert!(steering.boost().is_none());
    }

    #[test]
    fn tick_boost_uses_vanilla_post_increment_expiry() {
        let mut steering = ItemBasedSteering::new();

        steering.on_synced();
        steering.tick_boost(2);
        assert_eq!(steering.boost_time(), 1);
        assert!(steering.is_boosting());

        steering.tick_boost(2);
        assert_eq!(steering.boost_time(), 2);
        assert!(steering.is_boosting());

        steering.tick_boost(2);
        assert_eq!(steering.boost_time(), 3);
        assert!(steering.is_boosting());

        steering.tick_boost(2);
        assert_eq!(steering.boost_time(), 4);
        assert!(!steering.is_boosting());
    }
}
