//! Heightmap implementation for tracking the highest blocks in a chunk.
//!
//! Heightmaps are used for various purposes like spawning, pathfinding, and rendering.
//!
//! During worldgen, `ProtoHeightmaps` stores a dynamic set of heightmaps (worldgen types
//! before CARVERS, final types after). When a proto chunk is promoted to a full `LevelChunk`,
//! the final heightmaps are moved directly into `ChunkHeightmaps` via [`ChunkHeightmaps::from_proto`].

use std::sync::LazyLock;

use smallvec::SmallVec;
use steel_registry::{
    REGISTRY,
    blocks::{BlockRef, block_state_ext::BlockStateExt},
    vanilla_block_tags::BlockTag,
};
use steel_utils::BlockStateId;

use crate::behavior::BlockStateBehaviorExt as _;

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

    /// Sets the raw data from a slice of i64 values (network format).
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

// ─── ProtoHeightmaps ─────────────────────────────────────────────────────────

/// Heightmap storage for proto chunks during worldgen.
///
/// Stores heightmaps as `Option` fields since they are lazily initialized
/// based on the chunk's generation status. Worldgen types (`WorldSurfaceWg`,
/// `OceanFloorWg`) are used before CARVERS; final types are used after.
#[derive(Debug, Clone)]
pub struct ProtoHeightmaps {
    world_surface_wg: Option<Heightmap>,
    ocean_floor_wg: Option<Heightmap>,
    world_surface: Option<Heightmap>,
    motion_blocking: Option<Heightmap>,
    motion_blocking_no_leaves: Option<Heightmap>,
    ocean_floor: Option<Heightmap>,
}

impl ProtoHeightmaps {
    /// Creates empty proto heightmaps with no types initialized.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            world_surface_wg: None,
            ocean_floor_wg: None,
            world_surface: None,
            motion_blocking: None,
            motion_blocking_no_leaves: None,
            ocean_floor: None,
        }
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

    /// Takes a heightmap by type, leaving `None` in its place.
    /// Used during proto→full conversion to move heightmaps by value.
    pub const fn take(&mut self, heightmap_type: HeightmapType) -> Option<Heightmap> {
        match heightmap_type {
            HeightmapType::WorldSurfaceWg => self.world_surface_wg.take(),
            HeightmapType::OceanFloorWg => self.ocean_floor_wg.take(),
            HeightmapType::WorldSurface => self.world_surface.take(),
            HeightmapType::MotionBlocking => self.motion_blocking.take(),
            HeightmapType::MotionBlockingNoLeaves => self.motion_blocking_no_leaves.take(),
            HeightmapType::OceanFloor => self.ocean_floor.take(),
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

    /// Primes missing heightmaps by reading sections directly with batched locking.
    ///
    /// Instead of a per-block closure (which acquires a lock per call), this
    /// holds each section's read lock for all 16 Y values before moving on.
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

        for x in 0..16 {
            for z in 0..16 {
                let mut pending_mask = pending_mask_base;

                'sections: for section_idx in (0..sections.len()).rev() {
                    let guard = sections[section_idx].read();
                    for local_y in (0..16).rev() {
                        if pending_mask == 0 {
                            break 'sections;
                        }
                        let y = min_y + (section_idx * 16 + local_y) as i32;
                        let state = guard.states.get(x, local_y, z);
                        let matched_mask = heightmap_opacity_mask(state, pending_mask);
                        if matched_mask == 0 {
                            continue;
                        }
                        for &(hm_type, mask) in &types_to_prime {
                            if matched_mask & mask != 0 {
                                self.set_primed_height(hm_type, x, z, y + 1);
                            }
                        }
                        pending_mask &= !matched_mask;
                    }
                }
            }
        }
    }

    /// Primes missing heightmaps by scanning chunk columns from top to bottom.
    ///
    /// Only creates and primes heightmap types that don't already exist.
    /// For each column, scans downward and records the first opaque block
    /// for each heightmap type's predicate.
    pub fn prime<F>(&mut self, types: &[HeightmapType], min_y: i32, height: i32, get_block: F)
    where
        F: Fn(usize, i32, usize) -> BlockStateId,
    {
        // Collect types that need priming (don't exist yet)
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

        // Create missing heightmaps
        for &(hm_type, _) in &types_to_prime {
            self.get_or_insert(hm_type, min_y, height);
        }

        let max_y = min_y + height;

        // For each column, scan from top to bottom
        for x in 0..16 {
            for z in 0..16 {
                // Track which heightmaps still need to find their first opaque block
                let mut pending_mask = pending_mask_base;

                for y in (min_y..max_y).rev() {
                    if pending_mask == 0 {
                        break;
                    }

                    let state = get_block(x, y, z);
                    let matched_mask = heightmap_opacity_mask(state, pending_mask);
                    if matched_mask == 0 {
                        continue;
                    }
                    for &(hm_type, mask) in &types_to_prime {
                        if matched_mask & mask != 0 {
                            self.set_primed_height(hm_type, x, z, y + 1);
                        }
                    }
                    pending_mask &= !matched_mask;
                }
            }
        }
    }
}

impl Default for ProtoHeightmaps {
    fn default() -> Self {
        Self::new()
    }
}

// ─── ChunkHeightmaps ─────────────────────────────────────────────────────────

/// A collection of final heightmaps for a fully generated chunk.
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
    /// Creates a new set of heightmaps for a chunk (all heights at `min_y`).
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

    /// Creates chunk heightmaps by taking final heightmaps from proto heightmaps.
    ///
    /// Moves each final heightmap directly from the proto storage. Callers should
    /// prime missing final heightmaps before conversion; the fallback only handles
    /// malformed loaded data defensively.
    #[must_use]
    pub fn from_proto(proto: &mut ProtoHeightmaps, min_y: i32, height: i32) -> Self {
        Self {
            world_surface: proto
                .take(HeightmapType::WorldSurface)
                .unwrap_or_else(|| Heightmap::new(HeightmapType::WorldSurface, min_y, height)),
            motion_blocking: proto
                .take(HeightmapType::MotionBlocking)
                .unwrap_or_else(|| Heightmap::new(HeightmapType::MotionBlocking, min_y, height)),
            motion_blocking_no_leaves: proto
                .take(HeightmapType::MotionBlockingNoLeaves)
                .unwrap_or_else(|| {
                    Heightmap::new(HeightmapType::MotionBlockingNoLeaves, min_y, height)
                }),
            ocean_floor: proto
                .take(HeightmapType::OceanFloor)
                .unwrap_or_else(|| Heightmap::new(HeightmapType::OceanFloor, min_y, height)),
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
    use std::sync::Once;

    use steel_registry::{
        blocks::{block_state_ext::BlockStateExt, properties::BlockStateProperties},
        test_support::init_test_registry,
        vanilla_blocks,
    };

    use crate::behavior::init_behaviors;

    use super::*;

    static INIT_BEHAVIORS: Once = Once::new();

    fn init_test_state() {
        init_test_registry();
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
}
