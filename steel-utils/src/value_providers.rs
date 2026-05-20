//! Value providers matching vanilla's `VerticalAnchor`, `HeightProvider`,
//! and `FloatProvider`.
//!
//! JSON parsing follows vanilla's codec shape:
//! * `VerticalAnchor` is a single-key object: `{"absolute": 180}`,
//!   `{"above_bottom": 8}`, or `{"below_top": 1}`.
//! * `HeightProvider` accepts either a bare `VerticalAnchor` (shortcut for
//!   `ConstantHeight`) or a typed object with a namespaced vanilla registry id,
//!   e.g. `{"type": "minecraft:uniform", ...}`.
//! * `FloatProvider` accepts either a bare float (shortcut for `ConstantFloat`)
//!   or a typed object with a namespaced vanilla registry id.

use serde::{Deserialize, Deserializer, de::Error as _};

use crate::random::Random;

// ── VerticalAnchor ───────────────────────────────────────────────────────────

/// A vertical anchor resolving to a world Y coordinate given the dimension
/// bounds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VerticalAnchor {
    /// Absolute Y coordinate.
    Absolute(i32),
    /// `min_y + offset`.
    AboveBottom(i32),
    /// `min_y + height - 1 - offset` (i.e. `max_y - offset`).
    BelowTop(i32),
}

impl VerticalAnchor {
    /// Resolve this anchor to a world Y coordinate.
    ///
    /// Matches vanilla's `VerticalAnchor.resolveY(WorldGenerationContext)`.
    #[must_use]
    pub const fn resolve_y(self, min_y: i32, height: i32) -> i32 {
        match self {
            Self::Absolute(y) => y,
            Self::AboveBottom(offset) => min_y + offset,
            Self::BelowTop(offset) => min_y + height - 1 - offset,
        }
    }
}

impl<'de> Deserialize<'de> for VerticalAnchor {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Raw {
            #[serde(default)]
            absolute: Option<i32>,
            #[serde(default)]
            above_bottom: Option<i32>,
            #[serde(default)]
            below_top: Option<i32>,
        }
        let raw = Raw::deserialize(d)?;
        match (raw.absolute, raw.above_bottom, raw.below_top) {
            (Some(y), None, None) => Ok(Self::Absolute(y)),
            (None, Some(o), None) => Ok(Self::AboveBottom(o)),
            (None, None, Some(o)) => Ok(Self::BelowTop(o)),
            (None, None, None) => Err(D::Error::custom(
                "VerticalAnchor requires exactly one of absolute/above_bottom/below_top",
            )),
            _ => Err(D::Error::custom(
                "VerticalAnchor must have exactly one of absolute/above_bottom/below_top",
            )),
        }
    }
}

// ── HeightProvider ───────────────────────────────────────────────────────────

/// An `int`-valued provider parameterised by world-generation bounds
/// (`min_y`, `height`).
///
/// Mirrors vanilla's `HeightProvider` hierarchy.
#[derive(Debug, Clone, Copy)]
pub enum HeightProvider {
    /// Always resolves to a fixed anchor.
    Constant(VerticalAnchor),
    /// Uniform inclusive over \[min, max\].
    Uniform {
        /// Inclusive lower bound.
        min_inclusive: VerticalAnchor,
        /// Inclusive upper bound.
        max_inclusive: VerticalAnchor,
    },
    /// Sum of two `next_i32_bounded` draws — symmetric triangle when
    /// `plateau == 0`, trapezoid otherwise.
    Trapezoid {
        /// Inclusive lower bound.
        min_inclusive: VerticalAnchor,
        /// Inclusive upper bound.
        max_inclusive: VerticalAnchor,
        /// Flat-top width; `0` gives a pure triangle.
        plateau: i32,
    },
    /// Biased toward the bottom: two nested `nextInt` draws.
    BiasedToBottom {
        /// Inclusive lower bound.
        min_inclusive: VerticalAnchor,
        /// Inclusive upper bound.
        max_inclusive: VerticalAnchor,
        /// Minimum span of the inner window (default `1`).
        inner: i32,
    },
    /// Heavily biased toward the bottom: three nested `nextInt` draws.
    VeryBiasedToBottom {
        /// Inclusive lower bound.
        min_inclusive: VerticalAnchor,
        /// Inclusive upper bound.
        max_inclusive: VerticalAnchor,
        /// Minimum span of the inner window (default `1`).
        inner: i32,
    },
}

