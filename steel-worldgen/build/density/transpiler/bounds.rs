//! Static bounds analysis for density function subtrees.
//!
//! Computes conservative `(lower, upper)` intervals used during codegen to elide
//! unreachable branches (for example `min`/`max` when one operand is already
//! bounded). Resolves `Reference` nodes through the build-time registry.

use crate::density::{DensityFunction, MappedType, TwoArgType};

use super::TranspilerInput;

/// Static (lower, upper) bounds for a density function subtree.
///
/// Returned bounds satisfy `lower <= eval(df) <= upper` at runtime for all
/// inputs the function can be sampled at. When tight bounds aren't derivable
/// (e.g., free-form noise with unknown amplitude product, or potentially
/// unbounded operations like reciprocal), the corresponding side is set to
/// `f64::NEG_INFINITY` / `f64::INFINITY` and downstream short-circuit
/// optimizations correctly fall through to the unconditional codegen.
///
/// Mirrors the static-bounds analysis used by C2ME's
/// `MaxShortNode`/`MinShortNode` rewriters, with one extension: we resolve
/// `Reference` nodes through the build-time registry so cross-function
/// bounds propagate.
pub(super) fn compute_bounds(df: &DensityFunction, input: &TranspilerInput) -> (f64, f64) {
    compute_bounds_inner(df, input, &mut Vec::new())
}

