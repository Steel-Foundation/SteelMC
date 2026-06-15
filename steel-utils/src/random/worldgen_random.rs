use crate::random::{
    Random, RandomSplitter, gaussian::MarsagliaPolarGaussian, xoroshiro::Xoroshiro,
};

/// Vanilla's `WorldgenRandom` when constructed for biome decoration.
///
/// Feature decoration always constructs `WorldgenRandom(new XoroshiroRandomSource(...))`.
/// Sampling then goes through `BitRandomSource.next*`, so it does not match raw
/// `XoroshiroRandomSource` for `nextInt`, bounded ints, doubles, longs, or gaussians.
pub struct WorldgenRandom {
    source: Xoroshiro,
    next_gaussian: Option<f64>,
}

impl WorldgenRandom {
    /// Creates a new `WorldgenRandom` backed by vanilla's `XoroshiroRandomSource`.
    #[must_use]
    pub const fn from_seed(seed: u64) -> Self {
        Self {
            source: Xoroshiro::from_seed(seed),
            next_gaussian: None,
        }
    }

    /// Re-seeds the backing `XoroshiroRandomSource`.
    ///
    /// Vanilla `WorldgenRandom` inherits its gaussian cache from
    /// `LegacyRandomSource`, but overrides `setSeed` to only reseed the
    /// wrapped source. That means `setDecorationSeed` / `setFeatureSeed`
    /// intentionally preserve a pending gaussian value.
    pub const fn set_seed(&mut self, seed: i64) {
        self.source.set_seed(seed);
    }

    /// Vanilla's `WorldgenRandom.setDecorationSeed`.
    pub fn set_decoration_seed(&mut self, seed: i64, block_x: i32, block_z: i32) -> i64 {
        self.set_seed(seed);
        let x_scale = self.next_i64() | 1;
        let z_scale = self.next_i64() | 1;
        let decoration_seed = i64::from(block_x)
            .wrapping_mul(x_scale)
            .wrapping_add(i64::from(block_z).wrapping_mul(z_scale))
            ^ seed;
        self.set_seed(decoration_seed);
        decoration_seed
    }

    /// Vanilla's `WorldgenRandom.setFeatureSeed`.
    pub const fn set_feature_seed(&mut self, decoration_seed: i64, feature_index: i32, step: i32) {
        let feature_seed = decoration_seed
            .wrapping_add(feature_index as i64)
            .wrapping_add(10_000_i64.wrapping_mul(step as i64));
        self.set_seed(feature_seed);
    }

    fn next_bits(&mut self, bits: u64) -> u64 {
        self.source.next_i64() as u64 >> (64 - bits)
    }
}

impl MarsagliaPolarGaussian for WorldgenRandom {
    fn stored_next_gaussian(&self) -> Option<f64> {
        self.next_gaussian
    }

    fn set_stored_next_gaussian(&mut self, value: Option<f64>) {
        self.next_gaussian = value;
    }
}

impl Random for WorldgenRandom {
    fn fork(&mut self) -> Self {
        Self {
            source: self.source.fork(),
            next_gaussian: None,
        }
    }

    fn next_i32(&mut self) -> i32 {
        self.next_bits(32) as i32
    }

    fn next_i32_bounded(&mut self, bound: i32) -> i32 {
        if bound & bound.wrapping_sub(1) == 0 {
            (i64::from(bound).wrapping_mul(i64::from(self.next_bits(31) as i32)) >> 31) as i32
        } else {
            loop {
                let sample = self.next_bits(31) as i32;
                let modulo = sample % bound;
                if sample
                    .wrapping_sub(modulo)
                    .wrapping_add(bound.wrapping_sub(1))
                    >= 0
                {
                    return modulo;
                }
            }
        }
    }

    fn next_i64(&mut self) -> i64 {
        let upper = self.next_i32();
        let lower = self.next_i32();
        (i64::from(upper) << 32).wrapping_add(i64::from(lower))
    }

    fn next_f32(&mut self) -> f32 {
        self.next_bits(24) as f32 * 5.960_464_5e-8_f32
    }

    fn next_f64(&mut self) -> f64 {
        let combined = ((self.next_bits(26) as i64) << 27) + self.next_bits(27) as i64;
        combined as f64 * (1.0 / (1_i64 << 53) as f64)
    }

    fn next_bool(&mut self) -> bool {
        self.next_bits(1) != 0
    }

    fn next_gaussian(&mut self) -> f64 {
        self.calculate_gaussian()
    }

    fn next_positional(&mut self) -> RandomSplitter {
        self.source.next_positional()
    }
}

#[cfg(test)]
mod tests {
    use super::WorldgenRandom;
    use crate::random::Random;

    #[test]
    fn set_decoration_seed_matches_vanilla_trace() {
        let mut random = WorldgenRandom::from_seed(0);
        assert_eq!(
            random.set_decoration_seed(13_579, -6_695_392, 5_868_656),
            7_632_291_757_650_236_667,
        );
    }

    #[test]
    fn feature_seed_matches_vanilla_first_ore_dirt_origin() {
        let mut random = WorldgenRandom::from_seed(0);
        let decoration_seed = random.set_decoration_seed(13_579, -6_695_392, 5_868_656);
        random.set_feature_seed(decoration_seed, 0, 6);

        let x = -6_695_392 + random.next_i32_bounded(16);
        let z = 5_868_656 + random.next_i32_bounded(16);
        let y = random.next_i32_bounded(161);
        assert_eq!((x, y, z), (-6_695_386, 149, 5_868_662));
    }

    #[test]
    #[expect(
        clippy::float_cmp,
        reason = "gaussian cache parity must match vanilla exactly"
    )]
    fn feature_seed_preserves_pending_gaussian() {
        let mut random = WorldgenRandom::from_seed(123);
        let _ = random.next_gaussian();
        random.set_feature_seed(456, 7, 8);

        let mut cached_reference = WorldgenRandom::from_seed(123);
        let _ = cached_reference.next_gaussian();
        assert_eq!(random.next_gaussian(), cached_reference.next_gaussian());

        let mut reseeded_reference = WorldgenRandom::from_seed(0);
        reseeded_reference.set_feature_seed(456, 7, 8);
        assert_eq!(random.next_gaussian(), reseeded_reference.next_gaussian());
    }
}
