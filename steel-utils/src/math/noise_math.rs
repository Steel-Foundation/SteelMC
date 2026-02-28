//! Math utilities for noise generation, matching vanilla Minecraft's Mth.java

/// Smoothstep - quintic Hermite interpolation (NOT cubic!)
///
/// Formula: 6x^5 - 15x^4 + 10x^3
///
/// This is the standard smoothstep used in Perlin noise for smooth transitions.
/// Java reference: `Mth.smoothstep(double)`
#[inline]
#[must_use]
pub fn smoothstep(x: f64) -> f64 {
    x * x * x * (x * (x * 6.0 - 15.0) + 10.0)
}

/// Smoothstep derivative for noise with derivatives.
///
/// Formula: 30x^2(x-1)^2
///
/// Java reference: `Mth.smoothstepDerivative(double)`
#[inline]
#[must_use]
pub fn smoothstep_derivative(x: f64) -> f64 {
    30.0 * x * x * (x - 1.0) * (x - 1.0)
}

/// Floor function that matches Java behavior.
///
/// In Java, `(int)v` truncates toward zero, but we need floor behavior.
/// For negative values, we need to subtract 1 if there's a fractional part.
///
/// Java reference: `Mth.floor(double)`
#[inline]
#[must_use]
pub fn floor(v: f64) -> i32 {
    let i = v as i32;
    if v < f64::from(i) { i - 1 } else { i }
}

/// Long floor function matching Java behavior.
///
/// Java reference: `Mth.lfloor(double)`
#[inline]
#[must_use]
pub fn lfloor(v: f64) -> i64 {
    let i = v as i64;
    if v < i as f64 { i - 1 } else { i }
}

/// Linear interpolation.
///
/// Formula: a + alpha * (b - a)
///
/// Java reference: `Mth.lerp(double, double, double)`
#[inline]
#[must_use]
pub fn lerp(alpha: f64, a: f64, b: f64) -> f64 {
    a + alpha * (b - a)
}

/// Bilinear interpolation.
///
/// Interpolates between 4 values in a 2D grid.
///
/// Java reference: `Mth.lerp2(double, double, double, double, double, double)`
#[inline]
#[must_use]
pub fn lerp2(a1: f64, a2: f64, x00: f64, x10: f64, x01: f64, x11: f64) -> f64 {
    lerp(a2, lerp(a1, x00, x10), lerp(a1, x01, x11))
}

/// Trilinear interpolation.
///
/// Interpolates between 8 values in a 3D grid.
///
/// Java reference: `Mth.lerp3(...)`
#[inline]
#[must_use]
#[allow(clippy::too_many_arguments)]
pub fn lerp3(
    a1: f64,
    a2: f64,
    a3: f64,
    x000: f64,
    x100: f64,
    x010: f64,
    x110: f64,
    x001: f64,
    x101: f64,
    x011: f64,
    x111: f64,
) -> f64 {
    lerp(
        a3,
        lerp2(a1, a2, x000, x100, x010, x110),
        lerp2(a1, a2, x001, x101, x011, x111),
    )
}

/// Clamped linear interpolation.
///
/// Clamps the interpolation factor to [0, 1] before interpolating.
///
/// Java reference: `Mth.clampedLerp(double, double, double)`.
/// Note: Vanilla's parameter order is `(factor, min, max)`, ours is `(min, max, factor)`.
#[inline]
#[must_use]
pub fn clamped_lerp(min: f64, max: f64, factor: f64) -> f64 {
    if factor < 0.0 {
        min
    } else if factor > 1.0 {
        max
    } else {
        lerp(factor, min, max)
    }
}

/// Clamp a value to the range [min, max].
///
/// Java reference: `Mth.clamp(double, double, double)`
#[inline]
#[must_use]
pub fn clamp(value: f64, min: f64, max: f64) -> f64 {
    if value < min {
        min
    } else if value > max {
        max
    } else {
        value
    }
}

/// Clamp a value to the range [min, max] (i32 version).
#[inline]
#[must_use]
pub const fn clamp_i32(value: i32, min: i32, max: i32) -> i32 {
    if value < min {
        min
    } else if value > max {
        max
    } else {
        value
    }
}

/// Map a value from one range to another with clamped lerp.
///
/// Used for Y-clamped gradients in density functions.
#[inline]
#[must_use]
pub fn map_clamped(value: f64, from_min: f64, from_max: f64, to_min: f64, to_max: f64) -> f64 {
    let t = (value - from_min) / (from_max - from_min);
    clamped_lerp(to_min, to_max, t)
}

/// Square a value.
#[inline]
#[must_use]
pub fn square(x: f64) -> f64 {
    x * x
}

/// Cube a value.
#[inline]
#[must_use]
pub fn cube(x: f64) -> f64 {
    x * x * x
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_floor() {
        assert_eq!(floor(1.5), 1);
        assert_eq!(floor(1.0), 1);
        assert_eq!(floor(0.5), 0);
        assert_eq!(floor(0.0), 0);
        assert_eq!(floor(-0.5), -1);
        assert_eq!(floor(-1.0), -1);
        assert_eq!(floor(-1.5), -2);
    }

    #[test]
    fn test_smoothstep() {
        // At boundaries
        assert!((smoothstep(0.0) - 0.0).abs() < 1e-10);
        assert!((smoothstep(1.0) - 1.0).abs() < 1e-10);
        // At midpoint
        assert!((smoothstep(0.5) - 0.5).abs() < 1e-10);
    }

    #[test]
    fn test_lerp() {
        assert!((lerp(0.0, 10.0, 20.0) - 10.0).abs() < 1e-10);
        assert!((lerp(1.0, 10.0, 20.0) - 20.0).abs() < 1e-10);
        assert!((lerp(0.5, 10.0, 20.0) - 15.0).abs() < 1e-10);
    }
}
