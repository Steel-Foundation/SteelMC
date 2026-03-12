//! NoiseChunk: cell-based terrain density evaluation with trilinear interpolation.
//!
//! Matches vanilla's `NoiseChunk` + `NoiseBasedChunkGenerator.doFill()` flow.
//!
//! Vanilla wraps density functions with `Interpolated` markers. Only the inner
//! functions (arguments to `Interpolated`) are evaluated at cell corners; the
//! outer operations (squeeze, min, etc.) are applied per-block after trilinear
//! interpolation. Each `Interpolated` marker gets its own independent channel.
//!
//! Cell dimensions depend on the dimension's noise settings.

use std::marker::PhantomData;
use std::mem;

use steel_utils::density::{ColumnCache, DimensionNoises, NoiseSettings};
use steel_utils::math::lerp;

use crate::chunk::beardifier::Beardifier;

/// Maximum number of interpolation channels supported.
/// Overworld uses 5 (1 terrain + 4 noodle caves), nether/end use 1.
const MAX_INTERP: usize = 16;

/// Stores density values at cell corners for a single chunk and provides
/// trilinear interpolation between corners for block-level resolution.
///
/// Supports multiple interpolation channels matching vanilla's multi-interpolator
/// system. Each `Interpolated` marker in the density function tree gets its own
/// channel, filled at cell corners and interpolated independently.
pub struct NoiseChunk<N: DimensionNoises> {
    /// Density values at cell corners per interpolation channel.
    /// Flat layout: `channels[ch].slice[z * corners_y + y]` for current and next X.
    channels: Vec<ChannelSlices>,
    /// Number of active interpolation channels.
    interp_count: usize,
    /// Number of Y corners per Z column (`cell_count_y` + 1).
    corners_y: usize,

    /// First cell X/Z in world coordinates (cell index, not block).
    first_cell_x: i32,
    first_cell_z: i32,
    /// Minimum cell Y index.
    cell_min_y: i32,
    /// Number of cells in Y direction.
    cell_count_y: usize,
    /// Number of cells per chunk in XZ.
    cell_count_xz: usize,

    _phantom: PhantomData<N>,
}

/// Two slices (current X and next X) for one interpolation channel.
/// Flat layout: index with `z * corners_y + y`.
struct ChannelSlices {
    slice0: Vec<f64>,
    slice1: Vec<f64>,
}

impl<N: DimensionNoises> NoiseChunk<N> {
    /// Create a new `NoiseChunk` for the given chunk position.
    ///
    /// `chunk_min_block_x` and `chunk_min_block_z` are the world-space block
    /// coordinates of the chunk's northwest corner.
    #[must_use]
    pub fn new(chunk_min_block_x: i32, chunk_min_block_z: i32) -> Self {
        let cell_width = N::Settings::CELL_WIDTH;
        let cell_height = N::Settings::CELL_HEIGHT;
        let min_y = N::Settings::MIN_Y;
        let height = N::Settings::HEIGHT;

        let first_cell_x = chunk_min_block_x.div_euclid(cell_width);
        let first_cell_z = chunk_min_block_z.div_euclid(cell_width);
        let cell_min_y = min_y.div_euclid(cell_height);

        let cell_count_xz = (16 / cell_width) as usize;
        let cell_count_y = (height / cell_height) as usize;
        let corners_y = cell_count_y + 1;
        let z_corners = cell_count_xz + 1;
        let slice_len = z_corners * corners_y;

        let interp_count = N::interpolated_count();
        let channels = (0..interp_count)
            .map(|_| ChannelSlices {
                slice0: vec![0.0; slice_len],
                slice1: vec![0.0; slice_len],
            })
            .collect();

        Self {
            channels,
            interp_count,
            corners_y,
            first_cell_x,
            first_cell_z,
            cell_min_y,
            cell_count_y,
            cell_count_xz,
            _phantom: PhantomData,
        }
    }

    /// Fill all interpolation channel slices at the given cell X coordinate.
    #[allow(clippy::needless_range_loop)]
    fn fill_slice(
        &mut self,
        use_slice0: bool,
        cell_x: i32,
        noises: &N,
        cache: &mut N::ColumnCache,
        beardifier: Option<&Beardifier>,
    ) {
        let cell_width = N::Settings::CELL_WIDTH;
        let cell_height = N::Settings::CELL_HEIGHT;
        let corners_y = self.corners_y;
        let interp_count = self.interp_count;

        let block_x = cell_x * cell_width;

        let mut values = [0.0f64; MAX_INTERP];

        for cz in 0..=self.cell_count_xz {
            let cell_z = self.first_cell_z + cz as i32;
            let block_z = cell_z * cell_width;

            // Ensure column cache for this (x, z)
            cache.ensure(block_x, block_z, noises);

            for cy in 0..corners_y {
                let block_y = (cy as i32 + self.cell_min_y) * cell_height;

                // Evaluate all inner functions at this cell corner
                noises.fill_cell_corner_densities(
                    cache,
                    block_x,
                    block_y,
                    block_z,
                    &mut values[..interp_count],
                );

                // Beardifier contributes to channel 0 (main terrain density)
                if let Some(beard) = beardifier {
                    values[0] += beard.compute(block_x, block_y, block_z);
                }

                // Store in each channel's slice
                let flat_idx = cz * corners_y + cy;
                for ch in 0..interp_count {
                    let slice = if use_slice0 {
                        &mut self.channels[ch].slice0
                    } else {
                        &mut self.channels[ch].slice1
                    };
                    slice[flat_idx] = values[ch];
                }
            }
        }
    }

