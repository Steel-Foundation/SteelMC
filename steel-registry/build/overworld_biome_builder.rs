//! Overworld biome builder - generates biome parameter entries from compact table data.
//!
//! This is a Rust port of Minecraft's OverworldBiomeBuilder class.
//! Instead of storing 7500+ pre-generated entries, we store ~10KB of tables
//! and generate the entries at build time.

use serde::Deserialize;
use std::fs;

// Weirdness boundary constants from vanilla OverworldBiomeBuilder.
// These define the weirdness ranges for each terrain height category.
const VALLEY_SIZE: f64 = 0.05;
const LOW_START: f64 = 0.26666668;
const HIGH_START: f64 = 0.4;
const HIGH_END: f64 = 0.93333334;
const PEAK_START: f64 = 0.56666666;
const PEAK_END: f64 = 0.7666667;

/// A parameter range [min, max]
#[derive(Deserialize, Debug, Clone, Copy)]
#[serde(from = "[f64; 2]")]
pub struct Parameter {
    pub min: f64,
    pub max: f64,
}

impl From<[f64; 2]> for Parameter {
    fn from(arr: [f64; 2]) -> Self {
        Parameter {
            min: arr[0],
            max: arr[1],
        }
    }
}

impl Parameter {
    pub fn new(min: f64, max: f64) -> Self {
        Parameter { min, max }
    }

    pub fn point(value: f64) -> Self {
        Parameter {
            min: value,
            max: value,
        }
    }

    pub fn span(a: &Parameter, b: &Parameter) -> Self {
        Parameter {
            min: a.min.min(b.min),
            max: a.max.max(b.max),
        }
    }
}

/// Continentalness ranges
#[derive(Deserialize, Debug)]
pub struct Continentalness {
    pub mushroom_fields: Parameter,
    pub deep_ocean: Parameter,
    pub ocean: Parameter,
    pub coast: Parameter,
    pub inland: Parameter,
    pub near_inland: Parameter,
    pub mid_inland: Parameter,
    pub far_inland: Parameter,
}

/// The extracted biome builder data
#[derive(Deserialize, Debug)]
pub struct BiomeBuilderData {
    pub temperatures: Vec<Parameter>,
    pub humidities: Vec<Parameter>,
    pub erosions: Vec<Parameter>,
    pub continentalness: Continentalness,
    pub oceans: Vec<Vec<String>>,
    pub middle_biomes: Vec<Vec<String>>,
    pub middle_biomes_variant: Vec<Vec<Option<String>>>,
    pub plateau_biomes: Vec<Vec<String>>,
    pub plateau_biomes_variant: Vec<Vec<Option<String>>>,
    pub shattered_biomes: Vec<Vec<Option<String>>>,
}

/// A generated biome entry
#[derive(Debug, Clone)]
pub struct BiomeEntry {
    pub temperature: Parameter,
    pub humidity: Parameter,
    pub continentalness: Parameter,
    pub erosion: Parameter,
    pub depth: Parameter,
    pub weirdness: Parameter,
    pub offset: f64,
    pub biome: String,
}

/// Biome builder that generates entries from table data
pub struct OverworldBiomeBuilder {
    data: BiomeBuilderData,
    full_range: Parameter,
    frozen_range: Parameter,
    unfrozen_range: Parameter,
}

impl OverworldBiomeBuilder {
    pub fn from_json(path: &str) -> Self {
        let content =
            fs::read_to_string(path).unwrap_or_else(|e| panic!("Failed to read {}: {}", path, e));
        let data: BiomeBuilderData = serde_json::from_str(&content)
            .unwrap_or_else(|e| panic!("Failed to parse biome builder JSON: {}", e));

        let full_range = Parameter::new(-1.0, 1.0);
        let frozen_range = data.temperatures[0];
        let unfrozen_range = Parameter::span(&data.temperatures[1], &data.temperatures[4]);

        OverworldBiomeBuilder {
            data,
            full_range,
            frozen_range,
            unfrozen_range,
        }
    }

    /// Generate all biome entries
    pub fn build(&self) -> Vec<BiomeEntry> {
        let mut entries = Vec::new();
        self.add_off_coast_biomes(&mut entries);
        self.add_inland_biomes(&mut entries);
        self.add_underground_biomes(&mut entries);
        entries
    }

    fn add_off_coast_biomes(&self, entries: &mut Vec<BiomeEntry>) {
        // Mushroom fields
        self.add_surface_biome(
            entries,
            &self.full_range,
            &self.full_range,
            &self.data.continentalness.mushroom_fields,
            &self.full_range,
            &self.full_range,
            0.0,
            "minecraft:mushroom_fields",
        );

        // Oceans
        for temp_idx in 0..self.data.temperatures.len() {
            let temperature = &self.data.temperatures[temp_idx];

            // Deep ocean
            self.add_surface_biome(
                entries,
                temperature,
                &self.full_range,
                &self.data.continentalness.deep_ocean,
                &self.full_range,
                &self.full_range,
                0.0,
                &self.data.oceans[0][temp_idx],
            );

            // Ocean
            self.add_surface_biome(
                entries,
                temperature,
                &self.full_range,
                &self.data.continentalness.ocean,
                &self.full_range,
                &self.full_range,
                0.0,
                &self.data.oceans[1][temp_idx],
            );
        }
    }

