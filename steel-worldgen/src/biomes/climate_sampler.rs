//! Climate sampler for overworld world generation.
//!
//! Uses the compiled overworld density functions from steel-registry for fast
//! evaluation, bypassing the runtime tree interpreter entirely.
//!
//! This is overworld-specific because it uses `OverworldNoises` and the overworld
//! noise router (`router_temperature`, `router_vegetation`, etc.). Other dimensions
//! need their own climate samplers with their own transpiled density functions.

use std::f32::consts::TAU;

use steel_utils::BlockPos;
use steel_utils::climate::{Parameter, ParameterPoint, TargetPoint, quantize_coord};
use steel_utils::random::{Random, xoroshiro::Xoroshiro};
use steel_worldgen::density_functions::overworld::{self, OverworldColumnCache, OverworldNoises};
use steel_worldgen::noise_parameters::get_noise_parameters;

/// Climate sampler for the overworld using compiled density functions.
///
/// Evaluates the overworld noise router (temperature, vegetation, continentalness,
/// erosion, depth, ridges) to produce `TargetPoint` values for biome lookup.
///
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
        let noises = OverworldNoises::create(seed, &splitter, &noise_params);

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
            overworld::router_temperature(&self.noises, cache, block_x, block_y, block_z) as f32;
        let humidity =
            overworld::router_vegetation(&self.noises, cache, block_x, block_y, block_z) as f32;
        let cont = overworld::router_continentalness(&self.noises, cache, block_x, block_y, block_z)
            as f32;
        let erosion =
            overworld::router_erosion(&self.noises, cache, block_x, block_y, block_z) as f32;
        let depth = overworld::router_depth(&self.noises, cache, block_x, block_y, block_z) as f32;
        let weirdness =
            overworld::router_ridges(&self.noises, cache, block_x, block_y, block_z) as f32;

        TargetPoint::new(
            quantize_coord(f64::from(temp)),
            quantize_coord(f64::from(humidity)),
            quantize_coord(f64::from(cont)),
            quantize_coord(f64::from(erosion)),
            quantize_coord(f64::from(depth)),
            quantize_coord(f64::from(weirdness)),
        )
    }

    /// Finds the climate-biased overworld spawn origin.
    ///
    /// This mirrors vanilla's `Climate.Sampler.findSpawnPosition()` with
    /// `OverworldBiomeBuilder.spawnTarget()`.
    #[must_use]
    pub fn find_spawn_position(&self) -> BlockPos {
        let spawn_targets = overworld_spawn_targets();
        let mut result = self.spawn_position_and_fitness(&spawn_targets, 0, 0);
        result = self.radial_spawn_search(&spawn_targets, result, 2048.0, 512.0);
        self.radial_spawn_search(&spawn_targets, result, 512.0, 32.0)
            .pos
    }

    fn radial_spawn_search(
        &self,
        spawn_targets: &[ParameterPoint; 2],
        mut result: SpawnSearchResult,
        max_radius: f32,
        radius_increment: f32,
    ) -> SpawnSearchResult {
        let mut angle = 0.0_f32;
        let mut radius = radius_increment;
        let origin = result.pos;

        while radius <= max_radius {
            let x = origin.x() + (angle.sin() * radius) as i32;
            let z = origin.z() + (angle.cos() * radius) as i32;
            let candidate = self.spawn_position_and_fitness(spawn_targets, x, z);
            if candidate.fitness < result.fitness {
                result = candidate;
            }

            angle += radius_increment / radius;
            if angle > TAU {
                angle = 0.0;
                radius += radius_increment;
            }
        }

        result
    }

    fn spawn_position_and_fitness(
        &self,
        spawn_targets: &[ParameterPoint; 2],
        block_x: i32,
        block_z: i32,
    ) -> SpawnSearchResult {
        let mut cache = OverworldColumnCache::new();
        let target = self.sample(block_x >> 2, 0, block_z >> 2, &mut cache);
        let zero_depth_target = TargetPoint::new(
            target.temperature,
            target.humidity,
            target.continentalness,
            target.erosion,
            0,
            target.weirdness,
        );
        let min_fitness = spawn_targets
            .iter()
            .map(|point| point.fitness(&zero_depth_target))
            .min()
            .unwrap_or(i64::MAX);
        let distance_bias =
            i64::from(block_x) * i64::from(block_x) + i64::from(block_z) * i64::from(block_z);

        SpawnSearchResult {
            pos: BlockPos::new(block_x, 0, block_z),
            fitness: min_fitness * 2048_i64 * 2048_i64 + distance_bias,
        }
    }
}

#[derive(Clone, Copy)]
struct SpawnSearchResult {
    pos: BlockPos,
    fitness: i64,
}

fn overworld_spawn_targets() -> [ParameterPoint; 2] {
    let full_range = Parameter::span(-1.0, 1.0);
    let inland_continentalness = Parameter::span(-0.11, 0.55);
    let continentalness = Parameter::span_params(&inland_continentalness, &full_range);
    let surface_depth = Parameter::point(0.0);

    [
        ParameterPoint::new(
            full_range,
            full_range,
            continentalness,
            full_range,
            surface_depth,
            Parameter::span(-1.0, -0.16),
            0,
        ),
        ParameterPoint::new(
            full_range,
            full_range,
            continentalness,
            full_range,
            surface_depth,
            Parameter::span(0.16, 1.0),
            0,
        ),
    ]
}
