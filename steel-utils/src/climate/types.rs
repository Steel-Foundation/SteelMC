//! Climate types for biome selection.

use super::{PARAMETER_COUNT, QUANTIZATION_FACTOR, quantize_coord};

/// A target point representing sampled climate values.
///
/// All values are quantized (multiplied by 10000) to match vanilla's integer-based
/// distance calculations. This avoids floating-point precision issues in biome lookup.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TargetPoint {
    /// Temperature parameter
    pub temperature: i64,
    /// Humidity/vegetation parameter
    pub humidity: i64,
    /// Continentalness parameter (inland vs ocean)
    pub continentalness: i64,
    /// Erosion parameter
    pub erosion: i64,
    /// Depth parameter (surface vs underground)
    pub depth: i64,
    /// Weirdness/ridges parameter
    pub weirdness: i64,
}

impl TargetPoint {
    /// Create a new target point with quantized values.
    #[must_use]
    pub const fn new(
        temperature: i64,
        humidity: i64,
        continentalness: i64,
        erosion: i64,
        depth: i64,
        weirdness: i64,
    ) -> Self {
        Self {
            temperature,
            humidity,
            continentalness,
            erosion,
            depth,
            weirdness,
        }
    }

    /// Create a target point from f64 values (will be quantized).
    #[must_use]
    pub fn from_floats(
        temperature: f64,
        humidity: f64,
        continentalness: f64,
        erosion: f64,
        depth: f64,
        weirdness: f64,
    ) -> Self {
        Self {
            temperature: quantize_coord(temperature),
            humidity: quantize_coord(humidity),
            continentalness: quantize_coord(continentalness),
            erosion: quantize_coord(erosion),
            depth: quantize_coord(depth),
            weirdness: quantize_coord(weirdness),
        }
    }

    /// Convert to a 7-element array for tree lookups.
    /// The 7th element is always 0 (offset position).
    #[must_use]
    pub const fn to_parameter_array(&self) -> [i64; PARAMETER_COUNT] {
        [
            self.temperature,
            self.humidity,
            self.continentalness,
            self.erosion,
            self.depth,
            self.weirdness,
            0, // Offset target is always 0
        ]
    }
}

/// A parameter range for biome matching.
///
/// Represents a range [min, max] that a climate parameter can match.
/// A point matches if it falls within this range; distance is 0 inside
/// and increases linearly outside.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Parameter {
    /// Minimum value (quantized)
    pub min: i64,
    /// Maximum value (quantized)
    pub max: i64,
}

impl Parameter {
    /// Create a new parameter range.
    #[must_use]
    pub const fn new(min: i64, max: i64) -> Self {
        Self { min, max }
    }

    /// Create a point parameter (min == max).
    #[must_use]
    pub fn point(value: f32) -> Self {
        Self::span(value, value)
    }

    /// Create a parameter span from float values.
    #[must_use]
    pub fn span(min: f32, max: f32) -> Self {
        debug_assert!(min <= max, "min > max: {min} > {max}");
        Self {
            min: (min * QUANTIZATION_FACTOR) as i64,
            max: (max * QUANTIZATION_FACTOR) as i64,
        }
    }

    /// Create a parameter span from two parameters.
    #[must_use]
    pub const fn span_params(min: &Parameter, max: &Parameter) -> Self {
        debug_assert!(min.min <= max.max, "span_params: min > max");
        Self {
            min: min.min,
            max: max.max,
        }
    }

    /// Calculate the distance from a target value to this parameter range.
    ///
    /// Returns 0 if the target is within the range, otherwise the distance
    /// to the nearest edge.
    #[inline]
    #[must_use]
    pub const fn distance(&self, target: i64) -> i64 {
        let above = target - self.max;
        let below = self.min - target;
        if above > 0 {
            above
        } else if below > 0 {
            below
        } else {
            0
        }
    }

    /// Calculate the distance between two parameter ranges.
    #[inline]
    #[must_use]
    pub const fn distance_param(&self, target: &Parameter) -> i64 {
        let above = target.min - self.max;
        let below = self.min - target.max;
        if above > 0 {
            above
        } else if below > 0 {
            below
        } else {
            0
        }
    }

