//! Surface system for biome-specific block placement.
//!
//! Translates vanilla's `SurfaceSystem` — holds noise generators, clay band
//! data, and positional random sources needed by transpiled surface rules.

use rustc_hash::FxHashMap;
use steel_registry::biome::TemperatureModifier;
use steel_registry::blocks::block_state_ext::BlockStateExt;
use steel_registry::vanilla_blocks;
use steel_registry::{REGISTRY, RegistryExt};
use steel_utils::BlockStateId;
use steel_utils::density::NoiseParameters;
use steel_utils::noise::{NormalNoise, PerlinSimplexNoise};
use steel_utils::random::legacy_random::LegacyRandom;
use steel_utils::random::name_hash::NameHash;
use steel_utils::random::{PositionalRandom, Random, RandomSource, RandomSplitter};
use steel_utils::surface::SurfaceNoiseProvider;

const CLAY_BAND_LENGTH: usize = 192;

/// Runtime surface system holding noises and clay band data.
///
/// Matches vanilla's `SurfaceSystem`. Constructed once per generator and
/// shared across all chunk generation calls.
pub struct SurfaceSystem {
    /// Surface depth noise (`minecraft:surface`).
    surface_noise: NormalNoise,
    /// Surface secondary noise (`minecraft:surface_secondary`).
    surface_secondary_noise: NormalNoise,
    /// Clay bands offset noise (`minecraft:clay_bands_offset`).
    clay_bands_offset_noise: NormalNoise,
    /// Pre-generated terracotta band pattern (192 entries).
    clay_bands: [BlockStateId; CLAY_BAND_LENGTH],
    /// Positional random factory for surface depth jitter and frozen ocean.
    noise_random: RandomSplitter,
    /// Condition noises used by `NoiseThreshold` surface rules.
    /// Indexed in the same order as `DimensionNoises::surface_noise_ids()`.
    condition_noises: Vec<NormalNoise>,
    /// Global splitter for creating vertical gradient factories.
    splitter: RandomSplitter,

    // ── Extension noises (eroded badlands + frozen ocean) ──
    badlands_pillar_noise: NormalNoise,
    badlands_pillar_roof_noise: NormalNoise,
    badlands_surface_noise: NormalNoise,
    iceberg_pillar_noise: NormalNoise,
    iceberg_pillar_roof_noise: NormalNoise,
    iceberg_surface_noise: NormalNoise,

    // ── Temperature noises (static in vanilla Biome class) ──
    /// Temperature noise for height-based adjustments (seed 1234, octave 0).
    temperature_noise: PerlinSimplexNoise,
    /// Frozen biome temperature noise (seed 3456, octaves [-2,-1,0]).
    frozen_temperature_noise: PerlinSimplexNoise,
    /// Biome info noise for frozen patches (seed 2345, octave 0).
    biome_info_noise: PerlinSimplexNoise,

    /// Default block state for this dimension.
    pub default_block: BlockStateId,
    /// Sea level for this dimension.
    pub sea_level: i32,
}

