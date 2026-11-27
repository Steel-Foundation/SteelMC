//! Incremental eight-neighbor ticket propagation shared by loading and simulation.
//!
//! Mirrors Vanilla's `ChunkTracker` / `DynamicGraphMinFixedPoint`. Each domain
//! retains its own sources and levels; the const limit selects its outer boundary.
use std::mem;

use rustc_hash::FxHashMap;
use steel_utils::ChunkPos;

use super::{chunk_ticket_manager::ChunkTicketLevel, chunk_ticket_storage::SourceLevelUpdate};

/// Vanilla-style level buckets for pending graph corrections.
#[derive(Debug)]
struct LeveledPropagationQueue {
    buckets: Vec<Vec<ChunkPos>>,
    first_queued_level: u8,
}

impl LeveledPropagationQueue {
    fn new(level_count: u8) -> Self {
        Self {
            buckets: (0..usize::from(level_count)).map(|_| Vec::new()).collect(),
            first_queued_level: level_count,
        }
    }

    fn enqueue(&mut self, pos: ChunkPos, level: u8) {
        self.buckets[usize::from(level)].push(pos);
        self.first_queued_level = self.first_queued_level.min(level);
    }

    fn pop(&mut self) -> Option<(ChunkPos, u8)> {
        while usize::from(self.first_queued_level) < self.buckets.len() {
            let bucket = &mut self.buckets[usize::from(self.first_queued_level)];
            if let Some(pos) = bucket.pop() {
                return Some((pos, self.first_queued_level));
            }
            self.first_queued_level += 1;
        }
        None
    }

    const fn is_empty(&self) -> bool {
        self.first_queued_level as usize == self.buckets.len()
    }
}

/// A propagated loading or simulation level change.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChunkLevelChange {
    /// Chunk whose propagated level changed.
    pub pos: ChunkPos,
    /// `Some(level)` if the level changed or was added, `None` if it was removed.
    pub new_level: Option<ChunkTicketLevel>,
}

/// Tracks sources and their minimum propagated levels through `MAX_LEVEL`.
#[derive(Debug)]
pub struct ChunkTracker<const MAX_LEVEL: u8> {
    source_levels: FxHashMap<ChunkPos, u8>,
    pending_source_levels: FxHashMap<ChunkPos, u8>,
    levels: FxHashMap<ChunkPos, ChunkTicketLevel>,
    pending_levels: FxHashMap<ChunkPos, u8>,
    propagation_queue: LeveledPropagationQueue,
    changes: Vec<ChunkLevelChange>,
}

impl<const MAX_LEVEL: u8> Default for ChunkTracker<MAX_LEVEL> {
    fn default() -> Self {
        Self::new()
    }
}

impl<const MAX_LEVEL: u8> ChunkTracker<MAX_LEVEL> {
    const ABSENT_LEVEL: u8 = MAX_LEVEL + 1;
    /// Creates an empty tracker for this domain.
    #[must_use]
    pub fn new() -> Self {
        assert!(
            MAX_LEVEL <= ChunkTicketLevel::MAX.raw(),
            "tracker limit exceeds supported ticket levels"
        );
        Self {
            source_levels: FxHashMap::default(),
            pending_source_levels: FxHashMap::default(),
            levels: FxHashMap::default(),
            pending_levels: FxHashMap::default(),
            propagation_queue: LeveledPropagationQueue::new(Self::ABSENT_LEVEL + 1),
            changes: Vec::new(),
        }
    }

    /// Applies the latest effective ticket source level at one position.
    pub(crate) fn apply_source_update(&mut self, update: SourceLevelUpdate) {
        assert!(
            update.level.is_none_or(|level| level.raw() <= MAX_LEVEL),
            "source level exceeds tracker limit"
        );
        let new_level = update
            .level
            .map_or(Self::ABSENT_LEVEL, ChunkTicketLevel::raw);

        let old_level = self.source_level(update.pos);
        if old_level == new_level {
            return;
        }

        let original_level = *self
            .pending_source_levels
            .entry(update.pos)
            .or_insert(old_level);
        if new_level == Self::ABSENT_LEVEL {
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
    pub fn run_all_updates(&mut self) -> &[ChunkLevelChange] {
        self.changes.clear();

        if self.pending_source_levels.is_empty() {
            return &self.changes;
        }

        debug_assert!(self.pending_levels.is_empty());
        debug_assert!(self.propagation_queue.is_empty());

        let source_changes = mem::take(&mut self.pending_source_levels);
        let mut original_levels = FxHashMap::default();
        for (pos, old_level) in source_changes {
            let new_level = self.source_level(pos);
            self.check_edge(None, pos, new_level, new_level < old_level);
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
                self.changes.push(ChunkLevelChange { pos, new_level });
            }
        }
        self.changes
            .sort_unstable_by_key(|change| (change.pos.0.x, change.pos.0.y));

        &self.changes
    }

    /// Takes the change buffer produced by the last propagation pass.
    pub(crate) fn take_changes(&mut self) -> Vec<ChunkLevelChange> {
        mem::take(&mut self.changes)
    }

    /// Reuses a drained change buffer on the next propagation pass.
    pub(crate) fn recycle_changes(&mut self, mut changes: Vec<ChunkLevelChange>) {
        debug_assert_eq!(self.changes, []);
        changes.clear();
        self.changes = changes;
    }

    /// Returns the last propagated ticket level at `pos`.
    #[must_use]
    pub fn get_level(&self, pos: ChunkPos) -> Option<ChunkTicketLevel> {
        self.levels.get(&pos).copied()
    }

    #[cfg(test)]
    pub(super) fn is_dirty(&self) -> bool {
        !self.pending_source_levels.is_empty()
    }

    fn source_level(&self, pos: ChunkPos) -> u8 {
        self.source_levels
            .get(&pos)
            .copied()
            .unwrap_or(Self::ABSENT_LEVEL)
    }

    fn level(&self, pos: ChunkPos) -> u8 {
        self.levels
            .get(&pos)
            .copied()
            .map_or(Self::ABSENT_LEVEL, ChunkTicketLevel::raw)
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
            self.set_level(pos, Self::ABSENT_LEVEL, original_levels);
            if computed_level != Self::ABSENT_LEVEL {
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

        if level == Self::ABSENT_LEVEL {
            self.levels.remove(&pos);
            return;
        }

        let Some(level) = ChunkTicketLevel::new(level) else {
            panic!("propagated ticket level exceeds ChunkTicketLevel::MAX");
        };
        self.levels.insert(pos, level);
    }

    fn check_neighbors_after_update(&mut self, pos: ChunkPos, level: u8, only_decrease: bool) {
        if only_decrease && level >= MAX_LEVEL {
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
        let propagated_level = from_level.saturating_add(1).min(Self::ABSENT_LEVEL);
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
            self.check_edge(Some(from), to, Self::ABSENT_LEVEL, false);
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
        computed_level.min(Self::ABSENT_LEVEL)
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
mod tests;
