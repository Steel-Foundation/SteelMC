//! Bit-exact noise comparisons against `SteelExtractor` output.

use serde::Deserialize;
use steel_worldgen::noise::{BlendedNoise, NormalNoise, PerlinSimplexNoise};
use steel_worldgen::random::{RandomSource, legacy_random::LegacyRandom};

#[derive(Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
enum Sampler {
    Normal,
    LegacyNether,
    Blended,
    FrozenTemperature,
}

#[derive(Deserialize)]
struct NoiseSample {
    sampler: Sampler,
    seed: i64,
    x: i32,
    y: i32,
    z: i32,
    value_bits: u32,
}

#[test]
fn float_noise_matches_pre1_extractor() -> Result<(), serde_json::Error> {
    let samples: Vec<NoiseSample> =
        serde_json::from_str(include_str!("../test_assets/noise_samples.json"))?;
    for sample in samples {
        let x = f64::from(sample.x);
        let y = f64::from(sample.y);
        let z = f64::from(sample.z);
        let mut random = RandomSource::Legacy(LegacyRandom::from_seed(sample.seed as u64));
        let actual = match sample.sampler {
            Sampler::Normal => {
                NormalNoise::create_from_random(&mut random, -7, &[1.0, 1.0]).get_value_f32(x, y, z)
            }
            Sampler::LegacyNether => {
                NormalNoise::create_legacy_nether_biome(&mut random, -7, &[1.0, 1.0])
                    .get_value_f32(x, y, z)
            }
            Sampler::Blended => {
                BlendedNoise::new(&mut random, 0.25, 0.125, 80.0, 160.0, 8.0).compute(x, y, z)
            }
            Sampler::FrozenTemperature => {
                let mut random = RandomSource::Legacy(LegacyRandom::from_seed(3456));
                PerlinSimplexNoise::new(&mut random, &[-2, -1, 0]).get_value(x * 0.05, z * 0.05)
                    as f32
            }
        };
        assert_eq!(
            actual.to_bits(),
            sample.value_bits,
            "{:?}, seed {}, ({x}, {y}, {z})",
            sample.sampler,
            sample.seed
        );
    }
    Ok(())
}