impl SurfaceSystem {
    /// Create a new surface system.
    ///
    /// `condition_noise_ids` lists the noise IDs referenced by `NoiseThreshold`
    /// conditions in the transpiled surface rules.
    #[must_use]
    pub fn new(
        splitter: &RandomSplitter,
        noise_params: &FxHashMap<String, NoiseParameters>,
        condition_noise_ids: &[&str],
        default_block: BlockStateId,
        sea_level: i32,
    ) -> Self {
        // Clay band generation: vanilla does noiseRandom.fromHashOf("minecraft:clay_bands")
        const CLAY_BANDS_HASH: NameHash = NameHash::new("minecraft:clay_bands");

        // Vanilla passes the base PositionalRandomFactory (RandomState.this.random)
        // directly to SurfaceSystem as noiseRandom — no extra fromHashOf wrapping.
        let noise_random = splitter.clone();
        let mut band_random = noise_random.with_hash_of(&CLAY_BANDS_HASH);
        let clay_bands = Self::generate_bands(&mut band_random);

        // Create condition noises referenced by NoiseThreshold rules.
        // Order matches the indices emitted by the transpiler.
        let condition_noises: Vec<NormalNoise> = condition_noise_ids
            .iter()
            .map(|&id| create_noise(splitter, id, noise_params))
            .collect();

        Self {
            surface_noise: create_noise(splitter, "minecraft:surface", noise_params),
            surface_secondary_noise: create_noise(
                splitter,
                "minecraft:surface_secondary",
                noise_params,
            ),
            clay_bands_offset_noise: create_noise(
                splitter,
                "minecraft:clay_bands_offset",
                noise_params,
            ),
            clay_bands,
            noise_random,
            condition_noises,
            splitter: splitter.clone(),
            badlands_pillar_noise: create_noise(
                splitter,
                "minecraft:badlands_pillar",
                noise_params,
            ),
            badlands_pillar_roof_noise: create_noise(
                splitter,
                "minecraft:badlands_pillar_roof",
                noise_params,
            ),
            badlands_surface_noise: create_noise(
                splitter,
                "minecraft:badlands_surface",
                noise_params,
            ),
            iceberg_pillar_noise: create_noise(splitter, "minecraft:iceberg_pillar", noise_params),
            iceberg_pillar_roof_noise: create_noise(
                splitter,
                "minecraft:iceberg_pillar_roof",
                noise_params,
            ),
            iceberg_surface_noise: create_noise(
                splitter,
                "minecraft:iceberg_surface",
                noise_params,
            ),
            // Temperature noises — fixed seeds matching vanilla's Biome static initializer
            temperature_noise: {
                let mut rng = RandomSource::Legacy(LegacyRandom::from_seed(1234));
                PerlinSimplexNoise::new(&mut rng, &[0])
            },
            frozen_temperature_noise: {
                let mut rng = RandomSource::Legacy(LegacyRandom::from_seed(3456));
                PerlinSimplexNoise::new(&mut rng, &[-2, -1, 0])
            },
            biome_info_noise: {
                let mut rng = RandomSource::Legacy(LegacyRandom::from_seed(2345));
                PerlinSimplexNoise::new(&mut rng, &[0])
            },
            default_block,
            sea_level,
        }
    }

    /// Compute the surface depth at a column position.
    ///
    /// Matches vanilla's `SurfaceSystem.getSurfaceDepth()`:
    /// `(int)(noise * 2.75 + 3.0 + random.at(x, 0, z).nextDouble() * 0.25)`
    #[must_use]
    pub fn get_surface_depth(&self, x: i32, z: i32) -> i32 {
        let noise_value = self
            .surface_noise
            .get_value(f64::from(x), 0.0, f64::from(z));
        let jitter = self.noise_random.at(x, 0, z).next_f64() * 0.25;
        (noise_value * 2.75 + 3.0 + jitter) as i32
    }

    /// Sample the surface secondary noise at a column position.
    #[must_use]
    pub fn get_surface_secondary(&self, x: i32, z: i32) -> f64 {
        self.surface_secondary_noise
            .get_value(f64::from(x), 0.0, f64::from(z))
    }

    /// Compute the effective temperature at a position.
    ///
    /// Matches vanilla's `Biome.getTemperature()` with temperature modifier
    /// and height-based adjustment above `seaLevel + 17`.
    ///
    /// # Panics
    /// Panics if `biome_id` does not correspond to a registered biome.
    fn get_temperature(&self, biome_id: u16, block_x: i32, block_y: i32, block_z: i32) -> f32 {
        let biome = REGISTRY
            .biomes
            .by_id(biome_id as usize)
            .expect("invalid biome id");
        let base_temp = biome.temperature;

        // Apply temperature modifier (FROZEN biomes have special noise-based patches)
        let modified_temp = match biome.temperature_modifier {
            TemperatureModifier::None => base_temp,
            TemperatureModifier::Frozen => {
                let large = self
                    .frozen_temperature_noise
                    .get_value(f64::from(block_x) * 0.05, f64::from(block_z) * 0.05)
                    * 7.0;
                let edge = self
                    .biome_info_noise
                    .get_value(f64::from(block_x) * 0.2, f64::from(block_z) * 0.2);
                let combined = large + edge;
                if combined < 0.3 {
                    let small = self
                        .biome_info_noise
                        .get_value(f64::from(block_x) * 0.09, f64::from(block_z) * 0.09);
                    if small < 0.8 {
                        0.2 // Force warm
                    } else {
                        base_temp
                    }
                } else {
                    base_temp
                }
            }
        };

        // Height-based temperature adjustment above seaLevel + 17
        let snow_level = self.sea_level + 17;
        if block_y > snow_level {
            let v = self
                .temperature_noise
                .get_value(f64::from(block_x) / 8.0, f64::from(block_z) / 8.0)
                as f32
                * 8.0;
            modified_temp - (v + block_y as f32 - snow_level as f32) * 0.05 / 40.0
        } else {
            modified_temp
        }
    }

