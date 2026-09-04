//! Per-player Warden spawning state for Sculk Shriekers.
//!
//! Vanilla `ServerPlayer.WardenSpawnTracker` - tracks warning level and cooldown
//! for each player to determine when to spawn a Warden.

use simdnbt::borrow::NbtCompound as NbtCompoundView;
use simdnbt::owned::NbtCompound;

const WARN_DURATION_TICKS: i64 = 12000; // 10 minutes

/// Per-player state for Warden spawning
#[derive(Clone, Debug)]
pub struct WardenSpawnTracker {
    /// Current warning level (0-4)
    warning_level: i32,
    /// Game time when cooldown expires (can trigger again after this)
    cooldown_ends_at: i64,
    /// Game time when the last warning was issued
    last_warning_time: i64,
}

impl WardenSpawnTracker {
    /// Creates a new tracker with no warnings
    pub fn new() -> Self {
        Self {
            warning_level: 0,
            cooldown_ends_at: 0,
            last_warning_time: 0,
        }
    }

    /// Returns the current warning level (0-4)
    pub fn warning_level(&self) -> i32 {
        self.warning_level
    }

    /// Ticks the tracker, decaying warnings over time
    pub fn tick(&mut self, game_time: i64) {
        // Decay warning level if enough time has passed
        if self.warning_level > 0 && game_time >= self.last_warning_time + WARN_DURATION_TICKS {
            self.warning_level = 0;
        }
    }

    /// Increments the warning level, returns whether to spawn a Warden
    pub fn try_warn(&mut self, game_time: i64, can_summon: bool) -> WardenSpawnResult {
        // Check cooldown
        if game_time < self.cooldown_ends_at {
            return WardenSpawnResult::OnCooldown;
        }

        // Increment warning
        self.warning_level = (self.warning_level + 1).min(4);
        self.last_warning_time = game_time;

        log::debug!(
            "Warden spawn tracker: warning level {} at time {}",
            self.warning_level,
            game_time
        );

        // Spawn Warden on 4th warning if shrieker can summon
        if can_summon && self.warning_level >= 4 {
            // Reset and set cooldown
            self.warning_level = 0;
            self.cooldown_ends_at = game_time + WARN_DURATION_TICKS;
            WardenSpawnResult::SpawnWarden
        } else {
            WardenSpawnResult::Warning {
                level: self.warning_level,
            }
        }
    }

    /// Saves tracker state to NBT
    pub fn save(&self) -> NbtCompound {
        let mut nbt = NbtCompound::new();
        nbt.insert("warning_level", self.warning_level);
        nbt.insert("cooldown_ends_at", self.cooldown_ends_at);
        nbt.insert("last_warning_time", self.last_warning_time);
        nbt
    }

    /// Loads tracker state from NBT
    pub fn load(nbt: &NbtCompoundView<'_, '_>) -> Self {
        Self {
            warning_level: nbt.int("warning_level").unwrap_or(0).max(0).min(4),
            cooldown_ends_at: nbt.long("cooldown_ends_at").unwrap_or(0),
            last_warning_time: nbt.long("last_warning_time").unwrap_or(0),
        }
    }
}

impl Default for WardenSpawnTracker {
    fn default() -> Self {
        Self::new()
    }
}

/// Result of trying to increment warning level
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WardenSpawnResult {
    /// Player is on cooldown, no warning issued
    OnCooldown,
    /// Warning issued, current level returned
    Warning {
        /// Current warning level (1-3)
        level: i32
    },
    /// Fourth warning reached, spawn a Warden
    SpawnWarden,
}
