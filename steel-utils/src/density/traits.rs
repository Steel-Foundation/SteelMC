//! Traits for dimension-specific noise generation.
//!
//! These traits abstract over dimension-specific types (overworld, nether, etc.)
//! allowing generic chunk generation code to work with any dimension's transpiled
//! density functions.

use crate::BlockStateId;
use crate::random::RandomSplitter;
use rustc_hash::FxHashMap;

use super::NoiseParameters;

/// Noise settings for a dimension, parsed from the datapack.
///
/// These are compile-time constants generated from `noise_settings` JSON files.
pub trait NoiseSettings: Send + Sync {
    /// Minimum Y coordinate for this dimension.
    const MIN_Y: i32;
    /// Total height of the world in blocks.
    const HEIGHT: i32;
    /// Sea level Y coordinate.
    const SEA_LEVEL: i32;
    /// Cell width in blocks (XZ direction).
    const CELL_WIDTH: i32;
    /// Cell height in blocks (Y direction).
    const CELL_HEIGHT: i32;
    /// Whether aquifers are enabled for this dimension.
    const AQUIFERS_ENABLED: bool;
    /// Whether ore veins are enabled for this dimension.
    const ORE_VEINS_ENABLED: bool;

    /// Get the default block state ID for this dimension.
    fn default_block_id() -> BlockStateId;

    /// Get the default fluid state ID for this dimension.
    fn default_fluid_id() -> BlockStateId;
}

/// Column cache for a dimension's flat-cached density function results.
///
/// Stores Y-independent values that only need to be computed once per (x, z) column.
pub trait ColumnCache: Clone + Default + Send + Sync {
    /// The associated noises type for this cache.
    type Noises: DimensionNoises<ColumnCache = Self>;

    /// Ensure the cache is populated for the given block coordinates.
    ///
    /// If the cache already holds values for this column, this is a no-op.
    fn ensure(&mut self, x: i32, z: i32, noises: &Self::Noises);

    /// Pre-compute flat-cached values for all quart positions in a chunk.
    ///
    /// Matches vanilla's `NoiseChunk.FlatCache`: eagerly fills a 2D grid of
    /// `(quart_size+1)²` entries (size baked in per dimension at compile time).
    /// After this call, `ensure()` for in-bounds positions is an O(1) grid
    /// lookup. Out-of-bounds positions fall back to on-the-fly evaluation at
    /// raw (non-quantized) coordinates.
    fn init_grid(&mut self, chunk_block_x: i32, chunk_block_z: i32, noises: &Self::Noises);
}

/// All noise generators and density functions for a dimension.
///
/// This trait abstracts over dimension-specific noise types (`OverworldNoises`,
/// `NetherNoises`, etc.) allowing generic code to work with any dimension.
pub trait DimensionNoises: Sized + Send + Sync {
    /// The column cache type for this dimension.
    type ColumnCache: ColumnCache<Noises = Self>;
    /// The noise settings type for this dimension.
    type Settings: NoiseSettings;

    /// Create all noise generators from a world seed and its positional splitter.
    fn create(
        seed: u64,
        splitter: &RandomSplitter,
        params: &FxHashMap<String, NoiseParameters>,
    ) -> Self;

    // ── Router functions ────────────────────────────────────────────────────

    /// Final density for terrain generation (positive = solid, negative = air).
    fn router_final_density(&self, cache: &mut Self::ColumnCache, x: i32, y: i32, z: i32) -> f64;

    /// Depth from surface (used for terrain shaping).
    fn router_depth(&self, cache: &mut Self::ColumnCache, x: i32, y: i32, z: i32) -> f64;

    // ── Aquifer router functions ────────────────────────────────────────────

    /// Barrier noise for aquifer boundaries.
    fn router_barrier(&self, cache: &mut Self::ColumnCache, x: i32, y: i32, z: i32) -> f64;

