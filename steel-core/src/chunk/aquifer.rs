//! Noise-based aquifer system for underground fluid placement.
//!
//! Matches vanilla's `Aquifer.NoiseBasedAquifer`. Divides the world into a
//! 16×12×16 grid of aquifer cells, each with a randomly-jittered center point.
//! For each non-solid block, finds the 4 nearest aquifer centers and computes
//! fluid status (water vs lava, surface level) based on noise functions.
//! Barrier pressure between neighboring aquifer cells creates solid rock
//! walls between fluid pockets.

use steel_registry::{REGISTRY, vanilla_blocks};
use steel_utils::BlockStateId;
use steel_utils::density::{ColumnCache, DimensionNoises, NoiseSettings};
use steel_utils::math::{clamp, map, map_clamped};
use steel_utils::random::name_hash::NameHash;
use steel_utils::random::{PositionalRandom, Random, RandomSplitter};

// Grid spacing
const Y_SPACING: i32 = 12;

// Jitter range per cell center
const X_RANGE: i32 = 10;
const Y_RANGE: i32 = 9;
const Z_RANGE: i32 = 10;

// Anchor offsets for neighborhood lookup
const SAMPLE_OFFSET_X: i32 = -5;
const SAMPLE_OFFSET_Y: i32 = 1;
const SAMPLE_OFFSET_Z: i32 = -5;

const LAVA_LEVEL: i32 = -54;
/// Sentinel for "no fluid" — well below any real Y coordinate.
const WAY_BELOW_MIN_Y: i32 = -32512;

/// Chunk offsets (in chunks, ×16 for blocks) used when sampling
/// preliminary surface levels around an aquifer cell center.
const SURFACE_SAMPLING_OFFSETS: [[i32; 2]; 13] = [
    [0, 0],
    [-2, -1],
    [-1, -1],
    [0, -1],
    [1, -1],
    [-3, 0],
    [-2, 0],
    [-1, 0],
    [1, 0],
    [-2, 1],
    [-1, 1],
    [0, 1],
    [1, 1],
];

/// Fluid status at an aquifer cell center.
///
/// Matches vanilla's `Aquifer.FluidStatus` — stores the actual fluid block state
/// rather than a boolean flag, so the aquifer is agnostic to which fluids exist.
#[derive(Clone, Copy, PartialEq, Eq)]
struct FluidStatus {
    /// Y level of the fluid surface (exclusive upper bound).
    fluid_level: i32,
    /// Block state placed below `fluid_level`.
    fluid_type: BlockStateId,
}

impl FluidStatus {
    /// What block is at `block_y`? Returns the fluid type if below the surface,
    /// or `None` for air above the surface.
    const fn at(self, block_y: i32) -> Option<BlockStateId> {
        if block_y < self.fluid_level {
            Some(self.fluid_type)
        } else {
            None
        }
    }
}

/// Result of the aquifer substance check.
pub enum AquiferResult {
    /// Solid block (density > 0 or barrier makes it solid).
    Solid,
    /// Air (no block placed).
    Air,
    /// Fluid block to place.
    Fluid(BlockStateId),
}

/// Noise-based aquifer for a single chunk.
///
/// Constructed once per chunk, used throughout the fill loop.
pub struct Aquifer<N: DimensionNoises> {
    /// Packed (x, y, z) locations of aquifer cell centers.
    /// `i64::MAX` = not yet computed.
    location_cache: Vec<i64>,
    /// Lazily computed fluid statuses per grid cell.
    status_cache: Vec<Option<FluidStatus>>,
    /// Positional random for grid cell jitter.
    splitter: RandomSplitter,
    /// Column cache owned by the aquifer for density function evaluation.
    cache: N::ColumnCache,
    /// Grid bounds.
    min_grid_x: i32,
    min_grid_y: i32,
    min_grid_z: i32,
    grid_size_x: i32,
    grid_size_z: i32,
    /// Skip aquifer sampling above this Y (optimization).
    skip_sampling_above_y: i32,
    /// Sea level for this dimension.
    sea_level: i32,
    /// Block state IDs.
    water_id: BlockStateId,
    lava_id: BlockStateId,
    /// The dimension's default fluid (water for overworld, lava for nether).
    default_fluid_id: BlockStateId,
}

