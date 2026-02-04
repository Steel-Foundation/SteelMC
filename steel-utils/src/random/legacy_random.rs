use crate::random::{
    PositionalRandom, Random, RandomSource, RandomSplitter, gaussian::MarsagliaPolarGaussian,
    get_seed,
};

/// Legacy Minecraft random number generator based on a Linear Congruential Generator (LCG).
/// This implementation mirrors Java's `java.util.Random` which Minecraft originally used.
pub struct LegacyRandom {
    seed: i64,
    next_gauissian: Option<f64>,
}

/// A positional random number generator factory for the legacy Minecraft LCG algorithm.
/// This can create random sources based on position, hash, or seed.
pub struct LegacyRandomSplitter {
    seed: i64,
}

impl LegacyRandom {
    /// Creates a new `LegacyRandom` instance from the given seed.
    /// The seed is `XORed` with the LCG multiplier and masked to 48 bits, matching Java's behavior.
    #[must_use]
    pub const fn from_seed(seed: u64) -> Self {
        Self {
            seed: (seed as i64 ^ 0x0005_DEEC_E66D) & 0xFFFF_FFFF_FFFF,
            next_gauissian: None,
        }
    }

    const fn next(&mut self, bits: u64) -> i32 {
        (self.next_random() >> (48 - bits)) as i32
    }

    const fn next_random(&mut self) -> i64 {
        let l = self.seed;
        let m = l.wrapping_mul(0x0005_DEEC_E66D).wrapping_add(0xB) & 0xFFFF_FFFF_FFFF;
        self.seed = m;
        m
    }
}

impl MarsagliaPolarGaussian for LegacyRandom {
    fn stored_next_gaussian(&self) -> Option<f64> {
        self.next_gauissian
    }

    fn set_stored_next_gaussian(&mut self, value: Option<f64>) {
        self.next_gauissian = value;
    }
}

impl Random for LegacyRandom {
    fn fork(&mut self) -> Self {
        Self::from_seed(self.next_i64() as u64)
    }

    fn next_i32(&mut self) -> i32 {
        self.next(32)
    }

    fn next_i32_bounded(&mut self, bound: i32) -> i32 {
        if bound & bound.wrapping_sub(1) == 0 {
            (i64::from(bound).wrapping_mul(i64::from(self.next(31))) >> 31) as i32
        } else {
            loop {
                let i = self.next(31);
                let j = i % bound;
                if i.wrapping_sub(j).wrapping_add(bound.wrapping_sub(1)) >= 0 {
                    return j;
                }
            }
        }
    }

    fn next_i64(&mut self) -> i64 {
        let i = self.next_i32();
        let j = self.next_i32();
        (i64::from(i) << 32).wrapping_add(i64::from(j))
    }

    fn next_f32(&mut self) -> f32 {
        self.next(24) as f32 * 5.960_464_5e-8_f32
    }

    fn next_f64(&mut self) -> f64 {
        (((self.next(26) as u64) << 27) | (self.next(27) as u64)) as f64
            * f64::from(1.110_223e-16_f32)
    }

    fn next_bool(&mut self) -> bool {
        self.next(1) != 0
    }

    fn next_gaussian(&mut self) -> f64 {
        self.calculate_gaussian()
    }

    fn next_positional(&mut self) -> RandomSplitter {
        RandomSplitter::Legacy(LegacyRandomSplitter::new(self.next_i64()))
    }
}

impl LegacyRandomSplitter {
    /// Creates a new `LegacyRandomSplitter` with the given seed.
    /// This seed is used to initialize positional random sources.
    #[must_use]
    pub const fn new(seed: i64) -> Self {
        Self { seed }
    }
}

impl PositionalRandom for LegacyRandomSplitter {
    fn at(&self, x: i32, y: i32, z: i32) -> RandomSource {
        let seed = get_seed(x, y, z);
        RandomSource::Legacy(LegacyRandom::from_seed((seed as u64) ^ (self.seed as u64)))
    }

    fn with_hash_of(&self, name: &str) -> RandomSource {
        let mut hash = 0_i32;
        for b in name.encode_utf16() {
            hash = hash.wrapping_mul(31).wrapping_add(i32::from(b));
        }
        RandomSource::Legacy(LegacyRandom::from_seed((hash as u64) ^ (self.seed as u64)))
    }

    fn with_seed(&self, seed: u64) -> RandomSource {
        RandomSource::Legacy(LegacyRandom::from_seed(seed))
    }
}

#[cfg(test)]
mod test {
    use crate::random::{PositionalRandom, Random, RandomSplitter};

    use super::LegacyRandom;

    #[test]
    fn test_next_i32() {
        let mut rand = LegacyRandom::from_seed(0);

        let values = [
            -1_155_484_576,
            -723_955_400,
            1_033_096_058,
            -1_690_734_402,
            -1_557_280_266,
            1_327_362_106,
            -1_930_858_313,
            502_539_523,
            -1_728_529_858,
            -938_301_587,
        ];

        for value in values {
            assert_eq!(rand.next_i32(), value);
        }
    }

    #[test]
    fn test_next_i32_bounded() {
        let mut rand = LegacyRandom::from_seed(0);

        let values = [0, 13, 4, 2, 5, 8, 11, 6, 9, 14];

        for value in values {
            assert_eq!(rand.next_i32_bounded(0xf), value);
        }

        let mut rand = LegacyRandom::from_seed(0);
        for _ in 0..10 {
            assert_eq!(rand.next_i32_bounded(1), 0);
        }

        let mut rand = LegacyRandom::from_seed(0);
        let values = [1, 1, 0, 1, 1, 0, 1, 0, 1, 1];
        for value in values {
            assert_eq!(rand.next_i32_bounded(2), value);
        }
    }