    /// Fluid level floodedness for aquifers.
    fn router_fluid_level_floodedness(
        &self,
        cache: &mut Self::ColumnCache,
        x: i32,
        y: i32,
        z: i32,
    ) -> f64;

    /// Fluid level spread for aquifers.
    fn router_fluid_level_spread(
        &self,
        cache: &mut Self::ColumnCache,
        x: i32,
        y: i32,
        z: i32,
    ) -> f64;

    /// Lava placement noise for aquifers.
    fn router_lava(&self, cache: &mut Self::ColumnCache, x: i32, y: i32, z: i32) -> f64;

    // ── Ore vein router functions ───────────────────────────────────────────

    /// Vein toggle (sign determines copper vs iron).
    fn router_vein_toggle(&self, cache: &mut Self::ColumnCache, x: i32, y: i32, z: i32) -> f64;

    /// Vein ridged noise for ore placement.
    fn router_vein_ridged(&self, cache: &mut Self::ColumnCache, x: i32, y: i32, z: i32) -> f64;

    /// Vein gap noise for ore vs filler placement.
    fn router_vein_gap(&self, cache: &mut Self::ColumnCache, x: i32, y: i32, z: i32) -> f64;

    // ── Climate/biome router functions (Y-independent, cached) ──────────────

    /// Erosion value (cached in column cache).
    fn router_erosion(&self, cache: &mut Self::ColumnCache, x: i32, y: i32, z: i32) -> f64;

    /// Continentalness value (cached in column cache).
    fn router_continentalness(&self, cache: &mut Self::ColumnCache, x: i32, y: i32, z: i32) -> f64;

    /// Temperature value (cached in column cache).
    fn router_temperature(&self, cache: &mut Self::ColumnCache, x: i32, y: i32, z: i32) -> f64;

    /// Vegetation/humidity value (cached in column cache).
    fn router_vegetation(&self, cache: &mut Self::ColumnCache, x: i32, y: i32, z: i32) -> f64;

    /// Ridges/weirdness value (cached in column cache).
    fn router_ridges(&self, cache: &mut Self::ColumnCache, x: i32, y: i32, z: i32) -> f64;

    /// Preliminary surface level (cached in column cache).
    fn router_preliminary_surface_level(
        &self,
        cache: &mut Self::ColumnCache,
        x: i32,
        y: i32,
        z: i32,
    ) -> f64;

    // ── Interpolation functions ─────────────────────────────────────────────

    /// Total number of independently interpolated channels across all router
    /// entries (`final_density` + `vein_toggle` + `vein_ridged`).
    fn interpolated_count() -> usize;

    /// Whether vein functions have interpolation channels.
    fn vein_interp_enabled() -> bool;

    /// Evaluate the inner functions of all `Interpolated` markers at a cell corner.
    ///
    /// `out` must have length [`interpolated_count()`]. Each element receives
    /// the value of one `Interpolated` marker's inner function at `(x, y, z)`.
    fn fill_cell_corner_densities(
        &self,
        cache: &mut Self::ColumnCache,
        x: i32,
        y: i32,
        z: i32,
        out: &mut [f64],
    );

    /// Combine trilinearly interpolated values for `final_density`.
    fn combine_interpolated(
        &self,
        cache: &mut Self::ColumnCache,
        interpolated: &[f64],
        x: i32,
        y: i32,
        z: i32,
    ) -> f64;

    /// Combine trilinearly interpolated values for `vein_toggle`.
    fn combine_vein_toggle(
        &self,
        cache: &mut Self::ColumnCache,
        interpolated: &[f64],
        x: i32,
        y: i32,
        z: i32,
    ) -> f64;

    /// Combine trilinearly interpolated values for `vein_ridged`.
    fn combine_vein_ridged(
        &self,
        cache: &mut Self::ColumnCache,
        interpolated: &[f64],
        x: i32,
        y: i32,
        z: i32,
    ) -> f64;
}
