//! Value providers matching vanilla's `VerticalAnchor`, `HeightProvider`,
//! and `FloatProvider`.
//!
//! JSON parsing follows vanilla's codec shape:
//! * `VerticalAnchor` is a single-key object: `{"absolute": 180}`,
//!   `{"above_bottom": 8}`, or `{"below_top": 1}`.
//! * `HeightProvider` accepts either a bare `VerticalAnchor` (shortcut for
//!   `ConstantHeight`) or a typed object `{"type": "minecraft:uniform", ...}`.
//! * `FloatProvider` accepts either a bare float (shortcut for `ConstantFloat`)
//!   or a typed object `{"type": "minecraft:uniform", ...}`.

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
        #[serde(untagged)]
        enum Raw {
            Anchor(VerticalAnchor),
            Tagged(Tagged),
        }

        #[derive(Deserialize)]
        #[serde(tag = "type")]
        enum Tagged {
            #[serde(rename = "minecraft:constant", alias = "constant")]
            Constant { value: VerticalAnchor },
            #[serde(rename = "minecraft:uniform", alias = "uniform")]
            Uniform {
                min_inclusive: VerticalAnchor,
                max_inclusive: VerticalAnchor,
            },
            #[serde(rename = "minecraft:trapezoid", alias = "trapezoid")]
            Trapezoid {
                min_inclusive: VerticalAnchor,
                max_inclusive: VerticalAnchor,
                #[serde(default)]
                plateau: i32,
            },
            #[serde(rename = "minecraft:biased_to_bottom", alias = "biased_to_bottom")]
            BiasedToBottom {
                min_inclusive: VerticalAnchor,
                max_inclusive: VerticalAnchor,
                #[serde(default = "default_inner")]
                inner: i32,
            },
            #[serde(
                rename = "minecraft:very_biased_to_bottom",
                alias = "very_biased_to_bottom"
            )]
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

        Ok(match Raw::deserialize(d)? {
            Raw::Anchor(anchor) => Self::Constant(anchor),
            Raw::Tagged(Tagged::Constant { value }) => Self::Constant(value),
            Raw::Tagged(Tagged::Uniform {
                min_inclusive,
                max_inclusive,
            }) => Self::Uniform {
                min_inclusive,
                max_inclusive,
            },
            Raw::Tagged(Tagged::Trapezoid {
                min_inclusive,
                max_inclusive,
                plateau,
            }) => Self::Trapezoid {
                min_inclusive,
                max_inclusive,
                plateau,
            },
            Raw::Tagged(Tagged::BiasedToBottom {
                min_inclusive,
                max_inclusive,
                inner,
            }) => Self::BiasedToBottom {
                min_inclusive,
                max_inclusive,
                inner,
            },
            Raw::Tagged(Tagged::VeryBiasedToBottom {
                min_inclusive,
                max_inclusive,
                inner,
            }) => Self::VeryBiasedToBottom {
                min_inclusive,
                max_inclusive,
                inner,
            },
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
        #[serde(untagged)]
        enum Raw {
            Constant(f32),
            Tagged(Tagged),
        }

        #[derive(Deserialize)]
        #[serde(tag = "type")]
        enum Tagged {
            #[serde(rename = "minecraft:constant", alias = "constant")]
            Constant { value: f32 },
            #[serde(rename = "minecraft:uniform", alias = "uniform")]
            Uniform {
                min_inclusive: f32,
                max_exclusive: f32,
            },
            #[serde(rename = "minecraft:trapezoid", alias = "trapezoid")]
            Trapezoid { min: f32, max: f32, plateau: f32 },
            #[serde(rename = "minecraft:clamped_normal", alias = "clamped_normal")]
            ClampedNormal {
                mean: f32,
                deviation: f32,
                min: f32,
                max: f32,
            },
        }

        Ok(match Raw::deserialize(d)? {
            Raw::Constant(v) | Raw::Tagged(Tagged::Constant { value: v }) => Self::Constant(v),
            Raw::Tagged(Tagged::Uniform {
                min_inclusive,
                max_exclusive,
            }) => Self::Uniform {
                min_inclusive,
                max_exclusive,
            },
            Raw::Tagged(Tagged::Trapezoid { min, max, plateau }) => {
                Self::Trapezoid { min, max, plateau }
            }
            Raw::Tagged(Tagged::ClampedNormal {
                mean,
                deviation,
                min,
                max,
            }) => Self::ClampedNormal {
                mean,
                deviation,
                min,
                max,
            },
        })
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
