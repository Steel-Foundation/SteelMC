//! Climate sampler for vanilla world generation.
//!
//! This module bridges the density functions from steel-registry
//! with the climate sampling system in steel-utils.

use std::sync::Arc;

use rustc_hash::FxHashMap;

use steel_registry::density_functions;
use steel_registry::noise_parameters::get_noise_parameters;
use steel_utils::climate::{TargetPoint, quantize_coord};
use steel_utils::density::{DensityContext, DensityFunction, DensityFunctionOps, EvalCache};
use steel_utils::noise::NormalNoise;
use steel_utils::random::{Random, xoroshiro::Xoroshiro};

/// Build the density function registry from generated code.
///
/// Returns unresolved `DensityFunction` values; call
/// [`DensityFunction::resolve`] on each root to bake noises + cross-references.
fn build_density_function_registry() -> FxHashMap<String, Arc<DensityFunction>> {
    let mut registry = FxHashMap::default();

    let ids = [
        "minecraft:overworld/continents",
        "minecraft:overworld/erosion",
        "minecraft:overworld/depth",
        "minecraft:overworld/ridges",
        "minecraft:overworld/ridges_folded",
        "minecraft:overworld/offset",
        "minecraft:overworld/factor",
        "minecraft:overworld/jaggedness",
        "minecraft:overworld/sloped_cheese",
        "minecraft:overworld/base_3d_noise",
        "minecraft:overworld/caves/entrances",
        "minecraft:overworld/caves/spaghetti_2d",
        "minecraft:overworld/caves/spaghetti_2d_thickness_modulator",
        "minecraft:overworld/caves/spaghetti_roughness_function",
        "minecraft:overworld/caves/pillars",
        "minecraft:overworld/caves/noodle",
        "minecraft:shift_x",
        "minecraft:shift_z",
        "minecraft:y",
        "minecraft:zero",
    ];

    for id in ids {
        if let Some(func) = density_functions::get_density_function(id) {
            registry.insert(id.to_string(), func);
        }
    }

    registry
}

/// Climate sampler that uses the extracted vanilla density functions.
pub struct VanillaClimateSampler {
    /// Temperature density function (resolved with baked noises)
    temperature: Arc<DensityFunction>,
    /// Humidity/vegetation density function
    humidity: Arc<DensityFunction>,
    /// Continentalness density function
    continentalness: Arc<DensityFunction>,
    /// Erosion density function
    erosion: Arc<DensityFunction>,
    /// Depth density function
    depth: Arc<DensityFunction>,
    /// Weirdness/ridges density function
    weirdness: Arc<DensityFunction>,
}

impl VanillaClimateSampler {
    /// Create a new vanilla climate sampler with the given seed.
    #[must_use]
    pub fn new(seed: u64) -> Self {
        // Create random number generator from seed
        let mut rng = Xoroshiro::from_seed(seed);
        let splitter = rng.next_positional();

        // Build noise generators from vanilla datapack parameters
        let noise_param_map = get_noise_parameters();
        let mut noises = FxHashMap::default();
        for (id, params) in &noise_param_map {
            let noise = NormalNoise::create(&splitter, id, params.first_octave, &params.amplitudes);
            noises.insert(id.clone(), noise);
        }

        // Build density function registry from generated code (unresolved)
        let registry = build_density_function_registry();

        // Get the overworld noise router (uses runtime types directly)
        let router = &*density_functions::OVERWORLD_NOISE_ROUTER;

        // Resolve each density function (bakes noises + cross-references)
        let temperature = Arc::new(router.temperature.resolve(&registry, &noises));
        let humidity = Arc::new(router.vegetation.resolve(&registry, &noises));
        let continentalness = Arc::new(router.continentalness.resolve(&registry, &noises));
        let erosion = Arc::new(router.erosion.resolve(&registry, &noises));
        let depth = Arc::new(router.depth.resolve(&registry, &noises));
        let weirdness = Arc::new(router.ridges.resolve(&registry, &noises));

        Self {
            temperature,
            humidity,
            continentalness,
            erosion,
            depth,
            weirdness,
        }
    }

    /// Sample climate at a quart position using a reusable cache.
    ///
    /// The cache should persist across multiple calls (e.g. for an entire chunk)
    /// to benefit from `FlatCache` (caches by x,z) and `CacheOnce` (caches last position).
    #[must_use]
    pub fn sample(
        &self,
        quart_x: i32,
        quart_y: i32,
        quart_z: i32,
        cache: &mut EvalCache,
    ) -> TargetPoint {
        let block_x = quart_x << 2;
        let block_y = quart_y << 2;
        let block_z = quart_z << 2;

        let ctx = DensityContext::new(block_x, block_y, block_z);

        let temp = self.temperature.compute_cached(&ctx, cache) as f32;
        let humidity = self.humidity.compute_cached(&ctx, cache) as f32;
        let cont = self.continentalness.compute_cached(&ctx, cache) as f32;
        let erosion = self.erosion.compute_cached(&ctx, cache) as f32;
        let depth = self.depth.compute_cached(&ctx, cache) as f32;
        let weirdness = self.weirdness.compute_cached(&ctx, cache) as f32;

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
