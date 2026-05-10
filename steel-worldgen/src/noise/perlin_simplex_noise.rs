//! Multi-octave simplex noise matching vanilla's `PerlinSimplexNoise`.
//!
//! Used for biome temperature calculations. Not to be confused with
//! `PerlinNoise` which uses improved (gradient) noise octaves.

use std::collections::BTreeSet;

use crate::noise::SimplexNoise;
use crate::random::legacy_random::LegacyRandom;
use crate::random::{Random, RandomSource};

/// Multi-octave simplex noise generator.
///
/// Matches vanilla's `net.minecraft.world.level.levelgen.synth.PerlinSimplexNoise`.
/// Created from a set of octave levels; each octave uses a separate `SimplexNoise`.
///
/// Array layout matches vanilla: index 0 = highest frequency octave (largest
/// octave number), increasing index = decreasing frequency.
pub struct PerlinSimplexNoise {
    noise_levels: Vec<Option<SimplexNoise>>,
    highest_freq_input_factor: f64,
    highest_freq_value_factor: f64,
}

impl PerlinSimplexNoise {
    /// Create from a random source and a list of octave levels.
    ///
    /// Matches vanilla's constructor exactly: the zero octave is created first,
    /// then negative octaves (lower frequency) consume the same random, and
    /// positive octaves (higher frequency) use a derived random from the zero
    /// octave's self-evaluation.
    ///
    /// # Panics
    ///
    /// Panics if `octaves` is empty.
    #[must_use]
    pub fn new(random: &mut RandomSource, octaves: &[i32]) -> Self {
        let octave_set: BTreeSet<i32> = octaves.iter().copied().collect();
        assert!(!octave_set.is_empty(), "Need some octaves");

        // SAFETY: assert above guarantees non-empty
        let first_octave = *octave_set.first().expect("non-empty octave set");
        let last_octave = *octave_set.last().expect("non-empty octave set");
        let high_freq_octaves = last_octave;
        let total = (last_octave - first_octave + 1) as usize;

        // Zero octave is always created first (consuming random state),
        // matching vanilla's construction order
        let zero_octave = SimplexNoise::new(random);
        let zero_index = high_freq_octaves; // as i32, can be negative

        let mut noise_levels: Vec<Option<SimplexNoise>> = vec![None; total];

        // Compute seed for positive octaves before moving zero_octave
        let hf_seed = if high_freq_octaves > 0 {
            Some(
                (zero_octave.get_value_3d(zero_octave.xo, zero_octave.yo, zero_octave.zo)
                    * 9.223_372_036_854_776e18) as i64,
            )
        } else {
            None
        };

        // Place zero octave if octave 0 is in the set and index is valid
        if zero_index >= 0 && (zero_index as usize) < total && octave_set.contains(&0) {
            noise_levels[zero_index as usize] = Some(zero_octave);
        }

        // Lower-frequency octaves (negative octave numbers, array indices > zero_index)
        let start = (zero_index + 1).max(0) as usize;
        for (i, level) in noise_levels.iter_mut().enumerate().skip(start) {
            let octave_level = zero_index - i as i32;
            if octave_set.contains(&octave_level) {
                *level = Some(SimplexNoise::new(random));
            } else {
                random.consume_count(262);
            }
        }

        // Higher-frequency octaves (positive octave numbers, indices < zero_index)
        // Uses a separate random derived from the zero octave
        if let Some(seed) = hf_seed {
            let mut hf_random = RandomSource::Legacy(LegacyRandom::from_seed(seed as u64));

            for ix in (0..zero_index as usize).rev() {
                let octave_level = zero_index - ix as i32;
                if octave_set.contains(&octave_level) {
                    noise_levels[ix] = Some(SimplexNoise::new(&mut hf_random));
                } else {
                    hf_random.consume_count(262);
                }
            }
        }

        Self {
            noise_levels,
            highest_freq_input_factor: 2.0f64.powi(last_octave),
            highest_freq_value_factor: 1.0 / (2.0f64.powi(total as i32) - 1.0),
        }
    }

    /// Sample the 2D noise at the given coordinates.
    ///
    /// Matches vanilla's `getValue(x, z, false)` path (no offset applied).
    #[must_use]
    pub fn get_value(&self, x: f64, z: f64) -> f64 {
        let mut sum = 0.0;
        let mut factor = self.highest_freq_input_factor;
        let mut amplitude = self.highest_freq_value_factor;

        for noise in &self.noise_levels {
            if let Some(n) = noise {
                sum += n.get_value_2d(x * factor, z * factor) * amplitude;
            }
            factor /= 2.0;
            amplitude *= 2.0;
        }

        sum
    }
}
