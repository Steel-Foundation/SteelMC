use super::prelude::*;
use super::runner::FeatureDecorationRunner;

static TEMPERATURE_NOISE: LazyLock<PerlinSimplexNoise> = LazyLock::new(|| {
    let mut random = RandomSource::Legacy(LegacyRandom::from_seed(1234));
    PerlinSimplexNoise::new(&mut random, &[0])
});

static FROZEN_TEMPERATURE_NOISE: LazyLock<PerlinSimplexNoise> = LazyLock::new(|| {
    let mut random = RandomSource::Legacy(LegacyRandom::from_seed(3456));
    PerlinSimplexNoise::new(&mut random, &[-2, -1, 0])
});

static BIOME_INFO_NOISE: LazyLock<PerlinSimplexNoise> = LazyLock::new(|| {
    let mut random = RandomSource::Legacy(LegacyRandom::from_seed(2345));
    PerlinSimplexNoise::new(&mut random, &[0])
});

impl FeatureDecorationRunner {
    pub(super) fn biome_info_noise_value(x: f64, z: f64) -> f64 {
        BIOME_INFO_NOISE.get_value(x, z)
    }

    pub(super) fn biome_at_block(
        region: &WorldGenRegion<'_>,
        registry: &Registry,
        biome_zoom_seed: i64,
        pos: BlockPos,
    ) -> BiomeRef {
        let biome_id = fuzzed_biome_at_block(
            biome_zoom_seed,
            pos.x(),
            pos.y(),
            pos.z(),
            |quart_x, quart_y, quart_z| region.noise_biome_id(quart_x, quart_y, quart_z),
        );
        let Some(biome) = registry.biomes.by_id(usize::from(biome_id)) else {
            panic!("biome lookup resolved unknown biome id {biome_id}");
        };
        biome
    }

    pub(super) fn should_freeze(
        region: &WorldGenRegion<'_>,
        registry: &Registry,
        biome_zoom_seed: i64,
        pos: BlockPos,
        check_neighbors: bool,
    ) -> bool {
        let biome = Self::biome_at_block(region, registry, biome_zoom_seed, pos);
        Self::should_freeze_in_biome(region, biome, pos, check_neighbors)
    }

    pub(super) fn should_freeze_in_biome(
        region: &WorldGenRegion<'_>,
        biome: BiomeRef,
        pos: BlockPos,
        check_neighbors: bool,
    ) -> bool {
        if Self::warm_enough_to_rain(region, biome, pos)
            || region.is_outside_build_height(pos.y())
            || region.block_light_at(pos) >= 10
        {
            return false;
        }

        let state = region.block_state(pos);
        if state.get_block() != &vanilla_blocks::WATER
            || !get_fluid_state_from_block(state).is_water()
        {
            return false;
        }

        if !check_neighbors {
            return true;
        }

        !(Self::is_water_at(region, pos.west())
            && Self::is_water_at(region, pos.east())
            && Self::is_water_at(region, pos.north())
            && Self::is_water_at(region, pos.south()))
    }

    pub(super) fn should_snow_in_biome(
        region: &WorldGenRegion<'_>,
        biome: BiomeRef,
        pos: BlockPos,
    ) -> bool {
        if !biome.has_precipitation
            || Self::warm_enough_to_rain(region, biome, pos)
            || region.is_outside_build_height(pos.y())
            || region.block_light_at(pos) >= 10
        {
            return false;
        }

        let state = region.block_state(pos);
        if !state.is_air() && state.get_block() != &vanilla_blocks::SNOW {
            return false;
        }

        let snow = vanilla_blocks::SNOW.default_state();
        BLOCK_BEHAVIORS
            .get_behavior(snow.get_block())
            .can_survive(snow, region, pos)
    }

    fn is_water_at(region: &WorldGenRegion<'_>, pos: BlockPos) -> bool {
        get_fluid_state_from_block(region.block_state(pos)).is_water()
    }

    fn warm_enough_to_rain(region: &WorldGenRegion<'_>, biome: BiomeRef, pos: BlockPos) -> bool {
        Self::biome_temperature(region, biome, pos) >= 0.15
    }

    fn biome_temperature(region: &WorldGenRegion<'_>, biome: BiomeRef, pos: BlockPos) -> f32 {
        let base_temp = biome.temperature;
        let modified_temp = match biome.temperature_modifier {
            TemperatureModifier::None => base_temp,
            TemperatureModifier::Frozen => {
                let large = FROZEN_TEMPERATURE_NOISE
                    .get_value(f64::from(pos.x()) * 0.05, f64::from(pos.z()) * 0.05)
                    * 7.0;
                let edge =
                    BIOME_INFO_NOISE.get_value(f64::from(pos.x()) * 0.2, f64::from(pos.z()) * 0.2);
                if large + edge < 0.3 {
                    let small = BIOME_INFO_NOISE
                        .get_value(f64::from(pos.x()) * 0.09, f64::from(pos.z()) * 0.09);
                    if small < 0.8 {
                        return 0.2;
                    }
                }
                base_temp
            }
        };

        let snow_level = region.sea_level() + 17;
        if pos.y() <= snow_level {
            return modified_temp;
        }

        let value = TEMPERATURE_NOISE.get_value(f64::from(pos.x()) / 8.0, f64::from(pos.z()) / 8.0)
            as f32
            * 8.0;
        modified_temp - (value + pos.y() as f32 - snow_level as f32) * 0.05 / 40.0
    }
}
