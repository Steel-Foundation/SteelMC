//! Climate system for biome selection in world generation.
//!
//! This module implements vanilla Minecraft's Climate system for biome lookup.
//! Climate parameters (temperature, humidity, etc.) are quantized to long integers
//! and used to find the best matching biome from a parameter space.
//!
//! # Key Types
//!
//! - [`TargetPoint`] - A sampled climate point with 6 quantized parameters
//! - [`Parameter`] - A parameter range (min/max) for biome matching
//! - [`ParameterPoint`] - Full biome parameter specification
//! - [`ParameterList`] - Collection of biomes with their parameter points

mod types;

pub use types::{Parameter, ParameterList, ParameterPoint, TargetPoint};

/// Quantization factor used to convert floats to longs.
/// This is the exact value from vanilla Climate.java.
pub const QUANTIZATION_FACTOR: f32 = 10000.0;

/// Number of climate parameters (temperature, humidity, continentalness, erosion, depth, weirdness, + offset).
pub const PARAMETER_COUNT: usize = 7;

/// Quantize a coordinate value to a long integer.
///
/// This matches vanilla's `Climate.quantizeCoord()` exactly:
/// `(long)(coord * 10000.0F)`
///
/// **CRITICAL**: The input is cast to f32 first, then multiplied, then cast to i64.
/// This ensures bit-exact matching with vanilla Java's float behavior.
#[inline]
#[must_use]
pub fn quantize_coord(coord: f64) -> i64 {
    ((coord as f32) * QUANTIZATION_FACTOR) as i64
}

/// Unquantize a long integer back to a float.
///
/// This matches vanilla's `Climate.unquantizeCoord()`:
/// `(float)coord / 10000.0F`
#[inline]
#[must_use]
pub fn unquantize_coord(coord: i64) -> f32 {
    coord as f32 / QUANTIZATION_FACTOR
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_quantize_coord() {
        assert_eq!(quantize_coord(0.0), 0);
        assert_eq!(quantize_coord(1.0), 10000);
        assert_eq!(quantize_coord(-1.0), -10000);
        assert_eq!(quantize_coord(0.5), 5000);
        assert_eq!(quantize_coord(-0.5), -5000);
    }

    #[test]
    fn test_unquantize_coord() {
        assert!((unquantize_coord(0) - 0.0).abs() < 1e-6);
        assert!((unquantize_coord(10000) - 1.0).abs() < 1e-6);
        assert!((unquantize_coord(-10000) - -1.0).abs() < 1e-6);
    }

    #[test]
    fn test_quantize_roundtrip() {
        let values = [0.0, 0.5, -0.5, 1.0, -1.0, 0.123, -0.456];
        for v in values {
            let quantized = quantize_coord(v);
            let unquantized = unquantize_coord(quantized);
            // Allow small error due to float precision
            assert!(
                (v as f32 - unquantized).abs() < 0.0001,
                "Roundtrip failed for {v}: got {unquantized}",
            );
        }
    }
}