// Grid coordinate conversions
#[inline]
const fn grid_x(block: i32) -> i32 {
    block >> 4
}
#[inline]
const fn grid_z(block: i32) -> i32 {
    block >> 4
}
#[inline]
const fn grid_y(block: i32) -> i32 {
    block.div_euclid(Y_SPACING)
}
#[inline]
const fn from_grid_x(grid: i32, offset: i32) -> i32 {
    (grid << 4) + offset
}
#[inline]
const fn from_grid_y(grid: i32, offset: i32) -> i32 {
    grid * Y_SPACING + offset
}
#[inline]
const fn from_grid_z(grid: i32, offset: i32) -> i32 {
    (grid << 4) + offset
}

// BlockPos packing (matches vanilla's BlockPos.asLong / getX / getY / getZ)
const PACKED_X_MASK: i64 = 0x3FF_FFFF; // 26 bits
const PACKED_Y_MASK: i64 = 0xFFF; // 12 bits
const PACKED_Z_MASK: i64 = 0x3FF_FFFF; // 26 bits
const X_OFFSET: i32 = 38;
const Z_OFFSET: i32 = 12;

#[inline]
fn pack_pos(x: i32, y: i32, z: i32) -> i64 {
    ((i64::from(x) & PACKED_X_MASK) << X_OFFSET)
        | (i64::from(y) & PACKED_Y_MASK)
        | ((i64::from(z) & PACKED_Z_MASK) << Z_OFFSET)
}

#[inline]
const fn unpack_x(packed: i64) -> i32 {
    (packed >> X_OFFSET) as i32
}

#[inline]
const fn unpack_y(packed: i64) -> i32 {
    ((packed << 52) >> 52) as i32
}

#[inline]
const fn unpack_z(packed: i64) -> i32 {
    ((packed << 26) >> X_OFFSET) as i32
}

/// Similarity between two squared distances. Positive when the two nearest
/// aquifer cells are close together (near a boundary).
#[inline]
fn similarity(dist_sq1: i32, dist_sq2: i32) -> f64 {
    1.0 - f64::from(dist_sq2 - dist_sq1) / 25.0
}

/// Deep dark region check matching `OverworldBiomeBuilder.isDeepDarkRegion`.
fn is_deep_dark_region<N: DimensionNoises>(
    noises: &N,
    cache: &mut N::ColumnCache,
    x: i32,
    y: i32,
    z: i32,
) -> bool {
    cache.ensure(x, z, noises);
    let erosion = noises.router_erosion(cache, x, y, z);
    let depth = noises.router_depth(cache, x, y, z);
    erosion < -0.225 && depth > 0.9
}

/// Global fluid picker matching vanilla's `NoiseBasedChunkGenerator.createFluidPicker`.
///
/// Below `min(-54, sea_level)` → lava at Y=-54. Otherwise → the dimension's
/// default fluid at sea level (water for overworld, lava for nether).
fn global_fluid(
    y: i32,
    sea_level: i32,
    lava_id: BlockStateId,
    default_fluid_id: BlockStateId,
) -> FluidStatus {
    if y < LAVA_LEVEL.min(sea_level) {
        FluidStatus {
            fluid_level: LAVA_LEVEL,
            fluid_type: lava_id,
        }
    } else {
        FluidStatus {
            fluid_level: sea_level,
            fluid_type: default_fluid_id,
        }
    }
}