    #[test]
    fn test_next_i32_between() {
        let mut rand = LegacyRandom::from_seed(0);

        let values = [1, 5, 2, 12, 12, 6, 12, 10, 4, 3];

        for value in values {
            assert_eq!(rand.next_i32_between(1, 12), value);
        }
    }

    #[test]
    fn test_next_i32_between_exclusive() {
        let mut rand = LegacyRandom::from_seed(0);

        let values = [1, 7, 9, 6, 7, 3, 3, 7, 3, 1];

        for value in values {
            assert_eq!(rand.next_i32_between_exclusive(1, 12), value);
        }
    }

    #[test]
    #[allow(clippy::float_cmp)]
    fn test_next_f64() {
        let mut rand = LegacyRandom::from_seed(0);

        let values = [
            0.730_967_787_376_657,
            0.240_536_415_671_485_87,
            0.637_417_425_350_108_3,
            0.550_437_005_117_633_9,
            0.597_545_277_797_201_8,
            0.333_218_399_476_649_8,
            0.385_189_184_740_718_5,
            0.984_841_540_199_809,
            0.879_182_517_872_480_1,
            0.941_249_179_482_114_4,
        ];

        for value in values {
            assert_eq!(rand.next_f64(), value);
        }
    }

    #[test]
    #[allow(clippy::float_cmp)]
    fn test_next_f32() {
        let mut rand = LegacyRandom::from_seed(0);

        let values: [f32; 10] = [
            0.730_967_76,
            0.831_441,
            0.240_536_39,
            0.606_345_2,
            0.637_417_4,
            0.309_050_56,
            0.550_437,
            0.117_006_6,
            0.597_545_27,
            0.781_534_6,
        ];

        for value in values {
            assert_eq!(rand.next_f32(), value);
        }
    }

    #[test]
    fn test_next_i64() {
        let mut rand = LegacyRandom::from_seed(0);

        let values: [i64; 10] = [
            -4_962_768_465_676_381_896,
            4_437_113_781_045_784_766,
            -6_688_467_811_848_818_630,
            -8_292_973_307_042_192_125,
            -7_423_979_211_207_825_555,
            6_146_794_652_083_548_235,
            7_105_486_291_024_734_541,
            -279_624_296_851_435_688,
            -2_228_689_144_322_150_137,
            -1_083_761_183_081_836_303,
        ];

        for value in values {
            assert_eq!(rand.next_i64(), value);
        }
    }

    #[test]
    fn test_next_bool() {
        let mut rand = LegacyRandom::from_seed(0);

        let values = [
            true, true, false, true, true, false, true, false, true, true,
        ];

        for value in values {
            assert_eq!(rand.next_bool(), value);
        }
    }

    #[test]
    #[allow(clippy::float_cmp)]
    fn test_next_gaussian() {
        let mut rand = LegacyRandom::from_seed(0);

        let values = [
            0.802_533_063_739_030_5,
            -0.901_546_088_417_512_2,
            2.080_920_790_428_163,
            0.763_770_768_436_489_4,
            0.984_574_532_882_512_8,
            -1.683_412_258_767_342_8,
            -0.027_290_262_907_887_285,
            0.115_245_702_862_023_15,
            -0.390_167_041_379_937_74,
            -0.643_388_813_126_449,
        ];

        for value in values {
            assert_eq!(rand.next_gaussian(), value);
        }
    }

    #[test]
    #[allow(clippy::float_cmp)]
    fn test_triangle() {
        let mut rand = LegacyRandom::from_seed(0);

        let values = [
            124.521_568_585_258_56,
            104.349_021_011_623_72,
            113.216_343_916_027_6,
            70.017_382_227_045_47,
            96.896_666_919_518_28,
            107.302_840_758_085_41,
            106.168_176_758_131_44,
            79.112_644_826_080_78,
            73.967_216_139_270_62,
            81.724_195_210_806_46,
        ];

        for value in values {
            assert_eq!(rand.triangle(100_f64, 50_f64), value);
        }
    }

    #[test]
    fn test_fork() {
        let mut original_rand = LegacyRandom::from_seed(0);
        assert_eq!(original_rand.next_i64(), -4_962_768_465_676_381_896_i64);

        let mut original_rand = LegacyRandom::from_seed(0);
        {
            let RandomSplitter::Legacy(splitter) = original_rand.next_positional() else {
                unreachable!()
            };
            assert_eq!(splitter.seed, -4_962_768_465_676_381_896_i64);

            let mut rand = splitter.with_hash_of("minecraft:offset");
            assert_eq!(rand.next_i32(), 103_436_829);
        }

        let mut original_rand = LegacyRandom::from_seed(0);
        let mut new_rand = original_rand.fork();
        {
            let splitter = new_rand.next_positional();

            let mut rand1 = splitter.with_hash_of("TEST STRING");
            assert_eq!(rand1.next_i32(), -1_170_413_697);

            let mut rand2 = splitter.with_seed(10);
            assert_eq!(rand2.next_i32(), -1_157_793_070);

            let mut rand3 = splitter.at(1, 11, -111);
            assert_eq!(rand3.next_i32(), -1_213_890_343);
        }

        assert_eq!(original_rand.next_i32(), 1_033_096_058);
        assert_eq!(new_rand.next_i32(), -888_301_832);
    }
}