impl HeightProvider {
    /// Sample a Y coordinate.
    ///
    /// Matches vanilla's `HeightProvider.sample` — including the "empty range
    /// returns min" fallback (vanilla logs a warning once; we silently fall
    /// back to `min` since this branch isn't hit in practice).
    pub fn sample<R: Random + ?Sized>(self, random: &mut R, min_y: i32, height: i32) -> i32 {
        match self {
            Self::Constant(anchor) => anchor.resolve_y(min_y, height),
            Self::Uniform {
                min_inclusive,
                max_inclusive,
            } => {
                let min = min_inclusive.resolve_y(min_y, height);
                let max = max_inclusive.resolve_y(min_y, height);
                if min > max {
                    min
                } else {
                    random.next_i32_between(min, max)
                }
            }
            Self::Trapezoid {
                min_inclusive,
                max_inclusive,
                plateau,
            } => {
                let min = min_inclusive.resolve_y(min_y, height);
                let max = max_inclusive.resolve_y(min_y, height);
                if min > max {
                    min
                } else {
                    let range = max - min;
                    if plateau >= range {
                        random.next_i32_between(min, max)
                    } else {
                        let plateau_start = (range - plateau) / 2;
                        let plateau_end = range - plateau_start;
                        min + random.next_i32_between(0, plateau_end)
                            + random.next_i32_between(0, plateau_start)
                    }
                }
            }
            Self::BiasedToBottom {
                min_inclusive,
                max_inclusive,
                inner,
            } => {
                let min = min_inclusive.resolve_y(min_y, height);
                let max = max_inclusive.resolve_y(min_y, height);
                if max - min - inner < 0 {
                    min
                } else {
                    let limit = random.next_i32_bounded(max - min - inner + 1);
                    random.next_i32_bounded(limit + inner) + min
                }
            }
            Self::VeryBiasedToBottom {
                min_inclusive,
                max_inclusive,
                inner,
            } => {
                let min = min_inclusive.resolve_y(min_y, height);
                let max = max_inclusive.resolve_y(min_y, height);
                if max - min - inner < 0 {
                    min
                } else {
                    let upper_inclusive = random.next_i32_between(min + inner, max);
                    let biased_upper_inclusive = random.next_i32_between(min, upper_inclusive - 1);
                    random.next_i32_between(min, biased_upper_inclusive - 1 + inner)
                }
            }
        }
    }
}

impl<'de> Deserialize<'de> for HeightProvider {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(tag = "type", deny_unknown_fields)]
        enum Tagged {
            #[serde(rename = "minecraft:constant")]
            Constant { value: VerticalAnchor },
            #[serde(rename = "minecraft:uniform")]
            Uniform {
                min_inclusive: VerticalAnchor,
                max_inclusive: VerticalAnchor,
            },
            #[serde(rename = "minecraft:trapezoid")]
            Trapezoid {
                min_inclusive: VerticalAnchor,
                max_inclusive: VerticalAnchor,
                #[serde(default)]
                plateau: i32,
            },
            #[serde(rename = "minecraft:biased_to_bottom")]
            BiasedToBottom {
                min_inclusive: VerticalAnchor,
                max_inclusive: VerticalAnchor,
                #[serde(default = "default_inner")]
                inner: i32,
            },
            #[serde(rename = "minecraft:very_biased_to_bottom")]
            VeryBiasedToBottom {
                min_inclusive: VerticalAnchor,
                max_inclusive: VerticalAnchor,
                #[serde(default = "default_inner")]
                inner: i32,
            },
        }

        const fn default_inner() -> i32 {
            1
        }

        let value = serde_json::Value::deserialize(d)?;
        let has_type = value
            .as_object()
            .is_some_and(|object| object.contains_key("type"));

        if !has_type {
            let anchor = VerticalAnchor::deserialize(value).map_err(D::Error::custom)?;
            return Ok(Self::Constant(anchor));
        }

        Ok(
            match serde_json::from_value(value).map_err(D::Error::custom)? {
                Tagged::Constant { value } => Self::Constant(value),
                Tagged::Uniform {
                    min_inclusive,
                    max_inclusive,
                } => Self::Uniform {
                    min_inclusive,
                    max_inclusive,
                },
                Tagged::Trapezoid {
                    min_inclusive,
                    max_inclusive,
                    plateau,
                } => Self::Trapezoid {
                    min_inclusive,
                    max_inclusive,
                    plateau,
                },
                Tagged::BiasedToBottom {
                    min_inclusive,
                    max_inclusive,
                    inner,
                } => Self::BiasedToBottom {
                    min_inclusive,
                    max_inclusive,
                    inner,
                },
                Tagged::VeryBiasedToBottom {
                    min_inclusive,
                    max_inclusive,
                    inner,
                } => Self::VeryBiasedToBottom {
                    min_inclusive,
                    max_inclusive,
                    inner,
                },
            },
        )
    }
}

