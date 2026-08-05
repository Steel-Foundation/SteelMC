use serde::{Deserialize, Deserializer, de::Error as _};

use crate::random::Random;

use super::VerticalAnchor;

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
