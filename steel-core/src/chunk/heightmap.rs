//! Heightmap implementation for tracking the highest blocks in a chunk.
//!
//! Heightmaps are used for various purposes like spawning, pathfinding, and rendering.

use rustc_hash::FxHashMap;

use steel_registry::{
    REGISTRY,
    blocks::{BlockRef, block_state_ext::BlockStateExt},
    vanilla_block_tags,
};
use steel_utils::BlockStateId;

/// The different types of heightmaps.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum HeightmapType {
    // Final heightmaps (sent to client, used after CARVERS status)
    /// Tracks the highest non-air block. Used for world surface calculations.
    WorldSurface,
    /// Tracks the highest motion-blocking block (solid or fluid).
    MotionBlocking,
    /// Tracks the highest motion-blocking block that is not leaves.
    MotionBlockingNoLeaves,
    /// Tracks the highest solid block (ocean floor).
    OceanFloor,
    // Worldgen heightmaps (used before CARVERS status)
    /// Worldgen version of `WorldSurface`.
    WorldSurfaceWg,
    /// Worldgen version of `OceanFloor`.
    OceanFloorWg,
}

impl HeightmapType {
    /// Returns worldgen heightmap types (used before CARVERS status).
    #[must_use]
    pub const fn worldgen_types() -> &'static [HeightmapType] {
        &[HeightmapType::WorldSurfaceWg, HeightmapType::OceanFloorWg]
    }

    /// Returns final heightmap types (used at CARVERS status and after).
    #[must_use]
    pub const fn final_types() -> &'static [HeightmapType] {
        &[
            HeightmapType::WorldSurface,
            HeightmapType::MotionBlocking,
            HeightmapType::MotionBlockingNoLeaves,
            HeightmapType::OceanFloor,
        ]
    }

    /// Returns whether a block is "opaque" for this heightmap type.
    /// This determines whether the block counts towards the heightmap.
    ///
    /// # Panics
    /// Panics if the block state ID is invalid.
    #[must_use]
    pub fn is_opaque(&self, state: BlockStateId) -> bool {
        let block = REGISTRY
            .blocks
            .by_state_id(state)
            .expect("Invalid state ID");
        match self {
            Self::WorldSurface | Self::WorldSurfaceWg => !block.config.is_air,
            Self::MotionBlocking => block.config.has_collision || block.config.liquid,
            Self::MotionBlockingNoLeaves => {
                (block.config.has_collision || block.config.liquid) && !Self::is_leaves(block)
            }
            Self::OceanFloor | Self::OceanFloorWg => block.config.has_collision,
        }
    }

    /// Checks if a block is in the leaves tag.
    fn is_leaves(block: BlockRef) -> bool {
        REGISTRY
            .blocks
            .is_in_tag(block, &vanilla_block_tags::LEAVES_TAG)
    }
}

/// Primes heightmaps by scanning the chunk from top to bottom.
///
/// Used to lazily initialize heightmaps when they don't exist yet.
/// This scans each column from top to bottom, finding the first opaque block
/// for each heightmap type.
#[allow(clippy::missing_panics_doc, clippy::implicit_hasher)]
pub fn prime_heightmaps<F>(
    heightmaps: &mut FxHashMap<HeightmapType, Heightmap>,
    types: &[HeightmapType],
    min_y: i32,
    height: i32,
    get_block: F,
) where
    F: Fn(usize, i32, usize) -> BlockStateId,
{
    // Collect types that need priming (don't exist yet)
    let types_to_prime: Vec<HeightmapType> = types
        .iter()
        .filter(|&&hm_type| !heightmaps.contains_key(&hm_type))
        .copied()
        .collect();

    if types_to_prime.is_empty() {
        return;
    }

    // Create missing heightmaps
    for &hm_type in &types_to_prime {
        heightmaps.insert(hm_type, Heightmap::new(hm_type, min_y, height));
    }

    let max_y = min_y + height;

    // For each column, scan from top to bottom
    for x in 0..16 {
        for z in 0..16 {
            // Track which heightmaps still need to find their first opaque block
            let mut pending: Vec<HeightmapType> = types_to_prime.clone();

            for y in (min_y..max_y).rev() {
                if pending.is_empty() {
                    break;
                }

                let state = get_block(x, y, z);
                if state.is_air() {
                    continue;
                }

                // Check each pending heightmap type
                pending.retain(|&hm_type| {
                    if hm_type.is_opaque(state) {
                        heightmaps
                            .get_mut(&hm_type)
                            .expect("heightmap was just inserted")
                            .set_height(x, z, y + 1);
                        false // Remove from pending
                    } else {
                        true // Keep in pending
                    }
                });
            }
        }
    }
}

