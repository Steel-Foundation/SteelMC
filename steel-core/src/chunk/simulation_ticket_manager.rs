//! Simulation ticket propagation without chunk loading or generation concerns.
use std::mem;

use rustc_hash::FxHashMap;
use steel_utils::ChunkPos;

use super::{chunk_ticket_manager::ChunkTicketLevel, chunk_ticket_storage::SourceLevelUpdate};

const ABSENT_LEVEL: u8 = ChunkTicketLevel::BLOCK_TICKING_CHUNK.raw() + 1;
const LEVEL_COUNT: usize = ABSENT_LEVEL as usize + 1;

/// Vanilla-style level buckets for pending graph corrections.
#[derive(Debug)]
struct LeveledPropagationQueue {
    buckets: Vec<Vec<ChunkPos>>,
    first_queued_level: usize,
}

impl LeveledPropagationQueue {
    fn new() -> Self {
        Self {
            buckets: (0..LEVEL_COUNT).map(|_| Vec::new()).collect(),
            first_queued_level: LEVEL_COUNT,
        }
    }

    fn enqueue(&mut self, pos: ChunkPos, level: u8) {
        let level = usize::from(level);
        self.buckets[level].push(pos);
        self.first_queued_level = self.first_queued_level.min(level);
    }

    fn pop(&mut self) -> Option<(ChunkPos, u8)> {
        while self.first_queued_level < LEVEL_COUNT {
            let bucket = &mut self.buckets[self.first_queued_level];
            if let Some(pos) = bucket.pop() {
                return Some((pos, self.first_queued_level as u8));
            }
            self.first_queued_level += 1;
        }
        None
    }

    const fn is_empty(&self) -> bool {
        self.first_queued_level == LEVEL_COUNT
    }
}

/// A propagated simulation level change.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SimulationLevelChange {
    pub pos: ChunkPos,
    /// `Some(level)` if the level changed or was added, `None` if it was removed.
    pub new_level: Option<ChunkTicketLevel>,
}

#[derive(Debug, Clone, Copy)]
struct SourceChange {
    pos: ChunkPos,
    old_level: u8,
    new_level: u8,
}

/// Tracks simulation sources and their propagated levels.
#[derive(Debug)]
pub struct SimulationTicketManager {
    source_levels: FxHashMap<ChunkPos, u8>,
    pending_source_levels: FxHashMap<ChunkPos, u8>,
    levels: FxHashMap<ChunkPos, ChunkTicketLevel>,
    pending_levels: FxHashMap<ChunkPos, u8>,
    propagation_queue: LeveledPropagationQueue,
    changes: Vec<SimulationLevelChange>,
}

impl Default for SimulationTicketManager {
    fn default() -> Self {
        Self::new()
    }
}

impl SimulationTicketManager {
    /// Creates an empty simulation ticket manager.
    #[must_use]
    pub fn new() -> Self {
        Self {
            source_levels: FxHashMap::default(),
            pending_source_levels: FxHashMap::default(),
            levels: FxHashMap::default(),
            pending_levels: FxHashMap::default(),
            propagation_queue: LeveledPropagationQueue::new(),
            changes: Vec::new(),
        }
    }

    /// Applies the latest effective simulation source level at one position.
    pub(crate) fn apply_source_update(&mut self, update: SourceLevelUpdate) {
        debug_assert!(update.level.is_none_or(ChunkTicketLevel::is_block_ticking));
        let new_level = update.level.map_or(ABSENT_LEVEL, ChunkTicketLevel::raw);

        let old_level = self.source_level(update.pos);
        if old_level == new_level {
            return;
        }

        let original_level = *self
            .pending_source_levels
            .entry(update.pos)
            .or_insert(old_level);
        if new_level == ABSENT_LEVEL {
            self.source_levels.remove(&update.pos);
        } else {
            self.source_levels.insert(update.pos, new_level);
        }

        if original_level == new_level {
            self.pending_source_levels.remove(&update.pos);
        }
    }

