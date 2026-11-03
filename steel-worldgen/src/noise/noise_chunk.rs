//! `NoiseChunk`: cell-based terrain density evaluation with trilinear interpolation.
//!
//! Matches vanilla's `NoiseChunk` + `NoiseBasedChunkGenerator.doFill()` flow.
//!
//! Vanilla wraps density functions with `Interpolated` markers. Only the inner
//! functions (arguments to `Interpolated`) are evaluated at cell corners; the
//! outer operations (squeeze, min, etc.) are applied per-block after trilinear
//! interpolation. Markers with identical evaluations share a channel.
//!
//! Cell dimensions depend on the dimension's noise settings.

use std::marker::PhantomData;
use std::simd::f32x4;

use glam::IVec3;
use steel_utils::SIMD_BATCH;
use steel_worldgen::density::{ColumnCache, DimensionNoises, NoiseSettings};

use crate::noise::{Beardifier, CornerColumnStore};

/// Maximum number of interpolation channels supported.
const MAX_INTERP: usize = 16;
const INTERPOLATION_LANES: usize = 4;

/// Maximum slice length (`z_corners` * `corners_y`) across all dimensions.
/// Overworld: (16/4+1) * (384/8+1) = 5 * 49 = 245. Rounded up for headroom.
const MAX_SLICE_LEN: usize = 256;
/// The largest Vanilla noise cell height (overworld's is 8).
const MAX_CELL_HEIGHT: usize = 16;