// ── IntProvider ──────────────────────────────────────────────────────────────

/// An `int`-valued provider.
///
/// Mirrors vanilla's `IntProvider` hierarchy used by feature placement and
/// feature configuration data.
#[derive(Debug, Clone)]
pub enum IntProvider {
    /// Always returns the same value.
    Constant(i32),
    /// Uniform inclusive over `[min_inclusive, max_inclusive]`.
    Uniform {
        /// Inclusive lower bound.
        min_inclusive: i32,
        /// Inclusive upper bound.
        max_inclusive: i32,
    },
    /// Biased toward the bottom.
    BiasedToBottom {
        /// Inclusive lower bound.
        min_inclusive: i32,
        /// Inclusive upper bound.
        max_inclusive: i32,
    },
    /// Heavily biased toward the bottom.
    VeryBiasedToBottom {
        /// Inclusive lower bound.
        min_inclusive: i32,
        /// Inclusive upper bound.
        max_inclusive: i32,
        /// Minimum span of the inner window.
        inner: i32,
    },
    /// Sum of two uniform draws, symmetric triangle when `plateau == 0`.
    Trapezoid {
        /// Lower bound.
        min: i32,
        /// Upper bound.
        max: i32,
        /// Flat-top width.
        plateau: i32,
    },
    /// Gaussian with given mean/deviation, clamped to `[min_inclusive, max_inclusive]`.
    ClampedNormal {
        /// Distribution mean.
        mean: f32,
        /// Standard deviation.
        deviation: f32,
        /// Inclusive lower bound.
        min_inclusive: i32,
        /// Inclusive upper bound.
        max_inclusive: i32,
    },
    /// Clamps another provider to an inclusive range.
    Clamped {
        /// Source provider.
        source: Box<IntProvider>,
        /// Inclusive lower bound.
        min_inclusive: i32,
        /// Inclusive upper bound.
        max_inclusive: i32,
    },
    /// Weighted provider selection.
    WeightedList {
        /// Weighted alternatives.
        distribution: Vec<WeightedIntProvider>,
    },
}

/// A weighted int-provider entry.
#[derive(Debug, Clone)]
pub struct WeightedIntProvider {
    /// Provider data.
    pub data: IntProvider,
    /// Entry weight.
    pub weight: i32,
}

/// Uniform inclusive int provider.
///
/// This is used for vanilla fields whose codec is specifically `UniformInt`,
/// not the general `IntProvider` dispatch.
#[derive(Debug, Clone, Copy)]
pub struct UniformIntProvider {
    /// Inclusive lower bound.
    pub min_inclusive: i32,
    /// Inclusive upper bound.
    pub max_inclusive: i32,
}

impl UniformIntProvider {
    /// Sample a value.
    pub fn sample<R: Random + ?Sized>(self, random: &mut R) -> i32 {
        random.next_i32_between(self.min_inclusive, self.max_inclusive)
    }

    /// Returns a provider with the same lower bound and a different inclusive upper bound.
    #[must_use]
    pub const fn with_max_inclusive(self, max_inclusive: i32) -> Self {
        Self {
            min_inclusive: self.min_inclusive,
            max_inclusive,
        }
    }
}

impl<'de> Deserialize<'de> for UniformIntProvider {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Range {
            min_inclusive: i32,
            max_inclusive: i32,
        }

        #[derive(Deserialize)]
        #[serde(tag = "type", deny_unknown_fields)]
        enum Tagged {
            #[serde(rename = "minecraft:uniform")]
            Uniform {
                min_inclusive: i32,
                max_inclusive: i32,
            },
        }

        let value = serde_json::Value::deserialize(d)?;
        let has_type = value
            .as_object()
            .is_some_and(|object| object.contains_key("type"));

        let (min_inclusive, max_inclusive) = if has_type {
            match serde_json::from_value(value).map_err(D::Error::custom)? {
                Tagged::Uniform {
                    min_inclusive,
                    max_inclusive,
                } => (min_inclusive, max_inclusive),
            }
        } else {
            let Range {
                min_inclusive,
                max_inclusive,
            } = Range::deserialize(value).map_err(D::Error::custom)?;
            (min_inclusive, max_inclusive)
        };

