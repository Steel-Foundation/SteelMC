//! Level data persistence module.
//!
//! This module handles saving and loading world-level data like game rules,
//! time, weather, spawn point, and seed. This data is stored in `level.toml`
//! in each world's directory.

use std::{
    io,
    path::{Path, PathBuf},
};

use rustc_hash::FxHashMap;
use serde::{Deserialize, Serialize};
use steel_registry::REGISTRY;
use steel_registry::game_rules::{GameRuleValue, GameRuleValues};
use steel_utils::types::Difficulty;
use steel_utils::{BlockPos, Identifier};
use tokio::fs;

/// Persistent level data that gets saved to disk.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LevelData {
    /// World seed for terrain generation.
    pub seed: i64,
    /// Total game time in ticks.
    pub game_time: i64,
    /// Time of day in ticks (0-24000).
    pub day_time: i64,
    /// World spawn point.
    pub spawn: SpawnPoint,
    /// Weather state.
    pub weather: WeatherState,
    /// World difficulty.
    #[serde(default)]
    pub difficulty: Difficulty,
    /// Whether the difficulty is locked.
    #[serde(default)]
    pub difficulty_locked: bool,
    /// Game rules (stored as name -> value pairs for serialization).
    pub game_rules: FxHashMap<String, GameRuleValue>,
    /// Runtime game rule values (not serialized, loaded from `game_rules`).
    #[serde(skip)]
    pub game_rules_values: GameRuleValues,
    /// Whether the world has been initialized.
    pub initialized: bool,
    /// Generator settings this persisted world was created with.
    #[serde(default)]
    pub generation: Option<WorldGenerationSettings>,
}

/// Persisted generator metadata used to reject incompatible config changes.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WorldGenerationSettings {
    /// Generator factory identifier.
    pub generator: Identifier,
    /// Generator config after applying generator defaults.
    pub config: toml::Value,
    /// Dimension type used by the generator output.
    pub dimension_type: Identifier,
    /// Minimum build Y for the dimension type.
    pub min_y: i32,
    /// Total build height for the dimension type.
    pub height: i32,
}

impl WorldGenerationSettings {
    /// Builds persisted generator metadata from the resolved startup config.
    #[must_use]
    pub fn from_generator_config(
        generator: Identifier,
        config: &toml::Value,
        dimension_type: Identifier,
        min_y: i32,
        height: i32,
    ) -> Self {
        Self {
            generator,
            config: config.clone(),
            dimension_type,
            min_y,
            height,
        }
    }
}

fn describe_generation_settings(settings: &WorldGenerationSettings) -> String {
    format!(
        "generator {}, dimension_type {}, min_y {}, height {}, config {}",
        settings.generator,
        settings.dimension_type,
        settings.min_y,
        settings.height,
        generation_config_string(&settings.config),
    )
}

fn generation_config_string(config: &toml::Value) -> String {
    match toml::to_string(config) {
        Ok(value) => value.trim().to_owned(),
        Err(_) => "<invalid generator config>".to_owned(),
    }
}

/// Spawn point data.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpawnPoint {
    /// X coordinate.
    pub x: i32,
    /// Y coordinate.
    pub y: i32,
    /// Z coordinate.
    pub z: i32,
    /// Spawn angle (yaw).
    pub angle: f32,
}

impl Default for SpawnPoint {
    fn default() -> Self {
        Self {
            x: 0,
            y: 64,
            z: 0,
            angle: 0.0,
        }
    }
}

/// Weather state.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct WeatherState {
    /// Whether it is currently raining.
    pub raining: bool,
    /// Ticks until rain state changes.
    pub rain_time: i32,
    /// Whether it is currently thundering.
    pub thundering: bool,
    /// Ticks until thunder state changes.
    pub thunder_time: i32,
    /// Ticks of clear weather remaining.
    pub clear_weather_time: i32,
}

impl Default for LevelData {
    fn default() -> Self {
        Self::new_with_seed(rand::random())
    }
}

impl LevelData {
    /// Creates new level data with the given seed.
    #[must_use]
    pub fn new_with_seed(seed: i64) -> Self {
        Self::new_with_seed_and_difficulty(seed, Difficulty::default())
    }

    /// Creates new level data with the given seed and difficulty.
    #[must_use]
    pub fn new_with_seed_and_difficulty(seed: i64, difficulty: Difficulty) -> Self {
        Self {
            seed,
            game_time: 0,
            day_time: 0,
            spawn: SpawnPoint::default(),
            weather: WeatherState::default(),
            difficulty,
            difficulty_locked: false,
            game_rules: FxHashMap::default(),
            game_rules_values: GameRuleValues::new(&REGISTRY.game_rules),
            initialized: false,
            generation: None,
        }
    }