    /// Check if a position is cold enough to snow.
    ///
    /// Matches vanilla's `Biome.coldEnoughToSnow()` → `!warmEnoughToRain()` →
    /// `getTemperature() >= 0.15`.
    #[must_use]
    pub fn cold_enough_to_snow(
        &self,
        biome_id: u16,
        block_x: i32,
        block_y: i32,
        block_z: i32,
    ) -> bool {
        self.get_temperature(biome_id, block_x, block_y, block_z) < 0.15
    }

    /// Check if an iceberg at this position should melt slightly.
    ///
    /// Matches vanilla's `Biome.shouldMeltFrozenOceanIcebergSlightly()`.
    /// Temperature is evaluated at sea level.
    fn should_melt_frozen_ocean_iceberg_slightly(
        &self,
        biome_id: u16,
        block_x: i32,
        block_z: i32,
    ) -> bool {
        self.get_temperature(biome_id, block_x, self.sea_level, block_z) > 0.1
    }

    // ── Clay band generation ────────────────────────────────────────────────

    /// Generate the 192-element terracotta band pattern.
    ///
    /// Matches vanilla's `SurfaceSystem.generateBands()`.
    fn generate_bands(random: &mut RandomSource) -> [BlockStateId; CLAY_BAND_LENGTH] {
        let terracotta = vanilla_blocks::TERRACOTTA.default_state();
        let orange = vanilla_blocks::ORANGE_TERRACOTTA.default_state();
        let yellow = vanilla_blocks::YELLOW_TERRACOTTA.default_state();
        let brown = vanilla_blocks::BROWN_TERRACOTTA.default_state();
        let red = vanilla_blocks::RED_TERRACOTTA.default_state();
        let white = vanilla_blocks::WHITE_TERRACOTTA.default_state();
        let light_gray = vanilla_blocks::LIGHT_GRAY_TERRACOTTA.default_state();

        let mut bands = [terracotta; CLAY_BAND_LENGTH];

        // Orange terracotta bands — vanilla loop increments i in both the
        // for-header and body: `for(int i = 0; i < len; ++i) { i += rand(5)+1; ... }`
        let mut i = 0usize;
        while i < CLAY_BAND_LENGTH {
            i += random.next_i32_bounded(5) as usize + 1;
            if i < CLAY_BAND_LENGTH {
                bands[i] = orange;
            }
            i += 1;
        }

        Self::make_bands(random, &mut bands, 1, yellow);
        Self::make_bands(random, &mut bands, 2, brown);
        Self::make_bands(random, &mut bands, 1, red);

        // White + light gray terracotta bands
        let white_count = random.next_i32_between(9, 15);
        let mut placed = 0;
        let mut start = 0usize;
        while placed < white_count && start < CLAY_BAND_LENGTH {
            bands[start] = white;
            if start > 1 && random.next_bool() {
                bands[start - 1] = light_gray;
            }
            if start + 1 < CLAY_BAND_LENGTH && random.next_bool() {
                bands[start + 1] = light_gray;
            }
            placed += 1;
            start += random.next_i32_bounded(16) as usize + 4;
        }

        bands
    }