    /// Applies effective source updates in iterator order.
    pub(crate) fn apply_source_updates(
        &mut self,
        updates: impl IntoIterator<Item = SourceLevelUpdate>,
    ) {
        for update in updates {
            self.apply_source_update(update);
        }
    }

    /// Applies pending source changes with minimum 8-neighbor propagation.
    /// The returned changes include additions, updates, and removals.
    pub fn run_all_updates(&mut self) -> &[SimulationLevelChange] {
        self.changes.clear();

        if self.pending_source_levels.is_empty() {
            return &self.changes;
        }

        debug_assert!(self.pending_levels.is_empty());
        debug_assert!(self.propagation_queue.is_empty());

        let (source_changes, all_sources) = self.take_source_changes();
        if let Some((removed_source, added_source)) =
            Self::isolated_equal_level_move(&source_changes, &all_sources)
        {
            self.apply_isolated_move(removed_source, added_source);
            self.changes
                .sort_unstable_by_key(|change| (change.pos.0.x, change.pos.0.y));
            return &self.changes;
        }

        let mut original_levels = FxHashMap::default();
        for change in source_changes {
            let old_is_isolated = change.old_level != ABSENT_LEVEL
                && Self::source_is_isolated(change.pos, change.old_level, &all_sources);
            let new_is_isolated = change.new_level != ABSENT_LEVEL
                && Self::source_is_isolated(change.pos, change.new_level, &all_sources);

            if old_is_isolated && change.new_level == ABSENT_LEVEL {
                self.remove_isolated_source(change.pos, change.old_level);
                continue;
            }
            if change.old_level == ABSENT_LEVEL && new_is_isolated {
                self.add_isolated_source(change.pos, change.new_level);
                continue;
            }

            if old_is_isolated {
                self.apply_isolated_source(
                    change.pos,
                    change.old_level,
                    false,
                    &mut original_levels,
                );
                if change.new_level == ABSENT_LEVEL {
                    continue;
                }
                if new_is_isolated {
                    self.apply_isolated_source(
                        change.pos,
                        change.new_level,
                        true,
                        &mut original_levels,
                    );
                } else {
                    self.check_edge(None, change.pos, change.new_level, true);
                }
                continue;
            }

            self.check_edge(
                None,
                change.pos,
                change.new_level,
                change.new_level < change.old_level,
            );
        }

        while let Some((pos, queued_priority)) = self.propagation_queue.pop() {
            let Some(computed_level) = self.pending_levels.get(&pos).copied() else {
                continue;
            };
            if self.level(pos).min(computed_level) != queued_priority {
                continue;
            }
            self.pending_levels.remove(&pos);
            self.apply_pending_level(pos, computed_level, &mut original_levels);
        }

        for (pos, old_level) in original_levels {
            let new_level = self.levels.get(&pos).copied();
            if old_level != new_level {
                self.changes.push(SimulationLevelChange { pos, new_level });
            }
        }
        self.changes
            .sort_unstable_by_key(|change| (change.pos.0.x, change.pos.0.y));

        &self.changes
    }

    /// Takes the change buffer produced by the last propagation pass.
    pub(crate) fn take_changes(&mut self) -> Vec<SimulationLevelChange> {
        mem::take(&mut self.changes)
    }

    /// Reuses a drained change buffer on the next propagation pass.
    pub(crate) fn recycle_changes(&mut self, mut changes: Vec<SimulationLevelChange>) {
        debug_assert_eq!(self.changes, []);
        changes.clear();
        self.changes = changes;
    }

    /// Returns the last propagated simulation level at `pos`.
    #[must_use]
    pub fn get_level(&self, pos: ChunkPos) -> Option<ChunkTicketLevel> {
        self.levels.get(&pos).copied()
    }

