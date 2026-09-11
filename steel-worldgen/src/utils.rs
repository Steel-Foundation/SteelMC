use crate::density::traits::ColumnCache;
use crate::density::traits::NoiseSettings;
use crate::{
    density::DimensionNoises,
    noise::{Aquifer, AquiferResult, preliminary_surface_level},
};

#[inline]
#[expect(
    clippy::similar_names,
    reason = "the names mirror Vanilla's interpolation corners"
)]
#[expect(
    clippy::too_many_arguments,
    reason = "the scalar interpolation helper mirrors Vanilla's eight corners"
)]
fn interpolate_cell_value(
    factor_x: f32,
    factor_z: f32,
    y_in_cell: i32,
    cell_height: i32,
    v000: f32,
    v100: f32,
    v010: f32,
    v110: f32,
    v001: f32,
    v101: f32,
    v011: f32,
    v111: f32,
) -> f32 {
    let v00 = v000 + factor_z * (v001 - v000);
    let v01 = v010 + factor_z * (v011 - v010);
    let v10 = v100 + factor_z * (v101 - v100);
    let v11 = v110 + factor_z * (v111 - v110);
    let v0 = v00 + factor_x * (v10 - v00);
    let v1 = v01 + factor_x * (v11 - v01);
    let step = (v1 - v0) * (1.0_f32 / cell_height as f32);
    let mut value = v0;
    for _ in 0..y_in_cell {
        value += step;
    }
    value
}

