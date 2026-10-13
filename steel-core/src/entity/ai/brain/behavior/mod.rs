//! Brain behaviors: what a mob does once its memories say it can.

mod control;

#[cfg(test)]
mod tests;

use std::any::type_name;

pub use control::{BehaviorControl, BehaviorExt, BehaviorStatus, TimedBehavior};

use super::context::BrainContext;
use super::memory::{MemoryModuleTypeRef, MemoryStatus};

/// How long one activation of a behavior may last.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BehaviorDuration {
    min: i32,
    max: i32,
}

impl BehaviorDuration {
    /// Vanilla's default 60 tick timeout.
    pub const DEFAULT: Self = Self::fixed(60);

    /// A duration of exactly `ticks`.
    #[must_use]
    pub const fn fixed(ticks: i32) -> Self {
        Self::range(ticks, ticks)
    }

    /// A duration rolled from `min..=max` on every activation.
    ///
    /// # Panics
    ///
    /// Panics if `max` is below `min`.
    #[must_use]
    pub const fn range(min: i32, max: i32) -> Self {
        assert!(min <= max, "behavior duration range is inverted");
        Self { min, max }
    }

    /// Rolls one activation's length.
    fn roll(self) -> i32 {
        self.min + rand::random_range(0..=self.max - self.min)
    }
}

impl Default for BehaviorDuration {
    fn default() -> Self {
        Self::DEFAULT
    }
}

/// One thing a mob can do, gated by its memories.
///
/// Mirrors vanilla `Behavior`.
pub trait Behavior: Send {
    /// The memory states this behavior needs before it may start.
    ///
    /// Vanilla `Behavior.entryCondition`.
    fn entry_condition(&self) -> &[(MemoryModuleTypeRef, MemoryStatus)] {
        &[]
    }

    /// How long one activation lasts, rolled anew each time it starts.
    fn duration(&self) -> BehaviorDuration {
        BehaviorDuration::DEFAULT
    }

    /// Vanilla `Behavior.checkExtraStartConditions`.
    fn check_extra_start_conditions(&mut self, _context: &mut BrainContext<'_>) -> bool {
        true
    }

    /// Runs once when the behavior starts.
    fn start(&mut self, _context: &mut BrainContext<'_>) {}

    /// Whether the behavior should keep running.
    fn can_still_use(&mut self, _context: &mut BrainContext<'_>) -> bool {
        false
    }

    /// Runs every tick while the behavior is running.
    fn tick(&mut self, _context: &mut BrainContext<'_>) {}

    /// Runs once when the behavior stops, however it stopped.
    fn stop(&mut self, _context: &mut BrainContext<'_>) {}

    /// Name used in brain debug output. Vanilla `Behavior.debugString`.
    fn debug_name(&self) -> &'static str {
        type_name::<Self>()
    }
}