    /// Expand this parameter to include another parameter.
    #[must_use]
    pub const fn span_with(&self, other: Option<&Parameter>) -> Self {
        match other {
            Some(o) => Self {
                min: self.min.min(o.min),
                max: self.max.max(o.max),
            },
            None => *self,
        }
    }
}

/// A biome's full parameter specification.
///
/// Contains ranges for all 6 climate parameters plus an offset value
/// used as a tiebreaker in biome selection.
#[derive(Debug, Clone, Copy)]
pub struct ParameterPoint {
    /// Temperature range
    pub temperature: Parameter,
    /// Humidity range
    pub humidity: Parameter,
    /// Continentalness range
    pub continentalness: Parameter,
    /// Erosion range
    pub erosion: Parameter,
    /// Depth range
    pub depth: Parameter,
    /// Weirdness range
    pub weirdness: Parameter,
    /// Offset (quantized) - used as tiebreaker
    pub offset: i64,
}

impl ParameterPoint {
    /// Create a new parameter point.
    #[must_use]
    pub const fn new(
        temperature: Parameter,
        humidity: Parameter,
        continentalness: Parameter,
        erosion: Parameter,
        depth: Parameter,
        weirdness: Parameter,
        offset: i64,
    ) -> Self {
        Self {
            temperature,
            humidity,
            continentalness,
            erosion,
            depth,
            weirdness,
            offset,
        }
    }

    /// Calculate the fitness (distance) between this parameter point and a target.
    ///
    /// Lower fitness = better match. Uses squared distances.
    #[must_use]
    #[expect(
        clippy::many_single_char_names,
        reason = "single-letter abbreviations match vanilla's climate parameter names"
    )]
    pub const fn fitness(&self, target: &TargetPoint) -> i64 {
        let t = self.temperature.distance(target.temperature);
        let h = self.humidity.distance(target.humidity);
        let c = self.continentalness.distance(target.continentalness);
        let e = self.erosion.distance(target.erosion);
        let d = self.depth.distance(target.depth);
        let w = self.weirdness.distance(target.weirdness);

        // Sum of squared distances (matches vanilla Mth.square usage)
        t * t + h * h + c * c + e * e + d * d + w * w + self.offset * self.offset
    }

    /// Get the parameter space as a slice of parameters.
    #[must_use]
    pub const fn parameter_space(&self) -> [Parameter; PARAMETER_COUNT] {
        [
            self.temperature,
            self.humidity,
            self.continentalness,
            self.erosion,
            self.depth,
            self.weirdness,
            Parameter::new(self.offset, self.offset),
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_target_point_from_floats() {
        let target = TargetPoint::from_floats(0.5, -0.3, 0.0, 0.1, 0.0, 0.2);
        assert_eq!(target.temperature, 5000);
        assert_eq!(target.humidity, -3000);
        assert_eq!(target.continentalness, 0);
        assert_eq!(target.erosion, 1000);
        assert_eq!(target.depth, 0);
        assert_eq!(target.weirdness, 2000);
    }

    #[test]
    fn test_parameter_distance() {
        let param = Parameter::new(-5000, 5000);

        // Inside range
        assert_eq!(param.distance(0), 0);
        assert_eq!(param.distance(5000), 0);
        assert_eq!(param.distance(-5000), 0);

        // Outside range
        assert_eq!(param.distance(6000), 1000);
        assert_eq!(param.distance(-6000), 1000);
        assert_eq!(param.distance(10000), 5000);
    }

    #[test]
    fn test_parameter_point_fitness() {
        let params = ParameterPoint::new(
            Parameter::new(0, 0),
            Parameter::new(0, 0),
            Parameter::new(0, 0),
            Parameter::new(0, 0),
            Parameter::new(0, 0),
            Parameter::new(0, 0),
            0,
        );

        // Perfect match
        let target = TargetPoint::new(0, 0, 0, 0, 0, 0);
        assert_eq!(params.fitness(&target), 0);

        // Off by 100 in temperature
        let target = TargetPoint::new(100, 0, 0, 0, 0, 0);
        assert_eq!(params.fitness(&target), 100 * 100);

        // Off by 100 in two parameters
        let target = TargetPoint::new(100, 100, 0, 0, 0, 0);
        assert_eq!(params.fitness(&target), 100 * 100 + 100 * 100);
    }
}
