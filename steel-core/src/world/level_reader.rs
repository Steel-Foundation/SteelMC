//! Read-only world view shared by live worlds and world-generation regions.
//!
//! This mirrors vanilla's `LevelReader` role: block behavior such as
//! `canSurvive` should depend on the world-reading surface, not on the concrete
//! `World` type. `World` and `WorldGenRegion` both implement this trait.

use steel_registry::blocks::BlockRef;
use steel_registry::fluid::FluidRef;
use steel_utils::{BlockPos, BlockStateId};

/// Read-only level access needed by block behavior and worldgen predicates.
pub trait LevelReader {
    /// Gets the block state at a position.
    fn get_block_state(&self, pos: BlockPos) -> BlockStateId;

    /// Returns vanilla raw brightness at a position after sky darkening.
    fn raw_brightness(&self, pos: BlockPos, sky_darkening: u8) -> u8;

    /// Returns the minimum build height.
    fn min_y(&self) -> i32;

    /// Returns the build height.
    fn height(&self) -> i32;

    /// Returns the exclusive maximum build height.
    fn max_y_exclusive(&self) -> i32 {
        self.min_y() + self.height()
    }

    /// Checks if a Y coordinate is outside build height.
    fn is_outside_build_height(&self, y: i32) -> bool {
        y < self.min_y() || y >= self.max_y_exclusive()
    }
}

/// Level access needed by vanilla block `updateShape` logic.
///
/// Vanilla passes both `LevelReader` and `ScheduledTickAccess` to block shape updates.
/// Steel combines those surfaces so the same block behavior can run against a live
/// `World` and a `WorldGenRegion`.
pub trait ScheduledTickAccess: LevelReader {
    /// Returns the fluid tick delay in this level.
    fn fluid_tick_delay(&self, fluid: FluidRef) -> i32;

    /// Schedules a block tick using vanilla's default priority.
    fn schedule_block_tick_default(&self, pos: BlockPos, block: BlockRef, delay: i32) -> bool;

    /// Schedules a fluid tick using vanilla's default priority.
    fn schedule_fluid_tick_default(&self, pos: BlockPos, fluid: FluidRef, delay: i32) -> bool;
}