    /// Verifies saved generator metadata against the current config.
    ///
    /// Returns whether missing metadata was adopted and should be saved.
    pub fn validate_generation_settings(
        &mut self,
        expected: WorldGenerationSettings,
    ) -> io::Result<bool> {
        match self.generation.as_ref() {
            Some(saved) if saved == &expected => Ok(false),
            Some(saved) => Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!(
                    "world generation settings do not match saved level data: saved {}; configured {}. Delete or regenerate this world's saved chunks, or restore the previous generator config.",
                    describe_generation_settings(saved),
                    describe_generation_settings(&expected),
                ),
            )),
            None => {
                self.generation = Some(expected);
                Ok(true)
            }
        }
    }

    /// Loads game rules from the serialized map into the runtime values.
    pub fn load_game_rules(&mut self) {
        self.game_rules_values = GameRuleValues::new(&REGISTRY.game_rules);
        for (name, value) in &self.game_rules {
            self.game_rules_values
                .set_by_name(name, *value, &REGISTRY.game_rules);
        }
    }

    /// Saves game rules from the runtime values to the serialized map.
    pub fn save_game_rules(&mut self) {
        self.game_rules.clear();
        for (_, rule) in REGISTRY.game_rules.iter() {
            let name = rule.key.path.to_string();
            let value = self.game_rules_values.get(rule, &REGISTRY.game_rules);
            self.game_rules.insert(name, value);
        }
    }

    /// Gets the spawn position as a `BlockPos`.
    #[must_use]
    pub const fn spawn_pos(&self) -> BlockPos {
        BlockPos::new(self.spawn.x, self.spawn.y, self.spawn.z)
    }

    /// Sets the spawn position from a `BlockPos`.
    pub const fn set_spawn_pos(&mut self, pos: BlockPos) {
        self.spawn.x = pos.x();
        self.spawn.y = pos.y();
        self.spawn.z = pos.z();
    }
}

/// Manages level data persistence for a world.
pub struct LevelDataManager {
    /// Path to the level.toml file.
    path: Option<PathBuf>,
    /// Cached level data.
    data: LevelData,
    /// Whether data has been modified since last save.
    dirty: bool,
}

impl LevelDataManager {
    /// Creates a new level data manager for the given world directory.
    ///
    /// If `level.toml` exists, it will be loaded (the provided seed is ignored).
    /// Otherwise, new data will be created with the provided seed.
    pub async fn new(
        world_dir: Option<impl AsRef<Path>>,
        seed: i64,
        difficulty: Difficulty,
        generation: WorldGenerationSettings,
    ) -> io::Result<Self> {
        let (data, path, dirty) = if let Some(dir) = &world_dir {
            let path = dir.as_ref().join("level.toml");

            let (data, dirty) = if path.exists() {
                // Load existing level data (seed from file takes precedence)
                let content = fs::read_to_string(&path).await?;
                let mut loaded: LevelData = toml::from_str(&content).map_err(|e| {
                    io::Error::new(
                        io::ErrorKind::InvalidData,
                        format!("Invalid level.toml: {e}"),
                    )
                })?;
                // Initialize runtime game rules from serialized values
                loaded.load_game_rules();
                let adopted_generation = loaded.validate_generation_settings(generation)?;
                (loaded, adopted_generation)
            } else {
                // Create new level data with the provided defaults.
                let mut data = LevelData::new_with_seed_and_difficulty(seed, difficulty);
                data.generation = Some(generation);
                (data, true)
            };
            (data, Some(path), dirty)
        } else {
            let mut data = LevelData::new_with_seed_and_difficulty(seed, difficulty);
            data.generation = Some(generation);
            (data, None, false)
        };

        Ok(Self { path, data, dirty })
    }

    /// Gets a reference to the level data.
    #[must_use]
    pub const fn data(&self) -> &LevelData {
        &self.data
    }

    /// Gets a mutable reference to the level data and marks it as dirty.
    pub const fn data_mut(&mut self) -> &mut LevelData {
        self.dirty = true;
        &mut self.data
    }

    /// Returns whether the data has been modified since last save.
    #[must_use]
    pub const fn is_dirty(&self) -> bool {
        self.dirty
    }

    /// Marks the data as dirty (needs saving).
    pub const fn mark_dirty(&mut self) {
        self.dirty = true;
    }