/// A heightmap that tracks the highest blocks of a specific type in a chunk.
///
/// The heightmap stores heights for each column in a 16x16 chunk.
/// Heights are stored relative to `min_y`, so `data[index] + min_y` gives the actual Y coordinate.
#[derive(Debug, Clone)]
pub struct Heightmap {
    /// Height data stored as a flat array of 256 entries (16x16).
    /// Each entry stores the height relative to `min_y`.
    data: Box<[u16; 256]>,
    /// The type of this heightmap.
    map_type: HeightmapType,
    /// The minimum Y coordinate of the world.
    min_y: i32,
    /// The total height of the world.
    height: i32,
}

impl Heightmap {
    /// Creates a new heightmap with all heights initialized to `min_y`.
    #[must_use]
    pub fn new(map_type: HeightmapType, min_y: i32, height: i32) -> Self {
        Self {
            data: Box::new([0; 256]),
            map_type,
            min_y,
            height,
        }
    }

    /// Returns the heightmap type.
    #[must_use]
    pub const fn heightmap_type(&self) -> HeightmapType {
        self.map_type
    }

    /// Gets the index into the data array for the given local coordinates.
    #[inline]
    const fn get_index(local_x: usize, local_z: usize) -> usize {
        local_x + local_z * 16
    }

    /// Gets the first available Y coordinate (one above the highest block) at the given position.
    #[must_use]
    pub fn get_first_available(&self, local_x: usize, local_z: usize) -> i32 {
        debug_assert!(local_x < 16 && local_z < 16);
        let index = Self::get_index(local_x, local_z);
        i32::from(self.data[index]) + self.min_y
    }

    /// Gets the highest taken Y coordinate at the given position.
    #[must_use]
    pub fn get_highest_taken(&self, local_x: usize, local_z: usize) -> i32 {
        self.get_first_available(local_x, local_z) - 1
    }

    /// Sets the height at the given position.
    pub fn set_height(&mut self, local_x: usize, local_z: usize, height: i32) {
        debug_assert!(local_x < 16 && local_z < 16);
        let index = Self::get_index(local_x, local_z);
        self.data[index] = (height - self.min_y) as u16;
    }

    /// Updates the heightmap when a block changes.
    ///
    /// Returns `true` if the heightmap was modified.
    ///
    /// # Arguments
    /// * `local_x` - The local X coordinate (0-15)
    /// * `y` - The absolute Y coordinate
    /// * `local_z` - The local Z coordinate (0-15)
    /// * `state` - The new block state at this position
    /// * `get_block` - A function to get block states at other positions for scanning down
    pub fn update<F>(
        &mut self,
        local_x: usize,
        y: i32,
        local_z: usize,
        state: BlockStateId,
        get_block: F,
    ) -> bool
    where
        F: Fn(usize, i32, usize) -> BlockStateId,
    {
        let first_available = self.get_first_available(local_x, local_z);

        // If the block is well below the current height, it can't affect the heightmap
        if y <= first_available - 2 {
            return false;
        }

        if self.map_type.is_opaque(state) {
            // Block is opaque - if it's at or above current height, update
            if y >= first_available {
                self.set_height(local_x, local_z, y + 1);
                return true;
            }
        } else if first_available - 1 == y {
            // Block is not opaque and is at the current top - scan down to find new height
            for scan_y in (self.min_y..y).rev() {
                let scan_state = get_block(local_x, scan_y, local_z);
                if self.map_type.is_opaque(scan_state) {
                    self.set_height(local_x, local_z, scan_y + 1);
                    return true;
                }
            }
            // No opaque block found, set to min_y
            self.set_height(local_x, local_z, self.min_y);
            return true;
        }

        false
    }

    /// Gets the raw data as a slice of i64 values for serialization.
    ///
    /// The data is packed using the minimum number of bits required to store
    /// the height range (0 to `world_height`).
    #[must_use]
    pub fn get_raw_data(&self) -> Vec<i64> {
        let bits_per_value = Self::calculate_bits_per_value(self.height);
        let values_per_long = 64 / bits_per_value;
        let num_longs = 256_usize.div_ceil(values_per_long);

        let mut result = vec![0i64; num_longs];
        let mask = (1u64 << bits_per_value) - 1;

        for (i, &height) in self.data.iter().enumerate() {
            let long_index = i / values_per_long;
            let bit_offset = (i % values_per_long) * bits_per_value;
            result[long_index] |= ((u64::from(height) & mask) << bit_offset) as i64;
        }

        result
    }

    /// Sets the raw data from a slice of i64 values.
    ///
    /// # Panics
    /// Panics if the data length doesn't match the expected size.
    pub fn set_raw_data(&mut self, data: &[i64]) {
        let bits_per_value = Self::calculate_bits_per_value(self.height);
        let values_per_long = 64 / bits_per_value;
        let expected_longs = 256_usize.div_ceil(values_per_long);

        if data.len() != expected_longs {
            log::warn!(
                "Heightmap data size mismatch: expected {}, got {}. Ignoring.",
                expected_longs,
                data.len()
            );
            return;
        }

        let mask = (1u64 << bits_per_value) - 1;

        for i in 0..256 {
            let long_index = i / values_per_long;
            let bit_offset = (i % values_per_long) * bits_per_value;
            let value = ((data[long_index] as u64) >> bit_offset) & mask;
            self.data[i] = value as u16;
        }
    }

