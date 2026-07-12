use rustc_hash::FxHashMap;
use serde::{Deserialize, Serialize};
use steel_registry::{REGISTRY, RegistryExt as _, world_clock::WorldClockRef};
use steel_utils::Identifier;
use thiserror::Error;

/// One persisted instance of a registered world clock.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
struct ClockState {
    total_ticks: i64,
    partial_tick: f32,
    rate: f32,
    paused: bool,
}

impl Default for ClockState {
    fn default() -> Self {
        Self {
            total_ticks: 0,
            partial_tick: 0.0,
            rate: 1.0,
            paused: false,
        }
    }
}

/// Invalid persisted state for a world's clock manager.
#[derive(Debug, Error, PartialEq)]
pub(crate) enum WorldClockLoadError {
    #[error("saved state references unknown world clock {0}")]
    UnknownClock(Identifier),
    #[error("world clock {clock} has invalid partial tick {partial_tick}")]
    InvalidPartialTick {
        clock: Identifier,
        partial_tick: f32,
    },
    #[error("world clock {clock} has invalid rate {rate}")]
    InvalidRate { clock: Identifier, rate: f32 },
}

/// Per-world clock instances.
///
/// Vanilla 26.2 owns one clock manager at server scope and every loaded level
/// delegates to it. Steel intentionally persists one manager in each world's
/// level data instead: equal clock keys are not shared, so two overworlds in the
/// same domain can advance and be configured independently.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub(crate) struct WorldClockManager {
    clocks: FxHashMap<Identifier, ClockState>,
}

impl WorldClockManager {
    #[must_use]
    pub(crate) fn new() -> Self {
        let mut manager = Self::default();
        for (_, clock) in REGISTRY.world_clocks.iter() {
            manager
                .clocks
                .insert(clock.key.clone(), ClockState::default());
        }
        manager
    }

    /// Validates loaded states and creates default instances for newly registered clocks.
    pub(crate) fn initialize_registered_clocks(&mut self) -> Result<bool, WorldClockLoadError> {
        for (key, state) in &self.clocks {
            if REGISTRY.world_clocks.by_key(key).is_none() {
                return Err(WorldClockLoadError::UnknownClock(key.clone()));
            }
            if !state.partial_tick.is_finite() {
                return Err(WorldClockLoadError::InvalidPartialTick {
                    clock: key.clone(),
                    partial_tick: state.partial_tick,
                });
            }
            if !state.rate.is_finite() || state.rate <= 0.0 {
                return Err(WorldClockLoadError::InvalidRate {
                    clock: key.clone(),
                    rate: state.rate,
                });
            }
        }

        let previous_len = self.clocks.len();
        for (_, clock) in REGISTRY.world_clocks.iter() {
            self.clocks.entry(clock.key.clone()).or_default();
        }
        Ok(self.clocks.len() != previous_len)
    }

    #[must_use]
    pub(crate) fn total_ticks(&self, clock: WorldClockRef) -> Option<i64> {
        self.clocks.get(&clock.key).map(|state| state.total_ticks)
    }

    pub(crate) fn set_total_ticks(&mut self, clock: WorldClockRef, total_ticks: i64) -> Option<()> {
        let state = self.clocks.get_mut(&clock.key)?;
        state.total_ticks = total_ticks;
        state.partial_tick = 0.0;
        Some(())
    }

    pub(crate) fn add_ticks(&mut self, clock: WorldClockRef, ticks: i32) -> Option<i64> {
        let state = self.clocks.get_mut(&clock.key)?;
        state.total_ticks = state.total_ticks.wrapping_add(i64::from(ticks)).max(0);
        Some(state.total_ticks)
    }

    pub(crate) fn set_paused(&mut self, clock: WorldClockRef, paused: bool) -> Option<()> {
        let state = self.clocks.get_mut(&clock.key)?;
        state.paused = paused;
        Some(())
    }

    pub(crate) fn set_rate(&mut self, clock: WorldClockRef, rate: f32) -> Option<()> {
        if !rate.is_finite() || rate <= 0.0 {
            return None;
        }
        let state = self.clocks.get_mut(&clock.key)?;
        state.rate = rate;
        Some(())
    }

    /// Moves a clock to a marker, returning `false` when that marker is not defined for it.
    pub(crate) fn move_to_time_marker(
        &mut self,
        clock: WorldClockRef,
        marker_key: &Identifier,
    ) -> Option<bool> {
        let total_ticks = self.total_ticks(clock)?;
        let marker = REGISTRY
            .timelines
            .iter()
            .filter(|(_, timeline)| timeline.clock == clock)
            .find_map(|(_, timeline)| {
                timeline
                    .time_markers
                    .iter()
                    .find(|marker| &marker.key == marker_key)
                    .map(|marker| (marker, timeline.period_ticks))
            });
        let Some((marker, period_ticks)) = marker else {
            return Some(false);
        };
        let state = self.clocks.get_mut(&clock.key)?;
        state.total_ticks = marker.resolve_time_to_move_to(total_ticks, period_ticks);
        state.partial_tick = 0.0;
        Some(true)
    }

    /// Advances all unpaused clocks once when the world's `advance_time` rule allows it.
    pub(crate) fn tick(&mut self, advance_time: bool) -> bool {
        if !advance_time {
            return false;
        }
        let mut changed = false;
        for state in self.clocks.values_mut() {
            if state.paused {
                continue;
            }
            state.partial_tick += state.rate;
            let full_ticks = state.partial_tick.floor() as i32;
            state.partial_tick -= full_ticks as f32;
            state.total_ticks = state.total_ticks.wrapping_add(i64::from(full_ticks));
            changed = true;
        }
        changed
    }

