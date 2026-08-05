#[expect(
    clippy::struct_field_names,
    reason = "field names match vanilla weather state naming"
)]
#[derive(Debug, Default)]
pub struct Weather {
    pub rain_level: f32,
    pub previous_rain_level: f32,
    pub thunder_level: f32,
    pub previous_thunder_level: f32,
}

use super::{
    ADVANCE_WEATHER, BiomeRef, BlockPos, CGameEvent, ChunkGenerator, ChunkPos, GameEventType,
    HeightmapType, LazyLock, LegacyRandom, LightLayer, PerlinSimplexNoise, REGISTRY, RandomSource,
    RegistryEntry, RegistryExt, TemperatureModifier, World, environment, fuzzed_biome_at_block,
    obfuscate_biome_seed, vanilla_dimension_types,
};

static BIOME_TEMPERATURE_NOISE: LazyLock<PerlinSimplexNoise> = LazyLock::new(|| {
    let mut random = RandomSource::Legacy(LegacyRandom::from_seed(1234));
    PerlinSimplexNoise::new(&mut random, &[0])
});

static FROZEN_BIOME_TEMPERATURE_NOISE: LazyLock<PerlinSimplexNoise> = LazyLock::new(|| {
    let mut random = RandomSource::Legacy(LegacyRandom::from_seed(3456));
    PerlinSimplexNoise::new(&mut random, &[-2, -1, 0])
});

static BIOME_INFO_NOISE: LazyLock<PerlinSimplexNoise> = LazyLock::new(|| {
    let mut random = RandomSource::Legacy(LegacyRandom::from_seed(2345));
    PerlinSimplexNoise::new(&mut random, &[0])
});

