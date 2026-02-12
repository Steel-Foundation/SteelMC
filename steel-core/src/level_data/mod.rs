//! Level data persistence module.
//!
//! This module handles saving and loading world-level data like game rules,
//! time, weather, spawn point, and seed. This data is stored in `level.json`
//! in each world's directory.

use std::{
    io,
    path::{Path, PathBuf},
};

use rustc_hash::FxHashMap;
use serde::{Deserialize, Serialize};
use steel_registry::REGISTRY;
use steel_registry::game_rules::{GameRuleValue, GameRuleValues};
use steel_utils::BlockPos;
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
    /// Game rules (stored as name -> value pairs for serialization).
    pub game_rules: FxHashMap<String, GameRuleValue>,
    /// Runtime game rule values (not serialized, loaded from `game_rules`).
    #[serde(skip)]
    pub game_rules_values: GameRuleValues,
    /// Whether the world has been initialized.
    pub initialized: bool,
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
        Self {
            seed,
            game_time: 0,
            day_time: 0,
            spawn: SpawnPoint::default(),
            weather: WeatherState::default(),
            game_rules: FxHashMap::default(),
            game_rules_values: GameRuleValues::new(&REGISTRY.game_rules),
            initialized: false,
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
    /// Path to the level.json file.
    path: Option<PathBuf>,
    /// Cached level data.
    data: LevelData,
    /// Whether data has been modified since last save.
    dirty: bool,
}

impl LevelDataManager {
    /// Creates a new level data manager for the given world directory.
    ///
    /// If `level.json` exists, it will be loaded (the provided seed is ignored).
    /// Otherwise, new data will be created with the provided seed.
    pub async fn new(world_dir: Option<impl AsRef<Path>>, seed: i64) -> io::Result<Self> {
        let (data, path) = match &world_dir {
            Some(dir) => {
                let path = dir.as_ref().join("level.json");

                let data = if path.exists() {
                    // Load existing level data (seed from file takes precedence)
                    let content = fs::read_to_string(&path).await?;
                    let mut loaded: LevelData = serde_json::from_str(&content).map_err(|e| {
                        io::Error::new(
                            io::ErrorKind::InvalidData,
                            format!("Invalid level.json: {e}"),
                        )
                    })?;
                    // Initialize runtime game rules from serialized values
                    loaded.load_game_rules();
                    loaded
                } else {
                    // Create new level data with the provided seed
                    LevelData::new_with_seed(seed)
                };
                (data, Some(path))
            }
            None => (LevelData::new_with_seed(seed), None),
        };

        Ok(Self {
            path,
            data,
            dirty: false,
        })
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

        let content = serde_json::to_string_pretty(&self.data)
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
}
