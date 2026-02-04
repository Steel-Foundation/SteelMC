use crate::random::{
    PositionalRandom, Random, RandomSource, RandomSplitter, gaussian::MarsagliaPolarGaussian,
    get_seed,
};

// Ratios used in the mix functions
const GOLDEN_RATIO_64: u64 = 0x9E37_79B9_7F4A_7C15;
const SILVER_RATIO_64: u64 = 0x6A09_E667_F3BC_C909;

/// A Xoroshiro128++ random number generator.
pub struct Xoroshiro {
    seed_lo: u64,
    seed_hi: u64,
    next_gaussian: Option<f64>,
}

/// A splitter for the Xoroshiro128++ random number generator.
pub struct XoroshiroSplitter {
    seed_lo: u64,
    seed_hi: u64,
}

impl Xoroshiro {
    /// Creates a new `Xoroshiro` from a seed.
    #[must_use]
    pub const fn from_seed(seed: u64) -> Self {
        // From RandomSupport
        let (lo, hi) = Self::upgrade_seed_to_128_bit(seed);
        let lo = mix_stafford_13(lo);
        let hi = mix_stafford_13(hi);
        Self::new(lo, hi)
    }

    /// Creates a new `Xoroshiro` from a seed without mixing.
    #[must_use]
    pub const fn from_seed_unmixed(seed: u64) -> Self {
        // From RandomSupport and
        let (lo, hi) = Self::upgrade_seed_to_128_bit(seed);
        Self::new(lo, hi)
    }

    const fn new(lo: u64, hi: u64) -> Self {
        let (lo, hi) = if (lo | hi) == 0 {
            (GOLDEN_RATIO_64, SILVER_RATIO_64)
        } else {
            (lo, hi)
        };
        Self {
            seed_lo: lo,
            seed_hi: hi,
            next_gaussian: None,
        }
    }

    const fn upgrade_seed_to_128_bit(seed: u64) -> (u64, u64) {
        let lo = seed ^ SILVER_RATIO_64;
        let hi = lo.wrapping_add(GOLDEN_RATIO_64);
        (lo, hi)
    }

    const fn next(&mut self, bits: u64) -> u64 {
        self.next_random() >> (64 - bits)
    }

    const fn next_random(&mut self) -> u64 {
        let l = self.seed_lo;
        let m = self.seed_hi;
        let n = l.wrapping_add(m).rotate_left(17).wrapping_add(l);
        let m = m ^ l;
        self.seed_lo = l.rotate_left(49) ^ m ^ (m << 21);
        self.seed_hi = m.rotate_left(28);
        n
    }
}

#[allow(missing_docs)]
impl MarsagliaPolarGaussian for Xoroshiro {
    fn stored_next_gaussian(&self) -> Option<f64> {
        self.next_gaussian
    }

    fn set_stored_next_gaussian(&mut self, value: Option<f64>) {
        self.next_gaussian = value;
    }
}

const fn mix_stafford_13(z: u64) -> u64 {
    let z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    let z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

#[allow(missing_docs)]
impl Random for Xoroshiro {
    fn fork(&mut self) -> Self {
        Self::new(self.next_random(), self.next_random())
    }

    fn next_i32(&mut self) -> i32 {
        self.next_random() as i32
    }

    fn next_i32_bounded(&mut self, bound: i32) -> i32 {
        let mut l = (self.next_i32() as u64) & 0xFFFF_FFFF;
        let mut m = l.wrapping_mul(bound as u64);
        let mut n = m & 0xFFFF_FFFF;
        if n < bound as u64 {
            let i = (((!bound).wrapping_add(1)) as u64) % (bound as u64);
            while n < i {
                l = (self.next_i32() as u64) & 0xFFFF_FFFF;
                m = l.wrapping_mul(bound as u64);
                n = m & 0xFFFF_FFFF;
            }
        }
        let o = m >> 32;
        o as i32
    }

    fn next_i64(&mut self) -> i64 {
        self.next_random() as i64
    }

    fn next_f32(&mut self) -> f32 {
        self.next(24) as f32 * 5.960_464_5e-8
    }

    fn next_f64(&mut self) -> f64 {
        self.next(53) as f64 * f64::from(1.110_223e-16_f32)
    }

    fn next_bool(&mut self) -> bool {
        (self.next_random() & 1) != 0
    }

    fn next_gaussian(&mut self) -> f64 {
        self.calculate_gaussian()
    }

    fn next_positional(&mut self) -> RandomSplitter {
        RandomSplitter::Xoroshiro(XoroshiroSplitter {
            seed_lo: self.next_random(),
            seed_hi: self.next_random(),
        })
    }
}

#[allow(missing_docs)]
impl PositionalRandom for XoroshiroSplitter {
    #[allow(clippy::many_single_char_names)]
    fn at(&self, x: i32, y: i32, z: i32) -> RandomSource {
        let l = get_seed(x, y, z) as u64;
        let m = l ^ self.seed_lo;

        RandomSource::Xoroshiro(Xoroshiro::new(m, self.seed_hi))
    }

    fn with_hash_of(&self, name: &str) -> RandomSource {
        let bytes = md5::compute(name.as_bytes());
        let l = u64::from_be_bytes(
            bytes[0..8]
                .try_into()
                .expect("slice with 8 elements to array"),
        );
        let m = u64::from_be_bytes(
            bytes[8..16]
                .try_into()
                .expect("slice with 8 elements to array"),
        );
        RandomSource::Xoroshiro(Xoroshiro::new(l ^ self.seed_lo, m ^ self.seed_hi))
    }

    fn with_seed(&self, seed: u64) -> RandomSource {
        RandomSource::Xoroshiro(Xoroshiro::new(seed ^ self.seed_lo, seed ^ self.seed_hi))
    }
}

#[cfg(test)]
#[allow(
    clippy::unreadable_literal,
    clippy::cast_sign_loss,
    clippy::items_after_statements,
    clippy::float_cmp
)]
mod tests {
    use super::*;
    use crate::random::{PositionalRandom, Random};