impl<N: DimensionNoises> Aquifer<N> {
    /// Create a new aquifer for a chunk.
    ///
    /// `chunk_min_x/z` are the block coordinates of the chunk's NW corner.
    /// `min_block_y` and `y_block_size` define the vertical range.
    /// `splitter` is the seed's positional splitter.
    /// `cache` should be a pre-initialized column cache for this chunk
    /// (avoids a redundant `init_grid` call).
    #[must_use]
    pub fn new(
        chunk_min_x: i32,
        chunk_min_z: i32,
        min_block_y: i32,
        y_block_size: i32,
        splitter: &RandomSplitter,
        noises: &N,
        mut cache: N::ColumnCache,
    ) -> Self {
        const AQUIFER_HASH: NameHash = NameHash::new("minecraft:aquifer");

        let sea_level = N::Settings::SEA_LEVEL;
        let water_id = REGISTRY.blocks.get_default_state_id(vanilla_blocks::WATER);
        let lava_id = REGISTRY.blocks.get_default_state_id(vanilla_blocks::LAVA);
        let default_fluid_id = N::Settings::default_fluid_id();

        let mut aquifer_rng = splitter.with_hash_of(&AQUIFER_HASH);
        let splitter = aquifer_rng.next_positional();

        // When aquifers are disabled (nether/end), compute_substance uses only
        // the global fluid picker — skip grid allocation and surface sampling.
        if !N::Settings::AQUIFERS_ENABLED {
            return Self {
                location_cache: Vec::new(),
                status_cache: Vec::new(),
                splitter,
                cache,
                min_grid_x: 0,
                min_grid_y: 0,
                min_grid_z: 0,
                grid_size_x: 0,
                grid_size_z: 0,
                skip_sampling_above_y: 0,
                sea_level,
                water_id,
                lava_id,
                default_fluid_id,
            };
        }

        let chunk_max_x = chunk_min_x + 15;
        let chunk_max_z = chunk_min_z + 15;

        let min_grid_x = grid_x(chunk_min_x + SAMPLE_OFFSET_X);
        let max_grid_x = grid_x(chunk_max_x + SAMPLE_OFFSET_X) + 1;
        let grid_size_x = max_grid_x - min_grid_x + 1;

        let min_grid_y = grid_y(min_block_y + SAMPLE_OFFSET_Y) - 1;
        let max_grid_y = grid_y(min_block_y + y_block_size + SAMPLE_OFFSET_Y) + 1;
        let grid_size_y = max_grid_y - min_grid_y + 1;

        let min_grid_z = grid_z(chunk_min_z + SAMPLE_OFFSET_Z);
        let max_grid_z = grid_z(chunk_max_z + SAMPLE_OFFSET_Z) + 1;
        let grid_size_z = max_grid_z - min_grid_z + 1;

        let total = (grid_size_x * grid_size_y * grid_size_z) as usize;
        let location_cache = vec![i64::MAX; total];
        let status_cache = vec![None; total];

        // Compute skip_sampling_above_y from max preliminary surface level
        let max_surface = Self::max_preliminary_surface_level(
            noises,
            &mut cache,
            from_grid_x(min_grid_x, 0),
            from_grid_z(min_grid_z, 0),
            from_grid_x(max_grid_x, X_RANGE - 1),
            from_grid_z(max_grid_z, Z_RANGE - 1),
        );
        let adjusted = max_surface + 8;
        let skip_grid_y = grid_y(adjusted + 12) + 1;
        let skip_sampling_above_y = from_grid_y(skip_grid_y, Y_RANGE + 2) - 1;

        Self {
            location_cache,
            status_cache,
            splitter,
            cache,
            min_grid_x,
            min_grid_y,
            min_grid_z,
            grid_size_x,
            grid_size_z,
            skip_sampling_above_y,
            sea_level,
            water_id,
            lava_id,
            default_fluid_id,
        }
    }