        if min_inclusive > max_inclusive {
            return Err(D::Error::custom(
                "UniformIntProvider min_inclusive exceeds max_inclusive",
            ));
        }

        Ok(Self {
            min_inclusive,
            max_inclusive,
        })
    }
}

impl IntProvider {
    /// Static lower bound for this provider.
    #[must_use]
    pub fn min(&self) -> i32 {
        match self {
            Self::Constant(value) => *value,
            Self::Uniform { min_inclusive, .. }
            | Self::BiasedToBottom { min_inclusive, .. }
            | Self::VeryBiasedToBottom { min_inclusive, .. }
            | Self::Clamped { min_inclusive, .. }
            | Self::ClampedNormal { min_inclusive, .. } => *min_inclusive,
            Self::Trapezoid { min, .. } => *min,
            Self::WeightedList { distribution } => {
                let mut min = 0;
                let mut found = false;
                for entry in distribution {
                    let value = entry.data.min();
                    if !found || value < min {
                        min = value;
                        found = true;
                    }
                }
                min
            }
        }
    }

    /// Static upper bound for this provider.
    #[must_use]
    pub fn max(&self) -> i32 {
        match self {
            Self::Constant(value) => *value,
            Self::Uniform { max_inclusive, .. }
            | Self::BiasedToBottom { max_inclusive, .. }
            | Self::VeryBiasedToBottom { max_inclusive, .. }
            | Self::Clamped { max_inclusive, .. }
            | Self::ClampedNormal { max_inclusive, .. } => *max_inclusive,
            Self::Trapezoid { max, .. } => *max,
            Self::WeightedList { distribution } => {
                let mut max = 0;
                let mut found = false;
                for entry in distribution {
                    let value = entry.data.max();
                    if !found || value > max {
                        max = value;
                        found = true;
                    }
                }
                max
            }
        }
    }

    /// Sample a value.
    ///
    /// Matches vanilla's provider structure. Weighted-list selection is the
    /// standard total-weight draw used by vanilla's `SimpleWeightedRandomList`.
    pub fn sample<R: Random + ?Sized>(&self, random: &mut R) -> i32 {
        match self {
            Self::Constant(v) => *v,
            Self::Uniform {
                min_inclusive,
                max_inclusive,
            } => random.next_i32_between(*min_inclusive, *max_inclusive),
            Self::BiasedToBottom {
                min_inclusive,
                max_inclusive,
            } => {
                let span = *max_inclusive - *min_inclusive + 1;
                let bound = random.next_i32_bounded(span) + 1;
                *min_inclusive + random.next_i32_bounded(bound)
            }
            Self::VeryBiasedToBottom {
                min_inclusive,
                max_inclusive,
                inner,
            } => {
                let limit = *max_inclusive - *min_inclusive - *inner + 1;
                if limit <= 0 {
                    *min_inclusive
                } else {
                    let upper_inclusive = random.next_i32_bounded(limit) + *min_inclusive + *inner;
                    let biased_upper_inclusive =
                        random.next_i32_between(*min_inclusive, upper_inclusive - 1);
                    random.next_i32_between(*min_inclusive, biased_upper_inclusive - 1 + *inner)
                }
            }
            Self::Trapezoid { min, max, plateau } => {
                if *plateau == 0 && *max == -*min {
                    random.next_i32_bounded(*max + 1) - random.next_i32_bounded(*max + 1)
                } else {
                    let range = *max - *min;
                    if *plateau >= range {
                        random.next_i32_between(*min, *max)
                    } else {
                        let plateau_start = (range - *plateau) / 2;
                        let plateau_end = range - plateau_start;
                        *min + random.next_i32_between(0, plateau_end)
                            + random.next_i32_between(0, plateau_start)
                    }
                }
            }
            Self::ClampedNormal {
                mean,
                deviation,
                min_inclusive,
                max_inclusive,
            } => {
                let sample = *mean + *deviation * random.next_gaussian() as f32;
                sample.clamp(*min_inclusive as f32, *max_inclusive as f32) as i32
            }
            Self::Clamped {
                source,
                min_inclusive,
                max_inclusive,
            } => source.sample(random).clamp(*min_inclusive, *max_inclusive),
            Self::WeightedList { distribution } => {
                let total_weight: i32 = distribution.iter().map(|entry| entry.weight).sum();
                if total_weight <= 0 {
                    return 0;
                }
                let mut target = random.next_i32_bounded(total_weight);
                for entry in distribution {
                    target -= entry.weight;
                    if target < 0 {
                        return entry.data.sample(random);
                    }
                }
                0
            }
        }
    }
}