    fn add_inland_biomes(&self, entries: &mut Vec<BiomeEntry>) {
        self.add_mid_slice(entries, &Parameter::new(-1.0, -HIGH_END));
        self.add_high_slice(entries, &Parameter::new(-HIGH_END, -PEAK_END));
        self.add_peaks(entries, &Parameter::new(-PEAK_END, -PEAK_START));
        self.add_high_slice(entries, &Parameter::new(-PEAK_START, -HIGH_START));
        self.add_mid_slice(entries, &Parameter::new(-HIGH_START, -LOW_START));
        self.add_low_slice(entries, &Parameter::new(-LOW_START, -VALLEY_SIZE));
        self.add_valleys(entries, &Parameter::new(-VALLEY_SIZE, VALLEY_SIZE));
        self.add_low_slice(entries, &Parameter::new(VALLEY_SIZE, LOW_START));
        self.add_mid_slice(entries, &Parameter::new(LOW_START, HIGH_START));
        self.add_high_slice(entries, &Parameter::new(HIGH_START, PEAK_START));
        self.add_peaks(entries, &Parameter::new(PEAK_START, PEAK_END));
        self.add_high_slice(entries, &Parameter::new(PEAK_END, HIGH_END));
        self.add_mid_slice(entries, &Parameter::new(HIGH_END, 1.0));
    }

    fn add_peaks(&self, entries: &mut Vec<BiomeEntry>, weirdness: &Parameter) {
        for temp_idx in 0..self.data.temperatures.len() {
            let temperature = &self.data.temperatures[temp_idx];

            for hum_idx in 0..self.data.humidities.len() {
                let humidity = &self.data.humidities[hum_idx];

                let middle_biome = self.pick_middle_biome(temp_idx, hum_idx, weirdness);
                let middle_biome_or_badlands =
                    self.pick_middle_biome_or_badlands_if_hot(temp_idx, hum_idx, weirdness);
                let middle_biome_or_badlands_or_slope = self
                    .pick_middle_biome_or_badlands_if_hot_or_slope_if_cold(
                        temp_idx, hum_idx, weirdness,
                    );
                let plateau_biome = self.pick_plateau_biome(temp_idx, hum_idx, weirdness);
                let shattered_biome = self.pick_shattered_biome(temp_idx, hum_idx, weirdness);
                let shattered_or_windswept = self.maybe_pick_windswept_savanna(
                    temp_idx,
                    hum_idx,
                    weirdness,
                    &shattered_biome,
                );
                let peak_biome = self.pick_peak_biome(temp_idx, hum_idx, weirdness);

                // erosion 0
                self.add_surface_biome(
                    entries,
                    temperature,
                    humidity,
                    &Parameter::span(
                        &self.data.continentalness.coast,
                        &self.data.continentalness.far_inland,
                    ),
                    &self.data.erosions[0],
                    weirdness,
                    0.0,
                    &peak_biome,
                );

                // erosion 1 - coast to near inland
                self.add_surface_biome(
                    entries,
                    temperature,
                    humidity,
                    &Parameter::span(
                        &self.data.continentalness.coast,
                        &self.data.continentalness.near_inland,
                    ),
                    &self.data.erosions[1],
                    weirdness,
                    0.0,
                    &middle_biome_or_badlands_or_slope,
                );

                // erosion 1 - mid to far inland
                self.add_surface_biome(
                    entries,
                    temperature,
                    humidity,
                    &Parameter::span(
                        &self.data.continentalness.mid_inland,
                        &self.data.continentalness.far_inland,
                    ),
                    &self.data.erosions[1],
                    weirdness,
                    0.0,
                    &peak_biome,
                );

                // erosion 2-3 - coast to near inland
                self.add_surface_biome(
                    entries,
                    temperature,
                    humidity,
                    &Parameter::span(
                        &self.data.continentalness.coast,
                        &self.data.continentalness.near_inland,
                    ),
                    &Parameter::span(&self.data.erosions[2], &self.data.erosions[3]),
                    weirdness,
                    0.0,
                    &middle_biome,
                );

                // erosion 2 - mid to far inland
                self.add_surface_biome(
                    entries,
                    temperature,
                    humidity,
                    &Parameter::span(
                        &self.data.continentalness.mid_inland,
                        &self.data.continentalness.far_inland,
                    ),
                    &self.data.erosions[2],
                    weirdness,
                    0.0,
                    &plateau_biome,
                );

                // erosion 3 - mid inland
                self.add_surface_biome(
                    entries,
                    temperature,
                    humidity,
                    &self.data.continentalness.mid_inland,
                    &self.data.erosions[3],
                    weirdness,
                    0.0,
                    &middle_biome_or_badlands,
                );

                // erosion 3 - far inland
                self.add_surface_biome(
                    entries,
                    temperature,
                    humidity,
                    &self.data.continentalness.far_inland,
                    &self.data.erosions[3],
                    weirdness,
                    0.0,
                    &plateau_biome,
                );

                // erosion 4
                self.add_surface_biome(
                    entries,
                    temperature,
                    humidity,
                    &Parameter::span(
                        &self.data.continentalness.coast,
                        &self.data.continentalness.far_inland,
                    ),
                    &self.data.erosions[4],
                    weirdness,
                    0.0,
                    &middle_biome,
                );

                // erosion 5 - coast to near inland
                self.add_surface_biome(
                    entries,
                    temperature,
                    humidity,
                    &Parameter::span(
                        &self.data.continentalness.coast,
                        &self.data.continentalness.near_inland,
                    ),
                    &self.data.erosions[5],
                    weirdness,
                    0.0,
                    &shattered_or_windswept,
                );

                // erosion 5 - mid to far inland
                self.add_surface_biome(
                    entries,
                    temperature,
                    humidity,
                    &Parameter::span(
                        &self.data.continentalness.mid_inland,
                        &self.data.continentalness.far_inland,
                    ),
                    &self.data.erosions[5],
                    weirdness,
                    0.0,
                    &shattered_biome,
                );

                // erosion 6
                self.add_surface_biome(
                    entries,
                    temperature,
                    humidity,
                    &Parameter::span(
                        &self.data.continentalness.coast,
                        &self.data.continentalness.far_inland,
                    ),
                    &self.data.erosions[6],
                    weirdness,
                    0.0,
                    &middle_biome,
                );
            }
        }
    }