    /// Place random bands of a single color.
    ///
    /// Matches vanilla's `SurfaceSystem.makeBands()`.
    fn make_bands(
        random: &mut RandomSource,
        bands: &mut [BlockStateId; CLAY_BAND_LENGTH],
        base_width: i32,
        state: BlockStateId,
    ) {
        let band_count = random.next_i32_between(6, 15);
        for _ in 0..band_count {
            let width = (base_width + random.next_i32_bounded(3)) as usize;
            let start = random.next_i32_bounded(CLAY_BAND_LENGTH as i32) as usize;
            for p in 0..width {
                if start + p >= CLAY_BAND_LENGTH {
                    break;
                }
                bands[start + p] = state;
            }
        }
    }
}

impl SurfaceSystem {
    /// Eroded badlands extension — adds terracotta pillars above the surface.
    ///
    /// Matches vanilla's `SurfaceSystem.erodedBadlandsExtension()`.
    /// Returns the new `start_height` if blocks were added above the original surface.
    #[expect(
        clippy::too_many_arguments,
        reason = "matches vanilla SurfaceSystem.erodedBadlandsExtension signature"
    )]
    pub fn eroded_badlands_extension(
        &self,
        chunk: &super::chunk_access::ChunkAccess,
        local_x: usize,
        local_z: usize,
        block_x: i32,
        block_z: i32,
        height: i32,
        min_y: i32,
    ) -> i32 {
        let pillar_buffer = f64::min(
            (self
                .badlands_surface_noise
                .get_value(f64::from(block_x), 0.0, f64::from(block_z))
                * 8.25)
                .abs(),
            self.badlands_pillar_noise.get_value(
                f64::from(block_x) * 0.2,
                0.0,
                f64::from(block_z) * 0.2,
            ) * 15.0,
        );

        if pillar_buffer <= 0.0 {
            return height;
        }

        let pillar_floor = (self.badlands_pillar_roof_noise.get_value(
            f64::from(block_x) * 0.75,
            0.0,
            f64::from(block_z) * 0.75,
        ) * 1.5)
            .abs();

        let extension_top = 64.0
            + f64::min(
                pillar_buffer * pillar_buffer * 2.5,
                (pillar_floor * 50.0).ceil() + 24.0,
            );
        let start_y = extension_top.floor() as i32;

        if height > start_y {
            return height;
        }

        // Scan down from start_y: break on defaultBlock, return on water
        for y in (min_y..=start_y).rev() {
            let rel_y = (y - min_y) as usize;
            let state = chunk
                .get_relative_block(local_x, rel_y, local_z)
                .unwrap_or(BlockStateId(0));
            if state == self.default_block {
                break;
            }
            if state.get_block().config.liquid {
                return height; // Water found — no extension
            }
        }

        // Fill air from start_y downward with defaultBlock
        for y in (min_y..=start_y).rev() {
            let rel_y = (y - min_y) as usize;
            let state = chunk
                .get_relative_block(local_x, rel_y, local_z)
                .unwrap_or(BlockStateId(0));
            if !state.is_air() {
                break;
            }
            chunk.set_relative_block(local_x, rel_y, local_z, self.default_block);
        }

        // Return updated start height (one above the extension top)
        start_y + 1
    }

    /// Frozen ocean iceberg extension — adds packed ice and snow blocks.
    ///
    /// Matches vanilla's `SurfaceSystem.frozenOceanExtension()`.
    /// Called after surface rules for frozen ocean / deep frozen ocean biomes.
    #[expect(
        clippy::too_many_arguments,
        reason = "matches vanilla SurfaceSystem.frozenOceanExtension signature"
    )]
    pub fn frozen_ocean_extension(
        &self,
        chunk: &super::chunk_access::ChunkAccess,
        biome_id: u16,
        local_x: usize,
        local_z: usize,
        block_x: i32,
        block_z: i32,
        height: i32,
        min_surface_level: i32,
        min_y: i32,
    ) {
        let iceberg = f64::min(
            (self
                .iceberg_surface_noise
                .get_value(f64::from(block_x), 0.0, f64::from(block_z))
                * 8.25)
                .abs(),
            self.iceberg_pillar_noise.get_value(
                f64::from(block_x) * 1.28,
                0.0,
                f64::from(block_z) * 1.28,
            ) * 15.0,
        );

        if iceberg <= 1.8 {
            return;
        }

        let iceberg_roof = (self.iceberg_pillar_roof_noise.get_value(
            f64::from(block_x) * 1.17,
            0.0,
            f64::from(block_z) * 1.17,
        ) * 1.5)
            .abs();

        let mut top = f64::min(iceberg * iceberg * 1.2, (iceberg_roof * 40.0).ceil() + 14.0);

        if self.should_melt_frozen_ocean_iceberg_slightly(biome_id, block_x, block_z) {
            top -= 2.0;
        }

        let extension_bottom;
        if top > 2.0 {
            extension_bottom = f64::from(self.sea_level) - top - 7.0;
            top += f64::from(self.sea_level);
        } else {
            top = 0.0;
            extension_bottom = 0.0;
        }

        let extension_top = top;
        let mut random = self.noise_random.at(block_x, 0, block_z);
        let max_snow_depth = 2 + random.next_i32_bounded(4);
        let min_snow_height = self.sea_level + 18 + random.next_i32_bounded(10);
        let mut snow_depth = 0;

        let snow_block = vanilla_blocks::SNOW_BLOCK.default_state();
        let packed_ice = vanilla_blocks::PACKED_ICE.default_state();

        let start_y = i32::max(height, top as i32 + 1);
        for y in (min_surface_level..=start_y).rev() {
            let rel_y = (y - min_y) as usize;
            let state = chunk
                .get_relative_block(local_x, rel_y, local_z)
                .unwrap_or(BlockStateId(0));

            let is_air = state.is_air();
            let is_water = state.get_block() == vanilla_blocks::WATER;

            if (is_air && y < extension_top as i32 && random.next_f64() > 0.01)
                || (is_water
                    && y > extension_bottom as i32
                    && y < self.sea_level
                    && extension_bottom != 0.0
                    && random.next_f64() > 0.15)
            {
                if snow_depth <= max_snow_depth && y > min_snow_height {
                    chunk.set_relative_block(local_x, rel_y, local_z, snow_block);
                    snow_depth += 1;
                } else {
                    chunk.set_relative_block(local_x, rel_y, local_z, packed_ice);
                }
            }
        }
    }
}

