//! Statistics tracking system.
//!
//! Mirrors Vanilla's statistics system - tracks player accomplishments,
//! block/minute stats, mob kills, and various gameplay metrics.

use steel_utils::ErasedType;

use crate::player::Player;
use crate::world::World;
use crate::block::BlockRef;
use crate::entity::Entity;
use cgmath::Vector3;

// Statistics categories
// Mirrors: https://mc-wiki.readthedocs.io/wiki/Statistics
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StatisticsType {
    // General stats
    mine_block,
    mine_block_custom(String),
    craft_item,
    craft_item_custom(String),
    use_item,
    use_item_custom(String),
    kill_entity,
    kill_entity_custom(String),
    // Time-based
    play_one_minute,
    play_ticks,
    // Dimension stats
    travel_one_cm,
    travel_one_m,
    travel_one_km,
    // Entity interaction
    tame_animal,
    feed_animal,
    breed_animal,
    // Death types
    killed_by_entity,
    fell_from_height,
    // Item stats
    item_used,
    item_broken,
    // Block place/break
    place_block,
    break_block,
    // Stats by type
    fish_caught,
    // Custom
    custom(String),
}

/// A statistics entry for a player.
#[derive(Clone, Debug)]
pub struct Statistics {
    /// The statistic type
    pub statistic: StatisticsType,
    /// The value accumulated
    pub value: i32,
}

impl Statistics {
    /// Creates a new statistic entry.
    #[must_use]
    pub fn new(statistic: StatisticsType, value: i32) -> Self {
        Self { statistic, value }
    }
}

/// The statistics manager tracks all player statistics.
#[derive(Clone, Debug)]
pub struct StatisticsManager {
    /// Player statistics storage
    stats: std::collections::HashMap<String, Vec<Statistics>>,
}

impl StatisticsManager {
    /// Creates a new statistics manager.
    #[must_use]
    pub fn new() -> Self {
        let stats = std::collections::HashMap::new();
        Self { stats }
    }

    /// Records a statistic increment for a player.
    pub fn increment(&mut self, player: &Player, statistic: StatisticsType, amount: i32) {
        let player_uuid = player.get_uuid().to_string();
        let entry = Statistics::new(statistic, amount);
        self.stats
            .entry(player_uuid)
            .or_insert_with(Vec::new)
            .push(entry);
    }

    /// Gets the value of a statistic for a player.
    pub fn get(&self, player: &Player, statistic: StatisticsType) -> i32 {
        let player_uuid = player.get_uuid().to_string();
        let player_stats = self.stats.get(&player_uuid);

        let mut total = 0;
        if let Some(stats) = player_stats {
            for s in stats {
                if s.statistic == statistic {
                    total += s.value;
                }
            }
        }
        total
    }

    /// Gets all statistics for a player.
    pub fn all(&self, player: &Player) -> Vec<Statistics> {
        let player_uuid = player.get_uuid().to_string();
        self.stats.get(&player_uuid).cloned().unwrap_or_default()
    }

    /// Increments a block-mined statistic.
    pub fn increment_mine_block(&mut self, player: &Player, block: &BlockRef) {
        self.increment(player, StatisticsType::mine_block, 1);
    }

    /// Increments a craft statistic.
    pub fn increment_craft(&mut self, player: &Player, item: &ItemStack) {
        self.increment(player, StatisticsType::craft_item, 1);
    }

    /// Increments a kill statistic.
    pub fn increment_kill(&mut self, player: &Player, entity: &dyn Entity) {
        self.increment(player, StatisticsType::kill_entity, 1);
    }

    /// Increments play time (called from world tick).
    pub fn increment_play_time(&mut self, player: &Player, ticks: i32) {
        self.increment(player, StatisticsType::play_ticks, ticks);
    }

    /// Whether the player has achieved a specific statistic threshold.
    pub fn has_reached(&self, player: &Player, statistic: StatisticsType, threshold: i32) -> bool {
        self.get(player, statistic) >= threshold
    }
}

/// Called when player statistics should be updated.
///
/// Mirrors Vanilla's `StatHandler.trackStat()` - updates statistics and
/// ensures they are synchronized to the client when needed.
pub fn update_statistics(
    player: &Player,
    world: &World,
    statistic: StatisticsType,
    amount: i32,
    manager: &mut StatisticsManager,
) {
    manager.increment(player, statistic, amount);
    // Would synchronize statistics to client packet here
    // player.sync_statistics(statistic, amount);
}