    fn take_source_changes(&mut self) -> (Vec<SourceChange>, Vec<(ChunkPos, u8)>) {
        let old_source_levels = mem::take(&mut self.pending_source_levels);
        let mut all_sources: Vec<_> = self
            .source_levels
            .iter()
            .map(|(&pos, &level)| (pos, level))
            .collect();
        all_sources.extend(
            old_source_levels
                .iter()
                .filter(|(_, level)| **level != ABSENT_LEVEL)
                .map(|(&pos, &level)| (pos, level)),
        );
        let source_changes = old_source_levels
            .into_iter()
            .map(|(pos, old_level)| SourceChange {
                pos,
                old_level,
                new_level: self.source_level(pos),
            })
            .collect();
        (source_changes, all_sources)
    }

    fn source_is_isolated(
        source_pos: ChunkPos,
        source_level: u8,
        sources: &[(ChunkPos, u8)],
    ) -> bool {
        let source_radius = ChunkTicketLevel::BLOCK_TICKING_CHUNK.raw() - source_level;
        sources.iter().all(|&(other_pos, other_level)| {
            if other_pos == source_pos {
                return true;
            }
            !Self::source_footprints_overlap(source_pos, source_radius, other_pos, other_level)
        })
    }

    fn isolated_equal_level_move(
        source_changes: &[SourceChange],
        sources: &[(ChunkPos, u8)],
    ) -> Option<(SourceChange, SourceChange)> {
        let &[first_change, second_change] = source_changes else {
            return None;
        };

        let mut removed_source = None;
        let mut added_source = None;
        for change in [first_change, second_change] {
            if change.old_level != ABSENT_LEVEL && change.new_level == ABSENT_LEVEL {
                removed_source = Some(change);
            } else if change.old_level == ABSENT_LEVEL && change.new_level != ABSENT_LEVEL {
                added_source = Some(change);
            } else {
                return None;
            }
        }

        let (Some(removed_source), Some(added_source)) = (removed_source, added_source) else {
            return None;
        };
        if removed_source.old_level != added_source.new_level {
            return None;
        }

        let radius = ChunkTicketLevel::BLOCK_TICKING_CHUNK.raw() - removed_source.old_level;
        let is_isolated = sources.iter().all(|&(other_pos, other_level)| {
            if other_pos == removed_source.pos || other_pos == added_source.pos {
                return true;
            }

            !Self::source_footprints_overlap(removed_source.pos, radius, other_pos, other_level)
                && !Self::source_footprints_overlap(
                    added_source.pos,
                    radius,
                    other_pos,
                    other_level,
                )
        });
        is_isolated.then_some((removed_source, added_source))
    }

    fn source_footprints_overlap(
        source_pos: ChunkPos,
        source_radius: u8,
        other_pos: ChunkPos,
        other_level: u8,
    ) -> bool {
        let dx = (i64::from(source_pos.0.x) - i64::from(other_pos.0.x)).abs();
        let dz = (i64::from(source_pos.0.y) - i64::from(other_pos.0.y)).abs();
        let other_radius = ChunkTicketLevel::BLOCK_TICKING_CHUNK.raw() - other_level;
        let minimum_separation = i64::from(source_radius) + i64::from(other_radius);
        dx.max(dz) <= minimum_separation
    }

    fn apply_isolated_move(&mut self, removed_source: SourceChange, added_source: SourceChange) {
        let source_level = removed_source.old_level;
        let radius = i32::from(ChunkTicketLevel::BLOCK_TICKING_CHUNK.raw() - source_level);

        for dz in -radius..=radius {
            for dx in -radius..=radius {
                let pos = ChunkPos::new(removed_source.pos.0.x + dx, removed_source.pos.0.y + dz);
                let old_raw_level = source_level + dx.abs().max(dz.abs()) as u8;
                let new_raw_level = Self::level_from_source(added_source.pos, pos, source_level);
                if new_raw_level == Some(old_raw_level) {
                    continue;
                }

                let new_level = Self::ticket_level(new_raw_level);
                if let Some(level) = new_level {
                    self.levels.insert(pos, level);
                } else {
                    self.levels.remove(&pos);
                }
                self.changes.push(SimulationLevelChange { pos, new_level });
            }
        }

        for dz in -radius..=radius {
            for dx in -radius..=radius {
                let pos = ChunkPos::new(added_source.pos.0.x + dx, added_source.pos.0.y + dz);
                if Self::level_from_source(removed_source.pos, pos, source_level).is_some() {
                    continue;
                }

                let raw_level = source_level + dx.abs().max(dz.abs()) as u8;
                let new_level = Self::ticket_level(Some(raw_level));
                let Some(level) = new_level else {
                    panic!("isolated move produced an invalid simulation level");
                };
                self.levels.insert(pos, level);
                self.changes.push(SimulationLevelChange {
                    pos,
                    new_level: Some(level),
                });
            }
        }
    }