    fn max_preliminary_surface_level(
        noises: &N,
        cache: &mut N::ColumnCache,
        min_x: i32,
        min_z: i32,
        max_x: i32,
        max_z: i32,
    ) -> i32 {
        let mut max_level = i32::MIN;
        // Sample at 4-block intervals (quart-block resolution) across the chunk area
        let mut z = min_z;
        while z <= max_z {
            let mut x = min_x;
            while x <= max_x {
                let level = preliminary_surface_level(noises, cache, x, z);
                if level > max_level {
                    max_level = level;
                }
                x += 4;
            }
            z += 4;
        }
        max_level
    }

    #[inline]
    const fn get_index(&self, gx: i32, gy: i32, gz: i32) -> usize {
        let x = gx - self.min_grid_x;
        let y = gy - self.min_grid_y;
        let z = gz - self.min_grid_z;
        ((y * self.grid_size_z + z) * self.grid_size_x + x) as usize
    }

    /// Compute what block to place at this position given the interpolated density.
    #[allow(clippy::too_many_lines)]
    pub fn compute_substance(
        &mut self,
        noises: &N,
        world_x: i32,
        world_y: i32,
        world_z: i32,
        density: f64,
    ) -> AquiferResult {
        // Solid block — let the caller decide (stone or ore)
        if density > 0.0 {
            return AquiferResult::Solid;
        }

        // Disabled aquifers (nether/end): use global fluid picker directly,
        // matching vanilla's `Aquifer.createDisabled`.
        if !N::Settings::AQUIFERS_ENABLED {
            let gf = global_fluid(world_y, self.sea_level, self.lava_id, self.default_fluid_id);
            return match gf.at(world_y) {
                Some(id) => AquiferResult::Fluid(id),
                None => AquiferResult::Air,
            };
        }

        let gf = global_fluid(world_y, self.sea_level, self.lava_id, self.default_fluid_id);

        // Above the skip threshold: use global fluid directly
        if world_y > self.skip_sampling_above_y {
            return match gf.at(world_y) {
                Some(id) => AquiferResult::Fluid(id),
                None => AquiferResult::Air,
            };
        }

        // If global fluid is lava here, return lava
        if gf.fluid_type == self.lava_id && world_y < gf.fluid_level {
            return AquiferResult::Fluid(self.lava_id);
        }

        // Find 4 nearest aquifer cell centers from 2×3×2 neighborhood
        let x_anchor = grid_x(world_x + SAMPLE_OFFSET_X);
        let y_anchor = grid_y(world_y + SAMPLE_OFFSET_Y);
        let z_anchor = grid_z(world_z + SAMPLE_OFFSET_Z);

        let mut dist_sq = [i32::MAX; 4];
        let mut closest_idx = [0usize; 4];

        for x1 in 0..=1 {
            for y1 in -1..=1 {
                for z1 in 0..=1 {
                    let gx = x_anchor + x1;
                    let gy = y_anchor + y1;
                    let gz = z_anchor + z1;
                    let index = self.get_index(gx, gy, gz);

                    // Get or compute cell center location
                    let loc = self.location_cache[index];
                    let loc = if loc == i64::MAX {
                        let mut rng = self.splitter.at(gx, gy, gz);
                        let packed = pack_pos(
                            from_grid_x(gx, rng.next_i32_bounded(X_RANGE)),
                            from_grid_y(gy, rng.next_i32_bounded(Y_RANGE)),
                            from_grid_z(gz, rng.next_i32_bounded(Z_RANGE)),
                        );
                        self.location_cache[index] = packed;
                        packed
                    } else {
                        loc
                    };

                    let dx = unpack_x(loc) - world_x;
                    let dy = unpack_y(loc) - world_y;
                    let dz = unpack_z(loc) - world_z;
                    let new_dist = dx * dx + dy * dy + dz * dz;

                    // Insert into sorted top-4
                    if dist_sq[0] >= new_dist {
                        dist_sq[3] = dist_sq[2];
                        closest_idx[3] = closest_idx[2];
                        dist_sq[2] = dist_sq[1];
                        closest_idx[2] = closest_idx[1];
                        dist_sq[1] = dist_sq[0];
                        closest_idx[1] = closest_idx[0];
                        dist_sq[0] = new_dist;
                        closest_idx[0] = index;
                    } else if dist_sq[1] >= new_dist {
                        dist_sq[3] = dist_sq[2];
                        closest_idx[3] = closest_idx[2];
                        dist_sq[2] = dist_sq[1];
                        closest_idx[2] = closest_idx[1];
                        dist_sq[1] = new_dist;
                        closest_idx[1] = index;
                    } else if dist_sq[2] >= new_dist {
                        dist_sq[3] = dist_sq[2];
                        closest_idx[3] = closest_idx[2];
                        dist_sq[2] = new_dist;
                        closest_idx[2] = index;
                    } else if dist_sq[3] >= new_dist {
                        dist_sq[3] = new_dist;
                        closest_idx[3] = index;
                    }
                }
            }
        }

        let status1 = self.get_aquifer_status(closest_idx[0], noises);
        let sim12 = similarity(dist_sq[0], dist_sq[1]);

        let fluid_at = status1.at(world_y);

        if sim12 <= 0.0 {
            // Not near a boundary — return closest fluid
            return match fluid_at {
                Some(id) => AquiferResult::Fluid(id),
                None => AquiferResult::Air,
            };
        }

        // Water adjacent to global lava below → return water
        if let Some(id) = fluid_at
            && id == self.water_id
        {
            let below = global_fluid(
                world_y - 1,
                self.sea_level,
                self.lava_id,
                self.default_fluid_id,
            );
            if below.fluid_type == self.lava_id && (world_y - 1) < below.fluid_level {
                return AquiferResult::Fluid(id);
            }
        }

        // Compute barrier pressure between closest pairs
        let mut barrier_noise = f64::NAN;
        let status2 = self.get_aquifer_status(closest_idx[1], noises);
        let barrier12 = sim12
            * self.calculate_pressure(
                noises,
                world_x,
                world_y,
                world_z,
                &mut barrier_noise,
                status1,
                status2,
            );
        if density + barrier12 > 0.0 {
            return AquiferResult::Solid;
        }

        let status3 = self.get_aquifer_status(closest_idx[2], noises);
        let sim13 = similarity(dist_sq[0], dist_sq[2]);
        if sim13 > 0.0 {
            let barrier13 = sim12
                * sim13
                * self.calculate_pressure(
                    noises,
                    world_x,
                    world_y,
                    world_z,
                    &mut barrier_noise,
                    status1,
                    status3,
                );
            if density + barrier13 > 0.0 {
                return AquiferResult::Solid;
            }
        }

        let sim23 = similarity(dist_sq[1], dist_sq[2]);
        if sim23 > 0.0 {
            let barrier23 = sim12
                * sim23
                * self.calculate_pressure(
                    noises,
                    world_x,
                    world_y,
                    world_z,
                    &mut barrier_noise,
                    status2,
                    status3,
                );
            if density + barrier23 > 0.0 {
                return AquiferResult::Solid;
            }
        }

        // Return the closest fluid
        match fluid_at {
            Some(id) => AquiferResult::Fluid(id),
            None => AquiferResult::Air,
        }
    }

