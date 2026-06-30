//! Graph walks over density function trees.
//!
//! Utilities for flatness inference (`uses_y`, `is_flat_cached`), reference
//! collection, and detecting `Interpolated` / `BlendedNoise` markers transitively
//! (used to decide inlining vs function calls during codegen).

use std::collections::{BTreeMap, BTreeSet};

use crate::density::{CubicSpline, DensityFunction, MarkerType, SplineValue};

use super::fingerprint::fingerprint;

/// Check if a density function subtree directly uses the `y` coordinate.
/// Does NOT recurse into References (those are handled by the flat inference loop).
pub(super) fn uses_y(df: &DensityFunction) -> bool {
    match df {
        // uses y * 0.25
        DensityFunction::YClampedGradient(_)
        | DensityFunction::Shift(_)
        | DensityFunction::BlendedNoise(_) => true,
        DensityFunction::Noise(n) => n.y_scale != 0.0,
        DensityFunction::ShiftedNoise(sn) => sn.y_scale != 0.0 || uses_y(&sn.shift_y),
        DensityFunction::WeirdScaledSampler(ws) => uses_y(&ws.input),
        DensityFunction::TwoArgumentSimple(t) => uses_y(&t.argument1) || uses_y(&t.argument2),
        DensityFunction::Mapped(m) => uses_y(&m.input),
        DensityFunction::Clamp(c) => uses_y(&c.input),
        DensityFunction::RangeChoice(rc) => {
            uses_y(&rc.input) || uses_y(&rc.when_in_range) || uses_y(&rc.when_out_of_range)
        }
        DensityFunction::IntervalSelect(interval) => {
            uses_y(&interval.input) || interval.functions.iter().any(|function| uses_y(function))
        }
        DensityFunction::BlendDensity(bd) => uses_y(&bd.input),
        DensityFunction::Marker(m) => uses_y(&m.wrapped),
        DensityFunction::Spline(s) => uses_y_spline(&s.spline),
        // These don't use Y:
        // - FindTopSurface scans Y internally but result only depends on (x, z)
        // - References are handled at the analysis level
        // - Constants, shifts, blend, and end-islands are Y-independent
        DensityFunction::FindTopSurface(_)
        | DensityFunction::Reference(_)
        | DensityFunction::Constant(_)
        | DensityFunction::ShiftA(_)
        | DensityFunction::ShiftB(_)
        | DensityFunction::BlendAlpha(_)
        | DensityFunction::BlendOffset(_)
        | DensityFunction::EndIslands => false,
    }
}

pub(super) fn uses_y_spline(spline: &CubicSpline) -> bool {
    if uses_y(&spline.coordinate) {
        return true;
    }
    spline.points.iter().any(|p| {
        if let SplineValue::Spline(nested) = &p.value {
            uses_y_spline(nested)
        } else {
            false
        }
    })
}

pub(super) const fn is_flat_cached(df: &DensityFunction) -> bool {
    match df {
        DensityFunction::Marker(m) => matches!(m.kind, MarkerType::FlatCache | MarkerType::Cache2D),
        _ => false,
    }
}

pub(super) fn unwrap_markers(df: &DensityFunction) -> &DensityFunction {
    match df {
        DensityFunction::Marker(m) => unwrap_markers(&m.wrapped),
        other => other,
    }
}

pub(super) fn collect_references(df: &DensityFunction) -> Vec<String> {
    let mut refs = Vec::new();
    collect_refs_inner(df, &mut refs);
    refs
}