    /// Saves the level data to disk if it has been modified.
    pub async fn save(&mut self) -> io::Result<()> {
        if !self.dirty {
            return Ok(());
        }

        let Some(world_path) = &self.path else {
            self.dirty = false;
            return Ok(());
        };
        if let Some(parent) = world_path.parent() {
            fs::create_dir_all(parent).await?;
        }

        // Export runtime game rules to serializable format before saving
        self.data.save_game_rules();

        let content = toml::to_string_pretty(&self.data)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        fs::write(world_path, content).await?;
        self.dirty = false;

        log::debug!("Saved level data to {}", world_path.display());
        Ok(())
    }

    /// Gets the seed.
    #[must_use]
    pub const fn seed(&self) -> i64 {
        self.data.seed
    }

    /// Gets the game time.
    #[must_use]
    pub const fn game_time(&self) -> i64 {
        self.data.game_time
    }

    /// Sets the game time.
    pub const fn set_game_time(&mut self, time: i64) {
        self.data.game_time = time;
        self.dirty = true;
    }

    /// Calculates the day based on the game time
    #[must_use]
    pub const fn day(&self) -> i64 {
        self.data.game_time / 24000
    }

    /// Gets the day time.
    #[must_use]
    pub const fn day_time(&self) -> i64 {
        self.data.day_time
    }

    /// Sets the day time.
    pub const fn set_day_time(&mut self, time: i64) {
        self.data.day_time = time;
        self.dirty = true;
    }

    /// Gets the clear weather time
    #[must_use]
    pub const fn clear_weather_time(&self) -> i32 {
        self.data.weather.clear_weather_time
    }

    /// Sets the clear weather time
    pub const fn set_clear_weather_time(&mut self, time: i32) {
        self.data.weather.clear_weather_time = time;
        self.dirty = true;
    }

    /// Gets the rain time
    #[must_use]
    pub const fn rain_time(&self) -> i32 {
        self.data.weather.rain_time
    }

    /// Sets the rain time
    pub const fn set_rain_time(&mut self, time: i32) {
        self.data.weather.rain_time = time;
        self.dirty = true;
    }

    /// Gets the thunder time
    #[must_use]
    pub const fn thunder_time(&self) -> i32 {
        self.data.weather.thunder_time
    }

    /// Sets the thunder time
    pub const fn set_thunder_time(&mut self, time: i32) {
        self.data.weather.thunder_time = time;
        self.dirty = true;
    }

    /// Checks if it's raining
    #[must_use]
    pub const fn is_raining(&self) -> bool {
        self.data.weather.raining
    }

    /// Sets whether it's raining
    pub const fn set_raining(&mut self, raining: bool) {
        self.data.weather.raining = raining;
        self.dirty = true;
    }

    /// Checks if it's thundering
    #[must_use]
    pub const fn is_thundering(&self) -> bool {
        self.data.weather.thundering
    }

    /// Sets whether it's thundering
    pub const fn set_thundering(&mut self, thundering: bool) {
        self.data.weather.thundering = thundering;
        self.dirty = true;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use steel_registry::Registry;
    use toml::map::Map;

    fn init_registry() {
        let mut registry = Registry::new_vanilla();
        registry.freeze();
        let _ = REGISTRY.init(registry);
    }

    fn settings(dimension_type: &str, height: i32) -> WorldGenerationSettings {
        let mut config = Map::new();
        config.insert(
            "dimension_type".to_owned(),
            toml::Value::String(dimension_type.to_owned()),
        );
        WorldGenerationSettings {
            generator: Identifier::vanilla_static("flat"),
            config: toml::Value::Table(config),
            dimension_type: dimension_type
                .parse()
                .expect("valid dimension type identifier"),
            min_y: 0,
            height,
        }
    }

    #[test]
    fn adopts_missing_generation_settings() {
        init_registry();
        let mut data = LevelData::new_with_seed(1);

        let adopted = data
            .validate_generation_settings(settings("minecraft:overworld", 384))
            .expect("missing settings should be adopted");

        assert!(adopted);
        assert!(data.generation.is_some());
    }

    #[test]
    fn rejects_mismatched_generation_settings() {
        init_registry();
        let mut data = LevelData::new_with_seed(1);
        data.generation = Some(settings("minecraft:the_nether", 128));

        let error = data
            .validate_generation_settings(settings("minecraft:overworld", 384))
            .expect_err("mismatched settings should be rejected");

        let message = error.to_string();
        assert!(message.contains("world generation settings do not match"));
        assert!(message.contains("minecraft:the_nether"));
        assert!(message.contains("minecraft:overworld"));
    }
}
