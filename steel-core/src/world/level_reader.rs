//! Read-only world view shared by live worlds and world-generation regions.
//!
//! This mirrors vanilla's `LevelReader` role: block behavior such as
//! `canSurvive` should depend on the world-reading surface, not on the concrete
//! `World` type. `World` and `WorldGenRegion` both implement this trait.

use steel_utils::{BlockPos, BlockStateId};

/// Read-only level access needed by block behavior and worldgen predicates.
pub trait LevelReader {
    /// Gets the block state at a position.
    fn get_block_state(&self, pos: BlockPos) -> BlockStateId;

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