    /// Get or compute the fluid status for the aquifer cell at the given cache index.
    fn get_aquifer_status(&mut self, index: usize, noises: &N) -> FluidStatus {
        if let Some(status) = self.status_cache[index] {
            return status;
        }

        let loc = self.location_cache[index];
        let x = unpack_x(loc);
        let y = unpack_y(loc);
        let z = unpack_z(loc);
        let status = self.compute_fluid(x, y, z, noises);
        self.status_cache[index] = Some(status);
        status
    }

    /// Compute the fluid status for an aquifer cell centered at (x, y, z).
    fn compute_fluid(&mut self, x: i32, y: i32, z: i32, noises: &N) -> FluidStatus {
        let gf = global_fluid(y, self.sea_level, self.lava_id, self.default_fluid_id);
        let mut lowest_surface = i32::MAX;
        let top_of_cell = y + Y_SPACING;
        let bottom_of_cell = y - Y_SPACING;
        let mut surface_under_global = false;

        for offset in &SURFACE_SAMPLING_OFFSETS {
            let sx = x + offset[0] * 16; // sectionToBlockCoord
            let sz = z + offset[1] * 16;

            let preliminary = preliminary_surface_level(noises, &mut self.cache, sx, sz);
            let adjusted = preliminary + 8;

            let is_center = offset[0] == 0 && offset[1] == 0;

            if is_center && bottom_of_cell > adjusted {
                return gf;
            }

            let top_pokes_above = top_of_cell > adjusted;
            if top_pokes_above || is_center {
                let gf_at_surface = global_fluid(
                    adjusted,
                    self.sea_level,
                    self.lava_id,
                    self.default_fluid_id,
                );
                let has_fluid = adjusted < gf_at_surface.fluid_level;
                if has_fluid {
                    if is_center {
                        surface_under_global = true;
                    }
                    if top_pokes_above {
                        return gf_at_surface;
                    }
                }
            }

            if preliminary < lowest_surface {
                lowest_surface = preliminary;
            }
        }

        let fluid_level =
            self.compute_surface_level(x, y, z, noises, gf, lowest_surface, surface_under_global);
        let fluid_type = self.compute_fluid_type(x, y, z, noises, gf, fluid_level);
        FluidStatus {
            fluid_level,
            fluid_type,
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn compute_surface_level(
        &mut self,
        x: i32,
        y: i32,
        z: i32,
        noises: &N,
        gf: FluidStatus,
        lowest_surface: i32,
        surface_under_global: bool,
    ) -> i32 {
        let (partially_flooded, fully_flooded) =
            if is_deep_dark_region(noises, &mut self.cache, x, y, z) {
                (-1.0, -1.0)
            } else {
                let dist_below = lowest_surface + 8 - y;
                let floodedness_factor = if surface_under_global {
                    map_clamped(f64::from(dist_below), 0.0, 64.0, 1.0, 0.0)
                } else {
                    0.0
                };

                self.cache.ensure(x, z, noises);
                let floodedness_noise = clamp(
                    noises.router_fluid_level_floodedness(&mut self.cache, x, y, z),
                    -1.0,
                    1.0,
                );

                let fully_threshold = map(floodedness_factor, 1.0, 0.0, -0.3, 0.8);
                let partially_threshold = map(floodedness_factor, 1.0, 0.0, -0.8, 0.4);

                (
                    floodedness_noise - partially_threshold,
                    floodedness_noise - fully_threshold,
                )
            };

        if fully_flooded > 0.0 {
            gf.fluid_level
        } else if partially_flooded > 0.0 {
            self.compute_randomized_fluid_surface_level(x, y, z, noises, lowest_surface)
        } else {
            WAY_BELOW_MIN_Y
        }
    }

    fn compute_randomized_fluid_surface_level(
        &mut self,
        x: i32,
        y: i32,
        z: i32,
        noises: &N,
        lowest_surface: i32,
    ) -> i32 {
        let cell_x = x.div_euclid(16);
        let cell_y = y.div_euclid(40);
        let cell_z = z.div_euclid(16);
        let cell_middle_y = cell_y * 40 + 20;

        // fluid_level_spread is evaluated at grid coordinates (not block coordinates)
        self.cache.ensure(cell_x, cell_z, noises);
        let spread =
            noises.router_fluid_level_spread(&mut self.cache, cell_x, cell_y, cell_z) * 10.0;
        let spread_quantized = quantize(spread, 3);
        let target = cell_middle_y + spread_quantized;

        lowest_surface.min(target)
    }

    fn compute_fluid_type(
        &mut self,
        x: i32,
        y: i32,
        z: i32,
        noises: &N,
        gf: FluidStatus,
        fluid_level: i32,
    ) -> BlockStateId {
        if fluid_level <= -10 && fluid_level != WAY_BELOW_MIN_Y && gf.fluid_type != self.lava_id {
            let cell_x = x.div_euclid(64);
            let cell_y = y.div_euclid(40);
            let cell_z = z.div_euclid(64);
            self.cache.ensure(cell_x, cell_z, noises);
            let lava_noise = noises.router_lava(&mut self.cache, cell_x, cell_y, cell_z);
            if lava_noise.abs() > 0.3 {
                return self.lava_id;
            }
        }
        gf.fluid_type
    }

    /// Calculate barrier pressure between two aquifer cells.
    ///
    /// Matches vanilla's check: if lava meets water at this Y, return max pressure.
    #[allow(clippy::too_many_arguments)]
    fn calculate_pressure(
        &mut self,
        noises: &N,
        x: i32,
        y: i32,
        z: i32,
        barrier_noise: &mut f64,
        s1: FluidStatus,
        s2: FluidStatus,
    ) -> f64 {
        let f1 = s1.at(y);
        let f2 = s2.at(y);
        let f1_is_lava = f1 == Some(self.lava_id);
        let f2_is_lava = f2 == Some(self.lava_id);
        let f1_is_water = f1 == Some(self.water_id);
        let f2_is_water = f2 == Some(self.water_id);

        // Lava–water interface → max pressure
        if (f1_is_lava && f2_is_water) || (f1_is_water && f2_is_lava) {
            return 2.0;
        }

        let fluid_y_diff = (s1.fluid_level - s2.fluid_level).abs();
        if fluid_y_diff == 0 {
            return 0.0;
        }

        let avg_fluid_y = 0.5 * f64::from(s1.fluid_level + s2.fluid_level);
        let above_avg = f64::from(y) + 0.5 - avg_fluid_y;
        let base = f64::from(fluid_y_diff) / 2.0;
        let edge_dist = base - above_avg.abs();

        let gradient = if above_avg > 0.0 {
            if edge_dist > 0.0 {
                edge_dist / 1.5
            } else {
                edge_dist / 2.5
            }
        } else {
            let center = 3.0 + edge_dist;
            if center > 0.0 {
                center / 3.0
            } else {
                center / 10.0
            }
        };

        let noise_val = if !(-2.0..=2.0).contains(&gradient) {
            0.0
        } else if barrier_noise.is_nan() {
            self.cache.ensure(x, z, noises);
            let n = noises.router_barrier(&mut self.cache, x, y, z);
            *barrier_noise = n;
            n
        } else {
            *barrier_noise
        };

        2.0 * (noise_val + gradient)
    }
}

/// Quantize: snap value down to the nearest multiple of `quantum`.
#[inline]
fn quantize(value: f64, quantum: i32) -> i32 {
    let q = f64::from(quantum);
    (value / q).floor() as i32 * quantum
}

/// Evaluate preliminary surface level at quart-quantized coordinates.
///
/// Vanilla's `NoiseChunk.preliminarySurfaceLevel()` quantizes X/Z to quart
/// positions before lookup, matching `FlatCache`'s 4-block grid.
pub(crate) fn preliminary_surface_level<N: DimensionNoises>(
    noises: &N,
    cache: &mut N::ColumnCache,
    x: i32,
    z: i32,
) -> i32 {
    // Quantize to quart positions: (x >> 2) << 2
    let qx = (x >> 2) << 2;
    let qz = (z >> 2) << 2;
    cache.ensure(qx, qz, noises);
    // Vanilla uses Mth.floor(), not truncation
    noises
        .router_preliminary_surface_level(cache, qx, 0, qz)
        .floor() as i32
}