/// `getBaseHeight(WORLD_SURFACE_WG)`-compatible height scan. Uses
/// `preliminary_surface_level + 16` as an upper bound to avoid scanning empty
/// upper atmosphere, and the cell-based iterator so 8-corner density
/// evaluations are shared across Y values in each cell.
///
/// Exposed for `GenerationContext`.
pub(crate) fn column_base_height<N: DimensionNoises>(
    cache: &mut N::ColumnCache,
    noises: &N,
    aquifer: &mut Aquifer<N>,
    x: i32,
    z: i32,
    ocean_floor: bool,
) -> i32 {
    let estimate = preliminary_surface_level::<N>(noises, cache, x, z);
    let max_y = (estimate + 16).min(N::Settings::MIN_Y + N::Settings::HEIGHT - 1);
    iterate_noise_column_capped::<N>(cache, noises, aquifer, x, z, max_y, ocean_floor)
}
/// Single-point `getInterpolatedDensity`. Exposed for `GenerationContext`.
pub(crate) fn column_interpolated_density<N: DimensionNoises>(
    cache: &mut N::ColumnCache,
    noises: &N,
    x: i32,
    y: i32,
    z: i32,
    cell_w: i32,
    cell_h: i32,
) -> f64 {
    interpolated_density::<N>(cache, noises, x, y, z, cell_w, cell_h)
}
/// Finds the highest solid block below air in a single base-noise column.
///
/// Used by structure placement probes such as nether fossils. This preserves the
/// same base terrain classification as repeated `column_state` calls, but shares
/// the eight cell-corner density evaluations across adjacent Y positions.
#[expect(
    clippy::too_many_lines,
    reason = "keeps the vanilla density interpolation flow in one readable pass"
)]
pub(crate) fn find_solid_block_below_air<N: DimensionNoises>(
    cache: &mut N::ColumnCache,
    noises: &N,
    aquifer: &mut Aquifer<N>,
    block_x: i32,
    block_z: i32,
    start_y: i32,
    min_solid_y: i32,
) -> Option<i32> {
    const MAX_INTERP: usize = 16;

    if start_y <= min_solid_y {
        return None;
    }

    let cell_w = N::Settings::CELL_WIDTH;
    let cell_h = N::Settings::CELL_HEIGHT;
    let min_y = N::Settings::MIN_Y;
    let height = N::Settings::HEIGHT;
    let cell_min_y = min_y.div_euclid(cell_h);
    let cell_count_y = height.div_euclid(cell_h);

    let cell_x = block_x.div_euclid(cell_w);
    let cell_z = block_z.div_euclid(cell_w);
    let factor_x = block_x.rem_euclid(cell_w) as f32 / cell_w as f32;
    let factor_z = block_z.rem_euclid(cell_w) as f32 / cell_w as f32;
    let x0 = cell_x * cell_w;
    let x1 = x0 + cell_w;
    let z0 = cell_z * cell_w;
    let z1 = z0 + cell_w;

    let interp_count = N::interpolated_count();

    let mut c000 = [0.0_f32; MAX_INTERP];
    let mut c100 = [0.0_f32; MAX_INTERP];
    let mut c010 = [0.0_f32; MAX_INTERP];
    let mut c110 = [0.0_f32; MAX_INTERP];
    let mut c001 = [0.0_f32; MAX_INTERP];
    let mut c101 = [0.0_f32; MAX_INTERP];
    let mut c011 = [0.0_f32; MAX_INTERP];
    let mut c111 = [0.0_f32; MAX_INTERP];
    let mut interpolated = [0.0_f32; MAX_INTERP];

    macro_rules! fill {
        ($out:expr, $ex:expr, $ey:expr, $ez:expr, $blended:expr) => {{
            cache.ensure($ex, $ez, noises);
            noises.fill_cell_corner_densities(
                &mut *cache,
                $ex,
                $ey,
                $ez,
                $blended,
                &mut $out[..interp_count],
            );
        }};
    }

    let max_cell_y_idx = {
        let raw = start_y.div_euclid(cell_h) - cell_min_y;
        raw.clamp(0, cell_count_y - 1)
    };
    let min_cell_y_idx = {
        let raw = min_solid_y.div_euclid(cell_h) - cell_min_y;
        raw.clamp(0, cell_count_y - 1)
    };

    let mut above_is_air = false;
    let mut have_above = false;
    let mut blended_scratch = [0.0_f32; 2];

    for cell_y_idx in (min_cell_y_idx..=max_cell_y_idx).rev() {
        let y0 = (cell_min_y + cell_y_idx) * cell_h;
        let y1 = y0 + cell_h;
        let ys = [y0, y1];

        noises.compute_noise_column(x0, &ys, z0, &mut blended_scratch);
        let b000 = blended_scratch[0];
        let b010 = blended_scratch[1];
        noises.compute_noise_column(x1, &ys, z0, &mut blended_scratch);
        let b100 = blended_scratch[0];
        let b110 = blended_scratch[1];
        noises.compute_noise_column(x0, &ys, z1, &mut blended_scratch);
        let b001 = blended_scratch[0];
        let b011 = blended_scratch[1];
        noises.compute_noise_column(x1, &ys, z1, &mut blended_scratch);
        let b101 = blended_scratch[0];
        let b111 = blended_scratch[1];

        fill!(c000, x0, y0, z0, b000);
        fill!(c100, x1, y0, z0, b100);
        fill!(c010, x0, y1, z0, b010);
        fill!(c110, x1, y1, z0, b110);
        fill!(c001, x0, y0, z1, b001);
        fill!(c101, x1, y0, z1, b101);
        fill!(c011, x0, y1, z1, b011);
        fill!(c111, x1, y1, z1, b111);

        let top_y_in_cell = if cell_y_idx == max_cell_y_idx {
            (start_y - y0).clamp(0, cell_h - 1)
        } else {
            cell_h - 1
        };
        let bottom_y_in_cell = if cell_y_idx == min_cell_y_idx {
            (min_solid_y - y0).clamp(0, cell_h - 1)
        } else {
            0
        };

        for y_in_cell in (bottom_y_in_cell..=top_y_in_cell).rev() {
            let pos_y = y0 + y_in_cell;
            for ch in 0..interp_count {
                interpolated[ch] = interpolate_cell_value(
                    factor_x, factor_z, y_in_cell, cell_h, c000[ch], c100[ch], c010[ch], c110[ch],
                    c001[ch], c101[ch], c011[ch], c111[ch],
                );
            }

            let density = noises.combine_interpolated(
                &mut *cache,
                &interpolated[..interp_count],
                0,
                pos_y,
                0,
            );
            let state =
                aquifer.compute_substance(noises, block_x, pos_y, block_z, f64::from(density));
            let is_air = matches!(state, AquiferResult::Air);
            let is_solid = matches!(state, AquiferResult::Solid);

            if have_above && above_is_air && is_solid {
                return Some(pos_y);
            }

            above_is_air = is_air;
            have_above = true;
        }
    }

    None
}
/// Matches vanilla's `iterateNoiseColumn`: iterates by Y cells, evaluating
/// inner density functions at 8 cell corners, trilinearly interpolating each
/// channel independently, then applying outer operations (squeeze, min, etc.)
/// per-block via `combine_interpolated`.
///
/// Returns getBaseHeight (= getFirstFreeHeight = first Y above surface).
pub(crate) fn iterate_noise_column_with_aquifer<N: DimensionNoises>(
    cache: &mut N::ColumnCache,
    noises: &N,
    aquifer: &mut Aquifer<N>,
    block_x: i32,
    block_z: i32,
    ocean_floor: bool,
) -> i32 {
    let max_y = N::Settings::MIN_Y + N::Settings::HEIGHT - 1;
    iterate_noise_column_capped::<N>(cache, noises, aquifer, block_x, block_z, max_y, ocean_floor)
}

