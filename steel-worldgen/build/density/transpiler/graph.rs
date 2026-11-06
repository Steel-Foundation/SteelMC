//! Graph walks over density function trees.
//!
//! Utilities for flatness inference (`uses_y`, `is_flat_cached`), reference
//! collection, and detecting nodes transitively with `contains_function`
//! (used to decide inlining vs function calls during codegen).

use std::collections::{BTreeMap, BTreeSet};

use crate::density::{Axis, CubicSpline, DensityFunction, MarkerType, SplineValue};

use super::fingerprint::fingerprint;

/// Check if a density function subtree directly uses the `y` coordinate.
/// Does NOT recurse into References (those are handled by the flat inference loop).
pub(super) fn uses_y(df: &DensityFunction) -> bool {
    match df {
        // uses y * 0.25
        DensityFunction::YClampedGradient(_)
        | DensityFunction::Shift(_)
        | DensityFunction::BlendedNoise(_)
        | DensityFunction::DistanceToPoint(_) => true,
        DensityFunction::Noise(n) => n.y_scale != 0.0,
        DensityFunction::ShiftedNoise(sn) => sn.y_scale != 0.0 || uses_y(&sn.shift_y),
        DensityFunction::WeirdScaledSampler(ws) => uses_y(&ws.input),
        DensityFunction::TwoArgumentSimple(t) => uses_y(&t.argument1) || uses_y(&t.argument2),
        DensityFunction::Lerp(l) => uses_y(&l.alpha) || uses_y(&l.first) || uses_y(&l.second),
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
        DensityFunction::Slice(s) => !matches!(s.axis, Axis::Y) && uses_y(&s.input),
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
        DensityFunction::Lerp(l) => {
            collect_refs_inner(&l.alpha, refs);
            collect_refs_inner(&l.first, refs);
            collect_refs_inner(&l.second, refs);
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
        DensityFunction::Slice(s) => collect_refs_inner(&s.input, refs),
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
        DensityFunction::Lerp(l) => {
            collect_inline_flat_noises(&l.alpha, out);
            collect_inline_flat_noises(&l.first, out);
            collect_inline_flat_noises(&l.second, out);
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
        DensityFunction::Slice(s) => collect_inline_flat_noises(&s.input, out),
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
        DensityFunction::Lerp(l) => {
            collect_interpolated_walk(&l.alpha, registry, inners);
            collect_interpolated_walk(&l.first, registry, inners);
            collect_interpolated_walk(&l.second, registry, inners);
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
        DensityFunction::Slice(s) => {
            collect_interpolated_walk(&s.input, registry, inners);
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

/// Check if a density function tree transitively contains a node matching `is_target`.
pub(super) fn contains_function(
    df: &DensityFunction,
    registry: &BTreeMap<String, DensityFunction>,
    visited: &mut BTreeSet<String>,
    is_target: fn(&DensityFunction) -> bool,
) -> bool {
    if is_target(df) {
        return true;
    }
    match df {
        DensityFunction::TwoArgumentSimple(t) => {
            contains_function(&t.argument1, registry, visited, is_target)
                || contains_function(&t.argument2, registry, visited, is_target)
        }
        DensityFunction::Lerp(l) => {
            contains_function(&l.alpha, registry, visited, is_target)
                || contains_function(&l.first, registry, visited, is_target)
                || contains_function(&l.second, registry, visited, is_target)
        }
        DensityFunction::Slice(sl) => contains_function(&sl.input, registry, visited, is_target),
        DensityFunction::Mapped(m) => contains_function(&m.input, registry, visited, is_target),
        DensityFunction::Clamp(c) => contains_function(&c.input, registry, visited, is_target),
        DensityFunction::Marker(m) => contains_function(&m.wrapped, registry, visited, is_target),
        DensityFunction::RangeChoice(rc) => {
            contains_function(&rc.input, registry, visited, is_target)
                || contains_function(&rc.when_in_range, registry, visited, is_target)
                || contains_function(&rc.when_out_of_range, registry, visited, is_target)
        }
        DensityFunction::IntervalSelect(interval) => {
            contains_function(&interval.input, registry, visited, is_target)
                || interval
                    .functions
                    .iter()
                    .any(|function| contains_function(function, registry, visited, is_target))
        }
        DensityFunction::BlendDensity(bd) => {
            contains_function(&bd.input, registry, visited, is_target)
        }
        DensityFunction::WeirdScaledSampler(ws) => {
            contains_function(&ws.input, registry, visited, is_target)
        }
        DensityFunction::ShiftedNoise(sn) => {
            contains_function(&sn.shift_x, registry, visited, is_target)
                || contains_function(&sn.shift_y, registry, visited, is_target)
                || contains_function(&sn.shift_z, registry, visited, is_target)
        }
        DensityFunction::FindTopSurface(fts) => {
            contains_function(&fts.density, registry, visited, is_target)
                || contains_function(&fts.upper_bound, registry, visited, is_target)
        }
        DensityFunction::Spline(s) => {
            contains_function_spline(&s.spline, registry, visited, is_target)
        }
        DensityFunction::Reference(r) => {
            if visited.contains(&r.id) {
                return false;
            }
            visited.insert(r.id.clone());
            registry
                .get(&r.id)
                .is_some_and(|ref_df| contains_function(ref_df, registry, visited, is_target))
        }
        _ => false,
    }
}

fn contains_function_spline(
    spline: &CubicSpline,
    registry: &BTreeMap<String, DensityFunction>,
    visited: &mut BTreeSet<String>,
    is_target: fn(&DensityFunction) -> bool,
) -> bool {
    if contains_function(&spline.coordinate, registry, visited, is_target) {
        return true;
    }
    spline.points.iter().any(|p| {
        if let SplineValue::Spline(nested) = &p.value {
            contains_function_spline(nested, registry, visited, is_target)
        } else {
            false
        }
    })
}