pub(super) fn collect_refs_inner(df: &DensityFunction, refs: &mut Vec<String>) {
    match df {
        DensityFunction::Reference(r) if !refs.contains(&r.id) => {
            refs.push(r.id.clone());
        }
        DensityFunction::Marker(m) => collect_refs_inner(&m.wrapped, refs),
        DensityFunction::TwoArgumentSimple(t) => {
            collect_refs_inner(&t.argument1, refs);
            collect_refs_inner(&t.argument2, refs);
        }
        DensityFunction::Mapped(m) => collect_refs_inner(&m.input, refs),
        DensityFunction::Clamp(c) => collect_refs_inner(&c.input, refs),
        DensityFunction::RangeChoice(rc) => {
            collect_refs_inner(&rc.input, refs);
            collect_refs_inner(&rc.when_in_range, refs);
            collect_refs_inner(&rc.when_out_of_range, refs);
        }
        DensityFunction::IntervalSelect(interval) => {
            collect_refs_inner(&interval.input, refs);
            for function in &interval.functions {
                collect_refs_inner(function, refs);
            }
        }
        DensityFunction::ShiftedNoise(sn) => {
            collect_refs_inner(&sn.shift_x, refs);
            collect_refs_inner(&sn.shift_y, refs);
            collect_refs_inner(&sn.shift_z, refs);
        }
        DensityFunction::BlendDensity(bd) => collect_refs_inner(&bd.input, refs),
        DensityFunction::WeirdScaledSampler(ws) => collect_refs_inner(&ws.input, refs),
        DensityFunction::Spline(s) => collect_spline_refs(&s.spline, refs),
        DensityFunction::FindTopSurface(fts) => {
            collect_refs_inner(&fts.density, refs);
            collect_refs_inner(&fts.upper_bound, refs);
        }
        _ => {}
    }
}

/// Recursively collect `Noise` nodes with `y_scale == 0.0` in a density
/// function tree. These are Y-independent computations that can be cached
/// per (x, z) column. Keyed by structural hash → `(noise_id, xz_scale)`.
pub(super) fn collect_inline_flat_noises(
    df: &DensityFunction,
    out: &mut BTreeMap<u64, (String, f64)>,
) {
    if let DensityFunction::Noise(n) = df
        && n.y_scale == 0.0
    {
        let fp = fingerprint(df);
        out.entry(fp)
            .or_insert_with(|| (n.noise_id.clone(), n.xz_scale));
    }
    // Recurse into children (but NOT into References — those are separate functions)
    match df {
        DensityFunction::TwoArgumentSimple(t) => {
            collect_inline_flat_noises(&t.argument1, out);
            collect_inline_flat_noises(&t.argument2, out);
        }
        DensityFunction::Mapped(m) => collect_inline_flat_noises(&m.input, out),
        DensityFunction::Clamp(c) => collect_inline_flat_noises(&c.input, out),
        DensityFunction::RangeChoice(rc) => {
            collect_inline_flat_noises(&rc.input, out);
            collect_inline_flat_noises(&rc.when_in_range, out);
            collect_inline_flat_noises(&rc.when_out_of_range, out);
        }
        DensityFunction::IntervalSelect(interval) => {
            collect_inline_flat_noises(&interval.input, out);
            for function in &interval.functions {
                collect_inline_flat_noises(function, out);
            }
        }
        DensityFunction::WeirdScaledSampler(ws) => collect_inline_flat_noises(&ws.input, out),
        DensityFunction::BlendDensity(bd) => collect_inline_flat_noises(&bd.input, out),
        DensityFunction::Marker(m) => collect_inline_flat_noises(&m.wrapped, out),
        DensityFunction::ShiftedNoise(sn) => {
            collect_inline_flat_noises(&sn.shift_x, out);
            collect_inline_flat_noises(&sn.shift_y, out);
            collect_inline_flat_noises(&sn.shift_z, out);
        }
        _ => {}
    }
}
pub(super) fn collect_spline_refs(spline: &CubicSpline, refs: &mut Vec<String>) {
    collect_refs_inner(&spline.coordinate, refs);
    for point in &spline.points {
        if let SplineValue::Spline(nested) = &point.value {
            collect_spline_refs(nested, refs);
        }
    }
}

/// Collect the inner functions of all `Interpolated` markers in DFS order,
/// resolving references through the registry.
///
/// The DFS order must match `gen_expr` with `interpolated_param_mode` so that
/// the indices align between `fill_cell_corner_densities` and `combine_interpolated`.
pub(super) fn collect_interpolated_inners(
    df: &DensityFunction,
    registry: &BTreeMap<String, DensityFunction>,
) -> Vec<DensityFunction> {
    let mut inners = Vec::new();
    collect_interpolated_walk(df, registry, &mut inners);
    inners
}

