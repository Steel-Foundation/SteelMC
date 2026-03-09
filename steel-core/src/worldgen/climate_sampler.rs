//! Climate sampler for overworld world generation.
//!
//! Uses the compiled overworld density functions from steel-registry for fast
//! evaluation, bypassing the runtime tree interpreter entirely.
//!
//! This is overworld-specific because it uses `OverworldNoises` and the overworld
//! noise router (`router_temperature`, `router_vegetation`, etc.). Other dimensions
//! need their own climate samplers with their own transpiled density functions.

use steel_registry::density_functions::{self, OverworldColumnCache, OverworldNoises};
use steel_registry::noise_parameters::get_noise_parameters;
use steel_utils::climate::{TargetPoint, quantize_coord};
use steel_utils::random::{Random, xoroshiro::Xoroshiro};

/// Climate sampler for the overworld using compiled density functions.
///
/// Evaluates the overworld noise router (temperature, vegetation, continentalness,
/// erosion, depth, ridges) to produce `TargetPoint` values for biome lookup.
///
// TODO: Implement `spawn_target()` matching vanilla's `OverworldBiomeBuilder.spawnTarget()`
// and `Climate.Sampler.spawnTarget` for spawn point selection.
pub struct OverworldClimateSampler {
    /// All noise generators needed by the overworld density functions.
    /// Boxed because `OverworldNoises` is ~5600 bytes (35 `NormalNoise` fields).
    noises: Box<OverworldNoises>,
}

impl OverworldClimateSampler {
    /// Create a new overworld climate sampler with the given seed.
    #[must_use]
    pub fn new(seed: u64) -> Self {
        let mut rng = Xoroshiro::from_seed(seed);
        let splitter = rng.next_positional();
        let noise_params = get_noise_parameters();
        let noises = OverworldNoises::create(&splitter, &noise_params);

        Self {
            noises: Box::new(noises),
        }
    }

    /// Sample climate at a quart position.
    ///
    /// The `cache` holds column-level (xz-only) precomputed values.
    /// It should persist across calls for the same chunk to avoid redundant
    /// noise evaluations when only `y` changes.
    #[must_use]
    pub fn sample(
        &self,
        quart_x: i32,
        quart_y: i32,
        quart_z: i32,
        cache: &mut OverworldColumnCache,
    ) -> TargetPoint {
        let block_x = quart_x << 2;
        let block_y = quart_y << 2;
        let block_z = quart_z << 2;

        // Ensure column cache is populated for this (x, z)
        cache.ensure(block_x, block_z, &self.noises);

        // Density functions return f64 but vanilla truncates to float before quantizing.
        // The f64→f32→f64 round-trip through quantize_coord is intentional for parity.
        let temp =
            density_functions::router_temperature(&self.noises, cache, block_x, block_y, block_z)
                as f32;
        let humidity =
            density_functions::router_vegetation(&self.noises, cache, block_x, block_y, block_z)
                as f32;
        let cont = density_functions::router_continentalness(
            &self.noises,
            cache,
            block_x,
            block_y,
            block_z,
        ) as f32;
        let erosion =
            density_functions::router_erosion(&self.noises, cache, block_x, block_y, block_z)
                as f32;
        let depth =
            density_functions::router_depth(&self.noises, cache, block_x, block_y, block_z) as f32;
        let weirdness =
            density_functions::router_ridges(&self.noises, cache, block_x, block_y, block_z) as f32;

        TargetPoint::new(
            quantize_coord(f64::from(temp)),
            quantize_coord(f64::from(humidity)),
            quantize_coord(f64::from(cont)),
            quantize_coord(f64::from(erosion)),
            quantize_coord(f64::from(depth)),
            quantize_coord(f64::from(weirdness)),
        )
    }
}