/// Same as `iterate_noise_column_with_aquifer` but only scans Y values up to
/// `max_y_inclusive`. Used by `base_height` with an estimate from
/// `preliminary_surface_level + 16` to skip empty upper atmosphere — reducing
/// cell-corner density evaluations from O(height) to `O(estimate_depth)`.
#[expect(
    clippy::too_many_lines,
    reason = "inlines 8-corner density buffers + interpolation to match vanilla's iterateNoiseColumn fast path"
)]
fn iterate_noise_column_capped<N: DimensionNoises>(
    cache: &mut N::ColumnCache,
    noises: &N,
    aquifer: &mut Aquifer<N>,
    block_x: i32,
    block_z: i32,
    max_y_inclusive: i32,
    ocean_floor: bool,
) -> i32 {
    // Corner channel buffers for 8 cell corners
    const MAX_INTERP: usize = 16;

    let cell_w = N::Settings::CELL_WIDTH;
    let cell_h = N::Settings::CELL_HEIGHT;
    let min_y = N::Settings::MIN_Y;
    let height = N::Settings::HEIGHT;
    let cell_min_y = min_y.div_euclid(cell_h);
    let cell_count_y = height.div_euclid(cell_h);

    let cell_x = block_x.div_euclid(cell_w);
    let cell_z = block_z.div_euclid(cell_w);
    let factor_x = block_x.rem_euclid(cell_w) as f32 / cell_w as f32;
    let factor_z = block_z.rem_euclid(cell_w) as f32 / cell_w as f32;
    let x0 = cell_x * cell_w;
    let x1 = x0 + cell_w;
    let z0 = cell_z * cell_w;
    let z1 = z0 + cell_w;

    let interp_count = N::interpolated_count();

    let mut c000 = [0.0_f32; MAX_INTERP];
    let mut c100 = [0.0_f32; MAX_INTERP];
    let mut c010 = [0.0_f32; MAX_INTERP];
    let mut c110 = [0.0_f32; MAX_INTERP];
    let mut c001 = [0.0_f32; MAX_INTERP];
    let mut c101 = [0.0_f32; MAX_INTERP];
    let mut c011 = [0.0_f32; MAX_INTERP];
    let mut c111 = [0.0_f32; MAX_INTERP];
    let mut interpolated = [0.0_f32; MAX_INTERP];

    macro_rules! fill {
        ($out:expr, $ex:expr, $ey:expr, $ez:expr, $blended:expr) => {{
            cache.ensure($ex, $ez, noises);
            noises.fill_cell_corner_densities(
                &mut *cache,
                $ex,
                $ey,
                $ez,
                $blended,
                &mut $out[..interp_count],
            );
        }};
    }

    // Topmost cell containing max_y_inclusive.
    let max_cell_y_idx = {
        let raw = max_y_inclusive.div_euclid(cell_h) - cell_min_y;
        raw.clamp(0, cell_count_y - 1)
    };
    // Top Y-within-cell for the topmost cell.
    let top_cell_top_y_in_cell =
        (max_y_inclusive - (cell_min_y + max_cell_y_idx) * cell_h).clamp(0, cell_h - 1);

    // Precompute blended noise per corner (x, z) × two Y levels per cell.
    let mut blended_scratch = [0.0_f32; 2];
    for cell_y_idx in (0..=max_cell_y_idx).rev() {
        let y0 = (cell_min_y + cell_y_idx) * cell_h;
        let y1 = y0 + cell_h;
        let ys = [y0, y1];

        // `compute_noise_column` gives us the blended noise values at (x0,z0),
        // (x1,z0), (x0,z1), (x1,z1) for this Y pair. Query each corner once.
        noises.compute_noise_column(x0, &ys, z0, &mut blended_scratch);
        let b000 = blended_scratch[0];
        let b010 = blended_scratch[1];
        noises.compute_noise_column(x1, &ys, z0, &mut blended_scratch);
        let b100 = blended_scratch[0];
        let b110 = blended_scratch[1];
        noises.compute_noise_column(x0, &ys, z1, &mut blended_scratch);
        let b001 = blended_scratch[0];
        let b011 = blended_scratch[1];
        noises.compute_noise_column(x1, &ys, z1, &mut blended_scratch);
        let b101 = blended_scratch[0];
        let b111 = blended_scratch[1];

        // Evaluate inner functions at 8 cell corners (all channels)
        fill!(c000, x0, y0, z0, b000);
        fill!(c100, x1, y0, z0, b100);
        fill!(c010, x0, y1, z0, b010);
        fill!(c110, x1, y1, z0, b110);
        fill!(c001, x0, y0, z1, b001);
        fill!(c101, x1, y0, z1, b101);
        fill!(c011, x0, y1, z1, b011);
        fill!(c111, x1, y1, z1, b111);

        // For the topmost cell, start from the Y within cell that corresponds
        // to `max_y_inclusive`. For lower cells, start at cell_h - 1.
        let top_y_in_cell = if cell_y_idx == max_cell_y_idx {
            top_cell_top_y_in_cell
        } else {
            cell_h - 1
        };

        // Iterate Y within cell from top to bottom
        for y_in_cell in (0..=top_y_in_cell).rev() {
            let pos_y = (cell_min_y + cell_y_idx) * cell_h + y_in_cell;
            // Trilinearly interpolate each channel independently
            for ch in 0..interp_count {
                interpolated[ch] = interpolate_cell_value(
                    factor_x, factor_z, y_in_cell, cell_h, c000[ch], c100[ch], c010[ch], c110[ch],
                    c001[ch], c101[ch], c011[ch], c111[ch],
                );
            }

            // Apply outer operations (squeeze, min, etc.) per-block
            let density = noises.combine_interpolated(
                &mut *cache,
                &interpolated[..interp_count],
                0,
                pos_y,
                0,
            );

            // Use aquifer to determine block state (matches vanilla's getInterpolatedState)
            let opaque = match aquifer.compute_substance(
                noises,
                block_x,
                pos_y,
                block_z,
                f64::from(density),
            ) {
                AquiferResult::Solid => true,
                AquiferResult::Fluid(_) => !ocean_floor,
                AquiferResult::Air => false,
            };

            if opaque {
                return pos_y + 1;
            }
        }
    }
    min_y
}