    #[must_use]
    #[expect(
        clippy::cast_possible_truncation,
        reason = "registry IDs are bounded by the protocol's signed VarInt index space"
    )]
    pub(crate) fn network_updates(&self, advance_time: bool) -> Vec<(i32, i64, f32, f32)> {
        REGISTRY
            .world_clocks
            .iter()
            .filter_map(|(id, clock)| {
                self.clocks.get(&clock.key).map(|state| {
                    let rate = if state.paused || !advance_time {
                        0.0
                    } else {
                        state.rate
                    };
                    (id as i32, state.total_ticks, state.partial_tick, rate)
                })
            })
            .collect()
    }

    #[must_use]
    #[expect(
        clippy::cast_possible_truncation,
        reason = "registry IDs are bounded by the protocol's signed VarInt index space"
    )]
    pub(crate) fn network_update(
        &self,
        clock: WorldClockRef,
        advance_time: bool,
    ) -> Option<(i32, i64, f32, f32)> {
        let id = REGISTRY.world_clocks.id_from_key(&clock.key)?;
        let state = self.clocks.get(&clock.key)?;
        let rate = if state.paused || !advance_time {
            0.0
        } else {
            state.rate
        };
        Some((id as i32, state.total_ticks, state.partial_tick, rate))
    }
}

#[cfg(test)]
mod tests {
    use steel_registry::{test_support::init_test_registry, vanilla_world_clocks};

    use super::*;

    #[test]
    fn initializes_every_registered_clock() {
        init_test_registry();
        let manager = WorldClockManager::new();

        assert_eq!(
            manager.total_ticks(&vanilla_world_clocks::OVERWORLD),
            Some(0)
        );
        assert_eq!(manager.total_ticks(&vanilla_world_clocks::THE_END), Some(0));
        assert_eq!(manager.network_updates(true).len(), 2);
    }

    #[test]
    fn separate_worlds_keep_independent_clock_state() {
        init_test_registry();
        let mut first_world = WorldClockManager::new();
        let second_world = WorldClockManager::new();

        assert_eq!(
            first_world.set_total_ticks(&vanilla_world_clocks::OVERWORLD, 6_000),
            Some(())
        );
        assert_eq!(
            first_world.total_ticks(&vanilla_world_clocks::OVERWORLD),
            Some(6_000)
        );
        assert_eq!(
            second_world.total_ticks(&vanilla_world_clocks::OVERWORLD),
            Some(0)
        );
    }

    #[test]
    fn rate_accumulates_partial_ticks_like_vanilla() {
        init_test_registry();
        let mut manager = WorldClockManager::new();
        assert_eq!(
            manager.set_rate(&vanilla_world_clocks::OVERWORLD, 0.25),
            Some(())
        );

        for _ in 0..3 {
            assert!(manager.tick(true));
        }
        assert_eq!(
            manager.total_ticks(&vanilla_world_clocks::OVERWORLD),
            Some(0)
        );
        assert!(manager.tick(true));
        assert_eq!(
            manager.total_ticks(&vanilla_world_clocks::OVERWORLD),
            Some(1)
        );
    }

    #[test]
    #[expect(
        clippy::float_cmp,
        reason = "paused and non-advancing clocks must report the exact zero network rate"
    )]
    fn pause_and_advance_time_gate_network_rate_and_ticks() {
        init_test_registry();
        let mut manager = WorldClockManager::new();
        assert_eq!(
            manager.set_paused(&vanilla_world_clocks::OVERWORLD, true),
            Some(())
        );

        assert!(manager.tick(true));
        assert_eq!(
            manager.total_ticks(&vanilla_world_clocks::OVERWORLD),
            Some(0)
        );
        let Some(update) = manager.network_update(&vanilla_world_clocks::OVERWORLD, true) else {
            panic!("overworld clock update should exist");
        };
        assert_eq!(update.3, 0.0);

        let Some(update) = manager.network_update(&vanilla_world_clocks::THE_END, false) else {
            panic!("end clock update should exist");
        };
        assert_eq!(update.3, 0.0);
    }

    #[test]
    fn repeating_time_marker_moves_strictly_forward() {
        init_test_registry();
        let mut manager = WorldClockManager::new();
        assert_eq!(
            manager.set_total_ticks(&vanilla_world_clocks::OVERWORLD, 1_000),
            Some(())
        );

        assert_eq!(
            manager.move_to_time_marker(
                &vanilla_world_clocks::OVERWORLD,
                &Identifier::vanilla_static("day")
            ),
            Some(true)
        );
        assert_eq!(
            manager.total_ticks(&vanilla_world_clocks::OVERWORLD),
            Some(25_000)
        );
    }

    #[test]
    fn manager_round_trips_through_toml() {
        init_test_registry();
        let mut manager = WorldClockManager::new();
        assert_eq!(
            manager.set_total_ticks(&vanilla_world_clocks::OVERWORLD, 12_345),
            Some(())
        );
        assert_eq!(
            manager.set_rate(&vanilla_world_clocks::THE_END, 2.5),
            Some(())
        );

        let serialized = toml::to_string(&manager).expect("clock manager should serialize");
        let mut restored: WorldClockManager =
            toml::from_str(&serialized).expect("clock manager should deserialize");
        assert_eq!(restored.initialize_registered_clocks(), Ok(false));
        assert_eq!(restored, manager);
    }
}