pub(super) fn collect_interpolated_walk(
    df: &DensityFunction,
    registry: &BTreeMap<String, DensityFunction>,
    inners: &mut Vec<DensityFunction>,
) {
    match df {
        DensityFunction::Marker(m) if m.kind == MarkerType::Interpolated => {
            // Collect the inner function; do NOT recurse into it
            inners.push((*m.wrapped).clone());
        }
        DensityFunction::Marker(m) => collect_interpolated_walk(&m.wrapped, registry, inners),
        DensityFunction::TwoArgumentSimple(t) => {
            collect_interpolated_walk(&t.argument1, registry, inners);
            collect_interpolated_walk(&t.argument2, registry, inners);
        }
        DensityFunction::Mapped(m) => collect_interpolated_walk(&m.input, registry, inners),
        DensityFunction::Clamp(c) => collect_interpolated_walk(&c.input, registry, inners),
        DensityFunction::RangeChoice(rc) => {
            collect_interpolated_walk(&rc.input, registry, inners);
            collect_interpolated_walk(&rc.when_in_range, registry, inners);
            collect_interpolated_walk(&rc.when_out_of_range, registry, inners);
        }
        DensityFunction::IntervalSelect(interval) => {
            collect_interpolated_walk(&interval.input, registry, inners);
            for function in &interval.functions {
                collect_interpolated_walk(function, registry, inners);
            }
        }
        DensityFunction::BlendDensity(bd) => {
            collect_interpolated_walk(&bd.input, registry, inners);
        }
        DensityFunction::WeirdScaledSampler(ws) => {
            collect_interpolated_walk(&ws.input, registry, inners);
        }
        DensityFunction::Spline(s) => {
            collect_interpolated_spline_walk(&s.spline, registry, inners);
        }
        DensityFunction::ShiftedNoise(sn) => {
            collect_interpolated_walk(&sn.shift_x, registry, inners);
            collect_interpolated_walk(&sn.shift_y, registry, inners);
            collect_interpolated_walk(&sn.shift_z, registry, inners);
        }
        DensityFunction::FindTopSurface(fts) => {
            collect_interpolated_walk(&fts.density, registry, inners);
            collect_interpolated_walk(&fts.upper_bound, registry, inners);
        }
        DensityFunction::Reference(r) => {
            if let Some(ref_df) = registry.get(&r.id) {
                collect_interpolated_walk(ref_df, registry, inners);
            }
        }
        _ => {}
    }
}

pub(super) fn collect_interpolated_spline_walk(
    spline: &CubicSpline,
    registry: &BTreeMap<String, DensityFunction>,
    inners: &mut Vec<DensityFunction>,
) {
    collect_interpolated_walk(&spline.coordinate, registry, inners);
    for point in &spline.points {
        if let SplineValue::Spline(nested) = &point.value {
            collect_interpolated_spline_walk(nested, registry, inners);
        }
    }
}

/// Check if a density function tree transitively contains `BlendedNoise`.
pub(super) fn has_blended_noise(
    df: &DensityFunction,
    registry: &BTreeMap<String, DensityFunction>,
    visited: &mut BTreeSet<String>,
) -> bool {
    match df {
        DensityFunction::BlendedNoise(_) => true,
        DensityFunction::TwoArgumentSimple(t) => {
            has_blended_noise(&t.argument1, registry, visited)
                || has_blended_noise(&t.argument2, registry, visited)
        }
        DensityFunction::Mapped(m) => has_blended_noise(&m.input, registry, visited),
        DensityFunction::Clamp(c) => has_blended_noise(&c.input, registry, visited),
        DensityFunction::Marker(m) => has_blended_noise(&m.wrapped, registry, visited),
        DensityFunction::RangeChoice(rc) => {
            has_blended_noise(&rc.input, registry, visited)
                || has_blended_noise(&rc.when_in_range, registry, visited)
                || has_blended_noise(&rc.when_out_of_range, registry, visited)
        }
        DensityFunction::IntervalSelect(interval) => {
            has_blended_noise(&interval.input, registry, visited)
                || interval
                    .functions
                    .iter()
                    .any(|function| has_blended_noise(function, registry, visited))
        }
        DensityFunction::BlendDensity(bd) => has_blended_noise(&bd.input, registry, visited),
        DensityFunction::WeirdScaledSampler(ws) => has_blended_noise(&ws.input, registry, visited),
        DensityFunction::ShiftedNoise(sn) => {
            has_blended_noise(&sn.shift_x, registry, visited)
                || has_blended_noise(&sn.shift_y, registry, visited)
                || has_blended_noise(&sn.shift_z, registry, visited)
        }
        DensityFunction::FindTopSurface(fts) => {
            has_blended_noise(&fts.density, registry, visited)
                || has_blended_noise(&fts.upper_bound, registry, visited)
        }
        DensityFunction::Spline(s) => has_blended_noise_spline(&s.spline, registry, visited),
        DensityFunction::Reference(r) => {
            if visited.contains(&r.id) {
                return false;
            }
            visited.insert(r.id.clone());
            registry
                .get(&r.id)
                .is_some_and(|ref_df| has_blended_noise(ref_df, registry, visited))
        }
        _ => false,
    }
}

