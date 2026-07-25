use serde::{Deserialize, Deserializer, de::Error as _};

use crate::random::Random;

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