impl<'de> Deserialize<'de> for IntProvider {
    #[expect(
        clippy::too_many_lines,
        reason = "keeps the vanilla int-provider schema variants in one deserialization table"
    )]
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(tag = "type", deny_unknown_fields)]
        enum Tagged {
            #[serde(rename = "minecraft:constant")]
            Constant { value: i32 },
            #[serde(rename = "minecraft:uniform")]
            Uniform {
                min_inclusive: i32,
                max_inclusive: i32,
            },
            #[serde(rename = "minecraft:biased_to_bottom")]
            BiasedToBottom {
                min_inclusive: i32,
                max_inclusive: i32,
            },
            #[serde(rename = "minecraft:very_biased_to_bottom")]
            VeryBiasedToBottom {
                min_inclusive: i32,
                max_inclusive: i32,
                #[serde(default = "default_inner")]
                inner: i32,
            },
            #[serde(rename = "minecraft:trapezoid")]
            Trapezoid { min: i32, max: i32, plateau: i32 },
            #[serde(rename = "minecraft:clamped_normal")]
            ClampedNormal {
                mean: f32,
                deviation: f32,
                min_inclusive: i32,
                max_inclusive: i32,
            },
            #[serde(rename = "minecraft:clamped")]
            Clamped {
                source: Box<IntProvider>,
                min_inclusive: i32,
                max_inclusive: i32,
            },
            #[serde(rename = "minecraft:weighted_list")]
            WeightedList {
                distribution: Vec<WeightedIntProvider>,
            },
        }

        const fn default_inner() -> i32 {
            1
        }

        let value = serde_json::Value::deserialize(d)?;
        if value.is_number() {
            return Ok(Self::Constant(
                i32::deserialize(value).map_err(D::Error::custom)?,
            ));
        }

        Ok(
            match serde_json::from_value(value).map_err(D::Error::custom)? {
                Tagged::Constant { value } => Self::Constant(value),
                Tagged::Uniform {
                    min_inclusive,
                    max_inclusive,
                } => Self::Uniform {
                    min_inclusive,
                    max_inclusive,
                },
                Tagged::BiasedToBottom {
                    min_inclusive,
                    max_inclusive,
                } => Self::BiasedToBottom {
                    min_inclusive,
                    max_inclusive,
                },
                Tagged::VeryBiasedToBottom {
                    min_inclusive,
                    max_inclusive,
                    inner,
                } => Self::VeryBiasedToBottom {
                    min_inclusive,
                    max_inclusive,
                    inner,
                },
                Tagged::Trapezoid { min, max, plateau } => Self::Trapezoid { min, max, plateau },
                Tagged::ClampedNormal {
                    mean,
                    deviation,
                    min_inclusive,
                    max_inclusive,
                } => Self::ClampedNormal {
                    mean,
                    deviation,
                    min_inclusive,
                    max_inclusive,
                },
                Tagged::Clamped {
                    source,
                    min_inclusive,
                    max_inclusive,
                } => Self::Clamped {
                    source,
                    min_inclusive,
                    max_inclusive,
                },
                Tagged::WeightedList { distribution } => Self::WeightedList { distribution },
            },
        )
    }
}

impl<'de> Deserialize<'de> for WeightedIntProvider {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Raw {
            data: IntProvider,
            weight: i32,
        }

        let raw = Raw::deserialize(d)?;
        Ok(Self {
            data: raw.data,
            weight: raw.weight,
        })
    }
}

// ── FloatProvider ────────────────────────────────────────────────────────────

/// A `float`-valued provider.
///
/// Mirrors vanilla's `FloatProvider` hierarchy. `WeightedList` is omitted
/// until a carver or feature needs it.
#[derive(Debug, Clone, Copy)]
pub enum FloatProvider {
    /// Always returns the same value.
    Constant(f32),
    /// Uniform over `[min_inclusive, max_exclusive)`.
    Uniform {
        /// Inclusive lower bound.
        min_inclusive: f32,
        /// Exclusive upper bound.
        max_exclusive: f32,
    },
    /// Sum of two uniform draws — symmetric triangle when `plateau == 0`,
    /// trapezoid otherwise.
    Trapezoid {
        /// Lower bound.
        min: f32,
        /// Upper bound.
        max: f32,
        /// Flat-top width.
        plateau: f32,
    },
    /// Gaussian with given mean/deviation, clamped to `[min, max]`.
    ClampedNormal {
        /// Distribution mean.
        mean: f32,
        /// Standard deviation.
        deviation: f32,
        /// Inclusive lower bound.
        min: f32,
        /// Inclusive upper bound.
        max: f32,
    },
}

