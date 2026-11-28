//! Graph walks over density function trees.
//!
//! Utilities for flatness inference (`uses_y`, `is_flat_cached`), reference
//! collection, and detecting nodes transitively with `contains_function`
//! (used to decide inlining vs function calls during codegen).

use std::collections::{BTreeMap, BTreeSet};
use std::iter::once;

use crate::density::{Axis, CubicSpline, DensityFunction, MarkerType, SplineValue};

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
    if let DensityFunction::Reference(r) = df {
        if !refs.contains(&r.id) {
            refs.push(r.id.clone());
        }
        return;
    }
    for child in children(df) {
        collect_refs_inner(child, refs);
    }
}

/// The functions `df` evaluates directly, without following references. A spline's are
/// its coordinate and those of its nested splines, in depth-first order.
pub(super) fn children(df: &DensityFunction) -> Vec<&DensityFunction> {
    match df {
        DensityFunction::TwoArgumentSimple(t) => vec![&*t.argument1, &*t.argument2],
        DensityFunction::Lerp(l) => vec![&*l.alpha, &*l.first, &*l.second],
        DensityFunction::Mapped(m) => vec![&*m.input],
        DensityFunction::Clamp(c) => vec![&*c.input],
        DensityFunction::Marker(m) => vec![&*m.wrapped],
        DensityFunction::RangeChoice(rc) => {
            vec![&*rc.input, &*rc.when_in_range, &*rc.when_out_of_range]
        }
        DensityFunction::IntervalSelect(interval) => once(&*interval.input)
            .chain(interval.functions.iter().map(|function| &**function))
            .collect(),
        DensityFunction::BlendDensity(bd) => vec![&*bd.input],
        DensityFunction::WeirdScaledSampler(ws) => vec![&*ws.input],
        DensityFunction::ShiftedNoise(sn) => vec![&*sn.shift_x, &*sn.shift_y, &*sn.shift_z],
        DensityFunction::FindTopSurface(fts) => vec![&*fts.density, &*fts.upper_bound],
        DensityFunction::Slice(sl) => vec![&*sl.input],
        DensityFunction::Spline(s) => {
            let mut coordinates = Vec::new();
            push_spline_coordinates(&s.spline, &mut coordinates);
            coordinates
        }
        DensityFunction::Constant(_)
        | DensityFunction::Reference(_)
        | DensityFunction::YClampedGradient(_)
        | DensityFunction::Noise(_)
        | DensityFunction::ShiftA(_)
        | DensityFunction::ShiftB(_)
        | DensityFunction::Shift(_)
        | DensityFunction::BlendedNoise(_)
        | DensityFunction::EndIslands
        | DensityFunction::BlendAlpha(_)
        | DensityFunction::BlendOffset(_)
        | DensityFunction::DistanceToPoint(_) => Vec::new(),
    }
}

fn push_spline_coordinates<'a>(spline: &'a CubicSpline, out: &mut Vec<&'a DensityFunction>) {
    out.push(&spline.coordinate);
    for point in &spline.points {
        if let SplineValue::Spline(nested) = &point.value {
            push_spline_coordinates(nested, out);
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

pub(super) fn is_interpolated_marker(df: &DensityFunction) -> bool {
    matches!(df, DensityFunction::Marker(m) if m.kind == MarkerType::Interpolated)
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
    if let DensityFunction::Reference(r) = df {
        return visited.insert(r.id.clone())
            && registry
                .get(&r.id)
                .is_some_and(|ref_df| contains_function(ref_df, registry, visited, is_target));
    }
    children(df)
        .into_iter()
        .any(|child| contains_function(child, registry, visited, is_target))
}