    /// Calculates the number of bits required to store heights for a given world height.
    #[inline]
    const fn calculate_bits_per_value(height: i32) -> usize {
        // Need to store values from 0 to height (inclusive)
        // ceil(log2(height + 1))
        let max_value = height as u32 + 1;
        if max_value <= 1 {
            1
        } else {
            32 - (max_value - 1).leading_zeros() as usize
        }
    }
}

/// A collection of heightmaps for a chunk.
#[derive(Debug, Clone)]
pub struct ChunkHeightmaps {
    /// World surface heightmap.
    pub world_surface: Heightmap,
    /// Motion blocking heightmap.
    pub motion_blocking: Heightmap,
    /// Motion blocking (no leaves) heightmap.
    pub motion_blocking_no_leaves: Heightmap,
    /// Ocean floor heightmap.
    pub ocean_floor: Heightmap,
}

impl ChunkHeightmaps {
    /// Creates a new set of heightmaps for a chunk.
    #[must_use]
    pub fn new(min_y: i32, height: i32) -> Self {
        Self {
            world_surface: Heightmap::new(HeightmapType::WorldSurface, min_y, height),
            motion_blocking: Heightmap::new(HeightmapType::MotionBlocking, min_y, height),
            motion_blocking_no_leaves: Heightmap::new(
                HeightmapType::MotionBlockingNoLeaves,
                min_y,
                height,
            ),
            ocean_floor: Heightmap::new(HeightmapType::OceanFloor, min_y, height),
        }
    }

    /// Gets a reference to a heightmap by type.
    ///
    /// # Panics
    /// Panics if called with a worldgen heightmap type (`WorldSurfaceWg`, `OceanFloorWg`).
    #[must_use]
    pub fn get(&self, heightmap_type: HeightmapType) -> &Heightmap {
        match heightmap_type {
            HeightmapType::WorldSurface => &self.world_surface,
            HeightmapType::MotionBlocking => &self.motion_blocking,
            HeightmapType::MotionBlockingNoLeaves => &self.motion_blocking_no_leaves,
            HeightmapType::OceanFloor => &self.ocean_floor,
            HeightmapType::WorldSurfaceWg | HeightmapType::OceanFloorWg => {
                panic!("ChunkHeightmaps does not store worldgen heightmaps")
            }
        }
    }

    /// Gets a mutable reference to a heightmap by type.
    ///
    /// # Panics
    /// Panics if called with a worldgen heightmap type (`WorldSurfaceWg`, `OceanFloorWg`).
    #[must_use]
    pub fn get_mut(&mut self, heightmap_type: HeightmapType) -> &mut Heightmap {
        match heightmap_type {
            HeightmapType::WorldSurface => &mut self.world_surface,
            HeightmapType::MotionBlocking => &mut self.motion_blocking,
            HeightmapType::MotionBlockingNoLeaves => &mut self.motion_blocking_no_leaves,
            HeightmapType::OceanFloor => &mut self.ocean_floor,
            HeightmapType::WorldSurfaceWg | HeightmapType::OceanFloorWg => {
                panic!("ChunkHeightmaps does not store worldgen heightmaps")
            }
        }
    }

    /// Updates all heightmaps when a block changes.
    pub fn update<F>(
        &mut self,
        local_x: usize,
        y: i32,
        local_z: usize,
        state: BlockStateId,
        get_block: F,
    ) where
        F: Fn(usize, i32, usize) -> BlockStateId + Copy,
    {
        self.world_surface
            .update(local_x, y, local_z, state, get_block);
        self.motion_blocking
            .update(local_x, y, local_z, state, get_block);
        self.motion_blocking_no_leaves
            .update(local_x, y, local_z, state, get_block);
        self.ocean_floor
            .update(local_x, y, local_z, state, get_block);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bits_per_value() {
        // Standard overworld height (384 blocks: -64 to 319)
        assert_eq!(Heightmap::calculate_bits_per_value(384), 9);
        // Nether height (256 blocks)
        assert_eq!(Heightmap::calculate_bits_per_value(256), 9);
        // Small height
        assert_eq!(Heightmap::calculate_bits_per_value(16), 5);
    }

    #[test]
    fn test_get_index() {
        assert_eq!(Heightmap::get_index(0, 0), 0);
        assert_eq!(Heightmap::get_index(15, 0), 15);
        assert_eq!(Heightmap::get_index(0, 1), 16);
        assert_eq!(Heightmap::get_index(15, 15), 255);
    }
}