impl FloatProvider {
    /// Sample a value.
    ///
    /// Matches vanilla's `FloatProvider.sample` exactly. Order of
    /// `random.next_*` calls is preserved for hash-level determinism.
    pub fn sample<R: Random + ?Sized>(self, random: &mut R) -> f32 {
        match self {
            Self::Constant(v) => v,
            Self::Uniform {
                min_inclusive,
                max_exclusive,
            } => random.next_f32() * (max_exclusive - min_inclusive) + min_inclusive,
            Self::Trapezoid { min, max, plateau } => {
                let range = max - min;
                let plateau_start = (range - plateau) / 2.0;
                let plateau_end = range - plateau_start;
                min + random.next_f32() * plateau_end + random.next_f32() * plateau_start
            }
            Self::ClampedNormal {
                mean,
                deviation,
                min,
                max,
            } => {
                // Mth.normal: mean + deviation * (float)nextGaussian()
                let sample = mean + deviation * random.next_gaussian() as f32;
                sample.clamp(min, max)
            }
        }
    }

    /// Static lower bound.
    #[must_use]
    pub const fn min(self) -> f32 {
        match self {
            Self::Constant(v) => v,
            Self::Uniform { min_inclusive, .. } => min_inclusive,
            Self::Trapezoid { min, .. } | Self::ClampedNormal { min, .. } => min,
        }
    }

    /// Static upper bound.
    #[must_use]
    pub const fn max(self) -> f32 {
        match self {
            Self::Constant(v) => v,
            Self::Uniform { max_exclusive, .. } => max_exclusive,
            Self::Trapezoid { max, .. } | Self::ClampedNormal { max, .. } => max,
        }
    }
}

impl<'de> Deserialize<'de> for FloatProvider {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(tag = "type", deny_unknown_fields)]
        enum Tagged {
            #[serde(rename = "minecraft:constant")]
            Constant { value: f32 },
            #[serde(rename = "minecraft:uniform")]
            Uniform {
                min_inclusive: f32,
                max_exclusive: f32,
            },
            #[serde(rename = "minecraft:trapezoid")]
            Trapezoid { min: f32, max: f32, plateau: f32 },
            #[serde(rename = "minecraft:clamped_normal")]
            ClampedNormal {
                mean: f32,
                deviation: f32,
                min: f32,
                max: f32,
            },
        }

        let value = serde_json::Value::deserialize(d)?;
        if value.is_number() {
            return Ok(Self::Constant(
                f32::deserialize(value).map_err(D::Error::custom)?,
            ));
        }

        Ok(
            match serde_json::from_value(value).map_err(D::Error::custom)? {
                Tagged::Constant { value: v } => Self::Constant(v),
                Tagged::Uniform {
                    min_inclusive,
                    max_exclusive,
                } => Self::Uniform {
                    min_inclusive,
                    max_exclusive,
                },
                Tagged::Trapezoid { min, max, plateau } => Self::Trapezoid { min, max, plateau },
                Tagged::ClampedNormal {
                    mean,
                    deviation,
                    min,
                    max,
                } => Self::ClampedNormal {
                    mean,
                    deviation,
                    min,
                    max,
                },
            },
        )
    }
}

// ── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
#[expect(
    clippy::unwrap_used,
    clippy::float_cmp,
    reason = "test assertions: unwrap panics on parse failures, float equality is the check"
)]
mod test {
    use super::*;
    use crate::random::legacy_random::LegacyRandom;

    #[test]
    fn vertical_anchor_resolve() {
        assert_eq!(VerticalAnchor::Absolute(42).resolve_y(-64, 384), 42);
        assert_eq!(VerticalAnchor::AboveBottom(8).resolve_y(-64, 384), -56);
        assert_eq!(VerticalAnchor::BelowTop(1).resolve_y(0, 128), 126);
    }