    // Values checked against results from the equivalent Java source

    const MIX_STAFFORD_13_TEST_CASES: &[(u64, i64)] = &[
        (0, 0),
        (1, 6238072747940578789),
        (64, -8456553050427055661),
        (4096, -1125827887270283392),
        (262144, -120227641678947436),
        (16777216, 6406066033425044679),
        (1073741824, 3143522559155490559),
        (16, -2773008118984693571),
        (1024, 8101005175654470197),
        (65536, -3551754741763842827),
        (4194304, -2737109459693184599),
        (2, -2606959012126976886),
        (128, -5825874238589581082),
        (8192, 1111983794319025228),
        (524288, -7964047577924347155),
        (33554432, -5634612006859462257),
        (2147483648, -1436547171018572641),
        (137438953472, -4514638798598940860),
        (8796093022208, -610572083552328405),
        (562949953421312, -263574021372026223),
        (36028797018963968, 7868130499179604987),
        (253, -4045451768301188906),
        (127, -6873224393826578139),
        (8447, 6670985465942597767),
        (524543, -6228499289678716485),
        (33554687, 2630391896919662492),
        (2147483903, -6879633228472053040),
        (137438953727, -5817997684975131823),
        (8796093022463, 2384436581894988729),
        (562949953421567, -5076179956679497213),
        (36028797018964223, -5993365784811617721),
    ];

    #[test]
    fn test_mix_stafford_13() {
        for &(input, expected) in MIX_STAFFORD_13_TEST_CASES {
            assert_eq!(
                mix_stafford_13(input),
                expected as u64,
                "mix_stafford_13({input}) failed"
            );
        }
    }

    #[test]
    fn next_i32_matches_java() {
        const EXPECTED: [i32; 10] = [
            -160476802,
            781697906,
            653572596,
            1337520923,
            -505875771,
            -47281585,
            342195906,
            1417498593,
            -1478887443,
            1560080270,
        ];

        let mut rng = Xoroshiro::from_seed(0);
        for &expected in &EXPECTED {
            assert_eq!(rng.next_i32(), expected);
        }
    }

    #[test]
    fn next_i32_bounded() {
        const SMALL_EXPECTED: [i32; 10] = [9, 1, 1, 3, 8, 9, 0, 3, 6, 3];
        const LARGE_EXPECTED: [i32; 10] = [
            9784805, 470346, 13560642, 7320226, 14949645, 13460529, 2824352, 10938308, 14146127,
            4549185,
        ];
        let mut rng = Xoroshiro::from_seed(0);

        for &expected in &SMALL_EXPECTED {
            assert_eq!(rng.next_i32_bounded(10), expected);
        }

        for &expected in &LARGE_EXPECTED {
            assert_eq!(rng.next_i32_bounded(0xFF_FFFF), expected);
        }
    }

    #[test]
    fn next_i32_between_inclusive() {
        const EXPECTED: [i32; 10] = [99, 59, 57, 65, 94, 100, 54, 66, 83, 68];

        let mut rng = Xoroshiro::from_seed(0);
        for &expected in &EXPECTED {
            assert_eq!(rng.next_i32_between(50, 100), expected);
        }
    }

