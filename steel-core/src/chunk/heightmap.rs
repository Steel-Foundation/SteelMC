//! Heightmap implementation for tracking the highest blocks in a chunk.
//!
//! Heightmaps are used for various purposes like spawning, pathfinding, and rendering.
//!
//! `ChunkHeightmaps` stores the six vanilla heightmap slots across every chunk phase.
//! Individual maps are materialized as required by the chunk's generation status.

use std::sync::LazyLock;

use smallvec::SmallVec;
use steel_registry::{
    REGISTRY,
    blocks::{BlockRef, block_state_ext::BlockStateExt},
    vanilla_block_tags::BlockTag,
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
    const WORLD_SURFACE_MASK: u8 = 1 << 0;
    const MOTION_BLOCKING_MASK: u8 = 1 << 1;
    const MOTION_BLOCKING_NO_LEAVES_MASK: u8 = 1 << 2;
    const OCEAN_FLOOR_MASK: u8 = 1 << 3;
    const WORLD_SURFACE_WG_MASK: u8 = 1 << 4;
    const OCEAN_FLOOR_WG_MASK: u8 = 1 << 5;

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

    /// Returns all heightmap types in their stable persistence order.
    #[must_use]
    pub const fn all_types() -> &'static [HeightmapType] {
        &[
            HeightmapType::WorldSurface,
            HeightmapType::MotionBlocking,
            HeightmapType::MotionBlockingNoLeaves,
            HeightmapType::OceanFloor,
            HeightmapType::WorldSurfaceWg,
            HeightmapType::OceanFloorWg,
        ]
    }

    #[must_use]
    pub(crate) const fn persistence_id(self) -> u8 {
        match self {
            Self::WorldSurface => 0,
            Self::MotionBlocking => 1,
            Self::MotionBlockingNoLeaves => 2,
            Self::OceanFloor => 3,
            Self::WorldSurfaceWg => 4,
            Self::OceanFloorWg => 5,
        }
    }

    #[must_use]
    pub(crate) const fn from_persistence_id(id: u8) -> Option<Self> {
        match id {
            0 => Some(Self::WorldSurface),
            1 => Some(Self::MotionBlocking),
            2 => Some(Self::MotionBlockingNoLeaves),
            3 => Some(Self::OceanFloor),
            4 => Some(Self::WorldSurfaceWg),
            5 => Some(Self::OceanFloorWg),
            _ => None,
        }
    }

    /// Returns whether a block is "opaque" for this heightmap type.
    /// This determines whether the block counts towards the heightmap.
    ///
    /// # Panics
    /// Panics if the block state ID is invalid.
    #[must_use]
    pub fn is_opaque(self, state: BlockStateId) -> bool {
        heightmap_opacity_mask(state, self.mask()) != 0
    }

    /// Checks if a block is in the leaves tag.
    fn is_leaves(block: BlockRef) -> bool {
        block.has_tag(&BlockTag::LEAVES)
    }

    const fn mask(self) -> u8 {
        match self {
            Self::WorldSurface => Self::WORLD_SURFACE_MASK,
            Self::MotionBlocking => Self::MOTION_BLOCKING_MASK,
            Self::MotionBlockingNoLeaves => Self::MOTION_BLOCKING_NO_LEAVES_MASK,
            Self::OceanFloor => Self::OCEAN_FLOOR_MASK,
            Self::WorldSurfaceWg => Self::WORLD_SURFACE_WG_MASK,
            Self::OceanFloorWg => Self::OCEAN_FLOOR_WG_MASK,
        }
    }
}

static WORLD_SURFACE_OPACITY_MASK_BY_STATE: LazyLock<Box<[u8]>> =
    LazyLock::new(build_world_surface_opacity_masks);
static HEIGHTMAP_OPACITY_MASK_BY_STATE: LazyLock<Box<[u8]>> =
    LazyLock::new(build_state_opacity_masks);

fn build_world_surface_opacity_masks() -> Box<[u8]> {
    let mut masks = Vec::with_capacity(REGISTRY.blocks.state_to_block_lookup.len());
    for (state_index, &block) in REGISTRY.blocks.state_to_block_lookup.iter().enumerate() {
        let Ok(_) = u16::try_from(state_index) else {
            panic!("block state registry exceeded BlockStateId range");
        };
        let mask = if block.config.is_air {
            0
        } else {
            HeightmapType::WORLD_SURFACE_MASK | HeightmapType::WORLD_SURFACE_WG_MASK
        };
        masks.push(mask);
    }
    masks.into_boxed_slice()
}

