//! World generation region for cross-chunk block placement.
//!
//! During feature generation, features may place blocks across chunk boundaries.
//! `WorldGenRegion` provides safe access to neighboring chunks while enforcing
//! write radius limits.

use std::sync::Arc;

use parking_lot::RwLockReadGuard;
use steel_registry::blocks::block_state_ext::BlockStateExt;
use steel_utils::{BlockPos, BlockStateId, ChunkPos};

use crate::chunk::{
    chunk_access::{ChunkAccess, ChunkStatus},
    chunk_generation_task::StaticCache2D,
    chunk_holder::ChunkHolder,
    chunk_pyramid::ChunkStep,
    heightmap::HeightmapType,
};

/// A region for world generation that allows cross-chunk block placement.
///
/// Wraps a center chunk and its neighbors, enforcing write radius limits.
/// Similar to vanilla's `WorldGenRegion`.
pub struct WorldGenRegion {
    /// The center chunk position being generated.
    center: ChunkPos,
    /// The chunk cache containing center and neighbors.
    cache: Arc<StaticCache2D<Arc<ChunkHolder>>>,
    /// The current generation step (determines write radius).
    step: ChunkStep,
    /// Minimum Y coordinate of the world.
    min_y: i32,
    /// Total height of the world.
    height: i32,
}

impl WorldGenRegion {
    /// Creates a new world generation region.
    #[must_use]
    pub fn new(
        center: ChunkPos,
        cache: Arc<StaticCache2D<Arc<ChunkHolder>>>,
        step: ChunkStep,
        min_y: i32,
        height: i32,
    ) -> Self {
        Self {
            center,
            cache,
            step,
            min_y,
            height,
        }
    }

    /// Gets the center chunk position.
    #[must_use]
    pub fn center(&self) -> ChunkPos {
        self.center
    }

    /// Gets the minimum Y coordinate.
    #[must_use]
    pub fn min_y(&self) -> i32 {
        self.min_y
    }

    /// Gets the maximum Y coordinate (exclusive).
    #[must_use]
    pub fn max_y(&self) -> i32 {
        self.min_y + self.height
    }

    /// Checks if a position is within the write radius.
    ///
    /// Returns `true` if blocks can be written at this position.
    /// Logs a warning and returns `false` for out-of-bounds writes.
    #[must_use]
    pub fn ensure_can_write(&self, pos: BlockPos) -> bool {
        let chunk_x = pos.x() >> 4;
        let chunk_z = pos.z() >> 4;

        let distance_x = (self.center.0.x - chunk_x).abs();
        let distance_z = (self.center.0.y - chunk_z).abs();

        let write_radius = self.step.block_state_write_radius;

        if distance_x <= write_radius && distance_z <= write_radius {
            // Check Y bounds
            if pos.y() < self.min_y || pos.y() >= self.max_y() {
                return false;
            }
            true
        } else {
            tracing::warn!(
                "Detected setBlock in a far chunk [{}, {}], pos: {:?}, status: {:?}, \
                 write_radius: {}, center: {:?}",
                chunk_x,
                chunk_z,
                pos,
                self.step.target_status,
                write_radius,
                self.center
            );
            false
        }
    }

    /// Gets the chunk at the given block position.
    ///
    /// # Panics
    /// Panics if the chunk is not available in the cache.
    fn get_chunk_at(&self, pos: BlockPos) -> RwLockReadGuard<'_, ChunkAccess> {
        let chunk_x = pos.x() >> 4;
        let chunk_z = pos.z() >> 4;
        let holder = self.cache.get(chunk_x, chunk_z);
        // Features step requires Carvers at radius 1
        holder
            .try_chunk(ChunkStatus::Carvers)
            .expect("Chunk not at Carvers status")
    }

    /// Gets the block state at the given position.
    #[must_use]
    pub fn get_block(&self, pos: BlockPos) -> BlockStateId {
        if pos.y() < self.min_y || pos.y() >= self.max_y() {
            return BlockStateId(0); // Air
        }
        let chunk = self.get_chunk_at(pos);
        let local_x = (pos.x() & 15) as usize;
        let local_z = (pos.z() & 15) as usize;
        let relative_y = (pos.y() - self.min_y) as usize;
        chunk
            .get_relative_block(local_x, relative_y, local_z)
            .unwrap_or(BlockStateId(0))
    }

    /// Sets a block at the given position.
    ///
    /// Returns `true` if the block was set, `false` if the position was out of bounds.
    pub fn set_block(&self, pos: BlockPos, state: BlockStateId) -> bool {
        if !self.ensure_can_write(pos) {
            return false;
        }

        let chunk = self.get_chunk_at(pos);
        let local_x = (pos.x() & 15) as usize;
        let local_z = (pos.z() & 15) as usize;
        let relative_y = (pos.y() - self.min_y) as usize;
        chunk.set_relative_block(local_x, relative_y, local_z, state);
        true
    }

    /// Gets the height at the given position using the specified heightmap.
    ///
    /// Returns the Y coordinate of the first available position (one above the highest block).
    #[must_use]
    pub fn get_height(&self, heightmap: HeightmapType, x: i32, z: i32) -> i32 {
        let chunk_x = x >> 4;
        let chunk_z = z >> 4;
        let holder = self.cache.get(chunk_x, chunk_z);
        let chunk = holder
            .try_chunk(ChunkStatus::Carvers)
            .expect("Chunk not at Carvers status");

        let local_x = (x & 15) as usize;
        let local_z = (z & 15) as usize;

        // Use proto heightmaps (features step runs before chunk is full)
        let proto_heightmaps = chunk.proto_heightmaps();
        if let Some(hm) = proto_heightmaps.get(heightmap) {
            let height = hm.get_first_available(local_x, local_z);
            tracing::warn!(
                "get_height({:?}, {}, {}): found heightmap, returning {}",
                heightmap,
                x,
                z,
                height
            );
            return height;
        }

        tracing::warn!(
            "get_height({:?}, {}, {}): no heightmap found, using default {}",
            heightmap,
            x,
            z,
            self.min_y + 64
        );
        // Default to surface level if no heightmap
        self.min_y + 64
    }

    /// Gets the biome ID at the given quart position.
    #[must_use]
    pub fn get_biome(&self, quart_x: i32, quart_y: i32, quart_z: i32) -> u16 {
        let chunk_x = quart_x >> 2;
        let chunk_z = quart_z >> 2;
        let holder = self.cache.get(chunk_x, chunk_z);
        let chunk = holder
            .try_chunk(ChunkStatus::Biomes)
            .expect("Chunk not at Biomes status");

        let sections = chunk.sections();
        let local_qx = (quart_x - chunk_x * 4) as usize;
        let local_qz = (quart_z - chunk_z * 4) as usize;

        let min_qy = self.min_y >> 2;
        let total_quarts_y = sections.sections.len() * 4;
        let qy_clamped = (quart_y - min_qy).clamp(0, total_quarts_y as i32 - 1) as usize;
        let section_idx = qy_clamped / 4;
        let local_qy = qy_clamped % 4;

        sections.sections[section_idx]
            .read()
            .biomes
            .get(local_qx, local_qy, local_qz)
    }

    /// Checks if a block at the given position is air.
    #[must_use]
    pub fn is_air(&self, pos: BlockPos) -> bool {
        self.get_block(pos).is_air()
    }
}