    fn level_from_source(source_pos: ChunkPos, pos: ChunkPos, source_level: u8) -> Option<u8> {
        let dx = (i64::from(source_pos.0.x) - i64::from(pos.0.x)).abs();
        let dz = (i64::from(source_pos.0.y) - i64::from(pos.0.y)).abs();
        let distance = dx.max(dz);
        let radius = i64::from(ChunkTicketLevel::BLOCK_TICKING_CHUNK.raw() - source_level);
        if distance > radius {
            return None;
        }

        Some(source_level + distance as u8)
    }

    fn ticket_level(raw_level: Option<u8>) -> Option<ChunkTicketLevel> {
        let raw_level = raw_level?;
        let Some(level) = ChunkTicketLevel::new(raw_level) else {
            panic!("source produced an invalid simulation level");
        };
        Some(level)
    }

    fn apply_isolated_source(
        &mut self,
        source_pos: ChunkPos,
        source_level: u8,
        add: bool,
        original_levels: &mut FxHashMap<ChunkPos, Option<ChunkTicketLevel>>,
    ) {
        let radius = i32::from(ChunkTicketLevel::BLOCK_TICKING_CHUNK.raw() - source_level);
        for dz in -radius..=radius {
            for dx in -radius..=radius {
                let pos = ChunkPos::new(source_pos.0.x + dx, source_pos.0.y + dz);
                let level = if add {
                    source_level + dx.abs().max(dz.abs()) as u8
                } else {
                    ABSENT_LEVEL
                };
                self.set_level(pos, level, original_levels);
            }
        }
    }

    fn remove_isolated_source(&mut self, source_pos: ChunkPos, source_level: u8) {
        let radius = i32::from(ChunkTicketLevel::BLOCK_TICKING_CHUNK.raw() - source_level);
        for dz in -radius..=radius {
            for dx in -radius..=radius {
                let pos = ChunkPos::new(source_pos.0.x + dx, source_pos.0.y + dz);
                let old_level = self.levels.remove(&pos);
                debug_assert!(old_level.is_some());
                self.changes.push(SimulationLevelChange {
                    pos,
                    new_level: None,
                });
            }
        }
    }

    fn add_isolated_source(&mut self, source_pos: ChunkPos, source_level: u8) {
        let radius = i32::from(ChunkTicketLevel::BLOCK_TICKING_CHUNK.raw() - source_level);
        for dz in -radius..=radius {
            for dx in -radius..=radius {
                let pos = ChunkPos::new(source_pos.0.x + dx, source_pos.0.y + dz);
                let raw_level = source_level + dx.abs().max(dz.abs()) as u8;
                let Some(level) = ChunkTicketLevel::new(raw_level) else {
                    panic!("isolated source produced an invalid simulation level");
                };
                let old_level = self.levels.insert(pos, level);
                debug_assert!(old_level.is_none());
                self.changes.push(SimulationLevelChange {
                    pos,
                    new_level: Some(level),
                });
            }
        }
    }

    fn source_level(&self, pos: ChunkPos) -> u8 {
        self.source_levels
            .get(&pos)
            .copied()
            .unwrap_or(ABSENT_LEVEL)
    }

    fn level(&self, pos: ChunkPos) -> u8 {
        self.levels
            .get(&pos)
            .copied()
            .map_or(ABSENT_LEVEL, ChunkTicketLevel::raw)
    }