    fn add_high_slice(&self, entries: &mut Vec<BiomeEntry>, weirdness: &Parameter) {
        for temp_idx in 0..self.data.temperatures.len() {
            let temperature = &self.data.temperatures[temp_idx];

            for hum_idx in 0..self.data.humidities.len() {
                let humidity = &self.data.humidities[hum_idx];

                let middle_biome = self.pick_middle_biome(temp_idx, hum_idx, weirdness);
                let middle_biome_or_badlands =
                    self.pick_middle_biome_or_badlands_if_hot(temp_idx, hum_idx, weirdness);
                let middle_biome_or_badlands_or_slope = self
                    .pick_middle_biome_or_badlands_if_hot_or_slope_if_cold(
                        temp_idx, hum_idx, weirdness,
                    );
                let plateau_biome = self.pick_plateau_biome(temp_idx, hum_idx, weirdness);
                let shattered_biome = self.pick_shattered_biome(temp_idx, hum_idx, weirdness);
                let middle_or_windswept =
                    self.maybe_pick_windswept_savanna(temp_idx, hum_idx, weirdness, &middle_biome);
                let slope_biome = self.pick_slope_biome(temp_idx, hum_idx, weirdness);
                let peak_biome = self.pick_peak_biome(temp_idx, hum_idx, weirdness);

                // erosion 0-1 - coast
                self.add_surface_biome(
                    entries,
                    temperature,
                    humidity,
                    &self.data.continentalness.coast,
                    &Parameter::span(&self.data.erosions[0], &self.data.erosions[1]),
                    weirdness,
                    0.0,
                    &middle_biome,
                );

                // erosion 0 - near inland
                self.add_surface_biome(
                    entries,
                    temperature,
                    humidity,
                    &self.data.continentalness.near_inland,
                    &self.data.erosions[0],
                    weirdness,
                    0.0,
                    &slope_biome,
                );

                // erosion 0 - mid to far inland
                self.add_surface_biome(
                    entries,
                    temperature,
                    humidity,
                    &Parameter::span(
                        &self.data.continentalness.mid_inland,
                        &self.data.continentalness.far_inland,
                    ),
                    &self.data.erosions[0],
                    weirdness,
                    0.0,
                    &peak_biome,
                );

                // erosion 1 - near inland
                self.add_surface_biome(
                    entries,
                    temperature,
                    humidity,
                    &self.data.continentalness.near_inland,
                    &self.data.erosions[1],
                    weirdness,
                    0.0,
                    &middle_biome_or_badlands_or_slope,
                );

                // erosion 1 - mid to far inland
                self.add_surface_biome(
                    entries,
                    temperature,
                    humidity,
                    &Parameter::span(
                        &self.data.continentalness.mid_inland,
                        &self.data.continentalness.far_inland,
                    ),
                    &self.data.erosions[1],
                    weirdness,
                    0.0,
                    &slope_biome,
                );

                // erosion 2-3 - coast to near inland
                self.add_surface_biome(
                    entries,
                    temperature,
                    humidity,
                    &Parameter::span(
                        &self.data.continentalness.coast,
                        &self.data.continentalness.near_inland,
                    ),
                    &Parameter::span(&self.data.erosions[2], &self.data.erosions[3]),
                    weirdness,
                    0.0,
                    &middle_biome,
                );

                // erosion 2 - mid to far inland
                self.add_surface_biome(
                    entries,
                    temperature,
                    humidity,
                    &Parameter::span(
                        &self.data.continentalness.mid_inland,
                        &self.data.continentalness.far_inland,
                    ),
                    &self.data.erosions[2],
                    weirdness,
                    0.0,
                    &plateau_biome,
                );

                // erosion 3 - mid inland
                self.add_surface_biome(
                    entries,
                    temperature,
                    humidity,
                    &self.data.continentalness.mid_inland,
                    &self.data.erosions[3],
                    weirdness,
                    0.0,
                    &middle_biome_or_badlands,
                );

                // erosion 3 - far inland
                self.add_surface_biome(
                    entries,
                    temperature,
                    humidity,
                    &self.data.continentalness.far_inland,
                    &self.data.erosions[3],
                    weirdness,
                    0.0,
                    &plateau_biome,
                );

                // erosion 4
                self.add_surface_biome(
                    entries,
                    temperature,
                    humidity,
                    &Parameter::span(
                        &self.data.continentalness.coast,
                        &self.data.continentalness.far_inland,
                    ),
                    &self.data.erosions[4],
                    weirdness,
                    0.0,
                    &middle_biome,
                );

                // erosion 5 - coast to near inland
                self.add_surface_biome(
                    entries,
                    temperature,
                    humidity,
                    &Parameter::span(
                        &self.data.continentalness.coast,
                        &self.data.continentalness.near_inland,
                    ),
                    &self.data.erosions[5],
                    weirdness,
                    0.0,
                    &middle_or_windswept,
                );

                // erosion 5 - mid to far inland
                self.add_surface_biome(
                    entries,
                    temperature,
                    humidity,
                    &Parameter::span(
                        &self.data.continentalness.mid_inland,
                        &self.data.continentalness.far_inland,
                    ),
                    &self.data.erosions[5],
                    weirdness,
                    0.0,
                    &shattered_biome,
                );

                // erosion 6
                self.add_surface_biome(
                    entries,
                    temperature,
                    humidity,
                    &Parameter::span(
                        &self.data.continentalness.coast,
                        &self.data.continentalness.far_inland,
                    ),
                    &self.data.erosions[6],
                    weirdness,
                    0.0,
                    &middle_biome,
                );
            }
        }
    }