    #[test]
    fn next_i32_between_exclusive() {
        const EXPECTED: [i32; 10] = [98, 59, 57, 65, 94, 99, 53, 66, 82, 68];

        let mut rng = Xoroshiro::from_seed(0);
        for &expected in &EXPECTED {
            assert_eq!(rng.next_i32_between_exclusive(50, 100), expected);
        }
    }

    #[test]
    fn next_f64_matches_java() {
        const EXPECTED: [f64; 10] = [
            0.16474369376959186,
            0.7997457290026366,
            0.2511961888876212,
            0.11712489470639631,
            0.0997124786680137,
            0.7566797430601416,
            0.7723285712021574,
            0.9420469457586381,
            0.48056202536813664,
            0.6099690583914598,
        ];

        let mut rng = Xoroshiro::from_seed(0);
        for &expected in &EXPECTED {
            assert_eq!(rng.next_f64(), expected);
        }
    }

    #[test]
    fn next_f32_matches_java() {
        const EXPECTED: [f32; 10] = [
            0.16474366,
            0.7997457,
            0.25119615,
            0.117124856,
            0.09971243,
            0.7566797,
            0.77232856,
            0.94204694,
            0.48056197,
            0.609969,
        ];

        let mut rng = Xoroshiro::from_seed(0);
        for &expected in &EXPECTED {
            assert_eq!(rng.next_f32(), expected);
        }
    }

    #[test]
    fn next_i64_matches_java() {
        const EXPECTED: [i64; 10] = [
            3038984756725240190,
            -3694039286755638414,
            4633751808701151732,
            2160572957309072155,
            1839370574944072389,
            -4488466507718817201,
            -4199796579929588030,
            -1069045159880208415,
            8864804693509535725,
            -7194800960680693874,
        ];

        let mut rng = Xoroshiro::from_seed(0);
        for &expected in &EXPECTED {
            assert_eq!(rng.next_i64(), expected);
        }
    }

    #[test]
    fn next_bool_matches_java() {
        const EXPECTED: [bool; 10] = [
            false, false, false, true, true, true, false, true, true, false,
        ];

        let mut rng = Xoroshiro::from_seed(0);
        for &expected in &EXPECTED {
            assert_eq!(rng.next_bool(), expected);
        }
    }

    #[test]
    fn next_gaussian_matches_java() {
        const EXPECTED: [f64; 10] = [
            -0.48540690699780015,
            0.43399227545320296,
            -0.3283265251019599,
            -0.5052497078202575,
            -0.3772512828630807,
            0.2419080215945433,
            -0.42622066207565135,
            2.411315261138953,
            -1.1419147030553274,
            -0.05849758093810378,
        ];

        let mut rng = Xoroshiro::from_seed(0);
        for &expected in &EXPECTED {
            assert_eq!(rng.next_gaussian(), expected);
        }
    }

    #[test]
    fn triangle_matches_java() {
        const EXPECTED: [f64; 10] = [
            6.824989823834776,
            10.670356470906125,
            6.71516367803936,
            9.151408127217596,
            9.352964834883384,
            8.291618967842293,
            8.954549938640508,
            11.833001837470519,
            10.65851306020791,
            11.684676364031647,
        ];

        let mut rng = Xoroshiro::from_seed(0);
        for &expected in &EXPECTED {
            assert_eq!(rng.triangle(10.0, 5.0), expected);
        }
    }

    #[test]
    fn fork_creates_independent_rng() {
        let mut rng = Xoroshiro::from_seed(0);
        let mut forked = rng.fork();

        assert_eq!(forked.next_i32(), 542195535);
        assert_eq!(rng.next_i32(), 653572596);
    }

    #[test]
    fn positional_random_splitter() {
        let mut rng = Xoroshiro::from_seed(0);
        let mut forked = rng.fork();

        assert_eq!(forked.next_i32(), 542195535);

        let splitter = forked.next_positional();

        let RandomSource::Xoroshiro(mut rand1) = splitter.with_hash_of("TEST STRING") else {
            panic!("Expected Xoroshiro variant");
        };
        assert_eq!(rand1.next_i32(), -641435713);

        let RandomSource::Xoroshiro(mut rand2) = splitter.with_seed(42069) else {
            panic!("Expected Xoroshiro variant");
        };
        assert_eq!(rand2.next_i32(), -340700677);

        let RandomSource::Xoroshiro(mut rand3) = splitter.at(1337, 80085, -69420) else {
            panic!("Expected Xoroshiro variant");
        };
        assert_eq!(rand3.next_i32(), 790449132);

        assert_eq!(rng.next_i32(), 653572596);
        assert_eq!(forked.next_i32(), 435917842);
    }

    #[test]
    fn zero_seed_produces_fallback_values() {
        let mut rng = Xoroshiro::new(0, 0);
        assert_eq!(rng.next_i64(), 6807859099481836695);
    }
}
