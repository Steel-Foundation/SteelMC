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
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use steel_registry::REGISTRY;
use steel_registry::game_rules::GameRuleValues;
use steel_utils::types::Difficulty;
use steel_utils::{BlockPos, GlobalPos, Identifier};
use tokio::fs;

use crate::world::clock::WorldClockManager;

/// Persistent world border data stored with Steel level data.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct WorldBorderData {
    /// Border center X coordinate.
    pub center_x: f64,
    /// Border center Z coordinate.
    pub center_z: f64,
    /// Damage dealt per block outside the safe zone.
    pub damage_per_block: f64,
    /// Distance outside the border before damage starts.
    pub safe_zone: f64,
    /// Client warning distance in blocks.
    pub warning_blocks: i32,
    /// Client warning time in seconds.
    pub warning_time: i32,
    /// Current border size.
    pub size: f64,
    /// Remaining lerp time in ticks.
    pub lerp_time: i64,
    /// Target size for a moving border.
    pub lerp_target: f64,
}

impl Default for WorldBorderData {
    fn default() -> Self {
        Self {
            center_x: 0.0,
            center_z: 0.0,
            damage_per_block: 0.2,
            safe_zone: 5.0,
            warning_blocks: 5,
            warning_time: 300,
            size: f64::from(5.999_997E7_f32),
            lerp_time: 0,
            lerp_target: 0.0,
        }
    }
}

/// Persistent level data that gets saved to disk.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LevelData {
    /// World seed for terrain generation.
    pub seed: i64,
    /// Total game time in ticks.
    pub game_time: i64,
    /// Independently advancing world-clock instances for this loaded world.
    #[serde(default)]
    pub(crate) world_clocks: WorldClockManager,
    /// World spawn point.
    pub spawn: SpawnPoint,
    /// Vanilla global respawn data for this domain, stored on the domain default world.
    #[serde(default)]
    pub respawn: Option<RespawnData>,
    /// Weather state.
    pub weather: WeatherState,
    /// Persistent world border state.
    #[serde(default)]
    pub world_border: WorldBorderData,
    /// World difficulty.
    #[serde(default)]
    pub difficulty: Difficulty,
    /// Whether the difficulty is locked.
    #[serde(default)]
    pub difficulty_locked: bool,
    /// Game rules (stored as name -> value pairs for serialization).
    pub game_rules: FxHashMap<String, serde_json::Value>,
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

#[derive(Deserialize)]
struct SavedLevelSeed {
    seed: i64,
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

/// Vanilla default respawn data.
#[derive(Debug, Clone, PartialEq)]
pub struct RespawnData {
    /// Dimension and block position of the default respawn.
    pub global_pos: GlobalPos,
    /// Spawn yaw, wrapped with vanilla `Mth.wrapDegrees`.
    pub yaw: f32,
    /// Spawn pitch, clamped to vanilla's player pitch range.
    pub pitch: f32,
}

impl RespawnData {
    /// Creates respawn data for the given global position.
    #[must_use]
    pub fn new(global_pos: GlobalPos, yaw: f32, pitch: f32) -> Self {
        Self {
            global_pos,
            yaw: wrap_degrees(yaw),
            pitch: pitch.clamp(-90.0, 90.0),
        }
    }

    /// Creates respawn data for a dimension and block position.
    #[must_use]
    pub fn of(dimension: Identifier, pos: BlockPos, yaw: f32, pitch: f32) -> Self {
        Self::new(GlobalPos::new(dimension, pos), yaw, pitch)
    }

    /// Returns the respawn dimension.
    #[must_use]
    pub const fn dimension(&self) -> &Identifier {
        &self.global_pos.dimension
    }

    /// Returns the respawn block position.
    #[must_use]
    pub const fn pos(&self) -> BlockPos {
        self.global_pos.pos
    }
}

fn wrap_degrees(mut degrees: f32) -> f32 {
    degrees %= 360.0;
    if degrees >= 180.0 {
        degrees -= 360.0;
    }
    if degrees < -180.0 {
        degrees += 360.0;
    }
    degrees
}

#[derive(Serialize, Deserialize)]
struct SerializedRespawnData {
    dimension: Identifier,
    x: i32,
    y: i32,
    z: i32,
    yaw: f32,
    pitch: f32,
}

impl Serialize for RespawnData {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        SerializedRespawnData {
            dimension: self.global_pos.dimension.clone(),
            x: self.global_pos.pos.x(),
            y: self.global_pos.pos.y(),
            z: self.global_pos.pos.z(),
            yaw: self.yaw,
            pitch: self.pitch,
        }
        .serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for RespawnData {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let data = SerializedRespawnData::deserialize(deserializer)?;
        Ok(Self::of(
            data.dimension,
            BlockPos::new(data.x, data.y, data.z),
            data.yaw,
            data.pitch,
        ))
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
            world_clocks: WorldClockManager::new(),
            spawn: SpawnPoint::default(),
            respawn: None,
            weather: WeatherState::default(),
            world_border: WorldBorderData::default(),
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
                .set_serialized_by_name(name, value, &REGISTRY.game_rules);
        }
    }