    fn add_mid_slice(&self, entries: &mut Vec<BiomeEntry>, weirdness: &Parameter) {
        // Stony shore
        self.add_surface_biome(
            entries,
            &self.full_range,
            &self.full_range,
            &self.data.continentalness.coast,
            &Parameter::span(&self.data.erosions[0], &self.data.erosions[2]),
            weirdness,
            0.0,
            "minecraft:stony_shore",
        );

        // Swamp - temperatures 1-2
        self.add_surface_biome(
            entries,
            &Parameter::span(&self.data.temperatures[1], &self.data.temperatures[2]),
            &self.full_range,
            &Parameter::span(
                &self.data.continentalness.near_inland,
                &self.data.continentalness.far_inland,
            ),
            &self.data.erosions[6],
            weirdness,
            0.0,
            "minecraft:swamp",
        );

        // Mangrove swamp - temperatures 3-4
        self.add_surface_biome(
            entries,
            &Parameter::span(&self.data.temperatures[3], &self.data.temperatures[4]),
            &self.full_range,
            &Parameter::span(
                &self.data.continentalness.near_inland,
                &self.data.continentalness.far_inland,
            ),
            &self.data.erosions[6],
            weirdness,
            0.0,
            "minecraft:mangrove_swamp",
        );

        for temp_idx in 0..self.data.temperatures.len() {
            let temperature = &self.data.temperatures[temp_idx];

            for hum_idx in 0..self.data.humidities.len() {
                let humidity = &self.data.humidities[hum_idx];

                let middle_biome = self.pick_middle_biome(temp_idx, hum_idx, weirdness);
                let middle_biome_or_badlands =
                    self.pick_middle_biome_or_badlands_if_hot(temp_idx, hum_idx, weirdness);
                let middle_biome_or_badlands_or_slope = self
                    .pick_middle_biome_or_badlands_if_hot_or_slope_if_cold(
                        temp_idx, hum_idx, weirdness,
                    );
                let shattered_biome = self.pick_shattered_biome(temp_idx, hum_idx, weirdness);
                let plateau_biome = self.pick_plateau_biome(temp_idx, hum_idx, weirdness);
                let beach_biome = self.pick_beach_biome(temp_idx, hum_idx);
                let middle_or_windswept =
                    self.maybe_pick_windswept_savanna(temp_idx, hum_idx, weirdness, &middle_biome);
                let shattered_coast_biome =
                    self.pick_shattered_coast_biome(temp_idx, hum_idx, weirdness);
                let slope_biome = self.pick_slope_biome(temp_idx, hum_idx, weirdness);

                // erosion 0 - near to far inland
                self.add_surface_biome(
                    entries,
                    temperature,
                    humidity,
                    &Parameter::span(
                        &self.data.continentalness.near_inland,
                        &self.data.continentalness.far_inland,
                    ),
                    &self.data.erosions[0],
                    weirdness,
                    0.0,
                    &slope_biome,
                );

                // erosion 1 - near to mid inland
                self.add_surface_biome(
                    entries,
                    temperature,
                    humidity,
                    &Parameter::span(
                        &self.data.continentalness.near_inland,
                        &self.data.continentalness.mid_inland,
                    ),
                    &self.data.erosions[1],
                    weirdness,
                    0.0,
                    &middle_biome_or_badlands_or_slope,
                );

                // erosion 1 - far inland
                let far_inland_erosion1 = if temp_idx == 0 {
                    &slope_biome
                } else {
                    &plateau_biome
                };
                self.add_surface_biome(
                    entries,
                    temperature,
                    humidity,
                    &self.data.continentalness.far_inland,
                    &self.data.erosions[1],
                    weirdness,
                    0.0,
                    far_inland_erosion1,
                );

                // erosion 2 - near inland
                self.add_surface_biome(
                    entries,
                    temperature,
                    humidity,
                    &self.data.continentalness.near_inland,
                    &self.data.erosions[2],
                    weirdness,
                    0.0,
                    &middle_biome,
                );

                // erosion 2 - mid inland
                self.add_surface_biome(
                    entries,
                    temperature,
                    humidity,
                    &self.data.continentalness.mid_inland,
                    &self.data.erosions[2],
                    weirdness,
                    0.0,
                    &middle_biome_or_badlands,
                );

                // erosion 2 - far inland
                self.add_surface_biome(
                    entries,
                    temperature,
                    humidity,
                    &self.data.continentalness.far_inland,
                    &self.data.erosions[2],
                    weirdness,
                    0.0,
                    &plateau_biome,
                );

                // erosion 3 - coast to near inland
                self.add_surface_biome(
                    entries,
                    temperature,
                    humidity,
                    &Parameter::span(
                        &self.data.continentalness.coast,
                        &self.data.continentalness.near_inland,
                    ),
                    &self.data.erosions[3],
                    weirdness,
                    0.0,
                    &middle_biome,
                );

                // erosion 3 - mid to far inland
                self.add_surface_biome(
                    entries,
                    temperature,
                    humidity,
                    &Parameter::span(
                        &self.data.continentalness.mid_inland,
                        &self.data.continentalness.far_inland,
                    ),
                    &self.data.erosions[3],
                    weirdness,
                    0.0,
                    &middle_biome_or_badlands,
                );

                // erosion 4 - beach or middle biome depending on weirdness
                if weirdness.max < 0.0 {
                    self.add_surface_biome(
                        entries,
                        temperature,
                        humidity,
                        &self.data.continentalness.coast,
                        &self.data.erosions[4],
                        weirdness,
                        0.0,
                        &beach_biome,
                    );
                    self.add_surface_biome(
                        entries,
                        temperature,
                        humidity,
                        &Parameter::span(
                            &self.data.continentalness.near_inland,
                            &self.data.continentalness.far_inland,
                        ),
                        &self.data.erosions[4],
                        weirdness,
                        0.0,
                        &middle_biome,
                    );
                } else {
                    self.add_surface_biome(
                        entries,
                        temperature,
                        humidity,
                        &Parameter::span(
                            &self.data.continentalness.coast,
                            &self.data.continentalness.far_inland,
                        ),
                        &self.data.erosions[4],
                        weirdness,
                        0.0,
                        &middle_biome,
                    );
                }

                // erosion 5 - coast
                self.add_surface_biome(
                    entries,
                    temperature,
                    humidity,
                    &self.data.continentalness.coast,
                    &self.data.erosions[5],
                    weirdness,
                    0.0,
                    &shattered_coast_biome,
                );

                // erosion 5 - near inland
                self.add_surface_biome(
                    entries,
                    temperature,
                    humidity,
                    &self.data.continentalness.near_inland,
                    &self.data.erosions[5],
                    weirdness,
                    0.0,
                    &middle_or_windswept,
                );

                // erosion 5 - mid to far inland
                self.add_surface_biome(
                    entries,
                    temperature,
                    humidity,
                    &Parameter::span(
                        &self.data.continentalness.mid_inland,
                        &self.data.continentalness.far_inland,
                    ),
                    &self.data.erosions[5],
                    weirdness,
                    0.0,
                    &shattered_biome,
                );

                // erosion 6 - coast (beach or middle depending on weirdness)
                if weirdness.max < 0.0 {
                    self.add_surface_biome(
                        entries,
                        temperature,
                        humidity,
                        &self.data.continentalness.coast,
                        &self.data.erosions[6],
                        weirdness,
                        0.0,
                        &beach_biome,
                    );
                } else {
                    self.add_surface_biome(
                        entries,
                        temperature,
                        humidity,
                        &self.data.continentalness.coast,
                        &self.data.erosions[6],
                        weirdness,
                        0.0,
                        &middle_biome,
                    );
                }

                // erosion 6 - frozen temperature inland
                if temp_idx == 0 {
                    self.add_surface_biome(
                        entries,
                        temperature,
                        humidity,
                        &Parameter::span(
                            &self.data.continentalness.near_inland,
                            &self.data.continentalness.far_inland,
                        ),
                        &self.data.erosions[6],
                        weirdness,
                        0.0,
                        &middle_biome,
                    );
                }
            }
        }
    }