/// Evaluate terrain density at a single block position using cell-based
/// interpolation matching vanilla's `NoiseChunk`: inner functions at 8 cell
/// corners, trilinear interpolation per channel, then outer operations.
fn interpolated_density<N: DimensionNoises>(
    cache: &mut N::ColumnCache,
    noises: &N,
    x: i32,
    y: i32,
    z: i32,
    cell_w: i32,
    cell_h: i32,
) -> f64 {
    const MAX_INTERP: usize = 16;

    let cx = x.div_euclid(cell_w);
    let cy = y.div_euclid(cell_h);
    let cz = z.div_euclid(cell_w);
    let fx = x.rem_euclid(cell_w) as f32 / cell_w as f32;
    let fz = z.rem_euclid(cell_w) as f32 / cell_w as f32;

    let x0 = cx * cell_w;
    let x1 = x0 + cell_w;
    let y0 = cy * cell_h;
    let y1 = y0 + cell_h;
    let z0 = cz * cell_w;
    let z1 = z0 + cell_w;

    let interp_count = N::interpolated_count();

    let mut c000 = [0.0_f32; MAX_INTERP];
    let mut c100 = [0.0_f32; MAX_INTERP];
    let mut c010 = [0.0_f32; MAX_INTERP];
    let mut c110 = [0.0_f32; MAX_INTERP];
    let mut c001 = [0.0_f32; MAX_INTERP];
    let mut c101 = [0.0_f32; MAX_INTERP];
    let mut c011 = [0.0_f32; MAX_INTERP];
    let mut c111 = [0.0_f32; MAX_INTERP];
    let mut interpolated = [0.0_f32; MAX_INTERP];

    macro_rules! fill {
        ($out:expr, $ex:expr, $ey:expr, $ez:expr, $blended:expr) => {{
            cache.ensure($ex, $ez, noises);
            noises.fill_cell_corner_densities(
                &mut *cache,
                $ex,
                $ey,
                $ez,
                $blended,
                &mut $out[..interp_count],
            );
        }};
    }

    // Precompute blended noise at each corner (x, z) for the two cell Y levels.
    let ys = [y0, y1];
    let mut blended_scratch = [0.0_f32; 2];
    noises.compute_noise_column(x0, &ys, z0, &mut blended_scratch);
    let (b000, b010) = (blended_scratch[0], blended_scratch[1]);
    noises.compute_noise_column(x1, &ys, z0, &mut blended_scratch);
    let (b100, b110) = (blended_scratch[0], blended_scratch[1]);
    noises.compute_noise_column(x0, &ys, z1, &mut blended_scratch);
    let (b001, b011) = (blended_scratch[0], blended_scratch[1]);
    noises.compute_noise_column(x1, &ys, z1, &mut blended_scratch);
    let (b101, b111) = (blended_scratch[0], blended_scratch[1]);

    fill!(c000, x0, y0, z0, b000);
    fill!(c100, x1, y0, z0, b100);
    fill!(c010, x0, y1, z0, b010);
    fill!(c110, x1, y1, z0, b110);
    fill!(c001, x0, y0, z1, b001);
    fill!(c101, x1, y0, z1, b101);
    fill!(c011, x0, y1, z1, b011);
    fill!(c111, x1, y1, z1, b111);

    for ch in 0..interp_count {
        interpolated[ch] = interpolate_cell_value(
            fx,
            fz,
            y.rem_euclid(cell_h),
            cell_h,
            c000[ch],
            c100[ch],
            c010[ch],
            c110[ch],
            c001[ch],
            c101[ch],
            c011[ch],
            c111[ch],
        );
    }

    f64::from(noises.combine_interpolated(&mut *cache, &interpolated[..interp_count], 0, y, 0))
}
