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
use std::simd::f64x4;

use steel_math::lerp;
use steel_worldgen::density::{ColumnCache, DimensionNoises, NoiseSettings};

use crate::noise::Beardifier;

/// Maximum number of interpolation channels supported.
/// Overworld uses 8 (1 terrain + 4 noodle caves + 3 vein channels), nether/end use 1.
const MAX_INTERP: usize = 16;

/// Maximum slice length (`z_corners` * `corners_y`) across all dimensions.
/// Overworld: (16/4+1) * (384/8+1) = 5 * 49 = 245. Rounded up for headroom.
const MAX_SLICE_LEN: usize = 256;

/// Stores density values at cell corners for a single chunk and provides
/// trilinear interpolation between corners for block-level resolution.
///
/// Supports multiple interpolation channels matching vanilla's multi-interpolator
/// system. Each `Interpolated` marker in the density function tree gets its own
/// channel, filled at cell corners and interpolated independently.
///
/// Storage is per-corner `SoA` — `slice[corner_idx * MAX_INTERP + ch]` — so 4
/// adjacent channels' values at a given corner sit in contiguous memory,
/// enabling a single `f64x4` load and SIMD-batched trilinear interpolation
/// across 4 channels per block.
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
    slices: Vec<Box<[f64; MAX_INTERP * MAX_SLICE_LEN]>>,
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
            // The boxed fixed-size array keeps the `[f64; N]` type that the SIMD
            // `fill` path and its `get_unchecked` SAFETY proofs rely on. This is a
            // per-chunk constructor, not a hot path, so the stack temporary is fine.
            #[expect(
                clippy::large_stack_arrays,
                reason = "fixed-size boxed array keeps the [f64; N] type the SIMD fill path relies on; cold per-chunk constructor"
            )]
            slices.push(Box::new([0.0; MAX_INTERP * MAX_SLICE_LEN]));
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
        slice: &mut [f64; MAX_INTERP * MAX_SLICE_LEN],
        cell_x: i32,
        block_ys: &[i32],
        blended_column: &mut [f64],
        interp_count: usize,
        corners_y: usize,
        cell_count_xz: usize,
        first_cell_z: i32,
        noises: &N,
        cache: &mut N::ColumnCache,
    ) {
        let cell_width = N::Settings::CELL_WIDTH;

        let block_x = cell_x * cell_width;

        let mut values = [0.0f64; MAX_INTERP];

        // Scratch buffer for the 4-Y SIMD batch. Lane-major SoA: lane `i`'s
        // `interp_count` channels live at `values_4x[i * interp_count..]`.
        let mut values_4x = [0.0f64; 4 * MAX_INTERP];

        for cz in 0..=cell_count_xz {
            let cell_z = first_cell_z + cz as i32;
            let block_z = cell_z * cell_width;

            // Ensure column cache for this (x, z)
            cache.ensure(block_x, block_z, noises);

            // SIMD-batch blended noise for the entire Y column.
            noises.compute_noise_column(block_x, block_ys, block_z, blended_column);

            // 4-Y SIMD-batched corner fill. Tail is handled by the scalar
            // loop below for any remaining `corners_y % 4` corners.
            let mut cy = 0;
            while cy + 4 <= corners_y {
                let ys_v = f64x4::from_array([
                    f64::from(block_ys[cy]),
                    f64::from(block_ys[cy + 1]),
                    f64::from(block_ys[cy + 2]),
                    f64::from(block_ys[cy + 3]),
                ]);
                let blended_v = f64x4::from_array([
                    blended_column[cy],
                    blended_column[cy + 1],
                    blended_column[cy + 2],
                    blended_column[cy + 3],
                ]);

                noises.fill_cell_corner_densities_4x(
                    cache,
                    block_x,
                    ys_v,
                    block_z,
                    blended_v,
                    &mut values_4x[..4 * interp_count],
                );

                for lane in 0..4 {
                    let lane_cy = cy + lane;
                    let src = &values_4x[lane * interp_count..(lane + 1) * interp_count];
                    let corner_idx = cz * corners_y + lane_cy;
                    let base = corner_idx * MAX_INTERP;
                    slice[base..base + interp_count].copy_from_slice(src);
                }

                cy += 4;
            }

            while cy < corners_y {
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

                cy += 1;
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
    #[expect(
        clippy::similar_names,
        reason = "factor_{x,y,z}_v vector splats deliberately mirror their scalar factor_{x,y,z} sources"
    )]
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
        let mut local_blended = vec![0.0f64; corners_y];
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

        let mut interpolated = [0.0f64; MAX_INTERP];

        for cell_x_idx in 0..cell_count_xz {
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

                                // Trilinearly interpolate each channel.
                                //
                                // SoA layout puts the 4 (or fewer) channel
                                // values for one corner in contiguous memory,
                                // so a single `f64x4` load per corner replaces
                                // four scattered scalar loads in the legacy
                                // AoS path. Math is per-lane independent and
                                // matches the scalar order exactly, so the
                                // result is bit-identical to vanilla.
                                //
                                // SAFETY: max index = (z1_base + cell_y_idx + 1) * MAX_INTERP + (ch_batch+3)
                                //         ≤ ((cell_count_xz+1)*corners_y - 1) * MAX_INTERP + MAX_INTERP - 1
                                //         < MAX_SLICE_LEN * MAX_INTERP
                                let i0_base = (z0_base + cell_y_idx) * MAX_INTERP;
                                let i1_base = (z1_base + cell_y_idx) * MAX_INTERP;
                                let i0_next = i0_base + MAX_INTERP;
                                let i1_next = i1_base + MAX_INTERP;
                                let s0 = &*self.slices[cell_x_idx];
                                let s1 = &*self.slices[cell_x_idx + 1];
                                let factor_y_v = f64x4::splat(factor_y);
                                let factor_x_v = f64x4::splat(factor_x);
                                let factor_z_v = f64x4::splat(factor_z);

                                let mut ch_batch = 0;
                                while ch_batch + 4 <= interp_count {
                                    // SAFETY: ch_batch+3 < interp_count ≤ MAX_INTERP, all base indices in bounds.
                                    unsafe {
                                        let n000 = f64x4::from_slice(s0.get_unchecked(
                                            i0_base + ch_batch..i0_base + ch_batch + 4,
                                        ));
                                        let n001 = f64x4::from_slice(s0.get_unchecked(
                                            i1_base + ch_batch..i1_base + ch_batch + 4,
                                        ));
                                        let n100 = f64x4::from_slice(s1.get_unchecked(
                                            i0_base + ch_batch..i0_base + ch_batch + 4,
                                        ));
                                        let n101 = f64x4::from_slice(s1.get_unchecked(
                                            i1_base + ch_batch..i1_base + ch_batch + 4,
                                        ));
                                        let n010 = f64x4::from_slice(s0.get_unchecked(
                                            i0_next + ch_batch..i0_next + ch_batch + 4,
                                        ));
                                        let n011 = f64x4::from_slice(s0.get_unchecked(
                                            i1_next + ch_batch..i1_next + ch_batch + 4,
                                        ));
                                        let n110 = f64x4::from_slice(s1.get_unchecked(
                                            i0_next + ch_batch..i0_next + ch_batch + 4,
                                        ));
                                        let n111 = f64x4::from_slice(s1.get_unchecked(
                                            i1_next + ch_batch..i1_next + ch_batch + 4,
                                        ));

                                        let d00 = n000 + factor_y_v * (n010 - n000);
                                        let d10 = n100 + factor_y_v * (n110 - n100);
                                        let d01 = n001 + factor_y_v * (n011 - n001);
                                        let d11 = n101 + factor_y_v * (n111 - n101);
                                        let d0 = d00 + factor_x_v * (d10 - d00);
                                        let d1 = d01 + factor_x_v * (d11 - d01);
                                        let result = d0 + factor_z_v * (d1 - d0);
                                        let arr = result.to_array();
                                        let dst =
                                            interpolated.get_unchecked_mut(ch_batch..ch_batch + 4);
                                        dst.copy_from_slice(&arr);
                                    }
                                    ch_batch += 4;
                                }
                                // Scalar tail (when interp_count is not a multiple of 4).
                                while ch_batch < interp_count {
                                    let ch = ch_batch;
                                    // SAFETY: ch < interp_count ≤ MAX_INTERP; indices in bounds (see comment above).
                                    unsafe {
                                        let n000 = *s0.get_unchecked(i0_base + ch);
                                        let n001 = *s0.get_unchecked(i1_base + ch);
                                        let n100 = *s1.get_unchecked(i0_base + ch);
                                        let n101 = *s1.get_unchecked(i1_base + ch);
                                        let n010 = *s0.get_unchecked(i0_next + ch);
                                        let n011 = *s0.get_unchecked(i1_next + ch);
                                        let n110 = *s1.get_unchecked(i0_next + ch);
                                        let n111 = *s1.get_unchecked(i1_next + ch);

                                        let d00 = lerp(factor_y, n000, n010);
                                        let d10 = lerp(factor_y, n100, n110);
                                        let d01 = lerp(factor_y, n001, n011);
                                        let d11 = lerp(factor_y, n101, n111);
                                        let d0 = lerp(factor_x, d00, d10);
                                        let d1 = lerp(factor_x, d01, d11);
                                        *interpolated.get_unchecked_mut(ch) =
                                            lerp(factor_z, d0, d1);
                                    }
                                    ch_batch += 1;
                                }

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
                                    density += beard.compute(world_x, world_y, world_z);
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
}