    fn add_low_slice(&self, entries: &mut Vec<BiomeEntry>, weirdness: &Parameter) {
        // Stony shore
        self.add_surface_biome(
            entries,
            &self.full_range,
            &self.full_range,
            &self.data.continentalness.coast,
            &Parameter::span(&self.data.erosions[0], &self.data.erosions[2]),
            weirdness,
            0.0,
            "minecraft:stony_shore",
        );

        // Swamp - temperatures 1-2
        self.add_surface_biome(
            entries,
            &Parameter::span(&self.data.temperatures[1], &self.data.temperatures[2]),
            &self.full_range,
            &Parameter::span(
                &self.data.continentalness.near_inland,
                &self.data.continentalness.far_inland,
            ),
            &self.data.erosions[6],
            weirdness,
            0.0,
            "minecraft:swamp",
        );

        // Mangrove swamp - temperatures 3-4
        self.add_surface_biome(
            entries,
            &Parameter::span(&self.data.temperatures[3], &self.data.temperatures[4]),
            &self.full_range,
            &Parameter::span(
                &self.data.continentalness.near_inland,
                &self.data.continentalness.far_inland,
            ),
            &self.data.erosions[6],
            weirdness,
            0.0,
            "minecraft:mangrove_swamp",
        );

        for temp_idx in 0..self.data.temperatures.len() {
            let temperature = &self.data.temperatures[temp_idx];

            for hum_idx in 0..self.data.humidities.len() {
                let humidity = &self.data.humidities[hum_idx];

                let middle_biome = self.pick_middle_biome(temp_idx, hum_idx, weirdness);
                let middle_biome_or_badlands =
                    self.pick_middle_biome_or_badlands_if_hot(temp_idx, hum_idx, weirdness);
                let middle_biome_or_badlands_or_slope = self
                    .pick_middle_biome_or_badlands_if_hot_or_slope_if_cold(
                        temp_idx, hum_idx, weirdness,
                    );
                let beach_biome = self.pick_beach_biome(temp_idx, hum_idx);
                let middle_or_windswept =
                    self.maybe_pick_windswept_savanna(temp_idx, hum_idx, weirdness, &middle_biome);
                let shattered_coast_biome =
                    self.pick_shattered_coast_biome(temp_idx, hum_idx, weirdness);

                // erosion 0-1 - near inland
                self.add_surface_biome(
                    entries,
                    temperature,
                    humidity,
                    &self.data.continentalness.near_inland,
                    &Parameter::span(&self.data.erosions[0], &self.data.erosions[1]),
                    weirdness,
                    0.0,
                    &middle_biome_or_badlands,
                );

                // erosion 0-1 - mid to far inland
                self.add_surface_biome(
                    entries,
                    temperature,
                    humidity,
                    &Parameter::span(
                        &self.data.continentalness.mid_inland,
                        &self.data.continentalness.far_inland,
                    ),
                    &Parameter::span(&self.data.erosions[0], &self.data.erosions[1]),
                    weirdness,
                    0.0,
                    &middle_biome_or_badlands_or_slope,
                );

                // erosion 2-3 - near inland
                self.add_surface_biome(
                    entries,
                    temperature,
                    humidity,
                    &self.data.continentalness.near_inland,
                    &Parameter::span(&self.data.erosions[2], &self.data.erosions[3]),
                    weirdness,
                    0.0,
                    &middle_biome,
                );

                // erosion 2-3 - mid to far inland
                self.add_surface_biome(
                    entries,
                    temperature,
                    humidity,
                    &Parameter::span(
                        &self.data.continentalness.mid_inland,
                        &self.data.continentalness.far_inland,
                    ),
                    &Parameter::span(&self.data.erosions[2], &self.data.erosions[3]),
                    weirdness,
                    0.0,
                    &middle_biome_or_badlands,
                );

                // erosion 3-4 - coast
                self.add_surface_biome(
                    entries,
                    temperature,
                    humidity,
                    &self.data.continentalness.coast,
                    &Parameter::span(&self.data.erosions[3], &self.data.erosions[4]),
                    weirdness,
                    0.0,
                    &beach_biome,
                );

                // erosion 4 - near to far inland
                self.add_surface_biome(
                    entries,
                    temperature,
                    humidity,
                    &Parameter::span(
                        &self.data.continentalness.near_inland,
                        &self.data.continentalness.far_inland,
                    ),
                    &self.data.erosions[4],
                    weirdness,
                    0.0,
                    &middle_biome,
                );

                // erosion 5 - coast
                self.add_surface_biome(
                    entries,
                    temperature,
                    humidity,
                    &self.data.continentalness.coast,
                    &self.data.erosions[5],
                    weirdness,
                    0.0,
                    &shattered_coast_biome,
                );

                // erosion 5 - near inland
                self.add_surface_biome(
                    entries,
                    temperature,
                    humidity,
                    &self.data.continentalness.near_inland,
                    &self.data.erosions[5],
                    weirdness,
                    0.0,
                    &middle_or_windswept,
                );

                // erosion 5 - mid to far inland
                self.add_surface_biome(
                    entries,
                    temperature,
                    humidity,
                    &Parameter::span(
                        &self.data.continentalness.mid_inland,
                        &self.data.continentalness.far_inland,
                    ),
                    &self.data.erosions[5],
                    weirdness,
                    0.0,
                    &middle_biome,
                );

                // erosion 6 - coast
                self.add_surface_biome(
                    entries,
                    temperature,
                    humidity,
                    &self.data.continentalness.coast,
                    &self.data.erosions[6],
                    weirdness,
                    0.0,
                    &beach_biome,
                );

                // erosion 6 - frozen temperature inland
                if temp_idx == 0 {
                    self.add_surface_biome(
                        entries,
                        temperature,
                        humidity,
                        &Parameter::span(
                            &self.data.continentalness.near_inland,
                            &self.data.continentalness.far_inland,
                        ),
                        &self.data.erosions[6],
                        weirdness,
                        0.0,
                        &middle_biome,
                    );
                }
            }
        }
    }