    fn apply_pending_level(
        &mut self,
        pos: ChunkPos,
        computed_level: u8,
        original_levels: &mut FxHashMap<ChunkPos, Option<ChunkTicketLevel>>,
    ) {
        let current_level = self.level(pos);
        if computed_level < current_level {
            self.set_level(pos, computed_level, original_levels);
            self.check_neighbors_after_update(pos, computed_level, true);
        } else if computed_level > current_level {
            self.set_level(pos, ABSENT_LEVEL, original_levels);
            if computed_level != ABSENT_LEVEL {
                self.schedule_level(pos, computed_level);
            }
            self.check_neighbors_after_update(pos, current_level, false);
        }
    }

    fn set_level(
        &mut self,
        pos: ChunkPos,
        level: u8,
        original_levels: &mut FxHashMap<ChunkPos, Option<ChunkTicketLevel>>,
    ) {
        original_levels
            .entry(pos)
            .or_insert_with(|| self.levels.get(&pos).copied());

        if level == ABSENT_LEVEL {
            self.levels.remove(&pos);
            return;
        }

        let Some(level) = ChunkTicketLevel::new(level) else {
            panic!("propagated simulation level exceeds ChunkTicketLevel::MAX");
        };
        self.levels.insert(pos, level);
    }

    fn check_neighbors_after_update(&mut self, pos: ChunkPos, level: u8, only_decrease: bool) {
        if only_decrease && level >= ChunkTicketLevel::BLOCK_TICKING_CHUNK.raw() {
            return;
        }

        for neighbor in pos.neighbors() {
            self.check_neighbor(pos, neighbor, level, only_decrease);
        }
    }

    fn check_neighbor(
        &mut self,
        from: ChunkPos,
        to: ChunkPos,
        from_level: u8,
        only_decrease: bool,
    ) {
        let propagated_level = from_level.saturating_add(1).min(ABSENT_LEVEL);
        if only_decrease {
            self.check_edge(Some(from), to, propagated_level, true);
            return;
        }

        let old_computed_level = self
            .pending_levels
            .get(&to)
            .copied()
            .unwrap_or_else(|| self.level(to));
        if propagated_level == old_computed_level {
            self.check_edge(Some(from), to, ABSENT_LEVEL, false);
        }
    }

    fn check_edge(
        &mut self,
        known_parent: Option<ChunkPos>,
        pos: ChunkPos,
        level_from_parent: u8,
        only_decrease: bool,
    ) {
        let current_level = self.level(pos);
        let old_computed_level = self
            .pending_levels
            .get(&pos)
            .copied()
            .unwrap_or(current_level);
        let new_computed_level = if only_decrease {
            old_computed_level.min(level_from_parent)
        } else {
            self.compute_level(pos, known_parent, level_from_parent)
        };

        self.replace_pending_level(pos, current_level, new_computed_level);
    }

    fn compute_level(
        &self,
        pos: ChunkPos,
        known_parent: Option<ChunkPos>,
        level_from_parent: u8,
    ) -> u8 {
        let mut computed_level = level_from_parent.min(self.source_level(pos));
        for neighbor in pos.neighbors() {
            if Some(neighbor) == known_parent {
                continue;
            }

            computed_level = computed_level.min(self.level(neighbor).saturating_add(1));
            if computed_level == 0 {
                break;
            }
        }
        computed_level.min(ABSENT_LEVEL)
    }

    fn replace_pending_level(&mut self, pos: ChunkPos, current_level: u8, computed_level: u8) {
        if current_level == computed_level {
            self.pending_levels.remove(&pos);
        } else {
            self.pending_levels.insert(pos, computed_level);
            self.propagation_queue
                .enqueue(pos, current_level.min(computed_level));
        }
    }

