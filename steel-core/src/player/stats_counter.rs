use crate::player::Player;
use rustc_hash::{FxHashMap, FxHashSet};
use steel_protocol::packets::game::CAwardStats;
use steel_registry::RegistryExt;
use steel_registry::stat::custom::CustomStatRef;
use steel_registry::stat::{Stat, StatTypeRef, vanilla_stat_types};

/// Manages the counters for every statistic for a particular player.
/// Analogous to Vanilla's `ServerStatsCounter.java`.
pub struct StatsCounter {
    /// The map of values stored in the counters for each stat that is
    /// currently being tracked.
    pub(super) stats: FxHashMap<Stat, i32>,

    /// Stats that have been modified which haven't been updated to the client yet.
    pub(super) dirty: FxHashSet<Stat>,
}

impl StatsCounter {
    /// Creates a new, empty [`StatsCounter`].
    pub fn new() -> Self {
        Self {
            stats: FxHashMap::default(),
            dirty: FxHashSet::default(),
        }
    }

    /// Gets the value of the counter corresponding to the given stat.
    /// If this counter is not currently being tracked, `0` is returned instead.
    pub fn get(&self, stat: &Stat) -> i32 {
        self.stats.get(stat).copied().unwrap_or_default()
    }

    /// Sets the value of the counter corresponding to the given stat to a given value.
    pub fn set(&mut self, stat: Stat, count: i32) {
        self.stats.insert(stat, count);
        self.dirty.insert(stat);
    }

    /// Increments the value of the counter corresponding to the given stat by a given value.
    pub fn increment(&mut self, stat: Stat, count: i32) {
        let entry = self.stats.entry(stat).or_default();
        let sum = (i64::from(*entry) + i64::from(count)).min(i64::from(i32::MAX));
        *entry = sum as i32;
        self.dirty.insert(stat);
    }

    /// Marks all the stat counters of this player to be dirty. This means that the next time
    /// statistics are requested, all tracked stat counters will be sent to the client.
    pub fn mark_all_dirty(&mut self) {
        for stat in self.stats.keys() {
            self.dirty.insert(*stat);
        }
    }

    /// Gets all the counters of stats that are marked dirty and clears their dirty flag as
    /// well.
    pub(crate) fn get_dirty_and_clear(&mut self) -> Vec<(Stat, i32)> {
        let mut dirty = Vec::with_capacity(self.dirty.len());
        for stat in self.dirty.drain() {
            let count = self.stats.get(&stat).copied().unwrap_or_default();
            dirty.push((stat, count));
        }
        dirty
    }

    /// Returns the number of stats currently being tracked for this player.
    pub fn len(&self) -> usize {
        self.stats.len()
    }
}

impl Default for StatsCounter {
    fn default() -> Self {
        Self::new()
    }
}

impl Player {
    /// Awards one count of a particular stat to this player.
    pub fn award_stat<R: RegistryExt>(&self, stat_type: StatTypeRef<R>, value: &'static R::Entry)
    where
        R::Entry: Send + Sync,
    {
        self.award_erased_stat(stat_type.get(value));
    }

    /// Awards a given amount of a particular stat to this player.
    pub fn award_stat_with_count<R: RegistryExt>(
        &self,
        stat_type: StatTypeRef<R>,
        value: &'static R::Entry,
        count: i32,
    ) where
        R::Entry: Send + Sync,
    {
        self.award_erased_stat_with_count(stat_type.get(value), count);
    }

    /// Awards a given amount of a custom stat to this player.
    pub fn award_custom_stat(&self, stat: CustomStatRef) {
        self.award_stat(&vanilla_stat_types::CUSTOM, stat);
    }

    /// Awards a given amount of a custom stat to this player.
    pub fn award_custom_stat_with_count(&self, stat: CustomStatRef, count: i32) {
        self.award_stat_with_count(&vanilla_stat_types::CUSTOM, stat, count);
    }

    /// Awards one count of a particular stat to this player.
    pub(crate) fn award_erased_stat(&self, stat: Stat) {
        self.award_erased_stat_with_count(stat, 1);
    }

    /// Awards a given amount of a particular stat to this player.
    pub(crate) fn award_erased_stat_with_count(&self, stat: Stat, count: i32) {
        self.stats.lock().increment(stat, count);
        // TODO: Add score to the objectives having the criterion of this stat for the player.
    }

    /// Resets the counter of a stat from this player to zero.
    pub fn reset_stat(&self, stat: Stat) {
        self.stats.lock().set(stat, 0);
        // TODO: Reset score of the objectives having the criterion of this stat for the player.
    }

    /// Marks all the stat counters of this player to be dirty. This means that the next time
    /// statistics are requested, all tracked stat counters will be sent to the client.
    pub fn mark_all_stats_dirty(&self) {
        self.stats.lock().mark_all_dirty();
    }

    /// Sends all the dirty stats of this player to their client, and removes
    /// the dirty flag from all of them.
    pub fn send_stats(&self) {
        let stats = self.stats.lock().get_dirty_and_clear();
        self.send_packet(CAwardStats { stats });
    }
}