    fn add_valleys(&self, entries: &mut Vec<BiomeEntry>, weirdness: &Parameter) {
        // Frozen river/stony shore at coast erosion 0-1
        let frozen_coast_biome = if weirdness.max < 0.0 {
            "minecraft:stony_shore"
        } else {
            "minecraft:frozen_river"
        };
        self.add_surface_biome(
            entries,
            &self.frozen_range,
            &self.full_range,
            &self.data.continentalness.coast,
            &Parameter::span(&self.data.erosions[0], &self.data.erosions[1]),
            weirdness,
            0.0,
            frozen_coast_biome,
        );

        // River/stony shore at coast erosion 0-1 (unfrozen)
        let unfrozen_coast_biome = if weirdness.max < 0.0 {
            "minecraft:stony_shore"
        } else {
            "minecraft:river"
        };
        self.add_surface_biome(
            entries,
            &self.unfrozen_range,
            &self.full_range,
            &self.data.continentalness.coast,
            &Parameter::span(&self.data.erosions[0], &self.data.erosions[1]),
            weirdness,
            0.0,
            unfrozen_coast_biome,
        );

        // Frozen river - near inland erosion 0-1
        self.add_surface_biome(
            entries,
            &self.frozen_range,
            &self.full_range,
            &self.data.continentalness.near_inland,
            &Parameter::span(&self.data.erosions[0], &self.data.erosions[1]),
            weirdness,
            0.0,
            "minecraft:frozen_river",
        );

        // River - near inland erosion 0-1
        self.add_surface_biome(
            entries,
            &self.unfrozen_range,
            &self.full_range,
            &self.data.continentalness.near_inland,
            &Parameter::span(&self.data.erosions[0], &self.data.erosions[1]),
            weirdness,
            0.0,
            "minecraft:river",
        );

        // Frozen river - coast to far inland erosion 2-5
        self.add_surface_biome(
            entries,
            &self.frozen_range,
            &self.full_range,
            &Parameter::span(
                &self.data.continentalness.coast,
                &self.data.continentalness.far_inland,
            ),
            &Parameter::span(&self.data.erosions[2], &self.data.erosions[5]),
            weirdness,
            0.0,
            "minecraft:frozen_river",
        );

        // River - coast to far inland erosion 2-5
        self.add_surface_biome(
            entries,
            &self.unfrozen_range,
            &self.full_range,
            &Parameter::span(
                &self.data.continentalness.coast,
                &self.data.continentalness.far_inland,
            ),
            &Parameter::span(&self.data.erosions[2], &self.data.erosions[5]),
            weirdness,
            0.0,
            "minecraft:river",
        );

        // Frozen river - coast erosion 6
        self.add_surface_biome(
            entries,
            &self.frozen_range,
            &self.full_range,
            &self.data.continentalness.coast,
            &self.data.erosions[6],
            weirdness,
            0.0,
            "minecraft:frozen_river",
        );

        // River - coast erosion 6
        self.add_surface_biome(
            entries,
            &self.unfrozen_range,
            &self.full_range,
            &self.data.continentalness.coast,
            &self.data.erosions[6],
            weirdness,
            0.0,
            "minecraft:river",
        );

        // Swamp - temperatures 1-2 erosion 6 inland
        self.add_surface_biome(
            entries,
            &Parameter::span(&self.data.temperatures[1], &self.data.temperatures[2]),
            &self.full_range,
            &Parameter::span(
                &self.data.continentalness.inland,
                &self.data.continentalness.far_inland,
            ),
            &self.data.erosions[6],
            weirdness,
            0.0,
            "minecraft:swamp",
        );

        // Mangrove swamp - temperatures 3-4 erosion 6 inland
        self.add_surface_biome(
            entries,
            &Parameter::span(&self.data.temperatures[3], &self.data.temperatures[4]),
            &self.full_range,
            &Parameter::span(
                &self.data.continentalness.inland,
                &self.data.continentalness.far_inland,
            ),
            &self.data.erosions[6],
            weirdness,
            0.0,
            "minecraft:mangrove_swamp",
        );

        // Frozen river - inland erosion 6
        self.add_surface_biome(
            entries,
            &self.frozen_range,
            &self.full_range,
            &Parameter::span(
                &self.data.continentalness.inland,
                &self.data.continentalness.far_inland,
            ),
            &self.data.erosions[6],
            weirdness,
            0.0,
            "minecraft:frozen_river",
        );

        // Per temp/humidity combos for erosion 0-1 mid to far inland
        for temp_idx in 0..self.data.temperatures.len() {
            let temperature = &self.data.temperatures[temp_idx];

            for hum_idx in 0..self.data.humidities.len() {
                let humidity = &self.data.humidities[hum_idx];
                let middle_biome_or_badlands =
                    self.pick_middle_biome_or_badlands_if_hot(temp_idx, hum_idx, weirdness);

                self.add_surface_biome(
                    entries,
                    temperature,
                    humidity,
                    &Parameter::span(
                        &self.data.continentalness.mid_inland,
                        &self.data.continentalness.far_inland,
                    ),
                    &Parameter::span(&self.data.erosions[0], &self.data.erosions[1]),
                    weirdness,
                    0.0,
                    &middle_biome_or_badlands,
                );
            }
        }
    }