    fn schedule_level(&mut self, pos: ChunkPos, computed_level: u8) {
        let current_level = self.level(pos);
        debug_assert!(!self.pending_levels.contains_key(&pos));
        self.pending_levels.insert(pos, computed_level);
        self.propagation_queue
            .enqueue(pos, current_level.min(computed_level));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const REFERENCE_BLOCK_TICKING_LEVEL: u8 = 129;

    struct DeterministicSequence(u64);

    impl DeterministicSequence {
        fn next(&mut self, bound: u32) -> u32 {
            self.0 = self
                .0
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            (self.0 >> 32) as u32 % bound
        }
    }

    fn source(pos: ChunkPos, level: Option<u8>) -> SourceLevelUpdate {
        SourceLevelUpdate {
            pos,
            level: level
                .map(|raw| ChunkTicketLevel::new(raw).expect("test source level must be valid")),
        }
    }

    fn has_change(
        changes: &[SimulationLevelChange],
        pos: ChunkPos,
        new_level: Option<ChunkTicketLevel>,
    ) -> bool {
        changes.contains(&SimulationLevelChange { pos, new_level })
    }

    fn add_reference_source(
        levels: &mut FxHashMap<ChunkPos, ChunkTicketLevel>,
        source_pos: ChunkPos,
        source_level: u8,
    ) {
        if source_level > REFERENCE_BLOCK_TICKING_LEVEL {
            return;
        }

        let radius = i32::from(REFERENCE_BLOCK_TICKING_LEVEL - source_level);
        for dz in -radius..=radius {
            for dx in -radius..=radius {
                let distance = dx.abs().max(dz.abs()) as u8;
                let raw_level = source_level + distance;
                let Some(level) = ChunkTicketLevel::new(raw_level) else {
                    panic!("reference produced an invalid simulation level");
                };
                let pos = ChunkPos::new(source_pos.0.x + dx, source_pos.0.y + dz);
                levels
                    .entry(pos)
                    .and_modify(|stored| *stored = (*stored).min(level))
                    .or_insert(level);
            }
        }
    }

    fn reference_levels(
        manager: &SimulationTicketManager,
    ) -> FxHashMap<ChunkPos, ChunkTicketLevel> {
        let mut levels = FxHashMap::default();
        for (&pos, &source_level) in &manager.source_levels {
            add_reference_source(&mut levels, pos, source_level);
        }
        levels
    }

    fn reference_changes(
        old_levels: &FxHashMap<ChunkPos, ChunkTicketLevel>,
        new_levels: &FxHashMap<ChunkPos, ChunkTicketLevel>,
    ) -> Vec<SimulationLevelChange> {
        let mut changes = Vec::new();
        for (&pos, &new_level) in new_levels {
            if old_levels.get(&pos) != Some(&new_level) {
                changes.push(SimulationLevelChange {
                    pos,
                    new_level: Some(new_level),
                });
            }
        }
        for &pos in old_levels.keys() {
            if !new_levels.contains_key(&pos) {
                changes.push(SimulationLevelChange {
                    pos,
                    new_level: None,
                });
            }
        }
        changes.sort_unstable_by_key(|change| (change.pos.0.x, change.pos.0.y));
        changes
    }

    fn run_and_compare_with_reference(
        manager: &mut SimulationTicketManager,
        previous_levels: &mut FxHashMap<ChunkPos, ChunkTicketLevel>,
    ) {
        let actual_changes = manager.run_all_updates().to_vec();
        let expected_levels = reference_levels(manager);
        let expected_changes = reference_changes(previous_levels, &expected_levels);

        assert_eq!(manager.levels, expected_levels);
        assert_eq!(actual_changes, expected_changes);
        *previous_levels = expected_levels;
    }

    #[test]
    fn overlapping_sources_keep_the_strongest_propagated_level() {
        let mut manager = SimulationTicketManager::new();
        manager.apply_source_updates([
            source(ChunkPos::new(0, 0), Some(126)),
            source(ChunkPos::new(4, 0), Some(128)),
        ]);
        manager.run_all_updates();

        assert_eq!(
            manager
                .get_level(ChunkPos::new(2, 0))
                .map(ChunkTicketLevel::raw),
            Some(128)
        );
        assert_eq!(
            manager
                .get_level(ChunkPos::new(3, 0))
                .map(ChunkTicketLevel::raw),
            Some(129)
        );
        assert_eq!(
            manager
                .get_level(ChunkPos::new(4, 0))
                .map(ChunkTicketLevel::raw),
            Some(128)
        );
    }

    #[test]
    fn repeated_updates_coalesce_to_the_original_level() {
        let mut manager = SimulationTicketManager::new();
        let pos = ChunkPos::new(0, 0);
        manager.apply_source_update(source(pos, Some(126)));
        manager.apply_source_update(source(pos, Some(128)));
        manager.apply_source_update(source(pos, None));

        assert_eq!(manager.run_all_updates(), []);
        assert_eq!(manager.run_all_updates(), []);
        assert_eq!(manager.get_level(pos), None);

        manager.apply_source_updates([source(pos, None), source(pos, Some(126))]);
        assert_ne!(manager.run_all_updates(), []);
        manager.apply_source_update(source(pos, Some(128)));
        manager.apply_source_update(source(pos, Some(126)));

        assert_eq!(manager.run_all_updates(), []);
        assert_eq!(manager.get_level(pos).map(ChunkTicketLevel::raw), Some(126));
    }

    #[test]
    fn weakening_a_source_removes_its_old_outer_levels() {
        let mut manager = SimulationTicketManager::new();
        let pos = ChunkPos::new(0, 0);
        manager.apply_source_update(source(pos, Some(126)));
        manager.run_all_updates();

        manager.apply_source_update(source(pos, Some(128)));
        let changes = manager.run_all_updates();

        assert!(has_change(changes, pos, ChunkTicketLevel::new(128)));
        assert!(has_change(changes, ChunkPos::new(2, 0), None));
        assert_eq!(manager.get_level(pos).map(ChunkTicketLevel::raw), Some(128));
    }

    #[test]
    fn source_batch_order_does_not_change_the_result() {
        let positions = [
            ChunkPos::new(0, 0),
            ChunkPos::new(6, 1),
            ChunkPos::new(-3, 2),
            ChunkPos::new(4, -2),
        ];
        let levels = [Some(124), Some(127), Some(126), Some(128)];
        let mut forwards = SimulationTicketManager::new();
        let mut backwards = SimulationTicketManager::new();

        forwards.apply_source_updates(
            positions
                .into_iter()
                .zip(levels)
                .map(|(pos, level)| source(pos, level)),
        );
        backwards.apply_source_updates(
            positions
                .into_iter()
                .zip(levels)
                .rev()
                .map(|(pos, level)| source(pos, level)),
        );

        assert_eq!(forwards.run_all_updates(), backwards.run_all_updates());
        assert_eq!(forwards.levels, backwards.levels);
    }

    #[test]
    fn incremental_source_updates_match_reference() {
        let mut manager = SimulationTicketManager::new();
        let mut previous_levels = FxHashMap::default();
        manager.apply_source_updates([
            source(ChunkPos::new(0, 0), Some(124)),
            source(ChunkPos::new(6, 1), Some(127)),
            source(ChunkPos::new(-3, 2), Some(126)),
            source(ChunkPos::new(4, -2), Some(128)),
        ]);
        run_and_compare_with_reference(&mut manager, &mut previous_levels);

        for level in [Some(128), Some(127), Some(125), Some(129)] {
            manager.apply_source_update(source(ChunkPos::new(-3, 2), level));
            run_and_compare_with_reference(&mut manager, &mut previous_levels);
        }

        manager.apply_source_updates([
            source(ChunkPos::new(0, 0), None),
            source(ChunkPos::new(-3, 2), None),
            source(ChunkPos::new(9, 3), Some(125)),
            source(ChunkPos::new(4, -2), None),
            source(ChunkPos::new(6, 1), None),
        ]);
        run_and_compare_with_reference(&mut manager, &mut previous_levels);
    }

    #[test]
    fn deterministic_random_operations_match_reference() {
        let mut manager = SimulationTicketManager::new();
        let mut previous_levels = FxHashMap::default();
        let mut sequence = DeterministicSequence(0x5eed_cafe_d00d_f00d);
        let source_levels = [None, Some(124), Some(126), Some(128), Some(129)];

        for _ in 0..200 {
            for _ in 0..=sequence.next(3) {
                let pos = ChunkPos::new(sequence.next(13) as i32 - 6, sequence.next(13) as i32 - 6);
                let level = source_levels[sequence.next(source_levels.len() as u32) as usize];
                manager.apply_source_update(source(pos, level));
            }

            run_and_compare_with_reference(&mut manager, &mut previous_levels);
        }
    }

    #[test]
    fn batched_overlapping_moves_and_source_changes_match_reference() {
        let mut manager = SimulationTicketManager::new();
        let mut previous_levels = FxHashMap::default();
        manager.apply_source_updates([
            source(ChunkPos::new(0, 0), Some(124)),
            source(ChunkPos::new(3, 0), Some(125)),
            source(ChunkPos::new(2, 2), Some(126)),
        ]);
        run_and_compare_with_reference(&mut manager, &mut previous_levels);

        manager.apply_source_updates([
            source(ChunkPos::new(0, 0), None),
            source(ChunkPos::new(3, 0), None),
            source(ChunkPos::new(5, 0), Some(125)),
            source(ChunkPos::new(2, 2), None),
            source(ChunkPos::new(-30, 0), Some(126)),
        ]);
        run_and_compare_with_reference(&mut manager, &mut previous_levels);

        manager.apply_source_updates([
            source(ChunkPos::new(5, 0), None),
            source(ChunkPos::new(6, 0), Some(125)),
            source(ChunkPos::new(7, 0), Some(124)),
        ]);
        run_and_compare_with_reference(&mut manager, &mut previous_levels);
    }

    #[test]
    fn isolated_one_chunk_move_matches_reference() {
        let mut manager = SimulationTicketManager::new();
        let mut previous_levels = FxHashMap::default();
        let old_pos = ChunkPos::new(0, 0);
        let new_pos = ChunkPos::new(-1, 0);
        manager.apply_source_update(source(old_pos, Some(96)));
        run_and_compare_with_reference(&mut manager, &mut previous_levels);

        manager.apply_source_updates([source(old_pos, None), source(new_pos, Some(96))]);
        run_and_compare_with_reference(&mut manager, &mut previous_levels);
    }

    #[test]
    fn distant_isolated_move_matches_reference() {
        const SIMULATION_DISTANCE_CHUNKS: u8 = 6;

        let mut manager = SimulationTicketManager::new();
        let mut previous_levels = FxHashMap::default();
        let old_pos = ChunkPos::new(0, 0);
        let source_level =
            ChunkTicketLevel::for_entity_ticking_radius(SIMULATION_DISTANCE_CHUNKS).raw();
        let first_overflowing_distance_chunks = i32::from(u8::MAX - source_level) + 1;
        let new_pos = ChunkPos::new(first_overflowing_distance_chunks, 0);
        manager.apply_source_update(source(old_pos, Some(source_level)));
        run_and_compare_with_reference(&mut manager, &mut previous_levels);

        manager.apply_source_updates([source(old_pos, None), source(new_pos, Some(source_level))]);
        run_and_compare_with_reference(&mut manager, &mut previous_levels);
    }

    #[test]
    fn one_chunk_move_overlapping_an_unchanged_source_matches_reference() {
        let mut manager = SimulationTicketManager::new();
        let mut previous_levels = FxHashMap::default();
        let old_pos = ChunkPos::new(0, 0);
        let new_pos = ChunkPos::new(-1, 0);
        manager.apply_source_updates([
            source(old_pos, Some(124)),
            source(ChunkPos::new(3, 0), Some(124)),
        ]);
        run_and_compare_with_reference(&mut manager, &mut previous_levels);

        manager.apply_source_updates([source(old_pos, None), source(new_pos, Some(124))]);
        run_and_compare_with_reference(&mut manager, &mut previous_levels);
    }
}
