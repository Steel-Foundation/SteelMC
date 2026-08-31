//! Simulation ticket propagation without chunk loading or generation concerns.
use std::mem;

use rustc_hash::{FxHashMap, FxHashSet};
use smallvec::SmallVec;
use steel_utils::ChunkPos;
use uuid::Uuid;

use super::chunk_ticket_manager::{ChunkTicket, ChunkTicketLevel};

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

/// One ordered simulation ticket mutation.
#[cfg(test)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SimulationTicketOperation {
    Add { pos: ChunkPos, ticket: ChunkTicket },
    Remove { pos: ChunkPos, ticket: ChunkTicket },
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
    tickets: FxHashMap<ChunkPos, SmallVec<[ChunkTicket; 2]>>,
    players: FxHashMap<ChunkPos, FxHashSet<Uuid>>,
    player_positions: FxHashMap<Uuid, ChunkPos>,
    source_levels: FxHashMap<ChunkPos, u8>,
    dirty_sources: FxHashSet<ChunkPos>,
    levels: FxHashMap<ChunkPos, ChunkTicketLevel>,
    pending_levels: FxHashMap<ChunkPos, u8>,
    propagation_queue: LeveledPropagationQueue,
    last_simulation_distance: Option<u8>,
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
            tickets: FxHashMap::default(),
            players: FxHashMap::default(),
            player_positions: FxHashMap::default(),
            source_levels: FxHashMap::default(),
            dirty_sources: FxHashSet::default(),
            levels: FxHashMap::default(),
            pending_levels: FxHashMap::default(),
            propagation_queue: LeveledPropagationQueue::new(),
            last_simulation_distance: None,
            changes: Vec::new(),
        }
    }

    /// Applies ticket operations in iterator order.
    #[cfg(test)]
    pub fn apply_operations(
        &mut self,
        operations: impl IntoIterator<Item = SimulationTicketOperation>,
    ) {
        for operation in operations {
            self.apply_operation(operation);
        }
    }

    /// Applies one ticket operation. Returns whether stored simulation tickets changed.
    #[cfg(test)]
    pub fn apply_operation(&mut self, operation: SimulationTicketOperation) -> bool {
        match operation {
            SimulationTicketOperation::Add { pos, ticket } => self.add_ticket(pos, ticket),
            SimulationTicketOperation::Remove { pos, ticket } => self.remove_ticket(pos, ticket),
        }
    }

    /// Adds one simulation-bearing ticket, including a duplicate equal ticket.
    pub fn add_ticket(&mut self, pos: ChunkPos, ticket: ChunkTicket) -> bool {
        if ticket.simulation_level().is_none() {
            return false;
        }

        self.tickets.entry(pos).or_default().push(ticket);
        self.dirty_sources.insert(pos);
        true
    }

    /// Removes one equal simulation-bearing ticket.
    pub fn remove_ticket(&mut self, pos: ChunkPos, ticket: ChunkTicket) -> bool {
        if ticket.simulation_level().is_none() {
            return false;
        }

        let Some(tickets) = self.tickets.get_mut(&pos) else {
            return false;
        };
        let Some(index) = tickets.iter().position(|stored| *stored == ticket) else {
            return false;
        };

        tickets.swap_remove(index);
        if tickets.is_empty() {
            self.tickets.remove(&pos);
        }
        self.dirty_sources.insert(pos);
        true
    }

    /// Adds a player to a simulation source. Duplicate UUIDs are idempotent.
    ///
    /// # Panics
    ///
    /// Panics if the UUID-to-position index and source membership disagree.
    pub fn add_player(&mut self, pos: ChunkPos, player_id: Uuid) -> bool {
        if let Some(current_pos) = self.player_positions.get(&player_id).copied() {
            if current_pos == pos {
                return false;
            }
            self.remove_player_from_source(current_pos, player_id);
        }

        let players = self.players.entry(pos).or_default();
        let source_was_empty = players.is_empty();
        let inserted = players.insert(player_id);
        assert!(
            inserted,
            "player position index disagrees with source membership"
        );
        self.player_positions.insert(player_id, pos);

        if source_was_empty {
            self.dirty_sources.insert(pos);
        }
        true
    }

    /// Removes a player and returns whether that membership existed.
    pub fn remove_player(&mut self, pos: ChunkPos, player_id: Uuid) -> bool {
        if self.player_positions.get(&player_id).copied() != Some(pos) {
            return false;
        }

        self.remove_player_from_source(pos, player_id);
        self.player_positions.remove(&player_id);
        true
    }

    /// Moves an existing player membership to another simulation source.
    #[cfg(test)]
    pub fn move_player(&mut self, player_id: Uuid, old_pos: ChunkPos, new_pos: ChunkPos) -> bool {
        if old_pos == new_pos {
            return self.player_positions.get(&player_id).copied() == Some(old_pos);
        }
        if !self.remove_player(old_pos, player_id) {
            return false;
        }

        self.add_player(new_pos, player_id);
        true
    }

    fn remove_player_from_source(&mut self, pos: ChunkPos, player_id: Uuid) {
        let Some(players) = self.players.get_mut(&pos) else {
            panic!("player position index references a missing source");
        };
        assert!(
            players.remove(&player_id),
            "player position index disagrees with source membership"
        );

        if players.is_empty() {
            self.players.remove(&pos);
            self.dirty_sources.insert(pos);
        }
    }

    /// Applies pending source changes with minimum 8-neighbor propagation.
    ///
    /// Player sources use Vanilla's `ENTITY_TICKING - simulation_distance` level.
    /// The returned changes include additions, updates, and removals.
    pub fn run_all_updates(&mut self, simulation_distance: u8) -> &[SimulationLevelChange] {
        self.changes.clear();

        let distance_changed = self.last_simulation_distance != Some(simulation_distance);
        if distance_changed {
            self.dirty_sources.extend(self.players.keys().copied());
            self.last_simulation_distance = Some(simulation_distance);
        }
        if self.dirty_sources.is_empty() {
            return &self.changes;
        }

        debug_assert!(self.pending_levels.is_empty());
        debug_assert!(self.propagation_queue.is_empty());

        let (source_changes, all_sources) = self.take_source_changes(simulation_distance);
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

    fn compute_source_level(&self, pos: ChunkPos, simulation_distance: u8) -> u8 {
        let ticket_level = self.tickets.get(&pos).and_then(|tickets| {
            tickets
                .iter()
                .filter_map(|ticket| ticket.simulation_level())
                .filter(|level| level.is_block_ticking())
                .min()
        });
        let player_level = self
            .players
            .contains_key(&pos)
            .then(|| ChunkTicketLevel::for_entity_ticking_radius(simulation_distance));

        ticket_level
            .into_iter()
            .chain(player_level)
            .min()
            .map_or(ABSENT_LEVEL, ChunkTicketLevel::raw)
    }

    fn take_source_changes(
        &mut self,
        simulation_distance: u8,
    ) -> (Vec<SourceChange>, Vec<(ChunkPos, u8)>) {
        let dirty_sources = mem::take(&mut self.dirty_sources);
        let mut all_sources: Vec<_> = self
            .source_levels
            .iter()
            .map(|(&pos, &level)| (pos, level))
            .collect();
        let mut source_changes = Vec::with_capacity(dirty_sources.len());

        for pos in dirty_sources {
            let old_level = self.source_level(pos);
            let new_level = self.compute_source_level(pos, simulation_distance);
            if old_level == new_level {
                continue;
            }

            if new_level == ABSENT_LEVEL {
                self.source_levels.remove(&pos);
            } else {
                self.source_levels.insert(pos, new_level);
            }
            source_changes.push(SourceChange {
                pos,
                old_level,
                new_level,
            });
        }

        all_sources.extend(self.source_levels.iter().map(|(&pos, &level)| (pos, level)));
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
        if source_changes.len() != 2 {
            return None;
        }

        let mut removed_source = None;
        let mut added_source = None;
        for &change in source_changes {
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
        (distance <= radius).then_some(source_level + distance as u8)
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

    const SIMULATION_DISTANCE: u8 = 2;
    const REFERENCE_ENTITY_TICKING_LEVEL: u8 = 128;
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

    fn player_id(value: u128) -> Uuid {
        Uuid::from_u128(value)
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
        simulation_distance: u8,
    ) -> FxHashMap<ChunkPos, ChunkTicketLevel> {
        let mut levels = FxHashMap::default();
        for (&pos, tickets) in &manager.tickets {
            for source_level in tickets
                .iter()
                .filter_map(|ticket| ticket.simulation_level())
            {
                add_reference_source(&mut levels, pos, source_level.raw());
            }
        }

        let player_level = REFERENCE_ENTITY_TICKING_LEVEL.saturating_sub(simulation_distance);
        for &pos in manager.players.keys() {
            add_reference_source(&mut levels, pos, player_level);
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
        simulation_distance: u8,
        previous_levels: &mut FxHashMap<ChunkPos, ChunkTicketLevel>,
    ) {
        let actual_changes = manager.run_all_updates(simulation_distance).to_vec();
        let expected_levels = reference_levels(manager, simulation_distance);
        let expected_changes = reference_changes(previous_levels, &expected_levels);

        assert_eq!(manager.levels, expected_levels);
        assert_eq!(actual_changes, expected_changes);
        *previous_levels = expected_levels;
    }

    #[test]
    fn overlapping_sources_keep_the_strongest_propagated_level() {
        let mut manager = SimulationTicketManager::new();
        let strong = ChunkTicket::full_chunks_with_entity_ticking(2, 2);
        let weak = ChunkTicket::full_chunks_with_entity_ticking(0, 0);

        manager.apply_operations([
            SimulationTicketOperation::Add {
                pos: ChunkPos::new(0, 0),
                ticket: strong,
            },
            SimulationTicketOperation::Add {
                pos: ChunkPos::new(4, 0),
                ticket: weak,
            },
        ]);
        manager.run_all_updates(SIMULATION_DISTANCE);

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
    fn duplicate_equal_tickets_are_removed_one_at_a_time() {
        let mut manager = SimulationTicketManager::new();
        let pos = ChunkPos::new(0, 0);
        let ticket = ChunkTicket::full_chunks_with_entity_ticking(2, 2);

        assert!(manager.add_ticket(pos, ticket));
        assert!(manager.add_ticket(pos, ticket));
        manager.run_all_updates(SIMULATION_DISTANCE);

        assert!(manager.remove_ticket(pos, ticket));
        assert_eq!(manager.run_all_updates(SIMULATION_DISTANCE), []);
        assert_eq!(manager.get_level(pos).map(ChunkTicketLevel::raw), Some(126));

        assert!(manager.remove_ticket(pos, ticket));
        let changes = manager.run_all_updates(SIMULATION_DISTANCE);
        assert!(has_change(changes, pos, None));
        assert_eq!(manager.get_level(pos), None);
    }

    #[test]
    fn removing_the_strongest_ticket_reveals_the_next_source() {
        let mut manager = SimulationTicketManager::new();
        let pos = ChunkPos::new(0, 0);
        let strong = ChunkTicket::full_chunks_with_entity_ticking(2, 2);
        let next = ChunkTicket::full_chunks_with_entity_ticking(0, 0);
        manager.add_ticket(pos, strong);
        manager.add_ticket(pos, next);
        manager.run_all_updates(SIMULATION_DISTANCE);

        assert!(manager.remove_ticket(pos, strong));
        let changes = manager.run_all_updates(SIMULATION_DISTANCE);

        assert!(has_change(changes, pos, ChunkTicketLevel::new(128)));
        assert!(has_change(changes, ChunkPos::new(2, 0), None));
        assert_eq!(manager.get_level(pos).map(ChunkTicketLevel::raw), Some(128));
    }

    #[test]
    fn loading_only_tickets_are_ignored() {
        let mut manager = SimulationTicketManager::new();
        let pos = ChunkPos::new(0, 0);
        let ticket = ChunkTicket::loading(ChunkTicketLevel::STRONGEST);

        assert!(!manager.add_ticket(pos, ticket));
        assert!(!manager.remove_ticket(pos, ticket));
        assert_eq!(manager.run_all_updates(SIMULATION_DISTANCE), []);
        assert_eq!(manager.get_level(pos), None);
    }

    #[test]
    fn ordered_no_op_updates_do_not_report_changes() {
        let mut manager = SimulationTicketManager::new();
        let pos = ChunkPos::new(0, 0);
        let ticket = ChunkTicket::full_chunks_with_entity_ticking(1, 1);

        manager.apply_operations([
            SimulationTicketOperation::Add { pos, ticket },
            SimulationTicketOperation::Remove { pos, ticket },
        ]);

        assert_eq!(manager.run_all_updates(SIMULATION_DISTANCE), []);
        assert_eq!(manager.run_all_updates(SIMULATION_DISTANCE), []);

        manager.apply_operations([
            SimulationTicketOperation::Remove { pos, ticket },
            SimulationTicketOperation::Add { pos, ticket },
        ]);
        assert_ne!(manager.run_all_updates(SIMULATION_DISTANCE), []);
        assert_eq!(manager.get_level(pos).map(ChunkTicketLevel::raw), Some(127));
    }

    #[test]
    fn duplicate_player_uuid_is_idempotent() {
        let mut manager = SimulationTicketManager::new();
        let pos = ChunkPos::new(0, 0);
        let player = player_id(1);

        assert!(manager.add_player(pos, player));
        manager.run_all_updates(SIMULATION_DISTANCE);
        assert!(!manager.add_player(pos, player));
        assert_eq!(manager.run_all_updates(SIMULATION_DISTANCE), []);
    }

    #[test]
    fn stale_removal_does_not_remove_a_newer_player_position() {
        let mut manager = SimulationTicketManager::new();
        let old_pos = ChunkPos::new(0, 0);
        let new_pos = ChunkPos::new(10, 0);
        let player = player_id(1);
        manager.add_player(old_pos, player);
        manager.run_all_updates(SIMULATION_DISTANCE);

        assert!(manager.add_player(new_pos, player));
        assert!(!manager.remove_player(old_pos, player));
        manager.run_all_updates(SIMULATION_DISTANCE);

        assert_eq!(manager.get_level(old_pos), None);
        assert_eq!(
            manager.get_level(new_pos).map(ChunkTicketLevel::raw),
            Some(126)
        );
    }

    #[test]
    fn one_of_two_players_leaving_keeps_the_source() {
        let mut manager = SimulationTicketManager::new();
        let pos = ChunkPos::new(0, 0);
        manager.add_player(pos, player_id(1));
        manager.add_player(pos, player_id(2));
        manager.run_all_updates(SIMULATION_DISTANCE);

        assert!(manager.remove_player(pos, player_id(1)));
        assert!(!manager.remove_player(pos, player_id(3)));
        assert_eq!(manager.run_all_updates(SIMULATION_DISTANCE), []);
        assert_eq!(manager.get_level(pos).map(ChunkTicketLevel::raw), Some(126));
    }

    #[test]
    fn last_player_leaving_removes_the_source() {
        let mut manager = SimulationTicketManager::new();
        let pos = ChunkPos::new(0, 0);
        let player = player_id(1);
        manager.add_player(pos, player);
        manager.run_all_updates(SIMULATION_DISTANCE);

        assert!(manager.remove_player(pos, player));
        let changes = manager.run_all_updates(SIMULATION_DISTANCE);

        assert!(has_change(changes, pos, None));
        assert_eq!(manager.get_level(pos), None);
    }

    #[test]
    fn moving_a_player_moves_the_simulation_source() {
        let mut manager = SimulationTicketManager::new();
        let old_pos = ChunkPos::new(0, 0);
        let new_pos = ChunkPos::new(10, 0);
        let player = player_id(1);
        manager.add_player(old_pos, player);
        manager.run_all_updates(SIMULATION_DISTANCE);

        assert!(manager.move_player(player, old_pos, new_pos));
        let changes = manager.run_all_updates(SIMULATION_DISTANCE);

        assert!(has_change(changes, old_pos, None));
        assert!(has_change(changes, new_pos, ChunkTicketLevel::new(126)));
        assert_eq!(manager.get_level(old_pos), None);
        assert_eq!(
            manager.get_level(new_pos).map(ChunkTicketLevel::raw),
            Some(126)
        );
    }

    #[test]
    fn players_and_tickets_share_the_minimum_level() {
        let mut manager = SimulationTicketManager::new();
        let pos = ChunkPos::new(0, 0);
        let ticket = ChunkTicket::full_chunks_with_entity_ticking(1, 1);
        manager.add_ticket(pos, ticket);
        manager.add_player(pos, player_id(1));

        manager.run_all_updates(2);
        assert_eq!(manager.get_level(pos).map(ChunkTicketLevel::raw), Some(126));

        manager.run_all_updates(0);
        assert_eq!(manager.get_level(pos).map(ChunkTicketLevel::raw), Some(127));
    }

    #[test]
    fn incremental_updates_match_reference_across_distance_levels() {
        let mut manager = SimulationTicketManager::new();
        let mut previous_levels = FxHashMap::default();
        let strong = ChunkTicket::full_chunks_with_entity_ticking(4, 4);
        let weak = ChunkTicket::full_chunks_with_entity_ticking(1, 1);
        manager.add_ticket(ChunkPos::new(0, 0), strong);
        manager.add_ticket(ChunkPos::new(6, 1), weak);
        manager.add_player(ChunkPos::new(-3, 2), player_id(1));
        manager.add_player(ChunkPos::new(4, -2), player_id(2));

        for distance in [0, 1, 2, 10, 32, 128] {
            run_and_compare_with_reference(&mut manager, distance, &mut previous_levels);
        }

        assert!(manager.remove_ticket(ChunkPos::new(0, 0), strong));
        assert!(manager.move_player(player_id(1), ChunkPos::new(-3, 2), ChunkPos::new(9, 3)));
        run_and_compare_with_reference(&mut manager, 10, &mut previous_levels);

        assert!(manager.remove_player(ChunkPos::new(4, -2), player_id(2)));
        assert!(manager.remove_ticket(ChunkPos::new(6, 1), weak));
        run_and_compare_with_reference(&mut manager, 10, &mut previous_levels);
    }

    #[test]
    fn deterministic_random_operations_match_reference() {
        let mut manager = SimulationTicketManager::new();
        let mut previous_levels = FxHashMap::default();
        let mut sequence = DeterministicSequence(0x5eed_cafe_d00d_f00d);
        let tickets = [
            ChunkTicket::full_chunks_with_entity_ticking(0, 0),
            ChunkTicket::full_chunks_with_entity_ticking(2, 1),
            ChunkTicket::full_chunks_with_entity_ticking(4, 3),
        ];
        let distances = [0, 1, 2, 4, 8];
        let mut simulation_distance = SIMULATION_DISTANCE;

        for _ in 0..200 {
            let pos = ChunkPos::new(sequence.next(13) as i32 - 6, sequence.next(13) as i32 - 6);
            let ticket = tickets[sequence.next(tickets.len() as u32) as usize];
            let player = player_id(u128::from(sequence.next(5)) + 1);

            match sequence.next(6) {
                0 => {
                    manager.add_ticket(pos, ticket);
                }
                1 => {
                    manager.remove_ticket(pos, ticket);
                }
                2 => {
                    manager.add_player(pos, player);
                }
                3 => {
                    manager.remove_player(pos, player);
                }
                4 => {
                    if let Some(old_pos) = manager.player_positions.get(&player).copied() {
                        manager.move_player(player, old_pos, pos);
                    } else {
                        manager.add_player(pos, player);
                    }
                }
                5 => {
                    simulation_distance = distances[sequence.next(distances.len() as u32) as usize];
                }
                _ => unreachable!(),
            }

            run_and_compare_with_reference(&mut manager, simulation_distance, &mut previous_levels);
        }
    }

    #[test]
    fn batched_overlapping_moves_and_source_changes_match_reference() {
        let mut manager = SimulationTicketManager::new();
        let mut previous_levels = FxHashMap::default();
        let first_player = player_id(1);
        let second_player = player_id(2);
        let ticket = ChunkTicket::full_chunks_with_entity_ticking(4, 4);
        manager.add_player(ChunkPos::new(0, 0), first_player);
        manager.add_player(ChunkPos::new(3, 0), second_player);
        manager.add_ticket(ChunkPos::new(2, 2), ticket);
        run_and_compare_with_reference(&mut manager, 4, &mut previous_levels);

        assert!(manager.remove_player(ChunkPos::new(0, 0), first_player));
        assert!(manager.remove_player(ChunkPos::new(3, 0), second_player));
        assert!(manager.add_player(ChunkPos::new(5, 0), second_player));
        assert!(manager.remove_ticket(ChunkPos::new(2, 2), ticket));
        assert!(manager.add_ticket(ChunkPos::new(-30, 0), ticket));
        run_and_compare_with_reference(&mut manager, 4, &mut previous_levels);

        assert!(manager.remove_player(ChunkPos::new(5, 0), second_player));
        assert!(manager.add_player(ChunkPos::new(6, 0), second_player));
        assert!(manager.add_player(ChunkPos::new(7, 0), first_player));
        run_and_compare_with_reference(&mut manager, 4, &mut previous_levels);
    }

    #[test]
    fn isolated_one_chunk_move_matches_reference() {
        let mut manager = SimulationTicketManager::new();
        let mut previous_levels = FxHashMap::default();
        let player = player_id(1);
        let old_pos = ChunkPos::new(0, 0);
        let new_pos = ChunkPos::new(-1, 0);
        manager.add_player(old_pos, player);
        run_and_compare_with_reference(&mut manager, 32, &mut previous_levels);

        assert!(manager.remove_player(old_pos, player));
        assert!(manager.add_player(new_pos, player));
        run_and_compare_with_reference(&mut manager, 32, &mut previous_levels);
    }

    #[test]
    fn one_chunk_move_overlapping_an_unchanged_source_matches_reference() {
        let mut manager = SimulationTicketManager::new();
        let mut previous_levels = FxHashMap::default();
        let moving_player = player_id(1);
        let stationary_player = player_id(2);
        let old_pos = ChunkPos::new(0, 0);
        let new_pos = ChunkPos::new(-1, 0);
        manager.add_player(old_pos, moving_player);
        manager.add_player(ChunkPos::new(3, 0), stationary_player);
        run_and_compare_with_reference(&mut manager, 4, &mut previous_levels);

        assert!(manager.remove_player(old_pos, moving_player));
        assert!(manager.add_player(new_pos, moving_player));
        run_and_compare_with_reference(&mut manager, 4, &mut previous_levels);
    }
}