    fn add_underground_biomes(&self, entries: &mut Vec<BiomeEntry>) {
        // Dripstone caves
        self.add_underground_biome(
            entries,
            &self.full_range,
            &self.full_range,
            &Parameter::new(0.8, 1.0),
            &self.full_range,
            &self.full_range,
            0.0,
            "minecraft:dripstone_caves",
        );

        // Lush caves
        self.add_underground_biome(
            entries,
            &self.full_range,
            &Parameter::new(0.7, 1.0),
            &self.full_range,
            &self.full_range,
            &self.full_range,
            0.0,
            "minecraft:lush_caves",
        );

        // Deep dark
        self.add_bottom_biome(
            entries,
            &self.full_range,
            &self.full_range,
            &self.full_range,
            &Parameter::span(&self.data.erosions[0], &self.data.erosions[1]),
            &self.full_range,
            0.0,
            "minecraft:deep_dark",
        );
    }

    // Biome picker methods

    fn pick_middle_biome(&self, temp_idx: usize, hum_idx: usize, weirdness: &Parameter) -> String {
        if weirdness.max < 0.0 {
            self.data.middle_biomes[temp_idx][hum_idx].clone()
        } else {
            self.data.middle_biomes_variant[temp_idx][hum_idx]
                .clone()
                .unwrap_or_else(|| self.data.middle_biomes[temp_idx][hum_idx].clone())
        }
    }

    fn pick_middle_biome_or_badlands_if_hot(
        &self,
        temp_idx: usize,
        hum_idx: usize,
        weirdness: &Parameter,
    ) -> String {
        if temp_idx == 4 {
            self.pick_badlands_biome(hum_idx, weirdness)
        } else {
            self.pick_middle_biome(temp_idx, hum_idx, weirdness)
        }
    }

    fn pick_middle_biome_or_badlands_if_hot_or_slope_if_cold(
        &self,
        temp_idx: usize,
        hum_idx: usize,
        weirdness: &Parameter,
    ) -> String {
        if temp_idx == 0 {
            self.pick_slope_biome(temp_idx, hum_idx, weirdness)
        } else {
            self.pick_middle_biome_or_badlands_if_hot(temp_idx, hum_idx, weirdness)
        }
    }