    /// Fill the chunk with terrain blocks using multi-channel trilinear interpolation.
    ///
    /// For each block position:
    /// 1. Trilinearly interpolate each channel independently from cell corners
    /// 2. Apply outer operations (squeeze, min, etc.) via `combine_interpolated`
    /// 3. Call `place_block` with the final density
    pub fn fill<F>(
        &mut self,
        noises: &N,
        cache: &mut N::ColumnCache,
        beardifier: Option<&Beardifier>,
        mut place_block: F,
    ) where
        F: FnMut(usize, i32, usize, f64, &[f64], &mut N::ColumnCache),
    {
        let cell_width = N::Settings::CELL_WIDTH;
        let cell_height = N::Settings::CELL_HEIGHT;
        let cell_count_xz = self.cell_count_xz;
        let cell_count_y = self.cell_count_y;
        let interp_count = self.interp_count;
        let corners_y = self.corners_y;

        // Fill initial X slice (slice0)
        self.fill_slice(true, self.first_cell_x, noises, cache, beardifier);

        let mut interpolated = [0.0f64; MAX_INTERP];

        for cell_x_idx in 0..cell_count_xz {
            // Fill next X slice (slice1)
            self.fill_slice(
                false,
                self.first_cell_x + cell_x_idx as i32 + 1,
                noises,
                cache,
                beardifier,
            );

            for cell_z_idx in 0..cell_count_xz {
                for x_in_cell in 0..cell_width {
                    let factor_x = f64::from(x_in_cell) / f64::from(cell_width);
                    let local_x = (cell_x_idx as i32 * cell_width + x_in_cell) as usize;

                    for z_in_cell in 0..cell_width {
                        let factor_z = f64::from(z_in_cell) / f64::from(cell_width);
                        let local_z = (cell_z_idx as i32 * cell_width + z_in_cell) as usize;

                        // Pre-compute flat indices for this Z column
                        let z0_base = cell_z_idx * corners_y;
                        let z1_base = (cell_z_idx + 1) * corners_y;

                        // Process entire Y column at this (x, z)
                        for cell_y_idx in (0..cell_count_y).rev() {
                            for y_in_cell in (0..cell_height).rev() {
                                let factor_y = f64::from(y_in_cell) / f64::from(cell_height);

                                let world_y =
                                    (self.cell_min_y + cell_y_idx as i32) * cell_height + y_in_cell;

                                // Trilinearly interpolate each channel independently
                                #[allow(clippy::needless_range_loop)]
                                for ch in 0..interp_count {
                                    let s0 = &self.channels[ch].slice0;
                                    let s1 = &self.channels[ch].slice1;

                                    let n000 = s0[z0_base + cell_y_idx];
                                    let n001 = s0[z1_base + cell_y_idx];
                                    let n100 = s1[z0_base + cell_y_idx];
                                    let n101 = s1[z1_base + cell_y_idx];
                                    let n010 = s0[z0_base + cell_y_idx + 1];
                                    let n011 = s0[z1_base + cell_y_idx + 1];
                                    let n110 = s1[z0_base + cell_y_idx + 1];
                                    let n111 = s1[z1_base + cell_y_idx + 1];

                                    let d00 = lerp(factor_y, n000, n010);
                                    let d10 = lerp(factor_y, n100, n110);
                                    let d01 = lerp(factor_y, n001, n011);
                                    let d11 = lerp(factor_y, n101, n111);
                                    let d0 = lerp(factor_x, d00, d10);
                                    let d1 = lerp(factor_x, d01, d11);
                                    interpolated[ch] = lerp(factor_z, d0, d1);
                                }

                                // Apply outer operations per-block.
                                // x/z are 0 because vanilla's outer operations (squeeze, add, mul,
                                // quarter_negative, blend_alpha, blend_offset) are x/z-independent;
                                // only Y matters for YClampedGradient.
                                let density = noises.combine_interpolated(
                                    cache,
                                    &interpolated[..interp_count],
                                    0,
                                    world_y,
                                    0,
                                );

                                place_block(
                                    local_x,
                                    world_y,
                                    local_z,
                                    density,
                                    &interpolated[..interp_count],
                                    cache,
                                );
                            }
                        }
                    }
                }
            }

            // Swap slices: current next becomes current for the next iteration
            for ch in &mut self.channels {
                mem::swap(&mut ch.slice0, &mut ch.slice1);
            }
        }
    }
}