impl SurfaceNoiseProvider for SurfaceSystem {
    fn get_noise(&self, noise_index: usize, x: i32, z: i32) -> f64 {
        self.condition_noises[noise_index].get_value(f64::from(x), 0.0, f64::from(z))
    }

    fn get_band(&self, x: i32, y: i32, z: i32) -> BlockStateId {
        // Java: (int)Math.round(noise * 4.0)
        let offset = (self
            .clay_bands_offset_noise
            .get_value(f64::from(x), 0.0, f64::from(z))
            * 4.0
            + 0.5)
            .floor() as i32;
        let index = ((y + offset) % CLAY_BAND_LENGTH as i32 + CLAY_BAND_LENGTH as i32) as usize
            % CLAY_BAND_LENGTH;
        self.clay_bands[index]
    }

    fn vertical_gradient(
        &self,
        random_name: &NameHash,
        block_x: i32,
        block_y: i32,
        block_z: i32,
        true_at_and_below: i32,
        false_at_and_above: i32,
    ) -> bool {
        if block_y <= true_at_and_below {
            return true;
        }
        if block_y >= false_at_and_above {
            return false;
        }
        // Linear probability: 1.0 at true_at_and_below, 0.0 at false_at_and_above
        let probability = f64::from(false_at_and_above - block_y)
            / f64::from(false_at_and_above - true_at_and_below);

        // vanilla: randomState.getOrCreateRandomFactory(name) =
        //   this.random.fromHashOf(name).forkPositional()
        let factory = self.splitter.with_hash_of(random_name).next_positional();
        let random_value = f64::from(factory.at(block_x, block_y, block_z).next_f32());
        random_value < probability
    }
}

/// Helper to create a `NormalNoise` from the parameter registry.
fn create_noise(
    splitter: &RandomSplitter,
    id: &str,
    params: &FxHashMap<String, NoiseParameters>,
) -> NormalNoise {
    let p = params
        .get(id)
        .unwrap_or_else(|| panic!("Missing noise parameters for {id}"));
    NormalNoise::create(splitter, id, p.first_octave, &p.amplitudes)
}