    fn maybe_pick_windswept_savanna(
        &self,
        temp_idx: usize,
        hum_idx: usize,
        weirdness: &Parameter,
        underlying: &str,
    ) -> String {
        if temp_idx > 1 && hum_idx < 4 && weirdness.max >= 0.0 {
            "minecraft:windswept_savanna".to_string()
        } else {
            underlying.to_string()
        }
    }

    fn pick_shattered_coast_biome(
        &self,
        temp_idx: usize,
        hum_idx: usize,
        weirdness: &Parameter,
    ) -> String {
        let beach_or_middle = if weirdness.max >= 0.0 {
            self.pick_middle_biome(temp_idx, hum_idx, weirdness)
        } else {
            self.pick_beach_biome(temp_idx, hum_idx)
        };
        self.maybe_pick_windswept_savanna(temp_idx, hum_idx, weirdness, &beach_or_middle)
    }

    fn pick_beach_biome(&self, temp_idx: usize, _hum_idx: usize) -> String {
        if temp_idx == 0 {
            "minecraft:snowy_beach".to_string()
        } else if temp_idx == 4 {
            "minecraft:desert".to_string()
        } else {
            "minecraft:beach".to_string()
        }
    }

    fn pick_badlands_biome(&self, hum_idx: usize, weirdness: &Parameter) -> String {
        if hum_idx < 2 {
            if weirdness.max < 0.0 {
                "minecraft:badlands".to_string()
            } else {
                "minecraft:eroded_badlands".to_string()
            }
        } else if hum_idx < 3 {
            "minecraft:badlands".to_string()
        } else {
            "minecraft:wooded_badlands".to_string()
        }
    }

    fn pick_plateau_biome(&self, temp_idx: usize, hum_idx: usize, weirdness: &Parameter) -> String {
        if weirdness.max >= 0.0
            && let Some(ref variant) = self.data.plateau_biomes_variant[temp_idx][hum_idx]
        {
            return variant.clone();
        }
        self.data.plateau_biomes[temp_idx][hum_idx].clone()
    }

    fn pick_peak_biome(&self, temp_idx: usize, hum_idx: usize, weirdness: &Parameter) -> String {
        if temp_idx <= 2 {
            if weirdness.max < 0.0 {
                "minecraft:jagged_peaks".to_string()
            } else {
                "minecraft:frozen_peaks".to_string()
            }
        } else if temp_idx == 3 {
            "minecraft:stony_peaks".to_string()
        } else {
            self.pick_badlands_biome(hum_idx, weirdness)
        }
    }

    fn pick_slope_biome(&self, temp_idx: usize, hum_idx: usize, weirdness: &Parameter) -> String {
        if temp_idx >= 3 {
            self.pick_plateau_biome(temp_idx, hum_idx, weirdness)
        } else if hum_idx <= 1 {
            "minecraft:snowy_slopes".to_string()
        } else {
            "minecraft:grove".to_string()
        }
    }

    fn pick_shattered_biome(
        &self,
        temp_idx: usize,
        hum_idx: usize,
        weirdness: &Parameter,
    ) -> String {
        self.data.shattered_biomes[temp_idx][hum_idx]
            .clone()
            .unwrap_or_else(|| self.pick_middle_biome(temp_idx, hum_idx, weirdness))
    }

    // Add biome entry helpers

    #[allow(clippy::too_many_arguments)]
    fn add_surface_biome(
        &self,
        entries: &mut Vec<BiomeEntry>,
        temperature: &Parameter,
        humidity: &Parameter,
        continentalness: &Parameter,
        erosion: &Parameter,
        weirdness: &Parameter,
        offset: f64,
        biome: &str,
    ) {
        // Surface biomes are added at depth 0.0 and 1.0
        entries.push(BiomeEntry {
            temperature: *temperature,
            humidity: *humidity,
            continentalness: *continentalness,
            erosion: *erosion,
            depth: Parameter::point(0.0),
            weirdness: *weirdness,
            offset,
            biome: biome.to_string(),
        });
        entries.push(BiomeEntry {
            temperature: *temperature,
            humidity: *humidity,
            continentalness: *continentalness,
            erosion: *erosion,
            depth: Parameter::point(1.0),
            weirdness: *weirdness,
            offset,
            biome: biome.to_string(),
        });
    }

    #[allow(clippy::too_many_arguments)]
    fn add_underground_biome(
        &self,
        entries: &mut Vec<BiomeEntry>,
        temperature: &Parameter,
        humidity: &Parameter,
        continentalness: &Parameter,
        erosion: &Parameter,
        weirdness: &Parameter,
        offset: f64,
        biome: &str,
    ) {
        entries.push(BiomeEntry {
            temperature: *temperature,
            humidity: *humidity,
            continentalness: *continentalness,
            erosion: *erosion,
            depth: Parameter::new(0.2, 0.9),
            weirdness: *weirdness,
            offset,
            biome: biome.to_string(),
        });
    }

    #[allow(clippy::too_many_arguments)]
    fn add_bottom_biome(
        &self,
        entries: &mut Vec<BiomeEntry>,
        temperature: &Parameter,
        humidity: &Parameter,
        continentalness: &Parameter,
        erosion: &Parameter,
        weirdness: &Parameter,
        offset: f64,
        biome: &str,
    ) {
        entries.push(BiomeEntry {
            temperature: *temperature,
            humidity: *humidity,
            continentalness: *continentalness,
            erosion: *erosion,
            depth: Parameter::point(1.1),
            weirdness: *weirdness,
            offset,
            biome: biome.to_string(),
        });
    }
}