    /// Saves game rules from the runtime values to the serialized map.
    pub fn save_game_rules(&mut self) {
        self.game_rules.clear();
        for (_, rule) in REGISTRY.game_rules.iter() {
            let name = if rule.key().namespace == Identifier::VANILLA_NAMESPACE {
                rule.key().path.to_string()
            } else {
                rule.key().to_string()
            };
            let value = rule.serialize_erased_value(
                self.game_rules_values
                    .get_erased(rule, &REGISTRY.game_rules),
            );
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

    /// Returns saved respawn data, or the legacy local spawn as a compatibility default.
    #[must_use]
    pub fn respawn_data_or_local(&self, dimension: &Identifier) -> RespawnData {
        self.respawn.clone().unwrap_or_else(|| {
            RespawnData::of(dimension.clone(), self.spawn_pos(), self.spawn.angle, 0.0)
        })
    }

    /// Sets the saved respawn data.
    pub fn set_respawn_data(&mut self, respawn_data: RespawnData) {
        self.respawn = Some(respawn_data);
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
                let initialized_clocks = loaded
                    .world_clocks
                    .initialize_registered_clocks()
                    .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
                let adopted_generation = loaded.validate_generation_settings(generation)?;
                (loaded, adopted_generation || initialized_clocks)
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

    /// Loads the saved world seed from `level.toml`, or returns the provided default.
    pub async fn load_seed_or_default(
        world_dir: Option<impl AsRef<Path>>,
        default_seed: i64,
    ) -> io::Result<i64> {
        let Some(dir) = world_dir else {
            return Ok(default_seed);
        };

        let path = dir.as_ref().join("level.toml");
        if !path.exists() {
            return Ok(default_seed);
        }

        let content = fs::read_to_string(path).await?;
        let saved: SavedLevelSeed = toml::from_str(&content).map_err(|e| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!("Invalid level.toml: {e}"),
            )
        })?;
        Ok(saved.seed)
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

    /// Returns this world's clock manager.
    #[must_use]
    pub(crate) const fn world_clocks(&self) -> &WorldClockManager {
        &self.data.world_clocks
    }

    /// Returns this world's mutable clock manager and marks level data dirty.
    pub(crate) const fn world_clocks_mut(&mut self) -> &mut WorldClockManager {
        self.dirty = true;
        &mut self.data.world_clocks
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
    use std::{
        env, fs as std_fs,
        path::PathBuf,
        process,
        time::{SystemTime, UNIX_EPOCH},
    };
    use steel_registry::{
        test_support::init_test_registry,
        vanilla_game_rules::{KEEP_INVENTORY, RANDOM_TICK_SPEED},
        vanilla_world_clocks,
    };
    use toml::map::Map;

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

    fn temp_level_data_dir(test_name: &str) -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock should be after unix epoch")
            .as_nanos();
        let path = env::temp_dir().join(format!(
            "steel-level-data-{test_name}-{}-{unique}",
            process::id()
        ));
        std_fs::create_dir_all(&path).expect("temp level data dir should be created");
        path
    }

    #[tokio::test]
    async fn load_seed_prefers_saved_level_toml() {
        let dir = temp_level_data_dir("saved-seed");
        std_fs::write(dir.join("level.toml"), "seed = 42\n").expect("level.toml should be written");

        let seed = LevelDataManager::load_seed_or_default(Some(dir.as_path()), 7)
            .await
            .expect("saved level seed should load");
        let _ = std_fs::remove_dir_all(&dir);

        assert_eq!(seed, 42);
    }

    #[tokio::test]
    async fn load_seed_returns_default_when_level_toml_is_missing() {
        let dir = temp_level_data_dir("missing-seed");

        let seed = LevelDataManager::load_seed_or_default(Some(dir.as_path()), 7)
            .await
            .expect("missing level.toml should use default seed");
        let _ = std_fs::remove_dir_all(&dir);

        assert_eq!(seed, 7);
    }

    #[test]
    fn adopts_missing_generation_settings() {
        init_test_registry();
        let mut data = LevelData::new_with_seed(1);

        let adopted = data
            .validate_generation_settings(settings("minecraft:overworld", 384))
            .expect("missing settings should be adopted");

        assert!(adopted);
        assert!(data.generation.is_some());
    }

    #[test]
    fn rejects_mismatched_generation_settings() {
        init_test_registry();
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

    #[test]
    fn respawn_data_wraps_yaw_and_clamps_pitch() {
        let respawn_data = RespawnData::of(
            Identifier::vanilla_static("overworld"),
            BlockPos::new(1, 2, 3),
            181.0,
            120.0,
        );

        assert_eq!(respawn_data.yaw.to_bits(), (-179.0_f32).to_bits());
        assert_eq!(respawn_data.pitch.to_bits(), 90.0_f32.to_bits());
    }

    #[test]
    fn respawn_data_round_trips_through_toml() {
        let respawn_data = RespawnData::of(
            Identifier::vanilla_static("the_nether"),
            BlockPos::new(-4, 70, 8),
            -181.0,
            -120.0,
        );

        let serialized = toml::to_string(&respawn_data).expect("respawn data should serialize");
        let deserialized: RespawnData =
            toml::from_str(&serialized).expect("respawn data should deserialize");

        assert_eq!(
            deserialized.global_pos.dimension,
            Identifier::vanilla_static("the_nether")
        );
        assert_eq!(deserialized.pos(), BlockPos::new(-4, 70, 8));
        assert_eq!(deserialized.yaw.to_bits(), 179.0_f32.to_bits());
        assert_eq!(deserialized.pitch.to_bits(), (-90.0_f32).to_bits());
    }

    #[test]
    fn level_data_uses_legacy_spawn_as_respawn_default() {
        init_test_registry();
        let mut data = LevelData::new_with_seed(1);
        data.set_spawn_pos(BlockPos::new(10, 65, -3));
        data.spawn.angle = 270.0;

        let respawn_data = data.respawn_data_or_local(&Identifier::vanilla_static("overworld"));

        assert_eq!(
            respawn_data.global_pos.dimension,
            Identifier::vanilla_static("overworld")
        );
        assert_eq!(respawn_data.pos(), BlockPos::new(10, 65, -3));
        assert_eq!(respawn_data.yaw.to_bits(), (-90.0_f32).to_bits());
        assert_eq!(respawn_data.pitch.to_bits(), 0.0_f32.to_bits());
    }

    #[test]
    #[expect(
        clippy::float_cmp,
        reason = "the exactly representable configured clock rate must round-trip unchanged"
    )]
    fn level_data_round_trips_world_clock_state() {
        init_test_registry();
        let mut data = LevelData::new_with_seed(1);
        assert_eq!(
            data.world_clocks
                .set_total_ticks(&vanilla_world_clocks::OVERWORLD, 98_765),
            Some(())
        );
        assert_eq!(
            data.world_clocks
                .set_rate(&vanilla_world_clocks::OVERWORLD, 3.5),
            Some(())
        );

        let serialized = toml::to_string(&data).expect("level data should serialize");
        let mut restored: LevelData =
            toml::from_str(&serialized).expect("level data should deserialize");
        assert_eq!(
            restored.world_clocks.initialize_registered_clocks(),
            Ok(false)
        );
        assert_eq!(
            restored
                .world_clocks
                .total_ticks(&vanilla_world_clocks::OVERWORLD),
            Some(98_765)
        );
        let Some(update) = restored
            .world_clocks
            .network_update(&vanilla_world_clocks::OVERWORLD, true)
        else {
            panic!("overworld clock update should exist");
        };
        assert_eq!(update.3, 3.5);
    }

    #[test]
    fn level_data_round_trips_typed_game_rules_with_untagged_toml_values() {
        init_test_registry();
        let mut data = LevelData::new_with_seed(1);
        assert!(
            data.game_rules_values
                .set(&KEEP_INVENTORY, true, &REGISTRY.game_rules,)
        );
        assert!(
            data.game_rules_values
                .set(&RANDOM_TICK_SPEED, 9, &REGISTRY.game_rules,)
        );
        data.save_game_rules();

        let serialized = toml::to_string(&data).expect("level data should serialize");
        assert!(serialized.contains("keep_inventory = true"));
        assert!(serialized.contains("random_tick_speed = 9"));

        let mut restored: LevelData =
            toml::from_str(&serialized).expect("level data should deserialize");
        restored.load_game_rules();

        assert!(
            restored
                .game_rules_values
                .get(&KEEP_INVENTORY, &REGISTRY.game_rules)
        );
        assert_eq!(
            restored
                .game_rules_values
                .get(&RANDOM_TICK_SPEED, &REGISTRY.game_rules),
            9
        );
    }
}
