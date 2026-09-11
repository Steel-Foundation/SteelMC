//! `NoiseChunk`: cell-based terrain density evaluation with trilinear interpolation.
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
use steel_worldgen::density::{ColumnCache, DimensionNoises, NoiseSettings};

use crate::noise::Beardifier;

/// Maximum number of interpolation channels supported.
/// Overworld uses 8 (1 terrain + 4 noodle caves + 3 vein channels), nether/end use 1.
const MAX_INTERP: usize = 16;

/// Maximum slice length (`z_corners` * `corners_y`) across all dimensions.
/// Overworld: (16/4+1) * (384/8+1) = 5 * 49 = 245. Rounded up for headroom.
const MAX_SLICE_LEN: usize = 256;
/// The largest Vanilla noise cell height (overworld's is 8).
const MAX_CELL_HEIGHT: usize = 16;

/// Stores density values at cell corners for a single chunk and provides
/// trilinear interpolation between corners for block-level resolution.
///
/// Supports multiple interpolation channels matching vanilla's multi-interpolator
/// system. Each `Interpolated` marker in the density function tree gets its own
/// channel, filled at cell corners and interpolated independently.
///
/// Storage is per-corner `SoA` — `slice[corner_idx * MAX_INTERP + ch]` — so 4
/// adjacent channels' values at a given corner sit in contiguous memory,
/// allowing the terrain sampler to keep Vanilla's f32 density values without
/// widening an intermediate result.
pub struct NoiseChunk<N: DimensionNoises> {
    /// One slice per cell-X boundary, holding density values at the cell
    /// corners on that X-plane. Length is `cell_count_xz + 1`. Indexed as
    /// `slices[cx][corner_idx * MAX_INTERP + ch]` where
    /// `corner_idx = z_corner * corners_y + y_corner` (range `[0, slice_len)`)
    /// and `ch` is the interpolation channel (range `[0, interp_count)`).
    ///
    /// We keep all slices materialized rather than alternating two buffers so
    /// the slice-fill phase can run in parallel: each `cx` boundary's noise
    /// tree evaluation is independent. The per-block trilerp loop then
    /// indexes `slices[cx]` and `slices[cx + 1]` sequentially.
    slices: Vec<Box<[f32; MAX_INTERP * MAX_SLICE_LEN]>>,
    /// Number of active interpolation channels.
    interp_count: usize,
    /// Number of Y corners per Z column (`cell_count_y` + 1).
    corners_y: usize,