#[expect(
    clippy::too_many_lines,
    reason = "one match arm per DensityFunction variant; splitting the dispatch would obscure the per-variant bounds analysis"
)]
pub(super) fn compute_bounds_inner(
    df: &DensityFunction,
    input: &TranspilerInput,
    visiting: &mut Vec<String>,
) -> (f64, f64) {
    match df {
        DensityFunction::Constant(c) => (c.value, c.value),

        DensityFunction::Reference(r) => {
            // Avoid infinite recursion through self-referential cycles (shouldn't
            // happen in practice, but DF graphs are cycle-free only by convention).
            if visiting.iter().any(|n| n == &r.id) {
                return (f64::NEG_INFINITY, f64::INFINITY);
            }
            let Some(target) = input.registry.get(&r.id) else {
                return (f64::NEG_INFINITY, f64::INFINITY);
            };
            visiting.push(r.id.clone());
            let bounds = compute_bounds_inner(target, input, visiting);
            visiting.pop();
            bounds
        }

        DensityFunction::YClampedGradient(g) => {
            let lo = g.from_value.min(g.to_value);
            let hi = g.from_value.max(g.to_value);
            (lo, hi)
        }

        DensityFunction::Noise(_)
        | DensityFunction::ShiftedNoise(_)
        | DensityFunction::ShiftA(_)
        | DensityFunction::ShiftB(_)
        | DensityFunction::Shift(_)
        | DensityFunction::Spline(_)
        | DensityFunction::BlendedNoise(_) => (f64::NEG_INFINITY, f64::INFINITY),

        DensityFunction::TwoArgumentSimple(t) => {
            let (a_lo, a_hi) = compute_bounds_inner(&t.argument1, input, visiting);
            let (b_lo, b_hi) = compute_bounds_inner(&t.argument2, input, visiting);
            match t.op {
                TwoArgType::Add => (a_lo + b_lo, a_hi + b_hi),
                TwoArgType::Mul => {
                    // Interval arithmetic for sign-mixed multiplication.
                    let candidates = [a_lo * b_lo, a_lo * b_hi, a_hi * b_lo, a_hi * b_hi];
                    let mut lo = f64::INFINITY;
                    let mut hi = f64::NEG_INFINITY;
                    for c in candidates {
                        if c.is_nan() {
                            return (f64::NEG_INFINITY, f64::INFINITY);
                        }
                        if c < lo {
                            lo = c;
                        }
                        if c > hi {
                            hi = c;
                        }
                    }
                    (lo, hi)
                }
                TwoArgType::Min => (a_lo.min(b_lo), a_hi.min(b_hi)),
                TwoArgType::Max => (a_lo.max(b_lo), a_hi.max(b_hi)),
            }
        }

        DensityFunction::Mapped(m) => {
            let (lo, hi) = compute_bounds_inner(&m.input, input, visiting);
            match m.op {
                MappedType::Abs => {
                    if lo >= 0.0 {
                        (lo, hi)
                    } else if hi <= 0.0 {
                        (-hi, -lo)
                    } else {
                        (0.0, lo.abs().max(hi.abs()))
                    }
                }
                MappedType::Square => {
                    if lo >= 0.0 {
                        (lo * lo, hi * hi)
                    } else if hi <= 0.0 {
                        (hi * hi, lo * lo)
                    } else {
                        (0.0, (lo * lo).max(hi * hi))
                    }
                }
                MappedType::Cube => {
                    // x^3 is monotone over the whole real line, so endpoints suffice.
                    (lo * lo * lo, hi * hi * hi)
                }
                MappedType::HalfNegative => {
                    // `if v > 0 { v } else { v * 0.5 }` — monotone non-decreasing
                    // (slope 0.5 below 0, slope 1 above 0).
                    let map = |v: f64| if v > 0.0 { v } else { v * 0.5 };
                    (map(lo), map(hi))
                }
                MappedType::QuarterNegative => {
                    let map = |v: f64| if v > 0.0 { v } else { v * 0.25 };
                    (map(lo), map(hi))
                }
                MappedType::Invert => {
                    // 1/v is unbounded near 0; only safe if input doesn't straddle 0.
                    if lo > 0.0 || hi < 0.0 {
                        let a = 1.0 / lo;
                        let b = 1.0 / hi;
                        (a.min(b), a.max(b))
                    } else {
                        (f64::NEG_INFINITY, f64::INFINITY)
                    }
                }
                MappedType::Squeeze => {
                    // clamp(-1, 1) → c/2 - c³/24. Endpoints: -1/2 + 1/24, 1/2 - 1/24.
                    let map = |v: f64| {
                        let c = v.clamp(-1.0, 1.0);
                        c / 2.0 - c * c * c / 24.0
                    };
                    let lo_c = lo.clamp(-1.0, 1.0);
                    let hi_c = hi.clamp(-1.0, 1.0);
                    (map(lo_c), map(hi_c))
                }
            }
        }

        DensityFunction::Clamp(c) => (c.min, c.max),

        DensityFunction::RangeChoice(rc) => {
            let (in_lo, in_hi) = compute_bounds_inner(&rc.when_in_range, input, visiting);
            let (out_lo, out_hi) = compute_bounds_inner(&rc.when_out_of_range, input, visiting);
            (in_lo.min(out_lo), in_hi.max(out_hi))
        }

        DensityFunction::IntervalSelect(interval) => {
            let mut lo = f64::INFINITY;
            let mut hi = f64::NEG_INFINITY;
            for function in &interval.functions {
                let (function_lo, function_hi) = compute_bounds_inner(function, input, visiting);
                lo = lo.min(function_lo);
                hi = hi.max(function_hi);
            }
            if lo > hi {
                (f64::NEG_INFINITY, f64::INFINITY)
            } else {
                (lo, hi)
            }
        }

        DensityFunction::WeirdScaledSampler(_) => {
            // result = scale * noise.abs() where scale ∈ [0.5, 3.0] and
            // noise.abs() is non-negative. The upper bound is noise-parameter
            // dependent, so leave it unbounded for branch-elision purposes.
            (0.0, f64::INFINITY)
        }

        DensityFunction::EndIslands => (-100.0, 80.0),

        DensityFunction::BlendAlpha(_) => (1.0, 1.0),
        DensityFunction::BlendOffset(_) => (0.0, 0.0),
        DensityFunction::BlendDensity(bd) => compute_bounds_inner(&bd.input, input, visiting),

        DensityFunction::Marker(m) => compute_bounds_inner(&m.wrapped, input, visiting),

        DensityFunction::FindTopSurface(fts) => {
            // Returns a Y coordinate in [lower_bound, upper_bound rounded down].
            // upper_bound is itself a DF — its static upper bound caps the result.
            let (_, upper) = compute_bounds_inner(&fts.upper_bound, input, visiting);
            (f64::from(fts.lower_bound), upper)
        }
    }
}
