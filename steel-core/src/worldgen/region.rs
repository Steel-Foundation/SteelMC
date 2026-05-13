//! Region access for chunk feature generation.
//!
//! Feature placement needs the center chunk plus its direct dependencies, while writes must
//! stay inside the stage's block-state write radius. `WorldGenRegion` centralizes that
//! contract so feature, structure, and vegetation code cannot bypass the chunk pyramid.

use std::sync::Arc;

use parking_lot::RwLockReadGuard;
use steel_utils::{BlockPos, BlockStateId, ChunkPos, SectionPos, types::UpdateFlags};

use crate::chunk::{
    chunk_access::{ChunkAccess, ChunkStatus},
    chunk_generation_task::StaticCache2D,
    chunk_holder::ChunkHolder,
    chunk_pyramid::ChunkStep,
    heightmap::HeightmapType,
};
use crate::worldgen::context::WorldGenContext;

/// Chunk-cache backed worldgen view for the current generation step.
///
/// This deliberately differs from vanilla's `WorldGenRegion` in one area: it only exposes
/// chunks already collected by Steel's `StaticCache2D` and validated against `ChunkStep`.
/// That keeps generation deterministic and makes missing dependency declarations fail at
/// the region boundary instead of silently reading farther chunks.
pub struct WorldGenRegion<'a> {
    context: &'a WorldGenContext,
    step: &'a ChunkStep,
    cache: &'a StaticCache2D<Arc<ChunkHolder>>,
    center: ChunkPos,
}

impl<'a> WorldGenRegion<'a> {
    /// Creates a new region over the chunks collected for a generation step.
    #[must_use]
    pub const fn new(
        context: &'a WorldGenContext,
        step: &'a ChunkStep,
        cache: &'a StaticCache2D<Arc<ChunkHolder>>,
        center: ChunkPos,
    ) -> Self {
        Self {
            context,
            step,
            cache,
            center,
        }
    }

    /// Returns the center chunk being generated.
    #[must_use]
    pub const fn center(&self) -> ChunkPos {
        self.center
    }

    /// Returns the minimum build height.
    #[must_use]
    pub fn min_y(&self) -> i32 {
        self.context.min_y()
    }

    /// Returns the world height.
    #[must_use]
    pub fn height(&self) -> i32 {
        self.context.height()
    }

    /// Returns the exclusive maximum build height.
    #[must_use]
    pub fn max_y_exclusive(&self) -> i32 {
        self.min_y() + self.height()
    }

    /// Checks if a Y coordinate is outside the build height.
    #[must_use]
    pub fn is_outside_build_height(&self, y: i32) -> bool {
        y < self.min_y() || y >= self.max_y_exclusive()
    }

    /// Returns the strongest status directly available for a chunk position in this step.
    #[must_use]
    pub fn required_status_at(&self, chunk_x: i32, chunk_z: i32) -> Option<ChunkStatus> {
        self.step
            .direct_dependencies
            .get(Self::chessboard_distance(self.center, chunk_x, chunk_z))
    }

    /// Returns whether block writes are allowed in the given chunk.
    #[must_use]
    pub fn can_write_to_chunk(&self, chunk_x: i32, chunk_z: i32) -> bool {
        let radius = self.step.block_state_write_radius;
        radius >= 0
            && (chunk_x - self.center.0.x).abs() <= radius
            && (chunk_z - self.center.0.y).abs() <= radius
    }

    /// Gets a chunk if the step declares enough direct dependency status for it.
    #[must_use]
    pub fn try_chunk(
        &self,
        chunk_x: i32,
        chunk_z: i32,
        status: ChunkStatus,
    ) -> Option<RwLockReadGuard<'_, ChunkAccess>> {
        let available_status = self.required_status_at(chunk_x, chunk_z)?;
        if status > available_status {
            return None;
        }

