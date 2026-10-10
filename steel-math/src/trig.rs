//! Port of Mojang `net.minecraft.util.Mth`'s float trig lookup tables.
//!
//! Carvers, structures, entity physics, and a lot of other gameplay code call
//! `Mth.sin(float)` / `Mth.cos(float)`, not `Math.sin(double)`. Matching
//! block-level determinism with vanilla requires using the same 65536-entry
//! `float` table instead of the precise FPU sine.
//!
//! # Vanilla reference
//! ```java
//! private static final float[] SIN = new float[65536];
//! static {
//!     for (int i = 0; i < 65536; i++)
//!         SIN[i] = (float) Math.sin(i * (Math.PI * 2.0 / 65536.0));
//! }
//! public static float sin(double i)  { return SIN[(int)((long)(i * 10430.378350470453) & 65535L)]; }
//! public static float cos(double i)  { return SIN[(int)((long)(i * 10430.378350470453 + 16384.0) & 65535L)]; }
//! ```
//! Note that the multiplier `10430.378350470453` is exactly
//! `65536 / (2π)` rounded to a double, and the table is indexed by the
//! low 16 bits of the scaled angle.

use std::{array, f64::consts::PI, mem, sync::LazyLock};

const ATAN_TABLE_LEN: usize = 257;
const FRAC_BIAS: f64 = f64::from_bits(4_805_340_802_404_319_232);
/// `65536 / (2π)` — Mojang's stored constant.
const INDEX_SCALE: f64 = 10_430.378_350_470_453;
const ONE_SIXTH: f64 = 0.166_666_666_666_666_66;
const TABLE_LEN: usize = 65_536;
const TABLE_MASK: i64 = 0xFFFF;

/// The `Mth.SIN` table (one entry per 2π / 65536 radians, float-valued).
static SIN_TABLE: LazyLock<Box<[f32; TABLE_LEN]>> = LazyLock::new(|| {
    // Box on the heap to keep the binary's .bss section small.
    let mut table: Box<[f32; TABLE_LEN]> = vec![0.0_f32; TABLE_LEN]
        .into_boxed_slice()
        .try_into()
        .expect("65536-element vec");
    for (i, slot) in table.iter_mut().enumerate() {
        *slot = (i as f64 / INDEX_SCALE).sin() as f32;
    }
    table
});

/// Arcsine angles in radians for inputs from 0 to 1 in steps of 1/256.
static ASIN_TAB: LazyLock<[f64; ATAN_TABLE_LEN]> = LazyLock::new(|| {
    array::from_fn(|index| {
        let value = index as f64 / 256.0;
        value.asin()
    })
});

/// Cosines of the angles in [`ASIN_TAB`]
static COS_TAB: LazyLock<[f64; ATAN_TABLE_LEN]> =
    LazyLock::new(|| array::from_fn(|index| ASIN_TAB[index].cos()));

/// Vanilla `Mth.sin(double)` — returns the float from the 65536-entry sine
/// table indexed by the angle's low-16-bit bucket.
#[inline]
#[must_use]
pub fn sin(angle: f64) -> f32 {
    let idx = (((angle * INDEX_SCALE) as i64) & TABLE_MASK) as usize;
    SIN_TABLE[idx]
}

/// Vanilla `Mth.cos(double)` — like [`sin`] but phase-shifted by +π/2 using
/// the same table.
#[inline]
#[must_use]
pub fn cos(angle: f64) -> f32 {
    let idx = (((angle * INDEX_SCALE + 16_384.0) as i64) & TABLE_MASK) as usize;
    SIN_TABLE[idx]
}

/// Approximates `1 / sqrt(value)` using a fast inverse square root algorithm.
const fn fast_inv_sqrt(value: f64) -> f64 {
    let half = 0.5 * value;
    let bits = value.to_bits() as i64;
    let bits = 6_910_469_410_427_058_090_i64.wrapping_sub(bits >> 1);
    let estimate = f64::from_bits(bits as u64);

    estimate * (1.5 - half * estimate * estimate)
}

/// Returns vanilla's approximate angle in radians for the vector `(x, y)`.
#[must_use]
pub fn atan2(mut y: f64, mut x: f64) -> f64 {
    let d2 = x * x + y * y;
    if d2.is_nan() {
        return f64::NAN;
    }

    let neg_y = y < 0.0;
    if neg_y {
        y = -y;
    }

    let neg_x = x < 0.0;
    if neg_x {
        x = -x;
    }

    let steep = y > x;
    if steep {
        mem::swap(&mut x, &mut y);
    }

    let rinv = fast_inv_sqrt(d2);
    x *= rinv;
    y *= rinv;

    let yp = FRAC_BIAS + y;
    let index = (yp.to_bits() as i32) as usize;
    let phi = ASIN_TAB[index];
    let c_phi = COS_TAB[index];
    let s_phi = yp - FRAC_BIAS;
    let sd = y * c_phi - x * s_phi;
    let d = (6.0 + sd * sd) * sd * ONE_SIXTH;
    let mut theta = phi + d;

    if steep {
        theta = (PI / 2.0) - theta;
    }
    if neg_x {
        theta = PI - theta;
    }
    if neg_y {
        theta = -theta;
    }

    theta
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn sin_at_zero_is_zero() {
        assert_eq!(sin(0.0), 0.0);
        assert_eq!(cos(0.0), 1.0);
    }

    #[test]
    fn sin_matches_table_entries() {
        // Index 0 is sin(0), should be 0.
        // Index 16384 is sin(π/2), should be 1.
        // Verify a few well-known table entries.
        for &i in &[0usize, 1000, 16_384, 32_768, 49_152] {
            let expected = (i as f64 / INDEX_SCALE).sin() as f32;
            assert_eq!(SIN_TABLE[i], expected);
        }
    }

    #[test]
    fn cos_is_sin_shifted() {
        // cos(x) == sin(x + π/2) via the same table.
        let angle = 0.7;
        let expected = {
            let idx = (((angle * INDEX_SCALE + 16_384.0) as i64) & TABLE_MASK) as usize;
            SIN_TABLE[idx]
        };
        assert_eq!(cos(angle), expected);
    }
}