    /// Per-corner block-Y values, precomputed once at construction.
    /// Same for every slice fill (depends only on `cell_min_y`,
    /// `cell_height`, and `corners_y`).
    block_ys: Vec<i32>,

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

impl<N: DimensionNoises> NoiseChunk<N> {
    /// Create a new `NoiseChunk` for the given chunk position.
    ///
    /// `chunk_min_block_x` and `chunk_min_block_z` are the world-space block
    /// coordinates of the chunk's northwest corner.
    #[must_use]
    #[expect(
        clippy::missing_panics_doc,
        reason = "panic is a compile-time constant check"
    )]
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
        assert!(
            slice_len <= MAX_SLICE_LEN,
            "slice_len {slice_len} exceeds MAX_SLICE_LEN {MAX_SLICE_LEN}"
        );
        assert!(
            interp_count <= MAX_INTERP,
            "interp_count {interp_count} exceeds MAX_INTERP {MAX_INTERP}"
        );

        let block_ys: Vec<i32> = (0..corners_y)
            .map(|cy| (cy as i32 + cell_min_y) * cell_height)
            .collect();

        let n_slices = cell_count_xz + 1;
        let mut slices = Vec::with_capacity(n_slices);
        for _ in 0..n_slices {
            // This is a per-chunk constructor, so the stack temporary is fine.
            slices.push(Box::new([0.0_f32; MAX_INTERP * MAX_SLICE_LEN]));
        }

        Self {
            slices,
            interp_count,
            corners_y,
            block_ys,
            first_cell_x,
            first_cell_z,
            cell_min_y,
            cell_count_y,
            cell_count_xz,
            _phantom: PhantomData,
        }
    }

    /// Fill the slice buffer for the given cell X. Free-standing function so
    /// each parallel slice-fill can run on its own thread with its own
    /// `ColumnCache` clone.
    #[expect(
        clippy::too_many_arguments,
        reason = "slice filling needs the precomputed geometry and per-thread cache"
    )]
    fn fill_slice_into(
        slice: &mut [f32; MAX_INTERP * MAX_SLICE_LEN],
        cell_x: i32,
        block_ys: &[i32],
        blended_column: &mut [f32],
        interp_count: usize,
        corners_y: usize,
        cell_count_xz: usize,
        first_cell_z: i32,
        noises: &N,
        cache: &mut N::ColumnCache,
    ) {
        let cell_width = N::Settings::CELL_WIDTH;

        let block_x = cell_x * cell_width;

        let mut values = [0.0_f32; MAX_INTERP];

        for cz in 0..=cell_count_xz {
            let cell_z = first_cell_z + cz as i32;
            let block_z = cell_z * cell_width;

            // Ensure column cache for this (x, z)
            cache.ensure(block_x, block_z, noises);

            // SIMD-batch blended noise for the entire Y column.
            noises.compute_noise_column(block_x, block_ys, block_z, blended_column);

            for cy in 0..corners_y {
                let block_y = block_ys[cy];

                noises.fill_cell_corner_densities(
                    cache,
                    block_x,
                    block_y,
                    block_z,
                    blended_column[cy],
                    &mut values[..interp_count],
                );

                let corner_idx = cz * corners_y + cy;
                let base = corner_idx * MAX_INTERP;
                slice[base..base + interp_count].copy_from_slice(&values[..interp_count]);
            }
        }
    }

    /// Fill the chunk with terrain blocks using multi-channel trilinear interpolation.
    ///
    /// For each block position:
    /// 1. Trilinearly interpolate each channel independently from cell corners
    /// 2. Apply outer operations (squeeze, min, etc.) via `combine_interpolated`
    /// 3. Call `place_block` with the final density
    #[expect(
        clippy::too_many_lines,
        reason = "single SIMD trilinear-interpolation kernel; splitting the loop nest would scatter the per-corner SAFETY invariants"
    )]
    pub fn fill<F>(
        &mut self,
        noises: &N,
        cache: &mut N::ColumnCache,
        beardifier: Option<&Beardifier>,
        mut place_block: F,
    ) where
        F: FnMut(usize, i32, usize, f32, &[f32], &mut N::ColumnCache),
    {
        let cell_width = N::Settings::CELL_WIDTH;
        let cell_height = N::Settings::CELL_HEIGHT;
        let cell_count_xz = self.cell_count_xz;
        let cell_count_y = self.cell_count_y;
        let interp_count = self.interp_count;
        let corners_y = self.corners_y;
        let first_cell_x = self.first_cell_x;
        let first_cell_z = self.first_cell_z;
        let block_ys: &[i32] = &self.block_ys;

        // Pre-fill ALL slices sequentially. Each `(cell_x boundary)` slice is an
        // independent noise-tree evaluation; the grid in `cache` is set up by the
        // caller via `init_grid` and is read-only here, while each slice only
        // overwrites the cache's per-column active fields — so one cache is reused
        // across slices without cloning. The chunk pipeline already parallelises
        // across chunks, so parallelising the 5 slices here would nest rayon work
        // and add coordination + cache-clone overhead with no spare cores to use.
        let n_slices = cell_count_xz + 1;
        let mut local_blended = vec![0.0_f32; corners_y];
        for cx_off in 0..n_slices {
            let cell_x = first_cell_x + cx_off as i32;
            Self::fill_slice_into(
                &mut self.slices[cx_off],
                cell_x,
                block_ys,
                &mut local_blended,
                interp_count,
                corners_y,
                cell_count_xz,
                first_cell_z,
                noises,
                cache,
            );
        }

        let mut interpolated = [0.0_f32; MAX_INTERP];

        for cell_x_idx in 0..cell_count_xz {
            for cell_z_idx in 0..cell_count_xz {
                for x_in_cell in 0..cell_width {
                    let factor_x = x_in_cell as f32 / cell_width as f32;
                    let local_x = (cell_x_idx as i32 * cell_width + x_in_cell) as usize;

                    for z_in_cell in 0..cell_width {
                        let factor_z = z_in_cell as f32 / cell_width as f32;
                        let local_z = (cell_z_idx as i32 * cell_width + z_in_cell) as usize;

                        // Pre-compute flat indices for this Z column
                        let z0_base = cell_z_idx * corners_y;
                        let z1_base = (cell_z_idx + 1) * corners_y;

                        // Process entire Y column at this (x, z)
                        for cell_y_idx in (0..cell_count_y).rev() {
                            let i0_base = (z0_base + cell_y_idx) * MAX_INTERP;
                            let i1_base = (z1_base + cell_y_idx) * MAX_INTERP;
                            let i0_next = i0_base + MAX_INTERP;
                            let i1_next = i1_base + MAX_INTERP;
                            let s0 = &*self.slices[cell_x_idx];
                            let s1 = &*self.slices[cell_x_idx + 1];
                            let mut values_by_y = [[0.0_f32; MAX_INTERP]; MAX_CELL_HEIGHT];
                            let y_count = cell_height as usize;
                            debug_assert!(y_count <= MAX_CELL_HEIGHT);

                            for ch in 0..interp_count {
                                let n000 = s0[i0_base + ch];
                                let n001 = s0[i1_base + ch];
                                let n100 = s1[i0_base + ch];
                                let n101 = s1[i1_base + ch];
                                let n010 = s0[i0_next + ch];
                                let n011 = s0[i1_next + ch];
                                let n110 = s1[i0_next + ch];
                                let n111 = s1[i1_next + ch];
                                let v00 = n000 + factor_z * (n001 - n000);
                                let v01 = n010 + factor_z * (n011 - n010);
                                let v10 = n100 + factor_z * (n101 - n100);
                                let v11 = n110 + factor_z * (n111 - n110);
                                let v0 = v00 + factor_x * (v10 - v00);
                                let v1 = v01 + factor_x * (v11 - v01);
                                let step = (v1 - v0) * (1.0_f32 / cell_height as f32);
                                let mut value = v0;
                                for values in values_by_y.iter_mut().take(y_count) {
                                    values[ch] = value;
                                    value += step;
                                }
                            }

                            for y_in_cell in (0..cell_height).rev() {
                                let world_y =
                                    (self.cell_min_y + cell_y_idx as i32) * cell_height + y_in_cell;
                                interpolated[..interp_count].copy_from_slice(
                                    &values_by_y[y_in_cell as usize][..interp_count],
                                );

                                // Apply outer operations per-block.
                                // x/z are 0 because vanilla's outer operations (squeeze, add, mul,
                                // quarter_negative, blend_alpha, blend_offset) are x/z-independent;
                                // only Y matters for YClampedGradient.
                                let mut density = noises.combine_interpolated(
                                    cache,
                                    &interpolated[..interp_count],
                                    0,
                                    world_y,
                                    0,
                                );

                                // Vanilla integrates beardifier as `add(final_density, beardifier)`
                                // wrapped in `cacheAllInCell` — i.e. evaluated per-block, after the
                                // outer ops on `final_density` have run. Adding it at cell corners
                                // would put it inside the squeeze and trilerp it linearly across
                                // the cell, both of which diverge from vanilla for large beardifier
                                // values inside a structure's pieces.
                                let world_x = cell_x_idx as i32 * cell_width
                                    + x_in_cell
                                    + self.first_cell_x * cell_width;
                                let world_z = cell_z_idx as i32 * cell_width
                                    + z_in_cell
                                    + self.first_cell_z * cell_width;
                                if let Some(beard) = beardifier {
                                    density += beard.compute(world_x, world_y, world_z) as f32;
                                }

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

            // No swap needed: all slices are pre-filled and indexed directly
            // via `self.slices[cell_x_idx]` / `[cell_x_idx + 1]`.
        }
    }

    /// Pre-fills the density/richness buffers used by material ore rules.
    ///
    /// `MaterialRuleContext.getDensitiesInChunk` samples these functions over
    /// the complete chunk volume before the surface scan begins. Keeping the
    /// compact `f32` results instead of the full interpolation channels avoids
    /// retaining the noise chunk across generation stages.
    #[must_use]
    pub fn prefill_material_ore_vein_values(
        &self,
        noises: &N,
        cache: &mut N::ColumnCache,
    ) -> Box<[f32]> {
        let value_count = N::material_ore_vein_value_count();
        if value_count == 0 {
            return Box::default();
        }

        let cell_width = N::Settings::CELL_WIDTH;
        let cell_height = N::Settings::CELL_HEIGHT;
        let min_y = N::Settings::MIN_Y;
        let mut values = vec![0.0; 16 * 16 * N::Settings::HEIGHT as usize * value_count];
        let mut interpolated = [0.0_f32; MAX_INTERP];

        for cell_x_idx in 0..self.cell_count_xz {
            for cell_z_idx in 0..self.cell_count_xz {
                let z0_base = cell_z_idx * self.corners_y;
                let z1_base = (cell_z_idx + 1) * self.corners_y;
                let s0 = &*self.slices[cell_x_idx];
                let s1 = &*self.slices[cell_x_idx + 1];

                for x_in_cell in 0..cell_width {
                    let factor_x = x_in_cell as f32 / cell_width as f32;
                    let local_x = (cell_x_idx as i32 * cell_width + x_in_cell) as usize;
                    let world_x =
                        self.first_cell_x * cell_width + cell_x_idx as i32 * cell_width + x_in_cell;

                    for z_in_cell in 0..cell_width {
                        let factor_z = z_in_cell as f32 / cell_width as f32;
                        let local_z = (cell_z_idx as i32 * cell_width + z_in_cell) as usize;
                        let world_z = self.first_cell_z * cell_width
                            + cell_z_idx as i32 * cell_width
                            + z_in_cell;
                        cache.ensure(world_x, world_z, noises);

                        for cell_y_idx in (0..self.cell_count_y).rev() {
                            let i0_base = (z0_base + cell_y_idx) * MAX_INTERP;
                            let i1_base = (z1_base + cell_y_idx) * MAX_INTERP;
                            let i0_next = i0_base + MAX_INTERP;
                            let i1_next = i1_base + MAX_INTERP;
                            let mut values_by_y = [[0.0_f32; MAX_INTERP]; MAX_CELL_HEIGHT];
                            let y_count = cell_height as usize;
                            debug_assert!(y_count <= MAX_CELL_HEIGHT);

                            for channel in 0..self.interp_count {
                                let n000 = s0[i0_base + channel];
                                let n001 = s0[i1_base + channel];
                                let n100 = s1[i0_base + channel];
                                let n101 = s1[i1_base + channel];
                                let n010 = s0[i0_next + channel];
                                let n011 = s0[i1_next + channel];
                                let n110 = s1[i0_next + channel];
                                let n111 = s1[i1_next + channel];
                                let v00 = n000 + factor_z * (n001 - n000);
                                let v01 = n010 + factor_z * (n011 - n010);
                                let v10 = n100 + factor_z * (n101 - n100);
                                let v11 = n110 + factor_z * (n111 - n110);
                                let v0 = v00 + factor_x * (v10 - v00);
                                let v1 = v01 + factor_x * (v11 - v01);
                                let step = (v1 - v0) * (1.0_f32 / cell_height as f32);
                                let mut value = v0;
                                for values in values_by_y.iter_mut().take(y_count) {
                                    values[channel] = value;
                                    value += step;
                                }
                            }

                            for y_in_cell in (0..cell_height).rev() {
                                let world_y =
                                    (self.cell_min_y + cell_y_idx as i32) * cell_height + y_in_cell;
                                interpolated[..self.interp_count].copy_from_slice(
                                    &values_by_y[y_in_cell as usize][..self.interp_count],
                                );

                                let relative_y = (world_y - min_y) as usize;
                                let offset =
                                    (relative_y * 16 * 16 + local_z * 16 + local_x) * value_count;
                                noises.fill_material_ore_vein_values(
                                    cache,
                                    &interpolated[..self.interp_count],
                                    world_x,
                                    world_y,
                                    world_z,
                                    &mut values[offset..offset + value_count],
                                );
                            }
                        }
                    }
                }
            }
        }

        values.into_boxed_slice()
    }
}