        self.cache.get(chunk_x, chunk_z).try_chunk(status)
    }

    /// Gets a chunk or panics if generation requested an undeclared dependency.
    ///
    /// # Panics
    /// Panics if the chunk is outside this step's direct dependencies, if the requested
    /// status is higher than the dependency contract, or if the holder has not reached
    /// the declared status. Those cases indicate a chunk-pyramid or scheduler bug.
    pub fn chunk(
        &self,
        chunk_x: i32,
        chunk_z: i32,
        status: ChunkStatus,
    ) -> RwLockReadGuard<'_, ChunkAccess> {
        let Some(chunk) = self.try_chunk(chunk_x, chunk_z, status) else {
            let available = self.required_status_at(chunk_x, chunk_z);
            panic!(
                "Worldgen requested chunk ({chunk_x}, {chunk_z}) at status {status:?}, \
                 but the {:?} step only provides {available:?} at that distance from ({}, {})",
                self.step.target_status, self.center.0.x, self.center.0.y
            );
        };

        chunk
    }

    /// Gets a block state through the region dependency contract.
    ///
    /// # Panics
    /// Panics if the position's chunk is outside this step's direct dependencies.
    #[must_use]
    pub fn block_state(&self, pos: BlockPos) -> BlockStateId {
        let chunk_x = SectionPos::block_to_section_coord(pos.x());
        let chunk_z = SectionPos::block_to_section_coord(pos.z());
        self.chunk(chunk_x, chunk_z, ChunkStatus::Empty)
            .get_block_state(pos)
    }

    /// Sets a block state if the position is inside the step's write radius.
    ///
    /// Returns whether the write was accepted by the region. Positions outside the write
    /// radius are rejected without touching chunk data, matching vanilla's
    /// `WorldGenRegion.ensureCanWrite` gate.
    ///
    /// # Panics
    /// Panics if a position inside the write radius is not covered by this step's
    /// direct dependencies, or if the holder has not reached the declared status.
    pub fn set_block_state(&self, pos: BlockPos, state: BlockStateId, flags: UpdateFlags) -> bool {
        let chunk_x = SectionPos::block_to_section_coord(pos.x());
        let chunk_z = SectionPos::block_to_section_coord(pos.z());

        if !self.can_write_to_chunk(chunk_x, chunk_z) {
            log::error!(
                "Worldgen attempted to write block at ({}, {}, {}) outside {:?} write radius {} centered on ({}, {})",
                pos.x(),
                pos.y(),
                pos.z(),
                self.step.target_status,
                self.step.block_state_write_radius,
                self.center.0.x,
                self.center.0.y,
            );
            return false;
        }

        let Some(status) = self.required_status_at(chunk_x, chunk_z) else {
            panic!(
                "Worldgen attempted to write block at ({}, {}, {}) in chunk ({chunk_x}, {chunk_z}), \
                 but {:?} declares no direct dependency for that chunk",
                pos.x(),
                pos.y(),
                pos.z(),
                self.step.target_status,
            );
        };

        self.chunk(chunk_x, chunk_z, status)
            .set_block_state(pos, state, flags);
        true
    }

    /// Marks a position for vanilla proto-chunk postprocessing after full promotion.
    ///
    /// # Panics
    /// Panics if the target chunk is outside this step's direct dependencies.
    pub fn mark_pos_for_postprocessing(&self, pos: BlockPos) {
        let chunk_x = SectionPos::block_to_section_coord(pos.x());
        let chunk_z = SectionPos::block_to_section_coord(pos.z());
        self.chunk(chunk_x, chunk_z, ChunkStatus::Empty)
            .mark_pos_for_postprocessing(pos);
    }

    /// Gets the first available Y coordinate for a heightmap column.
    ///
    /// # Panics
    /// Panics if the target chunk is not available at `Carvers` status, or if the
    /// requested heightmap has not been primed before decoration.
    #[must_use]
    pub fn height_at(&self, heightmap_type: HeightmapType, x: i32, z: i32) -> i32 {
        let chunk_x = SectionPos::block_to_section_coord(x);
        let chunk_z = SectionPos::block_to_section_coord(z);
        let local_x = (x & 15) as usize;
        let local_z = (z & 15) as usize;

        let Some(height) = self
            .chunk(chunk_x, chunk_z, ChunkStatus::Carvers)
            .height_at(heightmap_type, local_x, local_z)
        else {
            panic!("Worldgen requested unprimed {heightmap_type:?} heightmap at block ({x}, {z})");
        };

        height
    }

    const fn chessboard_distance(center: ChunkPos, chunk_x: i32, chunk_z: i32) -> usize {
        let dx = abs_diff(center.0.x, chunk_x);
        let dz = abs_diff(center.0.y, chunk_z);
        if dx > dz { dx as usize } else { dz as usize }
    }
}

const fn abs_diff(left: i32, right: i32) -> i32 {
    if left >= right {
        left - right
    } else {
        right - left
    }
}

#[cfg(test)]
mod tests {
    use steel_utils::ChunkPos;

    use super::WorldGenRegion;

    #[test]
    fn chessboard_distance_matches_chunk_dependency_radius() {
        let center = ChunkPos::new(4, -2);

        assert_eq!(WorldGenRegion::chessboard_distance(center, 4, -2), 0);
        assert_eq!(WorldGenRegion::chessboard_distance(center, 5, -3), 1);
        assert_eq!(WorldGenRegion::chessboard_distance(center, -4, 6), 8);
    }
}