/// Stores density values at cell corners for a single chunk and provides
/// trilinear interpolation between corners for block-level resolution.
///
/// Supports multiple interpolation channels matching vanilla's multi-interpolator
/// system. Distinct marker evaluations get channels filled at cell corners
/// and interpolated independently.
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
        corner_columns: &CornerColumnStore,
    ) {
        let cell_width = N::Settings::CELL_WIDTH;

        let block_x = cell_x * cell_width;
        // Chunks sharing a corner column: two across each chunk boundary it lies on.
        let chunk_boundary = |cell: i32| cell.rem_euclid(cell_count_xz as i32) == 0;
        let x_users: u8 = if chunk_boundary(cell_x) { 2 } else { 1 };

        let mut values = [0.0_f32; MAX_INTERP * SIMD_BATCH];

        for cz in 0..=cell_count_xz {
            let cell_z = first_cell_z + cz as i32;
            let block_z = cell_z * cell_width;
            let users = x_users * if chunk_boundary(cell_z) { 2 } else { 1 };
            let column_base = cz * corners_y * MAX_INTERP;
            let column_len = corners_y * MAX_INTERP;

            if users > 1
                && corner_columns.consume((cell_x, cell_z), |values| {
                    let column = &mut slice[column_base..column_base + column_len];
                    for (corner, stored) in column
                        .as_chunks_mut::<MAX_INTERP>()
                        .0
                        .iter_mut()
                        .zip(values.chunks_exact(interp_count))
                    {
                        corner[..interp_count].copy_from_slice(stored);
                    }
                })
            {
                continue;
            }

            // Ensure column cache for this (x, z)
            cache.ensure(block_x, block_z, noises);

            // SIMD-batch blended noise for the entire Y column.
            noises.compute_noise_column(block_x, block_ys, block_z, blended_column);

            let (y_batches, y_tail) = block_ys.as_chunks::<SIMD_BATCH>();
            let (blended_batches, blended_tail) = blended_column.as_chunks::<SIMD_BATCH>();
            debug_assert_eq!(y_batches.len(), blended_batches.len());
            debug_assert_eq!(y_tail.len(), blended_tail.len());

            for (batch, (ys, blended_noise_values)) in
                y_batches.iter().zip(blended_batches.iter()).enumerate()
            {
                let cy = batch * SIMD_BATCH;
                noises.fill_cell_corner_densities_y_simd::<SIMD_BATCH>(
                    cache,
                    block_x,
                    *ys,
                    block_z,
                    *blended_noise_values,
                    &mut values[..interp_count * SIMD_BATCH],
                );
                for lane in 0..SIMD_BATCH {
                    let corner_idx = cz * corners_y + cy + lane;
                    let base = corner_idx * MAX_INTERP;
                    let value_base = lane * interp_count;
                    slice[base..base + interp_count]
                        .copy_from_slice(&values[value_base..value_base + interp_count]);
                }
            }

            let tail_start = y_batches.len() * SIMD_BATCH;
            for (tail_offset, (&block_y, &blended_noise_value)) in
                y_tail.iter().zip(blended_tail.iter()).enumerate()
            {
                let cy = tail_start + tail_offset;

                noises.fill_cell_corner_densities(
                    cache,
                    block_x,
                    block_y,
                    block_z,
                    blended_noise_value,
                    &mut values[..interp_count],
                );

                let corner_idx = cz * corners_y + cy;
                let base = corner_idx * MAX_INTERP;
                slice[base..base + interp_count].copy_from_slice(&values[..interp_count]);
            }

            if users > 1 {
                corner_columns.produce((cell_x, cell_z), users, || {
                    slice[column_base..column_base + column_len]
                        .as_chunks::<MAX_INTERP>()
                        .0
                        .iter()
                        .flat_map(|corner| &corner[..interp_count])
                        .copied()
                        .collect()
                });
            }
        }
    }

    /// Fill the chunk with terrain blocks using multi-channel trilinear interpolation.
    ///
    /// For each block position:
    /// 1. Trilinearly interpolate each channel independently from cell corners
    /// 2. Apply outer operations (squeeze, min, etc.) via `combine_interpolated`
    /// 3. Call `place_block` with the final density
    ///
    /// Cells at or above `unsampled_air_min_y` whose proving channels
    /// ([`DimensionNoises::final_density_nonpositive_channels`]) are `<= 0`
    /// at every interpolated block value, and that the beardifier cannot reach,
    /// are skipped without calling `place_block`: their density is non-positive,
    /// which the caller guarantees places nothing and changes no state there.
    #[expect(
        clippy::too_many_lines,
        reason = "keeps the cell interpolation and block traversal together"
    )]
    pub fn fill<F>(
        &mut self,
        noises: &N,
        cache: &mut N::ColumnCache,
        beardifier: Option<&Beardifier>,
        unsampled_air_min_y: i32,
        corner_columns: &CornerColumnStore,
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
        for (cx_off, slice) in self.slices[..n_slices].iter_mut().enumerate() {
            let cell_x = first_cell_x + cx_off as i32;
            Self::fill_slice_into(
                slice,
                cell_x,
                block_ys,
                &mut local_blended,
                interp_count,
                corners_y,
                cell_count_xz,
                first_cell_z,
                noises,
                cache,
                corner_columns,
            );
        }

        let air_channels = N::final_density_nonpositive_channels();
        let mut air_cells = vec![false; cell_count_y];
        let y_count = cell_height as usize;
        debug_assert!(y_count <= MAX_CELL_HEIGHT);
        let inverse_height = f32x4::splat(1.0 / cell_height as f32);
        let channel_batches = interp_count.div_ceil(INTERPOLATION_LANES);
        let all_batches = (1_usize << channel_batches) - 1;
        let proving_batches = air_channels.map_or(0, |channels| {
            channels.iter().fold(0, |mask, &channel| {
                mask | (1 << (channel / INTERPOLATION_LANES))
            })
        });

        // Reuse Z partials across X without changing vanilla's Z/X/Y arithmetic.
        let mut z_partials =
            vec![[f32x4::splat(0.0); 2]; cell_width as usize * corners_y * channel_batches];

        for cell_x_idx in 0..cell_count_xz {
            let s0: &[f32] = &self.slices[cell_x_idx][..];
            let s1: &[f32] = &self.slices[cell_x_idx + 1][..];

            for cell_z_idx in 0..cell_count_xz {
                let z0_base = cell_z_idx * corners_y;
                let z1_base = (cell_z_idx + 1) * corners_y;

                let cell_min_x = (self.first_cell_x + cell_x_idx as i32) * cell_width;
                let cell_min_z = (self.first_cell_z + cell_z_idx as i32) * cell_width;
                for (cell_y_idx, air) in air_cells.iter_mut().enumerate() {
                    let cell_min_world_y = (self.cell_min_y + cell_y_idx as i32) * cell_height;
                    *air = air_channels.is_some_and(|channels| {
                        cell_min_world_y >= unsampled_air_min_y
                            && !beardifier.is_some_and(|beard| {
                                beard.may_affect(
                                    IVec3::new(cell_min_x, cell_min_world_y, cell_min_z),
                                    IVec3::new(
                                        cell_min_x + cell_width - 1,
                                        cell_min_world_y + cell_height - 1,
                                        cell_min_z + cell_width - 1,
                                    ),
                                )
                            })
                            && channels.iter().all(|&ch| {
                                let i0 = (z0_base + cell_y_idx) * MAX_INTERP + ch;
                                let i1 = (z1_base + cell_y_idx) * MAX_INTERP + ch;
                                [i0, i0 + MAX_INTERP, i1, i1 + MAX_INTERP]
                                    .into_iter()
                                    .all(|i| s0[i] <= 0.0 && s1[i] <= 0.0)
                            })
                    });
                }

                for z_in_cell in 0..cell_width as usize {
                    let factor_z = f32x4::splat(z_in_cell as f32 / cell_width as f32);
                    for corner_y in 0..corners_y {
                        let i0 = (z0_base + corner_y) * MAX_INTERP;
                        let i1 = (z1_base + corner_y) * MAX_INTERP;
                        let row = (z_in_cell * corners_y + corner_y) * channel_batches;
                        for (batch, channel) in
                            (0..interp_count).step_by(INTERPOLATION_LANES).enumerate()
                        {
                            let corner = |slice: &[f32], base| {
                                // SAFETY: construction bounds the corner count by
                                // MAX_SLICE_LEN and interp_count by MAX_INTERP.
                                // Both Z planes and corner_y are in that grid;
                                // MAX_INTERP is a multiple of four, so the padded
                                // final batch also stays inside its corner.
                                f32x4::from_slice(unsafe {
                                    slice.get_unchecked(
                                        base + channel..base + channel + INTERPOLATION_LANES,
                                    )
                                })
                            };

                            let left0 = corner(s0, i0);
                            let left1 = corner(s0, i1);
                            let right0 = corner(s1, i0);
                            let right1 = corner(s1, i1);
                            z_partials[row + batch] = [
                                left0 + factor_z * (left1 - left0),
                                right0 + factor_z * (right1 - right0),
                            ];
                        }
                    }
                }

                for x_in_cell in 0..cell_width {
                    let factor_x = x_in_cell as f32 / cell_width as f32;
                    let factor_x_v = f32x4::splat(factor_x);
                    let local_x = (cell_x_idx as i32 * cell_width + x_in_cell) as usize;

                    for z_in_cell in 0..cell_width {
                        let local_z = (cell_z_idx as i32 * cell_width + z_in_cell) as usize;

                        for cell_y_idx in (0..cell_count_y).rev() {
                            let cell_world_y = (self.cell_min_y + cell_y_idx as i32) * cell_height;
                            let row =
                                (z_in_cell as usize * corners_y + cell_y_idx) * channel_batches;
                            let mut values_by_y = [[0.0_f32; MAX_INTERP]; MAX_CELL_HEIGHT];

                            if N::interpolated_count() <= INTERPOLATION_LANES {
                                for (batch, channel) in
                                    (0..interp_count).step_by(INTERPOLATION_LANES).enumerate()
                                {
                                    let [v00, v10] = z_partials[row + batch];
                                    let [v01, v11] = z_partials[row + channel_batches + batch];
                                    let v0 = v00 + factor_x_v * (v10 - v00);
                                    let v1 = v01 + factor_x_v * (v11 - v01);
                                    let step = (v1 - v0) * inverse_height;
                                    let mut value = v0 + step * f32x4::splat(0.0);

                                    for values in values_by_y.iter_mut().take(y_count) {
                                        values[channel..channel + INTERPOLATION_LANES]
                                            .copy_from_slice(&value.to_array());
                                        value += step;
                                    }
                                }

                                if air_cells[cell_y_idx]
                                    && air_channels.is_some_and(|channels| {
                                        Self::channels_are_nonpositive(
                                            &values_by_y[..y_count],
                                            channels,
                                        )
                                    })
                                {
                                    continue;
                                }
                            } else {
                                let fill_batches =
                                    |batches: usize, values_by_y: &mut [[f32; MAX_INTERP]]| {
                                        for (batch, channel) in (0..interp_count)
                                            .step_by(INTERPOLATION_LANES)
                                            .enumerate()
                                        {
                                            if batches & (1 << batch) == 0 {
                                                continue;
                                            }

                                            let [v00, v10] = z_partials[row + batch];
                                            let [v01, v11] =
                                                z_partials[row + channel_batches + batch];
                                            let v0 = v00 + factor_x_v * (v10 - v00);
                                            let v1 = v01 + factor_x_v * (v11 - v01);
                                            let step = (v1 - v0) * inverse_height;
                                            let mut value = v0 + step * f32x4::splat(0.0);

                                            for values in values_by_y.iter_mut().take(y_count) {
                                                values[channel..channel + INTERPOLATION_LANES]
                                                    .copy_from_slice(&value.to_array());
                                                value += step;
                                            }
                                        }
                                    };
                                let first_batches = if air_cells[cell_y_idx] {
                                    proving_batches
                                } else {
                                    all_batches
                                };

                                fill_batches(first_batches, &mut values_by_y);

                                if air_cells[cell_y_idx]
                                    && air_channels.is_some_and(|channels| {
                                        Self::channels_are_nonpositive(
                                            &values_by_y[..y_count],
                                            channels,
                                        )
                                    })
                                {
                                    continue;
                                }

                                let remaining_batches = all_batches & !first_batches;
                                if remaining_batches != 0 {
                                    fill_batches(remaining_batches, &mut values_by_y);
                                }
                            }

                            for y_in_cell in (0..cell_height).rev() {
                                let world_y = cell_world_y + y_in_cell;
                                let interpolated = &values_by_y[y_in_cell as usize][..interp_count];

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

    fn channels_are_nonpositive(values_by_y: &[[f32; MAX_INTERP]], channels: &[usize]) -> bool {
        values_by_y
            .iter()
            .all(|values| channels.iter().all(|&channel| values[channel] <= 0.0))
    }

    /// Pre-fills the density/richness buffers used by material ore rules.
    ///
    /// `MaterialRuleContext.getDensitiesInChunk` samples these functions over
    /// complete chunk volume before the surface scan begins. Steel keeps the
    /// dense Y prefix where a rule may accept a block. Compact `f32` results
    /// avoid retaining the noise chunk across generation stages.
    /// The result has `256 * material_ore_vein_value_height() *
    /// material_ore_vein_value_count()` entries, ordered by Y, Z, X, then value.
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
        let value_height = N::material_ore_vein_value_height();
        let mut values = vec![0.0; 16 * 16 * value_height * value_count];
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
                                let mut value = v0 + step * 0.0;
                                for values in values_by_y.iter_mut().take(y_count) {
                                    values[channel] = value;
                                    value += step;
                                }
                            }

                            for y_in_cell in (0..cell_height).rev() {
                                let world_y =
                                    (self.cell_min_y + cell_y_idx as i32) * cell_height + y_in_cell;
                                let relative_y = (world_y - min_y) as usize;
                                if relative_y >= value_height {
                                    continue;
                                }

                                interpolated[..self.interp_count].copy_from_slice(
                                    &values_by_y[y_in_cell as usize][..self.interp_count],
                                );

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

#[cfg(test)]
mod tests {
    use steel_utils::random::{Random, legacy_random::LegacyRandom, xoroshiro::Xoroshiro};

    use super::NoiseChunk;
    use crate::density::{ColumnCache, DimensionNoises, NoiseSettings};
    use crate::density_functions::{
        end::EndNoises, nether::NetherNoises, overworld::OverworldNoises,
    };
    use crate::noise::CornerColumnStore;
    use crate::noise_parameters::get_noise_parameters;

    type Visit = ((usize, i32, usize), u32);

    fn create_noises<N: DimensionNoises>() -> N {
        let seed = 1;
        let splitter = if N::Settings::LEGACY_RANDOM_SOURCE {
            LegacyRandom::from_seed(seed).next_positional()
        } else {
            Xoroshiro::from_seed(seed).next_positional()
        };
        N::create(seed, &splitter, &get_noise_parameters())
    }

    fn fill_visits<N: DimensionNoises>(
        noises: &N,
        chunk: (i32, i32),
        air_min_y: i32,
        corner_columns: &CornerColumnStore,
    ) -> Vec<Visit> {
        let (min_x, min_z) = (chunk.0 * 16, chunk.1 * 16);
        let mut noise_chunk = NoiseChunk::<N>::new(min_x, min_z);
        let mut cache = N::ColumnCache::default();
        cache.init_grid(min_x, min_z, noises);
        let mut visits = Vec::new();
        noise_chunk.fill(
            noises,
            &mut cache,
            None,
            air_min_y,
            corner_columns,
            |x, y, z, density, _, _| {
                visits.push(((x, y, z), density.to_bits()));
            },
        );
        visits
    }

    /// Skipping every provable cell must leave the visited blocks and their
    /// densities unchanged, and only drop blocks whose density is not positive.
    #[expect(
        clippy::neg_cmp_op_on_partial_ord,
        reason = "NaN counts as not positive, matching the aquifer's `density > 0` solid test"
    )]
    fn assert_air_skip_sound<N: DimensionNoises>() {
        let noises = create_noises::<N>();

        let mut skipped = 0;
        for chunk in [(0, 0), (-7, 3), (40, -25), (-300, -120), (1000, 800)] {
            let all = fill_visits(&noises, chunk, i32::MAX, &CornerColumnStore::default());
            let mut kept = fill_visits(&noises, chunk, i32::MIN, &CornerColumnStore::default())
                .into_iter()
                .peekable();
            for (pos, bits) in all {
                if kept.peek().is_some_and(|&(kept_pos, _)| kept_pos == pos) {
                    let (_, kept_bits) = kept.next().expect("peeked");
                    assert_eq!(kept_bits, bits, "density changed at {pos:?} in {chunk:?}");
                } else {
                    assert!(
                        !(f32::from_bits(bits) > 0.0),
                        "skipped solid block {pos:?} in {chunk:?}"
                    );
                    skipped += 1;
                }
            }
            assert!(
                kept.next().is_none(),
                "skip run visited a block the full run did not"
            );
        }
        assert!(skipped > 0, "no cell was skipped");
    }

    /// Neighbors filled through one shared store must match independent fills.
    fn assert_corner_reuse_matches<N: DimensionNoises>() {
        let noises = create_noises::<N>();
        let shared = CornerColumnStore::default();
        for x in -1..=1 {
            for z in -1..=1 {
                let chunk = (x + 30, z - 12);
                assert_eq!(
                    fill_visits(&noises, chunk, i32::MIN, &shared),
                    fill_visits(&noises, chunk, i32::MIN, &CornerColumnStore::default()),
                    "chunk {chunk:?}"
                );
            }
        }
    }

    #[test]
    fn air_column_rejects_rounding_overshoot() {
        let lower = -5.0 * f32::from_bits(1);
        let upper = 0.0_f32;
        assert!(lower <= 0.0 && upper <= 0.0);
        let step = (upper - lower) * (1.0 / 8.0);
        let mut value = lower + step * 0.0;
        let mut values_by_y = [[0.0; super::MAX_INTERP]; 8];
        for values in &mut values_by_y {
            values[0] = value;
            value += step;
        }
        assert!(values_by_y[7][0] > 0.0);
        assert!(!NoiseChunk::<OverworldNoises>::channels_are_nonpositive(
            &values_by_y,
            &[0],
        ));
    }

    #[test]
    fn overworld_corner_reuse_matches() {
        assert_corner_reuse_matches::<OverworldNoises>();
    }

    #[test]
    fn nether_corner_reuse_matches() {
        assert_corner_reuse_matches::<NetherNoises>();
    }

    #[test]
    fn end_corner_reuse_matches() {
        assert_corner_reuse_matches::<EndNoises>();
    }

    #[test]
    fn overworld_air_skip_is_sound() {
        assert_air_skip_sound::<OverworldNoises>();
    }

    #[test]
    fn nether_air_skip_is_sound() {
        assert_air_skip_sound::<NetherNoises>();
    }

    #[test]
    fn end_air_skip_is_sound() {
        assert_air_skip_sound::<EndNoises>();
    }
}