impl World {
    #[expect(
        clippy::too_many_lines,
        reason = "splitting would hurt readability of the weather state machine"
    )]
    pub(super) fn tick_weather(&self) {
        if !self.can_have_weather() {
            return;
        }

        let mut weather = self.weather.lock();
        let raining_before = self.is_raining_with_guard(&weather);

        // Advance the weather state machine (only if gamerule allows)
        {
            let mut level_data = self.level_data.write();

            if self.get_game_rule_with_guard(&ADVANCE_WEATHER, &level_data) {
                let clear_weather_time = level_data.clear_weather_time();
                if clear_weather_time > 0 {
                    level_data.set_clear_weather_time(clear_weather_time - 1);
                    if level_data.is_thundering() {
                        level_data.set_thunder_time(0);
                        level_data.set_thundering(false);
                    } else {
                        level_data.set_thunder_time(1);
                    }
                    if level_data.is_raining() {
                        level_data.set_rain_time(0);
                        level_data.set_raining(false);
                    } else {
                        level_data.set_rain_time(1);
                    }
                } else {
                    let thundering_time = level_data.thunder_time();
                    if thundering_time > 0 {
                        level_data.set_thunder_time(thundering_time - 1);
                        if level_data.thunder_time() == 0 {
                            let thundering = level_data.is_thundering();
                            level_data.set_thundering(!thundering);
                        }
                    } else if level_data.is_thundering() {
                        level_data.set_thunder_time(rand::random_range(3_600..=15_600));
                    } else {
                        level_data.set_thunder_time(rand::random_range(12_000..=180_000));
                    }

                    let rain_time = level_data.rain_time();
                    if rain_time > 0 {
                        level_data.set_rain_time(rain_time - 1);
                        if level_data.rain_time() == 0 {
                            let raining = level_data.is_raining();
                            level_data.set_raining(!raining);
                        }
                    } else if level_data.is_raining() {
                        level_data.set_rain_time(rand::random_range(12_000..=24_000));
                    } else {
                        level_data.set_rain_time(rand::random_range(12_000..=180_000));
                    }
                }
            }
        }

        // Interpolate visual levels (always runs, even when ADVANCE_WEATHER is off)
        let is_thundering = self.level_data.read().is_thundering();
        let is_raining = self.level_data.read().is_raining();

        weather.previous_thunder_level = weather.thunder_level;
        if is_thundering {
            weather.thunder_level += 0.01;
        } else {
            weather.thunder_level -= 0.01;
        }
        weather.thunder_level = weather.thunder_level.clamp(0.0, 1.0);

        weather.previous_rain_level = weather.rain_level;
        if is_raining {
            weather.rain_level += 0.01;
        } else {
            weather.rain_level -= 0.01;
        }
        weather.rain_level = weather.rain_level.clamp(0.0, 1.0);

        // Broadcast weather changes to clients
        let raining_now = self.is_raining_with_guard(&weather);
        if raining_before == raining_now {
            #[expect(
                clippy::float_cmp,
                reason = "comparing against the exact previously-assigned value to detect any change"
            )]
            if weather.previous_rain_level != weather.rain_level {
                self.broadcast_to_all(CGameEvent {
                    event: GameEventType::RainLevelChange,
                    data: weather.rain_level,
                });
            }

            #[expect(
                clippy::float_cmp,
                reason = "comparing against the exact previously-assigned value to detect any change"
            )]
            if weather.previous_thunder_level != weather.thunder_level {
                self.broadcast_to_all(CGameEvent {
                    event: GameEventType::ThunderLevelChange,
                    data: weather.thunder_level,
                });
            }
        } else {
            if raining_before {
                self.broadcast_to_all(CGameEvent {
                    event: GameEventType::StopRaining,
                    data: 0.0,
                });
            } else {
                self.broadcast_to_all(CGameEvent {
                    event: GameEventType::StartRaining,
                    data: 0.0,
                });
            }

            self.broadcast_to_all(CGameEvent {
                event: GameEventType::RainLevelChange,
                data: weather.rain_level,
            });

            self.broadcast_to_all(CGameEvent {
                event: GameEventType::ThunderLevelChange,
                data: weather.thunder_level,
            });
        }
    }

    /// Sets this world's weather timers and flags.
    ///
    /// Minecraft 26.2 owns this state at server scope. Steel intentionally owns
    /// it per world so multiple worlds in one domain can have independent weather.
    pub(crate) fn set_weather_parameters(
        &self,
        clear_time: i32,
        rain_time: i32,
        raining: bool,
        thundering: bool,
    ) {
        let mut level_data = self.level_data.write();
        level_data.set_clear_weather_time(clear_time);
        level_data.set_rain_time(rain_time);
        level_data.set_thunder_time(rain_time);
        level_data.set_raining(raining);
        level_data.set_thundering(thundering);
    }

    /// Checks whether the rain level is high enough to be considered raining.
    /// Used for both visual rendering and gameplay logic (crop growth, fire, mob behavior).
    ///
    /// WARNING: this function acquires a lock on the `weather` field.
    /// if you already have a lock on the `weather` field, this will DEADLOCK.
    pub fn is_raining(&self) -> bool {
        let guard = self.weather.lock();
        self.is_raining_with_guard(&guard)
    }

    /// Checks whether rain reaches the given block position.
    ///
    /// Mirrors vanilla `Level.isRainingAt`: global rain state, sky exposure,
    /// motion-blocking height, and biome precipitation must all allow rain.
    pub fn is_raining_at(&self, pos: BlockPos) -> bool {
        if !self.is_raining() || !self.can_see_sky_for_precipitation(pos) {
            return false;
        }

        self.biome_at(pos).is_some_and(|biome| {
            biome.has_precipitation && self.biome_temperature(biome, pos) >= 0.15
        })
    }

    /// Checks whether the rain level is sufficient to render rain clientside using the provided guard.
    pub fn is_raining_with_guard(&self, guard: &Weather) -> bool {
        guard.rain_level > 0.2 && self.can_have_weather()
    }

    /// Checks whether the thunder level and rain level are high enough to be considered thundering.
    /// Used for lightning spawning and gameplay logic.
    ///
    /// WARNING: this function acquires a lock on the `weather` field.
    /// if you already have a lock on the `weather` field, this will DEADLOCK.
    pub fn is_thundering(&self) -> bool {
        let guard = self.weather.lock();
        self.is_thundering_with_guard(&guard)
    }

    /// Checks whether the thunder level and rain level are sufficient to spawn thunderbolts using the provided guard.
    pub fn is_thundering_with_guard(&self, guard: &Weather) -> bool {
        guard.rain_level * guard.thunder_level > 0.9 && self.can_have_weather()
    }

    /// Returns the current vanilla `SKY_LIGHT_LEVEL` environment attribute.
    pub fn sky_light_level(&self) -> f32 {
        let (rain_level, thunder_level) = if self.can_have_weather() {
            let weather = self.weather.lock();
            (weather.rain_level, weather.thunder_level)
        } else {
            (0.0, 0.0)
        };

        let level_data = self.level_data.read();
        environment::sky_light_level(
            self.dimension_type,
            level_data.world_clocks(),
            rain_level,
            thunder_level,
            self.can_have_weather(),
        )
    }

    /// Returns vanilla `Level.skyDarken`.
    pub fn sky_darkening(&self) -> u8 {
        environment::sky_darkening(self.sky_light_level())
    }

    /// Returns the current vanilla `SUN_ANGLE` environment attribute in degrees.
    pub fn sun_angle_degrees(&self) -> f32 {
        let level_data = self.level_data.read();
        environment::sun_angle_degrees(self.dimension_type, level_data.world_clocks())
    }

    /// Returns sky-layer light after the current sky darkening is subtracted.
    ///
    /// Mirrors vanilla `LevelReader.getEffectiveSkyBrightness` without allowing
    /// block light to raise the result.
    pub fn effective_sky_brightness(&self, pos: BlockPos) -> u8 {
        if !self.dimension_type.has_skylight {
            return 0;
        }
        self.light_value_at(LightLayer::Sky, pos)
            .saturating_sub(self.sky_darkening())
    }

    /// Returns vanilla `Level.isBrightOutside`.
    pub fn is_bright_outside(&self) -> bool {
        self.dimension_type.fixed_time.is_none() && self.sky_darkening() < 4
    }

    /// Returns vanilla `Level.isDarkOutside`.
    pub fn is_dark_outside(&self) -> bool {
        self.dimension_type.fixed_time.is_none() && !self.is_bright_outside()
    }

    /// Checks whether the world can have weather.
    pub fn can_have_weather(&self) -> bool {
        self.dimension_type.has_skylight
            && !self.dimension_type.has_ceiling
            && self.dimension_type.key != vanilla_dimension_types::THE_END.key
    }

    /// Returns whether the position has unobstructed sky exposure.
    ///
    /// Live worlds use the motion-blocking heightmap until Steel has a full
    /// live sky-light engine.
    pub fn can_see_sky(&self, pos: BlockPos) -> bool {
        if !self.dimension_type.has_skylight {
            return false;
        }
        self.height_at(HeightmapType::MotionBlocking, pos.x(), pos.z())
            .is_some_and(|height| height <= pos.y())
    }

    pub(super) fn can_see_sky_for_precipitation(&self, pos: BlockPos) -> bool {
        self.can_see_sky(pos)
    }

    pub(crate) fn biome_at(&self, pos: BlockPos) -> Option<BiomeRef> {
        let biome_zoom_seed = obfuscate_biome_seed(self.seed());
        let mut missing_chunk = false;
        let biome_id = fuzzed_biome_at_block(biome_zoom_seed, pos, |quart| {
            self.noise_biome_id(quart.x, quart.y, quart.z)
                .unwrap_or_else(|| {
                    missing_chunk = true;
                    0
                })
        });

        if missing_chunk {
            return None;
        }

        REGISTRY.biomes.by_id(usize::from(biome_id))
    }

    pub(super) fn noise_biome_id(&self, quart_x: i32, quart_y: i32, quart_z: i32) -> Option<u16> {
        let chunk_pos = ChunkPos::new(quart_x >> 2, quart_z >> 2);
        let local_quart_x = (quart_x & 3) as usize;
        let local_quart_z = (quart_z & 3) as usize;

        if let Some(Some(biome_id)) = self.chunk_map.with_full_chunk(chunk_pos, |chunk| {
            let sections = chunk.sections();
            let (section_index, local_quart_y) =
                Self::biome_quart_y_indices(chunk.min_y(), sections.sections.len(), quart_y)?;
            let section = sections.sections.get(section_index)?;
            Some(
                section
                    .read()
                    .biomes
                    .get(local_quart_x, local_quart_y, local_quart_z),
            )
        }) {
            return Some(biome_id);
        }

        let biome = self
            .chunk_map
            .world_gen_context
            .generator
            .noise_biome(quart_x, quart_y, quart_z);
        u16::try_from(biome.try_id()?).ok()
    }

    pub(super) fn biome_quart_y_indices(
        min_y: i32,
        section_count: usize,
        quart_y: i32,
    ) -> Option<(usize, usize)> {
        let total_quart_y = section_count.checked_mul(4)?;
        if total_quart_y == 0 {
            return None;
        }

        let relative_quart_y = i64::from(quart_y) - i64::from(min_y >> 2);
        let max_relative_quart_y = total_quart_y - 1;
        let clamped_relative_quart_y = if relative_quart_y <= 0 {
            0
        } else {
            usize::try_from(relative_quart_y).map_or(max_relative_quart_y, |relative| {
                relative.min(max_relative_quart_y)
            })
        };

        Some((clamped_relative_quart_y / 4, clamped_relative_quart_y & 3))
    }

    pub(super) fn biome_temperature(&self, biome: BiomeRef, pos: BlockPos) -> f32 {
        let modified_temp = match biome.temperature_modifier {
            TemperatureModifier::None => biome.temperature,
            TemperatureModifier::Frozen => {
                let large = FROZEN_BIOME_TEMPERATURE_NOISE
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
                biome.temperature
            }
        };

        let snow_level = self.sea_level + 17;
        if pos.y() <= snow_level {
            return modified_temp;
        }

        let noise = BIOME_TEMPERATURE_NOISE
            .get_value(f64::from(pos.x()) / 8.0, f64::from(pos.z()) / 8.0)
            as f32
            * 8.0;
        modified_temp - (noise + pos.y() as f32 - snow_level as f32) * 0.05 / 40.0
    }
}