pub(super) fn has_blended_noise_spline(
    spline: &CubicSpline,
    registry: &BTreeMap<String, DensityFunction>,
    visited: &mut BTreeSet<String>,
) -> bool {
    if has_blended_noise(&spline.coordinate, registry, visited) {
        return true;
    }
    spline.points.iter().any(|p| {
        if let SplineValue::Spline(nested) = &p.value {
            has_blended_noise_spline(nested, registry, visited)
        } else {
            false
        }
    })
}

/// Check if a named function (transitively) contains `Interpolated` markers.
pub(super) fn has_interpolated_markers(
    df: &DensityFunction,
    registry: &BTreeMap<String, DensityFunction>,
    visited: &mut BTreeSet<String>,
) -> bool {
    match df {
        DensityFunction::Marker(m) if m.kind == MarkerType::Interpolated => true,
        DensityFunction::Marker(m) => has_interpolated_markers(&m.wrapped, registry, visited),
        DensityFunction::TwoArgumentSimple(t) => {
            has_interpolated_markers(&t.argument1, registry, visited)
                || has_interpolated_markers(&t.argument2, registry, visited)
        }
        DensityFunction::Mapped(m) => has_interpolated_markers(&m.input, registry, visited),
        DensityFunction::Clamp(c) => has_interpolated_markers(&c.input, registry, visited),
        DensityFunction::RangeChoice(rc) => {
            has_interpolated_markers(&rc.input, registry, visited)
                || has_interpolated_markers(&rc.when_in_range, registry, visited)
                || has_interpolated_markers(&rc.when_out_of_range, registry, visited)
        }
        DensityFunction::IntervalSelect(interval) => {
            has_interpolated_markers(&interval.input, registry, visited)
                || interval
                    .functions
                    .iter()
                    .any(|function| has_interpolated_markers(function, registry, visited))
        }
        DensityFunction::BlendDensity(bd) => has_interpolated_markers(&bd.input, registry, visited),
        DensityFunction::WeirdScaledSampler(ws) => {
            has_interpolated_markers(&ws.input, registry, visited)
        }
        DensityFunction::ShiftedNoise(sn) => {
            has_interpolated_markers(&sn.shift_x, registry, visited)
                || has_interpolated_markers(&sn.shift_y, registry, visited)
                || has_interpolated_markers(&sn.shift_z, registry, visited)
        }
        DensityFunction::FindTopSurface(fts) => {
            has_interpolated_markers(&fts.density, registry, visited)
                || has_interpolated_markers(&fts.upper_bound, registry, visited)
        }
        DensityFunction::Reference(r) => {
            if visited.contains(&r.id) {
                return false;
            }
            visited.insert(r.id.clone());
            registry
                .get(&r.id)
                .is_some_and(|ref_df| has_interpolated_markers(ref_df, registry, visited))
        }
        // Splines could contain interpolated markers in theory
        DensityFunction::Spline(s) => has_interpolated_spline(&s.spline, registry, visited),
        _ => false,
    }
}

pub(super) fn has_interpolated_spline(
    spline: &CubicSpline,
    registry: &BTreeMap<String, DensityFunction>,
    visited: &mut BTreeSet<String>,
) -> bool {
    if has_interpolated_markers(&spline.coordinate, registry, visited) {
        return true;
    }
    spline.points.iter().any(|p| {
        if let SplineValue::Spline(nested) = &p.value {
            has_interpolated_spline(nested, registry, visited)
        } else {
            false
        }
    })
}
