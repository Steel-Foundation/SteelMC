//! find channels that prove final density is not positive
//! check interpolated block values: repeated f32 additions can overshoot cell endpoints
//! only propagate signs through operations that preserve them in the generated code

use std::cmp::Reverse;
use std::collections::{BTreeMap, BTreeSet};

use crate::density::{DensityFunction, MappedType, MarkerType, TwoArgType};

use super::graph::collect_interpolated_inners;

/// How strongly an expression is known to be non-positive.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Strength {
    /// `!(v > 0.0)`: non-positive or NaN. Enough for the "density > 0 means
    /// solid" test, but a NaN operand of `min` would select the other side.
    NotPositive,
    /// `v <= 0.0`, never NaN.
    NonPositive,
}

/// Channels that must be non-positive for the proof, and what it proves.
#[derive(Debug, Clone)]
struct Condition {
    strength: Strength,
    channels: BTreeSet<usize>,
}

impl Condition {
    fn weaken(self, strength: Strength) -> Self {
        Self {
            strength: self.strength.min(strength),
            channels: self.channels,
        }
    }

    /// Prefers fewer channels to check, then the stronger proof.
    fn better(a: Option<Self>, b: Option<Self>) -> Option<Self> {
        match (a, b) {
            (Some(a), Some(b)) => Some(
                if (a.channels.len(), Reverse(a.strength))
                    <= (b.channels.len(), Reverse(b.strength))
                {
                    a
                } else {
                    b
                },
            ),
            (a, b) => a.or(b),
        }
    }
}

/// channels whose non-positive block values prove final density is not positive
/// indices start at `first_channel`; none if no proof is available
///
/// # Panics
/// Panics if the walk's channel numbering disagrees with
/// [`collect_interpolated_inners`], which assigns the indices.
pub(super) fn final_density_nonpositive_channels(
    df: &DensityFunction,
    registry: &BTreeMap<String, DensityFunction>,
    first_channel: usize,
) -> Option<Vec<usize>> {
    let mut next_channel = first_channel;
    let condition = walk(df, registry, &mut next_channel);
    assert_eq!(
        next_channel - first_channel,
        collect_interpolated_inners(df, registry).len(),
        "nonpositive analysis walked the interpolated channels in a different order than the codegen"
    );
    condition.map(|condition| condition.channels.into_iter().collect())
}

/// Walks `df` in `collect_interpolated_walk` order so `next_channel` matches
/// the codegen's channel numbering, including through unsupported subtrees.
fn walk(
    df: &DensityFunction,
    registry: &BTreeMap<String, DensityFunction>,
    next_channel: &mut usize,
) -> Option<Condition> {
    match df {
        DensityFunction::Marker(m) if m.kind == MarkerType::Interpolated => {
            let channel = *next_channel;
            *next_channel += 1;
            Some(Condition {
                strength: Strength::NonPositive,
                channels: BTreeSet::from([channel]),
            })
        }
        DensityFunction::Marker(m) => walk(&m.wrapped, registry, next_channel),
        // Codegen emits the input unchanged (no blending during generation).
        DensityFunction::BlendDensity(bd) => walk(&bd.input, registry, next_channel),
        DensityFunction::Reference(r) => registry
            .get(&r.id)
            .and_then(|ref_df| walk(ref_df, registry, next_channel)),
        DensityFunction::Constant(c) => (c.value <= 0.0).then(|| Condition {
            strength: Strength::NonPositive,
            channels: BTreeSet::new(),
        }),
        // `clamp(v, min, max)`: `v < min` yields `min <= 0`; `v > max`
        // yields `max < v <= 0`; otherwise `v`. NaN passes through.
        DensityFunction::Clamp(c) => {
            let input = walk(&c.input, registry, next_channel);
            if c.min <= 0.0 { input } else { None }
        }
        DensityFunction::Mapped(m) => {
            let input = walk(&m.input, registry, next_channel);
            match m.op {
                // squeeze on [-1, 0]: c³ >= c, so c/2 - c³/24 <= 0
                // positive scaling and cubing also keep the sign
                MappedType::Squeeze
                | MappedType::HalfNegative
                | MappedType::QuarterNegative
                | MappedType::Cube => input,
                _ => None,
            }
        }
        DensityFunction::TwoArgumentSimple(t) => {
            let a = walk(&t.argument1, registry, next_channel);
            let b = walk(&t.argument2, registry, next_channel);
            match t.op {
                // Both `<= 0` (or NaN) sums to `<= 0` (or NaN).
                TwoArgType::Add => {
                    let (a, b) = (a?, b?);
                    Some(Condition {
                        strength: a.strength.min(b.strength),
                        channels: a.channels.union(&b.channels).copied().collect(),
                    })
                }
                // codegen picks a if a <= lower_bound(b) or a < b, else b
                // a proven right side stays proven; a proven left may yield NaN
                TwoArgType::Min => {
                    let from_b = b.filter(|b| b.strength == Strength::NonPositive);
                    let from_a = a
                        .filter(|a| a.strength == Strength::NonPositive)
                        .map(|a| a.weaken(Strength::NotPositive));
                    Condition::better(from_b, from_a)
                }
                TwoArgType::Sub | TwoArgType::Mul | TwoArgType::Div | TwoArgType::Max => None,
            }
        }
        other => {
            *next_channel += collect_interpolated_inners(other, registry).len();
            None
        }
    }
}