fn build_state_opacity_masks() -> Box<[u8]> {
    let mut masks = Vec::with_capacity(REGISTRY.blocks.state_to_block_lookup.len());
    for (state_index, &block) in REGISTRY.blocks.state_to_block_lookup.iter().enumerate() {
        let Ok(raw_state_id) = u16::try_from(state_index) else {
            panic!("block state registry exceeded BlockStateId range");
        };
        let state = BlockStateId(raw_state_id);
        let mut mask = 0;
        if !block.config.is_air {
            mask |= HeightmapType::WORLD_SURFACE_MASK | HeightmapType::WORLD_SURFACE_WG_MASK;

            let blocks_motion = BlockStateExt::blocks_motion(&state);
            if blocks_motion {
                mask |= HeightmapType::OCEAN_FLOOR_MASK | HeightmapType::OCEAN_FLOOR_WG_MASK;
            }

            if blocks_motion || state.has_fluid() {
                mask |= HeightmapType::MOTION_BLOCKING_MASK;
                if !HeightmapType::is_leaves(block) {
                    mask |= HeightmapType::MOTION_BLOCKING_NO_LEAVES_MASK;
                }
            }
        }
        masks.push(mask);
    }
    masks.into_boxed_slice()
}

#[inline]
fn heightmap_opacity_mask(state: BlockStateId, requested_mask: u8) -> u8 {
    let world_surface_mask =
        HeightmapType::WORLD_SURFACE_MASK | HeightmapType::WORLD_SURFACE_WG_MASK;
    if requested_mask & !world_surface_mask == 0 {
        let Some(&state_mask) = WORLD_SURFACE_OPACITY_MASK_BY_STATE.get(state.0 as usize) else {
            panic!("invalid block state id {}", state.0);
        };
        return state_mask & requested_mask;
    }

    let Some(&state_mask) = HEIGHTMAP_OPACITY_MASK_BY_STATE.get(state.0 as usize) else {
        panic!("invalid block state id {}", state.0);
    };
    state_mask & requested_mask
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

    /// Creates a heightmap from raw height data loaded from disk.
    #[must_use]
    pub const fn from_raw_data(
        map_type: HeightmapType,
        min_y: i32,
        height: i32,
        data: Box<[u16; 256]>,
    ) -> Self {
        Self {
            data,
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

    /// Updates this heightmap for a direct write into a previously-air block.
    ///
    /// Vanilla's noise fill writes sections directly and updates the worldgen
    /// heightmaps beside those writes. There is no downward scan in that path
    /// because blocks are only being added to an empty terrain column.
    pub fn update_for_initial_fill(
        &mut self,
        local_x: usize,
        y: i32,
        local_z: usize,
        state: BlockStateId,
    ) -> bool {
        let first_available = self.get_first_available(local_x, local_z);
        if self.map_type.is_opaque(state) && y >= first_available {
            self.set_height(local_x, local_z, y + 1);
            return true;
        }

        false
    }

    /// Returns a direct reference to the raw height data array.
    ///
    /// Values are stored relative to `min_y`. Used for persistence.
    #[must_use]
    pub fn raw_data(&self) -> &[u16; 256] {
        &self.data
    }

    /// Gets the raw data as a slice of i64 values for network serialization.
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

// ─── ChunkHeightmaps ─────────────────────────────────────────────────────────

/// Heightmap storage retained across every phase of a chunk.
///
/// Stores heightmaps as `Option` fields since they are lazily initialized
/// based on the chunk's generation status. Worldgen types (`WorldSurfaceWg`,
/// `OceanFloorWg`) are used before CARVERS; final types are used after.
#[derive(Debug, Clone)]
pub struct ChunkHeightmaps {
    world_surface_wg: Option<Heightmap>,
    ocean_floor_wg: Option<Heightmap>,
    world_surface: Option<Heightmap>,
    motion_blocking: Option<Heightmap>,
    motion_blocking_no_leaves: Option<Heightmap>,
    ocean_floor: Option<Heightmap>,
}

impl ChunkHeightmaps {
    /// Creates empty heightmap storage with no types initialized.
    #[must_use]
    pub const fn empty() -> Self {
        Self {
            world_surface_wg: None,
            ocean_floor_wg: None,
            world_surface: None,
            motion_blocking: None,
            motion_blocking_no_leaves: None,
            ocean_floor: None,
        }
    }

    /// Creates heightmap storage with every final chunk map initialized.
    #[must_use]
    pub fn new(min_y: i32, height: i32) -> Self {
        Self::with_types(HeightmapType::final_types(), min_y, height)
    }

    /// Creates storage with the requested heightmap types initialized at `min_y`.
    #[must_use]
    pub fn with_types(types: &[HeightmapType], min_y: i32, height: i32) -> Self {
        let mut heightmaps = Self::empty();
        for &heightmap_type in types {
            heightmaps.get_or_insert(heightmap_type, min_y, height);
        }
        heightmaps
    }

    /// Returns a reference to a heightmap by type, if it exists.
    #[must_use]
    pub const fn get(&self, heightmap_type: HeightmapType) -> Option<&Heightmap> {
        match heightmap_type {
            HeightmapType::WorldSurfaceWg => self.world_surface_wg.as_ref(),
            HeightmapType::OceanFloorWg => self.ocean_floor_wg.as_ref(),
            HeightmapType::WorldSurface => self.world_surface.as_ref(),
            HeightmapType::MotionBlocking => self.motion_blocking.as_ref(),
            HeightmapType::MotionBlockingNoLeaves => self.motion_blocking_no_leaves.as_ref(),
            HeightmapType::OceanFloor => self.ocean_floor.as_ref(),
        }
    }

    /// Returns a mutable reference to a heightmap by type, if it exists.
    #[must_use]
    pub const fn get_mut(&mut self, heightmap_type: HeightmapType) -> Option<&mut Heightmap> {
        match heightmap_type {
            HeightmapType::WorldSurfaceWg => self.world_surface_wg.as_mut(),
            HeightmapType::OceanFloorWg => self.ocean_floor_wg.as_mut(),
            HeightmapType::WorldSurface => self.world_surface.as_mut(),
            HeightmapType::MotionBlocking => self.motion_blocking.as_mut(),
            HeightmapType::MotionBlockingNoLeaves => self.motion_blocking_no_leaves.as_mut(),
            HeightmapType::OceanFloor => self.ocean_floor.as_mut(),
        }
    }

    /// Replaces one stored heightmap with a fully built instance.
    pub fn replace(&mut self, heightmap: Heightmap) {
        let heightmap_type = heightmap.heightmap_type();
        match heightmap_type {
            HeightmapType::WorldSurfaceWg => self.world_surface_wg = Some(heightmap),
            HeightmapType::OceanFloorWg => self.ocean_floor_wg = Some(heightmap),
            HeightmapType::WorldSurface => self.world_surface = Some(heightmap),
            HeightmapType::MotionBlocking => self.motion_blocking = Some(heightmap),
            HeightmapType::MotionBlockingNoLeaves => {
                self.motion_blocking_no_leaves = Some(heightmap);
            }
            HeightmapType::OceanFloor => self.ocean_floor = Some(heightmap),
        }
    }

    /// Returns a mutable reference to a heightmap, creating it if it doesn't exist.
    fn get_or_insert(
        &mut self,
        heightmap_type: HeightmapType,
        min_y: i32,
        height: i32,
    ) -> &mut Heightmap {
        let slot = match heightmap_type {
            HeightmapType::WorldSurfaceWg => &mut self.world_surface_wg,
            HeightmapType::OceanFloorWg => &mut self.ocean_floor_wg,
            HeightmapType::WorldSurface => &mut self.world_surface,
            HeightmapType::MotionBlocking => &mut self.motion_blocking,
            HeightmapType::MotionBlockingNoLeaves => &mut self.motion_blocking_no_leaves,
            HeightmapType::OceanFloor => &mut self.ocean_floor,
        };
        slot.get_or_insert_with(|| Heightmap::new(heightmap_type, min_y, height))
    }

    /// Returns a final heightmap required by a full chunk.
    ///
    /// # Panics
    /// Panics if a worldgen type is requested or the final map is missing.
    #[must_use]
    pub fn get_final(&self, heightmap_type: HeightmapType) -> &Heightmap {
        if matches!(
            heightmap_type,
            HeightmapType::WorldSurfaceWg | HeightmapType::OceanFloorWg
        ) {
            panic!("worldgen heightmap {heightmap_type:?} is not a final chunk heightmap");
        }
        let Some(heightmap) = self.get(heightmap_type) else {
            panic!("full chunk is missing required heightmap {heightmap_type:?}");
        };
        heightmap
    }

    /// Returns a mutable final heightmap required by a full chunk.
    ///
    /// # Panics
    /// Panics if a worldgen type is requested or the final map is missing.
    #[must_use]
    pub fn get_final_mut(&mut self, heightmap_type: HeightmapType) -> &mut Heightmap {
        if matches!(
            heightmap_type,
            HeightmapType::WorldSurfaceWg | HeightmapType::OceanFloorWg
        ) {
            panic!("worldgen heightmap {heightmap_type:?} is not a final chunk heightmap");
        }
        let Some(heightmap) = self.get_mut(heightmap_type) else {
            panic!("full chunk is missing required heightmap {heightmap_type:?}");
        };
        heightmap
    }

    /// Updates every final heightmap after a full-chunk block change.
    ///
    /// # Panics
    /// Panics if any required final heightmap is missing.
    pub fn update_final<F>(
        &mut self,
        local_x: usize,
        y: i32,
        local_z: usize,
        state: BlockStateId,
        get_block: F,
    ) where
        F: Fn(usize, i32, usize) -> BlockStateId + Copy,
    {
        for &heightmap_type in HeightmapType::final_types() {
            self.get_final_mut(heightmap_type)
                .update(local_x, y, local_z, state, get_block);
        }
    }

    fn set_primed_height(
        &mut self,
        heightmap_type: HeightmapType,
        local_x: usize,
        local_z: usize,
        height: i32,
    ) {
        let Some(heightmap) = self.get_mut(heightmap_type) else {
            panic!("heightmap {heightmap_type:?} missing after priming");
        };
        heightmap.set_height(local_x, local_z, height);
    }

    /// Primes missing heightmaps by scanning each section under one read lock.
    pub fn prime_from_sections(
        &mut self,
        types: &[HeightmapType],
        min_y: i32,
        height: i32,
        sections: &[super::section::SectionHolder],
    ) {
        let mut types_to_prime = SmallVec::<[(HeightmapType, u8); 4]>::new();
        let mut pending_mask_base = 0;
        for &hm_type in types {
            if self.get(hm_type).is_none() {
                let mask = hm_type.mask();
                types_to_prime.push((hm_type, mask));
                pending_mask_base |= mask;
            }
        }

        if types_to_prime.is_empty() {
            return;
        }

        for &(hm_type, _) in &types_to_prime {
            self.get_or_insert(hm_type, min_y, height);
        }

        let mut pending_masks = [pending_mask_base; 16 * 16];
        let mut pending_columns = pending_masks.len();

        'sections: for section_idx in (0..sections.len()).rev() {
            let guard = sections[section_idx].read();
            if matches!(
                &guard.states,
                super::paletted_container::BlockPalette::Homogeneous(state) if state.is_air()
            ) {
                continue;
            }

            // Paletted block storage is contiguous in y-z-x order.
            for local_y in (0..16).rev() {
                let y = min_y + (section_idx * 16 + local_y) as i32;
                let layer_start = local_y * 16 * 16;

                for (column_index, pending_mask) in pending_masks.iter_mut().enumerate() {
                    if *pending_mask == 0 {
                        continue;
                    }

                    let state = guard.states.get_at_index(layer_start + column_index);
                    let matched_mask = heightmap_opacity_mask(state, *pending_mask);
                    if matched_mask == 0 {
                        continue;
                    }

                    let x = column_index % 16;
                    let z = column_index / 16;
                    for &(hm_type, mask) in &types_to_prime {
                        if matched_mask & mask != 0 {
                            self.set_primed_height(hm_type, x, z, y + 1);
                        }
                    }
                    *pending_mask &= !matched_mask;
                    if *pending_mask == 0 {
                        pending_columns -= 1;
                    }
                }

                if pending_columns == 0 {
                    break 'sections;
                }
            }
        }
    }
}

impl Default for ChunkHeightmaps {
    fn default() -> Self {
        Self::empty()
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Once;

    use steel_registry::{
        blocks::{block_state_ext::BlockStateExt, properties::BlockStateProperties},
        init_vanilla_registry, vanilla_blocks,
    };

    use crate::behavior::init_behaviors;
    use crate::chunk::section::{ChunkSection, Sections};

    use super::*;

    static INIT_BEHAVIORS: Once = Once::new();

    fn init_test_state() {
        init_vanilla_registry();
        INIT_BEHAVIORS.call_once(init_behaviors);
    }

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

    #[test]
    fn heightmap_predicates_use_blocks_motion_and_fluid_state() {
        init_test_state();

        let water = REGISTRY.blocks.get_default_state_id(&vanilla_blocks::WATER);
        assert!(!HeightmapType::OceanFloorWg.is_opaque(water));
        assert!(HeightmapType::MotionBlocking.is_opaque(water));

        let slab = REGISTRY
            .blocks
            .get_default_state_id(&vanilla_blocks::OAK_SLAB);
        let waterlogged_slab = slab.set_value(&BlockStateProperties::WATERLOGGED, true);
        assert!(waterlogged_slab.has_fluid());
        assert!(HeightmapType::MotionBlocking.is_opaque(waterlogged_slab));

        let cobweb = REGISTRY
            .blocks
            .get_default_state_id(&vanilla_blocks::COBWEB);
        assert!(!HeightmapType::OceanFloorWg.is_opaque(cobweb));
    }

    #[test]
    fn initial_fill_update_tracks_only_matching_blocks() {
        init_test_state();

        let water = REGISTRY.blocks.get_default_state_id(&vanilla_blocks::WATER);
        let stone = REGISTRY.blocks.get_default_state_id(&vanilla_blocks::STONE);

        let mut ocean_floor = Heightmap::new(HeightmapType::OceanFloorWg, 0, 16);
        assert!(!ocean_floor.update_for_initial_fill(0, 12, 0, water));
        assert_eq!(ocean_floor.get_first_available(0, 0), 0);

        assert!(ocean_floor.update_for_initial_fill(0, 5, 0, stone));
        assert_eq!(ocean_floor.get_first_available(0, 0), 6);

        assert!(!ocean_floor.update_for_initial_fill(0, 4, 0, stone));
        assert_eq!(ocean_floor.get_first_available(0, 0), 6);
    }

    #[test]
    fn section_priming_preserves_heightmap_predicates_across_sections() {
        init_test_state();

        let stone = REGISTRY.blocks.get_default_state_id(&vanilla_blocks::STONE);
        let water = REGISTRY.blocks.get_default_state_id(&vanilla_blocks::WATER);
        let leaves = REGISTRY
            .blocks
            .get_default_state_id(&vanilla_blocks::OAK_LEAVES);
        let cobweb = REGISTRY
            .blocks
            .get_default_state_id(&vanilla_blocks::COBWEB);
        let mut lower = ChunkSection::new_empty();
        let mut upper = ChunkSection::new_empty();

        lower.set_block_state(0, 15, 0, stone);
        upper.set_block_state(0, 10, 0, water);
        upper.set_block_state(1, 4, 2, stone);
        upper.set_block_state(1, 12, 2, leaves);
        upper.set_block_state(3, 3, 4, stone);
        upper.set_block_state(3, 14, 4, cobweb);

        let sections = Sections::from_owned(vec![lower, upper].into_boxed_slice());
        let mut heightmaps = ChunkHeightmaps::empty();
        heightmaps.prime_from_sections(
            &[
                HeightmapType::WorldSurface,
                HeightmapType::MotionBlocking,
                HeightmapType::MotionBlockingNoLeaves,
                HeightmapType::OceanFloor,
                HeightmapType::WorldSurfaceWg,
                HeightmapType::OceanFloorWg,
            ],
            -16,
            32,
            &sections.sections,
        );

        let first_available = |heightmap_type, x, z| {
            let Some(heightmap) = heightmaps.get(heightmap_type) else {
                panic!("heightmap {heightmap_type:?} was not primed");
            };
            heightmap.get_first_available(x, z)
        };

        assert_eq!(first_available(HeightmapType::WorldSurface, 0, 0), 11);
        assert_eq!(first_available(HeightmapType::MotionBlocking, 0, 0), 11);
        assert_eq!(
            first_available(HeightmapType::MotionBlockingNoLeaves, 0, 0),
            11
        );
        assert_eq!(first_available(HeightmapType::OceanFloor, 0, 0), 0);
        assert_eq!(first_available(HeightmapType::WorldSurfaceWg, 0, 0), 11);
        assert_eq!(first_available(HeightmapType::OceanFloorWg, 0, 0), 0);

        assert_eq!(first_available(HeightmapType::WorldSurface, 1, 2), 13);
        assert_eq!(first_available(HeightmapType::MotionBlocking, 1, 2), 13);
        assert_eq!(
            first_available(HeightmapType::MotionBlockingNoLeaves, 1, 2),
            5
        );
        assert_eq!(first_available(HeightmapType::OceanFloor, 1, 2), 13);

        assert_eq!(first_available(HeightmapType::WorldSurface, 3, 4), 15);
        assert_eq!(first_available(HeightmapType::MotionBlocking, 3, 4), 4);
        assert_eq!(first_available(HeightmapType::OceanFloor, 3, 4), 4);
        assert_eq!(first_available(HeightmapType::WorldSurface, 15, 15), -16);
    }
}
