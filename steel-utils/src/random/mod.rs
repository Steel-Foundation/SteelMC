//! This module contains utilities for random number generation.
use enum_dispatch::enum_dispatch;

use crate::random::{
    legacy_random::{LegacyRandom, LegacyRandomSplitter},
    xoroshiro::{Xoroshiro, XoroshiroSplitter},
};

/// This module contains the gaussian random number generator.
pub mod gaussian;
/// This module contains the legacy random number generator implementation.
pub mod legacy_random;
/// This module contains the xoroshiro random number generator.
pub mod xoroshiro;

/// A trait for random number generators.
#[enum_dispatch]
#[allow(missing_docs)]
pub trait Random {
    #[must_use]
    fn fork(&mut self) -> Self;

    fn next_i32(&mut self) -> i32;

    fn next_i32_bounded(&mut self, bound: i32) -> i32;

    fn next_i32_between(&mut self, min: i32, max: i32) -> i32 {
        self.next_i32_bounded(max - min + 1) + min
    }

    fn next_i32_between_exclusive(&mut self, min: i32, max: i32) -> i32 {
        min + self.next_i32_bounded(max - min)
    }

    fn next_i64(&mut self) -> i64;

    fn next_f32(&mut self) -> f32;

    fn next_f64(&mut self) -> f64;

    fn next_bool(&mut self) -> bool;

    fn next_gaussian(&mut self) -> f64;

    fn triangle(&mut self, min: f64, max: f64) -> f64 {
        min + max * (self.next_f64() - self.next_f64())
    }

    fn triangle_f32(&mut self, min: f32, max: f32) -> f32 {
        min + max * (self.next_f32() - self.next_f32())
    }

    fn next_positional(&mut self) -> RandomSplitter;

    fn consume_count(&mut self, count: i32) {
        for _ in 0..count {
            self.next_i32();
        }
    }
}

/// A trait for positional random number generators.
#[enum_dispatch]
#[allow(missing_docs)]
pub trait PositionalRandom {
    fn at(&self, x: i32, y: i32, z: i32) -> RandomSource;

    fn with_hash_of(&self, name: &str) -> RandomSource;

    fn with_seed(&self, seed: u64) -> RandomSource;
}

/// A source of random numbers.
#[enum_dispatch(Random)]
pub enum RandomSource {
    /// A xoroshiro random number generator.
    Xoroshiro(Xoroshiro),
    /// A legacy Minecraft random number generator.
    Legacy(LegacyRandom),
}

/// A random number generator that can be split.
#[enum_dispatch(PositionalRandom)]
pub enum RandomSplitter {
    /// A xoroshiro random number generator.
    Xoroshiro(XoroshiroSplitter),
    /// A legacy Minecraft random number generator splitter.
    Legacy(LegacyRandomSplitter),
}

/// Gets a seed from a position.
#[allow(clippy::cast_sign_loss)]
#[must_use]
pub fn get_seed(x: i32, y: i32, z: i32) -> i64 {
    let l = i64::from(x.wrapping_mul(3_129_871))
        ^ (i64::from(z).wrapping_mul(116_129_781_i64))
        ^ i64::from(y);
    let l = l
        .wrapping_mul(l)
        .wrapping_mul(42_317_861_i64)
        .wrapping_add(l.wrapping_mul(11));
    l >> 16
}
