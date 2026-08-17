//! This module provides the [`StatsCounter`], which keeps track of stats with their counters, and
//! implements some stat-related functions for the player.

use crate::player::Player;
use rustc_hash::FxHashMap;
use steel_protocol::packets::game::CAwardStats;
use steel_registry::RegistryExt;
use steel_registry::stat::custom::CustomStatRef;
use steel_registry::stat::{Stat, StatTypeRef, vanilla_stat_types};

/// Manages the counters for every statistic for a particular player.
/// Analogous to Vanilla's `ServerStatsCounter.java`.
pub struct StatsCounter {
    /// The map of each stat currently being tracked to its value and dirty flag.
    // Vanilla uses a map and set separately for the counters and dirty flag respectively,
    // but it is faster to just use one map to store both the count and dirty flag in the same map.
    pub(super) stats: FxHashMap<Stat, (i32, bool)>,
}

impl StatsCounter {
    /// Creates a new, empty [`StatsCounter`].
    #[must_use]
    pub fn new() -> Self {
        Self {
            stats: FxHashMap::default(),
        }
    }

    /// Gets the value of the counter corresponding to the given stat.
    /// If this counter is not currently being tracked, `0` is returned instead.
    #[must_use]
    pub fn get(&self, stat: &Stat) -> i32 {
        self.stats.get(stat).map_or_default(|(count, _)| *count)
    }

    /// Sets the value of the counter corresponding to the given stat to a given value.
    pub fn set(&mut self, stat: Stat, count: i32) {
        self.stats.insert(stat, (count, true));
    }

    /// Increments the value of the counter corresponding to the given stat by a given value.
    pub fn increment(&mut self, stat: Stat, count: i32) {
        let entry = self.stats.entry(stat).or_default();
        let sum = (i64::from(entry.0) + i64::from(count)).min(i64::from(i32::MAX));
        *entry = (sum as i32, true);
    }

    /// Marks all the stat counters of this player to be dirty. This means that the next time
    /// statistics are requested, all tracked stat counters will be sent to the client.
    pub fn mark_all_dirty(&mut self) {
        for (_, dirty_flag) in self.stats.values_mut() {
            *dirty_flag = true;
        }
    }

    /// Gets all the counters of stats that are marked dirty and clears their dirty flag as
    /// well.
    pub(crate) fn get_dirty_and_clear(&mut self) -> Vec<(Stat, i32)> {
        let mut dirty_stats = Vec::new();
        for (&stat, (count, dirty)) in &mut self.stats {
            if *dirty {
                dirty_stats.push((stat, *count));
                *dirty = false;
            }
        }
        dirty_stats
    }

    /// Clears the counters of all stats in this counter to zero,
    /// and makes all stats dirty.
    pub fn clear(&mut self) {
        for tuple in self.stats.values_mut() {
            *tuple = (0, true);
        }
    }

    /// Returns the number of stats currently being tracked for this player.
    #[must_use]
    pub fn len(&self) -> usize {
        self.stats.len()
    }

    /// Returns whether there are no stats are currently being tracked or not.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.stats.is_empty()
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

    /// Returns the player's currently tracked stats and their counters.
    #[must_use]
    pub fn stats(&self) -> Vec<(Stat, i32)> {
        self.stats
            .lock()
            .stats
            .iter()
            .map(|(&stat, &(count, _))| (stat, count))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use crate::player::stats_counter::StatsCounter;
    use steel_registry::stat::{Stat, vanilla_stat_types};
    use steel_registry::{init_vanilla_registry, vanilla_custom_stats};

    fn deterministic_dirty_and_clear(counter: &mut StatsCounter) -> Vec<(Stat, i32)> {
        let mut dirty = counter.get_dirty_and_clear();
        dirty.sort_by_key(|(stat, _)| stat.stat_value_key().clone());

        dirty
    }

    #[test]
    fn stat_counter_query_dirty_and_modifications() {
        init_vanilla_registry();

        let mut stats_counter = StatsCounter::new();

        let jump_stat = vanilla_stat_types::CUSTOM.get(&vanilla_custom_stats::JUMP);
        let deaths_stat = vanilla_stat_types::CUSTOM.get(&vanilla_custom_stats::DEATHS);

        stats_counter.increment(jump_stat, 9);
        stats_counter.increment(jump_stat, 4);

        assert_eq!(stats_counter.get(&jump_stat), 13);
        assert_eq!(stats_counter.get(&deaths_stat), 0);

        stats_counter.increment(deaths_stat, 1);
        assert_eq!(
            deterministic_dirty_and_clear(&mut stats_counter),
            vec![(deaths_stat, 1), (jump_stat, 13)]
        );

        stats_counter.increment(deaths_stat, 1);
        assert_eq!(
            deterministic_dirty_and_clear(&mut stats_counter),
            vec![(deaths_stat, 2)]
        );

        stats_counter.mark_all_dirty();
        assert_eq!(
            deterministic_dirty_and_clear(&mut stats_counter),
            vec![(deaths_stat, 2), (jump_stat, 13)]
        );

        assert_eq!(deterministic_dirty_and_clear(&mut stats_counter), vec![]);

        stats_counter.set(deaths_stat, 7);
        assert_eq!(
            deterministic_dirty_and_clear(&mut stats_counter),
            vec![(deaths_stat, 7)]
        );

        assert_eq!(stats_counter.get(&jump_stat), 13);
    }

    #[test]
    fn overflow_cap() {
        init_vanilla_registry();

        let mut stats_counter = StatsCounter::new();
        let jump_stat = vanilla_stat_types::CUSTOM.get(&vanilla_custom_stats::JUMP);

        stats_counter.set(jump_stat, i32::MAX - 1);

        stats_counter.increment(jump_stat, 1);
        assert_eq!(stats_counter.get(&jump_stat), i32::MAX);

        stats_counter.increment(jump_stat, 1);
        assert_eq!(stats_counter.get(&jump_stat), i32::MAX);

        stats_counter.increment(jump_stat, 1000);
        assert_eq!(stats_counter.get(&jump_stat), i32::MAX);

        stats_counter.increment(jump_stat, i32::MAX);
        assert_eq!(stats_counter.get(&jump_stat), i32::MAX);

        stats_counter.increment(jump_stat, i32::MIN + 1);
        assert_eq!(stats_counter.get(&jump_stat), 0);
    }

    #[test]
    fn no_underflow_cap() {
        init_vanilla_registry();

        let mut stats_counter = StatsCounter::new();
        let jump_stat = vanilla_stat_types::CUSTOM.get(&vanilla_custom_stats::JUMP);

        stats_counter.set(jump_stat, i32::MIN + 1);

        stats_counter.increment(jump_stat, -1);
        assert_eq!(stats_counter.get(&jump_stat), i32::MIN);

        stats_counter.increment(jump_stat, -1);
        assert_eq!(stats_counter.get(&jump_stat), i32::MAX);

        stats_counter.increment(jump_stat, i32::MAX);
        assert_eq!(stats_counter.get(&jump_stat), i32::MAX);
    }
}