    #[test]
    fn vertical_anchor_deserialize() {
        let a: VerticalAnchor = serde_json::from_str(r#"{"absolute": 180}"#).unwrap();
        assert_eq!(a, VerticalAnchor::Absolute(180));
        let b: VerticalAnchor = serde_json::from_str(r#"{"above_bottom": 8}"#).unwrap();
        assert_eq!(b, VerticalAnchor::AboveBottom(8));
        let c: VerticalAnchor = serde_json::from_str(r#"{"below_top": 1}"#).unwrap();
        assert_eq!(c, VerticalAnchor::BelowTop(1));
        assert!(serde_json::from_str::<VerticalAnchor>(r"{}").is_err());
        assert!(
            serde_json::from_str::<VerticalAnchor>(r#"{"absolute": 1, "above_bottom": 2}"#)
                .is_err()
        );
    }

    #[test]
    fn height_provider_deserialize_shortcut() {
        // A bare VerticalAnchor is a ConstantHeight.
        let hp: HeightProvider = serde_json::from_str(r#"{"absolute": 180}"#).unwrap();
        match hp {
            HeightProvider::Constant(VerticalAnchor::Absolute(180)) => (),
            other => panic!("expected Constant(Absolute(180)), got {other:?}"),
        }
    }

    #[test]
    fn height_provider_uniform_from_carver_json() {
        let hp: HeightProvider = serde_json::from_str(
            r#"{
                "type": "minecraft:uniform",
                "max_inclusive": {"absolute": 180},
                "min_inclusive": {"above_bottom": 8}
            }"#,
        )
        .unwrap();
        match hp {
            HeightProvider::Uniform {
                min_inclusive,
                max_inclusive,
            } => {
                assert_eq!(min_inclusive, VerticalAnchor::AboveBottom(8));
                assert_eq!(max_inclusive, VerticalAnchor::Absolute(180));
            }
            other => panic!("expected Uniform, got {other:?}"),
        }
    }

    #[test]
    fn float_provider_bare_float() {
        let fp: FloatProvider = serde_json::from_str("3.0").unwrap();
        match fp {
            FloatProvider::Constant(v) => assert!((v - 3.0).abs() < 1e-6),
            other => panic!("expected Constant, got {other:?}"),
        }
    }

    #[test]
    fn float_provider_uniform_from_carver_json() {
        let fp: FloatProvider = serde_json::from_str(
            r#"{
                "type": "minecraft:uniform",
                "max_exclusive": 1.4,
                "min_inclusive": 0.7
            }"#,
        )
        .unwrap();
        match fp {
            FloatProvider::Uniform {
                min_inclusive,
                max_exclusive,
            } => {
                assert!((min_inclusive - 0.7).abs() < 1e-6);
                assert!((max_exclusive - 1.4).abs() < 1e-6);
            }
            other => panic!("expected Uniform, got {other:?}"),
        }
    }

    #[test]
    fn float_provider_trapezoid_from_carver_json() {
        let fp: FloatProvider = serde_json::from_str(
            r#"{
                "type": "minecraft:trapezoid",
                "max": 6.0,
                "min": 0.0,
                "plateau": 2.0
            }"#,
        )
        .unwrap();
        match fp {
            FloatProvider::Trapezoid { min, max, plateau } => {
                assert_eq!(min, 0.0);
                assert_eq!(max, 6.0);
                assert_eq!(plateau, 2.0);
            }
            other => panic!("expected Trapezoid, got {other:?}"),
        }
    }

    #[test]
    fn int_provider_clamped_normal_prefers_tagged_shape() {
        let provider: IntProvider = serde_json::from_str(
            r#"{
                "type": "minecraft:clamped_normal",
                "mean": 0.0,
                "deviation": 3.0,
                "min_inclusive": -10,
                "max_inclusive": 10
            }"#,
        )
        .unwrap();

        match provider {
            IntProvider::ClampedNormal {
                mean,
                deviation,
                min_inclusive,
                max_inclusive,
            } => {
                assert_eq!(mean, 0.0);
                assert_eq!(deviation, 3.0);
                assert_eq!(min_inclusive, -10);
                assert_eq!(max_inclusive, 10);
            }
            other => panic!("expected ClampedNormal, got {other:?}"),
        }
    }

    #[test]
    fn provider_type_tags_require_extracted_registry_ids() {
        assert!(
            serde_json::from_str::<HeightProvider>(
                r#"{
                    "type": "uniform",
                    "max_inclusive": {"absolute": 180},
                    "min_inclusive": {"above_bottom": 8}
                }"#,
            )
            .is_err()
        );
        assert!(
            serde_json::from_str::<UniformIntProvider>(
                r#"{
                    "type": "uniform",
                    "min_inclusive": 0,
                    "max_inclusive": 10
                }"#,
            )
            .is_err()
        );
        assert!(
            serde_json::from_str::<IntProvider>(
                r#"{
                    "type": "uniform",
                    "min_inclusive": 0,
                    "max_inclusive": 10
                }"#,
            )
            .is_err()
        );
        assert!(
            serde_json::from_str::<FloatProvider>(
                r#"{
                    "type": "uniform",
                    "min_inclusive": 0.0,
                    "max_exclusive": 1.0
                }"#,
            )
            .is_err()
        );
    }

    #[test]
    fn provider_typed_payloads_deny_unknown_fields() {
        assert!(
            serde_json::from_str::<HeightProvider>(
                r#"{
                    "type": "minecraft:uniform",
                    "max_inclusive": {"absolute": 180},
                    "min_inclusive": {"above_bottom": 8},
                    "extra": 0
                }"#,
            )
            .is_err()
        );
        assert!(
            serde_json::from_str::<UniformIntProvider>(
                r#"{
                    "type": "minecraft:uniform",
                    "min_inclusive": 0,
                    "max_inclusive": 10,
                    "extra": 0
                }"#,
            )
            .is_err()
        );
        assert!(
            serde_json::from_str::<IntProvider>(
                r#"{
                    "type": "minecraft:clamped",
                    "source": 4,
                    "min_inclusive": 0,
                    "max_inclusive": 10,
                    "extra": 0
                }"#,
            )
            .is_err()
        );
        assert!(
            serde_json::from_str::<FloatProvider>(
                r#"{
                    "type": "minecraft:uniform",
                    "min_inclusive": 0.0,
                    "max_exclusive": 1.0,
                    "extra": 0.0
                }"#,
            )
            .is_err()
        );
    }

    #[test]
    fn int_provider_requires_typed_object_or_bare_constant() {
        assert!(
            serde_json::from_str::<IntProvider>(
                r#"{
                    "min_inclusive": 0,
                    "max_inclusive": 10
                }"#,
            )
            .is_err()
        );
        assert!(
            serde_json::from_str::<IntProvider>(
                r#"{
                    "type": "minecraft:weighted_list",
                    "distribution": [
                        {
                            "data": 1,
                            "weight": 2,
                            "extra": 3
                        }
                    ]
                }"#,
            )
            .is_err()
        );
    }

    #[test]
    fn int_provider_symmetric_trapezoid_sample_matches_vanilla_shortcut() {
        let provider = IntProvider::Trapezoid {
            min: -7,
            max: 7,
            plateau: 0,
        };
        let mut rng = LegacyRandom::from_seed(123);
        let mut rng_ref = LegacyRandom::from_seed(123);
        let sample = provider.sample(&mut rng);
        let expected = rng_ref.next_i32_bounded(8) - rng_ref.next_i32_bounded(8);
        assert_eq!(sample, expected);
    }

    /// Matches vanilla's `Mth.randomBetween`: `min + nextFloat()*(max-min)`.
    #[test]
    fn float_provider_uniform_sample_matches_vanilla() {
        let fp = FloatProvider::Uniform {
            min_inclusive: 0.7,
            max_exclusive: 1.4,
        };
        let mut rng = LegacyRandom::from_seed(0);
        let mut rng_ref = LegacyRandom::from_seed(0);
        let sample = fp.sample(&mut rng);
        let expected = rng_ref.next_f32() * (1.4 - 0.7) + 0.7;
        assert_eq!(sample, expected);
    }

    /// Height uniform sample: `random.nextInt(max - min + 1) + min`.
    #[test]
    fn height_provider_uniform_sample_matches_vanilla() {
        let hp = HeightProvider::Uniform {
            min_inclusive: VerticalAnchor::AboveBottom(8),
            max_inclusive: VerticalAnchor::Absolute(180),
        };
        let min_y = -64;
        let height = 384;
        let mut rng = LegacyRandom::from_seed(42);
        let mut rng_ref = LegacyRandom::from_seed(42);
        let sample = hp.sample(&mut rng, min_y, height);
        // min_y + 8 = -56, absolute 180
        let expected = rng_ref.next_i32_between(-56, 180);
        assert_eq!(sample, expected);
    }
}
