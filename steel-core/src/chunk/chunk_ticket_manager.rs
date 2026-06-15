//! Chunk ticket management for tracking chunk levels and propagation.
#![expect(missing_docs, reason = "internal module; items are self-explanatory")]

use std::mem;

use rustc_hash::{FxBuildHasher, FxHashMap};
use smallvec::SmallVec;
use steel_utils::ChunkPos;

use crate::chunk::{chunk_access::ChunkStatus, chunk_pyramid::GENERATION_PYRAMID};

/// The maximum view distance for players.
pub const MAX_VIEW_DISTANCE: u8 = 32;
const RADIUS_AROUND_FULL_CHUNK: u8 = GENERATION_PYRAMID
    .get_step_to(ChunkStatus::Full)
    .accumulated_dependencies
    .get_radius_of(ChunkStatus::Empty) as u8;
const MAX_LEVEL_RAW: u8 = MAX_VIEW_DISTANCE + RADIUS_AROUND_FULL_CHUNK;

/// A chunk ticket level.
///
/// Lower levels are stronger tickets. `MAX_VIEW_DISTANCE` is the boundary where
/// a propagated ticket can still make a chunk full; larger levels only keep
/// dependency chunks loaded far enough for generation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ChunkTicketLevel(u8);

impl ChunkTicketLevel {
    /// The weakest level that still permits a full chunk.
    pub const FULL_CHUNK: Self = Self(MAX_VIEW_DISTANCE);
    /// The weakest level kept by ticket propagation.
    pub const MAX: Self = Self(MAX_LEVEL_RAW);

    /// Builds a ticket level from its raw propagated value.
    #[must_use]
    pub const fn new(raw: u8) -> Option<Self> {
        if raw <= MAX_LEVEL_RAW {
            Some(Self(raw))
        } else {
            None
        }
    }

    /// Builds a full-chunk ticket level from a square radius.
    #[must_use]
    pub const fn for_full_chunk_radius(radius: u8) -> Self {
        Self(MAX_VIEW_DISTANCE.saturating_sub(radius))
    }

    /// Returns the raw level value used for compact storage.
    #[must_use]
    pub const fn raw(self) -> u8 {
        self.0
    }

    #[must_use]
    pub const fn is_full(self) -> bool {
        self.0 <= Self::FULL_CHUNK.0
    }

    #[must_use]
    const fn with_distance(self, distance: u8) -> Option<Self> {
        let level = self.0.saturating_add(distance);
        Self::new(level)
    }

    #[must_use]
    const fn distance_to_max(self) -> u8 {
        MAX_LEVEL_RAW - self.0
    }

    #[must_use]
    const fn distance_to_full(self) -> u8 {
        MAX_VIEW_DISTANCE - self.0
    }
}

/// A chunk ticket's load and optional simulation strength.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChunkTicket {
    load_level: ChunkTicketLevel,
    simulation_level: Option<ChunkTicketLevel>,
}

impl ChunkTicket {
    /// Creates a loading-only ticket.
    #[must_use]
    pub const fn loading(load_level: ChunkTicketLevel) -> Self {
        Self {
            load_level,
            simulation_level: None,
        }
    }

    /// Creates a loading-only ticket that makes chunks full within `radius`.
    #[must_use]
    pub const fn full_chunks(radius: u8) -> Self {
        Self::loading(ChunkTicketLevel::for_full_chunk_radius(radius))
    }

    /// Creates a ticket that loads and simulates full chunks within `radius`.
    #[must_use]
    pub const fn simulated_full_chunks(radius: u8) -> Self {
        let level = ChunkTicketLevel::for_full_chunk_radius(radius);
        Self {
            load_level: level,
            simulation_level: Some(level),
        }
    }

    /// Creates a ticket with separate load and simulation radii.
    #[must_use]
    pub const fn full_chunks_with_simulation(load_radius: u8, simulation_radius: u8) -> Self {
        let simulation_radius = if simulation_radius > load_radius {
            load_radius
        } else {
            simulation_radius
        };

        Self {
            load_level: ChunkTicketLevel::for_full_chunk_radius(load_radius),
            simulation_level: Some(ChunkTicketLevel::for_full_chunk_radius(simulation_radius)),
        }
    }

    /// Creates a player ticket, capping simulation to the loaded radius.
    #[must_use]
    pub const fn player(load_radius: u8, simulation_radius: u8) -> Self {
        Self::full_chunks_with_simulation(load_radius, simulation_radius)
    }

    #[must_use]
    pub const fn load_level(self) -> ChunkTicketLevel {
        self.load_level
    }

    #[must_use]
    pub const fn simulation_level(self) -> Option<ChunkTicketLevel> {
        self.simulation_level
    }
}

#[must_use]
pub const fn is_full(level: ChunkTicketLevel) -> bool {
    level.is_full()
}

#[must_use]
pub const fn is_ticked(level: Option<ChunkTicketLevel>) -> bool {
    match level {
        Some(level) => level.is_full(),
        None => false,
    }
}

#[must_use]
pub const fn generation_status(level: Option<ChunkTicketLevel>) -> Option<ChunkStatus> {
    match level {
        None => None,
        Some(level) => {
            if is_full(level) {
                Some(ChunkStatus::Full)
            } else {
                let distance = (level.raw() - MAX_VIEW_DISTANCE) as usize;
                // Fallback to None if distance is out of bounds (simulating Vanilla logic)
                GENERATION_PYRAMID
                    .get_step_to(ChunkStatus::Full)
                    .accumulated_dependencies
                    .get(distance)
            }
        }
    }
}

/// Returns the ticket level that permits a chunk to reach at least `status`.
///
/// This is derived from the full-chunk dependency pyramid so request tickets use
/// the same propagation rules as player tickets.
#[must_use]
pub const fn ticket_level_for_status(status: ChunkStatus) -> ChunkTicketLevel {
    if matches!(status, ChunkStatus::Full) {
        ChunkTicketLevel::FULL_CHUNK
    } else {
        ChunkTicketLevel(
            MAX_VIEW_DISTANCE
                + GENERATION_PYRAMID
                    .get_step_to(ChunkStatus::Full)
                    .accumulated_dependencies
                    .get_radius_of(status) as u8,
        )
    }
}

/// Up to 4 tickets stored inline per position.
type TicketLevels = SmallVec<[ChunkTicket; 4]>;

/// A level change for a chunk position.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LevelChange {
    pub pos: ChunkPos,
    /// `Some(level)` if level changed or added, `None` if removed.
    pub new_level: Option<ChunkTicketLevel>,
    /// `Some(level)` if simulation changed or added, `None` if removed.
    pub new_simulation_level: Option<ChunkTicketLevel>,
}

/// Chunk ticket propagation.
/// Lower levels = higher priority. Multiple tickets per position supported.
#[derive(Debug)]
pub struct ChunkTicketManager {
    tickets: FxHashMap<ChunkPos, TicketLevels>,
    levels: FxHashMap<ChunkPos, ChunkTicketLevel>,
    simulation_levels: FxHashMap<ChunkPos, ChunkTicketLevel>,
    dirty: bool,
    /// Tracks changes from the last `run_all_updates()` call.
    changes: Vec<LevelChange>,
}

impl Default for ChunkTicketManager {
    fn default() -> Self {
        Self::new()
    }
}

impl ChunkTicketManager {
    #[must_use]
    pub fn new() -> Self {
        Self {
            tickets: FxHashMap::default(),
            levels: FxHashMap::default(),
            simulation_levels: FxHashMap::default(),
            dirty: false,
            changes: Vec::new(),
        }
    }

    /// Adds a ticket. Multiple tickets can exist at the same position.
    pub fn add_ticket(&mut self, pos: ChunkPos, ticket: ChunkTicket) {
        self.tickets.entry(pos).or_default().push(ticket);
        self.dirty = true;
    }

    /// Removes one ticket matching `(pos, ticket)`. Returns true if found.
    pub fn remove_ticket(&mut self, pos: ChunkPos, ticket: ChunkTicket) -> bool {
        if let Some(tickets) = self.tickets.get_mut(&pos)
            && let Some(idx) = tickets.iter().position(|&existing| existing == ticket)
        {
            tickets.swap_remove(idx);
            self.dirty = true;
            if tickets.is_empty() {
                self.tickets.remove(&pos);
            }
            return true;
        }
        false
    }

    /// Removes all tickets at position.
    pub fn remove_all_tickets_at(&mut self, pos: ChunkPos) -> Option<TicketLevels> {
        let removed = self.tickets.remove(&pos);
        if removed.is_some() {
            self.dirty = true;
        }
        removed
    }

    /// Returns the minimum ticket level at position.
    #[must_use]
    pub fn get_ticket(&self, pos: ChunkPos) -> Option<ChunkTicketLevel> {
        self.tickets
            .get(&pos)
            .and_then(|tickets| tickets.iter().map(|ticket| ticket.load_level()).min())
    }

    #[must_use]
    pub fn get_tickets_at(&self, pos: ChunkPos) -> Option<&[ChunkTicket]> {
        self.tickets.get(&pos).map(smallvec::SmallVec::as_slice)
    }

    /// Iterator over (position, `min_level`) for all ticket sources.
    pub fn tickets(&self) -> impl Iterator<Item = (ChunkPos, ChunkTicketLevel)> + '_ {
        self.tickets.iter().filter_map(|(&pos, tickets)| {
            tickets
                .iter()
                .map(|ticket| ticket.load_level())
                .min()
                .map(|level| (pos, level))
        })
    }

    #[must_use]
    pub fn ticket_count(&self) -> usize {
        self.tickets.values().map(smallvec::SmallVec::len).sum()
    }

    #[must_use]
    pub fn ticket_position_count(&self) -> usize {
        self.tickets.len()
    }

    /// Propagates all tickets. Only runs if dirty.
    /// Returns a slice of changes (added/updated/removed levels).
    pub fn run_all_updates(&mut self) -> &[LevelChange] {
        self.changes.clear();

        if !self.dirty {
            return &self.changes;
        }

        // Swap out old levels to compare against later, reusing capacity
        let old_capacity = self.levels.capacity();
        let old_levels = mem::replace(
            &mut self.levels,
            FxHashMap::with_capacity_and_hasher(old_capacity, FxBuildHasher),
        );
        let old_simulation_capacity = self.simulation_levels.capacity();
        let old_simulation_levels = mem::replace(
            &mut self.simulation_levels,
            FxHashMap::with_capacity_and_hasher(old_simulation_capacity, FxBuildHasher),
        );

        self.dirty = false;

        // Propagate each ticket source
        for (&source_pos, tickets) in &self.tickets {
            let Some(source_level) = tickets.iter().map(|ticket| ticket.load_level()).min() else {
                continue;
            };

            let radius = i32::from(source_level.distance_to_max());
            let sx = source_pos.0.x;
            let sy = source_pos.0.y;

            for dy in -radius..=radius {
                for dx in -radius..=radius {
                    let distance = dx.abs().max(dy.abs()) as u8;
                    let Some(level) = source_level.with_distance(distance) else {
                        continue;
                    };

                    let pos = ChunkPos::new(sx + dx, sy + dy);
                    self.levels
                        .entry(pos)
                        .and_modify(|e| *e = (*e).min(level))
                        .or_insert(level);
                }
            }

            let Some(simulation_level) = tickets
                .iter()
                .filter_map(|ticket| ticket.simulation_level())
                .min()
            else {
                continue;
            };

            let radius = i32::from(simulation_level.distance_to_full());
            for dy in -radius..=radius {
                for dx in -radius..=radius {
                    let distance = dx.abs().max(dy.abs()) as u8;
                    let Some(level) = simulation_level.with_distance(distance) else {
                        continue;
                    };

                    let pos = ChunkPos::new(sx + dx, sy + dy);
                    self.simulation_levels
                        .entry(pos)
                        .and_modify(|e| *e = (*e).min(level))
                        .or_insert(level);
                }
            }
        }

        // Find changed/added levels
        for (&pos, &new_level) in &self.levels {
            match old_levels.get(&pos) {
                Some(&old_level) if old_level == new_level => {} // No change
                _ => self.changes.push(LevelChange {
                    pos,
                    new_level: Some(new_level),
                    new_simulation_level: self.simulation_levels.get(&pos).copied(),
                }),
            }
        }

        // Find removed levels
        for &pos in old_levels.keys() {
            if !self.levels.contains_key(&pos) {
                self.changes.push(LevelChange {
                    pos,
                    new_level: None,
                    new_simulation_level: None,
                });
            }
        }

        self.record_simulation_only_changes(&old_levels, &old_simulation_levels);

        &self.changes
    }

    fn record_simulation_only_changes(
        &mut self,
        old_levels: &FxHashMap<ChunkPos, ChunkTicketLevel>,
        old_simulation_levels: &FxHashMap<ChunkPos, ChunkTicketLevel>,
    ) {
        for (&pos, &new_level) in &self.simulation_levels {
            let load_changed = old_levels.get(&pos) != self.levels.get(&pos);
            if load_changed {
                continue;
            }

            match old_simulation_levels.get(&pos) {
                Some(&old_level) if old_level == new_level => {}
                _ => self.changes.push(LevelChange {
                    pos,
                    new_level: self.levels.get(&pos).copied(),
                    new_simulation_level: Some(new_level),
                }),
            }
        }

        for &pos in old_simulation_levels.keys() {
            let load_changed = old_levels.get(&pos) != self.levels.get(&pos);
            if load_changed || self.simulation_levels.contains_key(&pos) {
                continue;
            }

            self.changes.push(LevelChange {
                pos,
                new_level: self.levels.get(&pos).copied(),
                new_simulation_level: None,
            });
        }
    }

    /// Returns the propagated level at position. Call `run_all_updates()` first.
    #[must_use]
    pub fn get_level(&self, pos: ChunkPos) -> Option<ChunkTicketLevel> {
        self.levels.get(&pos).copied()
    }

    /// Returns the propagated simulation level at position. Call `run_all_updates()` first.
    #[must_use]
    pub fn get_simulation_level(&self, pos: ChunkPos) -> Option<ChunkTicketLevel> {
        self.simulation_levels.get(&pos).copied()
    }

    #[cfg(test)]
    #[must_use]
    const fn is_dirty(&self) -> bool {
        self.dirty
    }

    #[expect(dead_code, reason = "utility method for tests and future use")]
    fn clear(&mut self) {
        self.tickets.clear();
        self.levels.clear();
        self.simulation_levels.clear();
        self.dirty = false;
        self.changes.clear();
    }

    pub fn iter_levels(&self) -> impl Iterator<Item = (ChunkPos, ChunkTicketLevel)> + '_ {
        self.levels.iter().map(|(&pos, &level)| (pos, level))
    }

    pub fn iter_simulation_levels(
        &self,
    ) -> impl Iterator<Item = (ChunkPos, ChunkTicketLevel)> + '_ {
        self.simulation_levels
            .iter()
            .map(|(&pos, &level)| (pos, level))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_single_ticket_propagation() {
        let mut manager = ChunkTicketManager::new();
        manager.add_ticket(
            ChunkPos::new(0, 0),
            ChunkTicket::full_chunks(MAX_VIEW_DISTANCE),
        );
        manager.run_all_updates();

        assert_eq!(
            manager.get_level(ChunkPos::new(0, 0)),
            ChunkTicketLevel::new(0)
        );
        assert_eq!(
            manager.get_level(ChunkPos::new(-1, -1)),
            ChunkTicketLevel::new(1)
        );
        assert_eq!(
            manager.get_level(ChunkPos::new(0, -1)),
            ChunkTicketLevel::new(1)
        );
        assert_eq!(
            manager.get_level(ChunkPos::new(1, 0)),
            ChunkTicketLevel::new(1)
        );
        assert_eq!(
            manager.get_level(ChunkPos::new(-2, -2)),
            ChunkTicketLevel::new(2)
        );
    }

    #[test]
    fn test_deferred_updates() {
        let mut manager = ChunkTicketManager::new();
        manager.add_ticket(
            ChunkPos::new(0, 0),
            ChunkTicket::full_chunks(MAX_VIEW_DISTANCE),
        );

        assert!(manager.is_dirty());
        assert_eq!(manager.get_level(ChunkPos::new(0, 0)), None);

        manager.run_all_updates();
        assert!(!manager.is_dirty());
        assert_eq!(
            manager.get_level(ChunkPos::new(0, 0)),
            ChunkTicketLevel::new(0)
        );
    }

    #[test]
    fn test_multiple_tickets_same_position() {
        let mut manager = ChunkTicketManager::new();
        manager.add_ticket(
            ChunkPos::new(0, 0),
            ChunkTicket::loading(ChunkTicketLevel::new(2).expect("test level is valid")),
        );
        manager.add_ticket(
            ChunkPos::new(0, 0),
            ChunkTicket::full_chunks(MAX_VIEW_DISTANCE),
        );
        manager.add_ticket(
            ChunkPos::new(0, 0),
            ChunkTicket::loading(ChunkTicketLevel::new(1).expect("test level is valid")),
        );
        manager.run_all_updates();

        assert_eq!(
            manager.get_ticket(ChunkPos::new(0, 0)),
            ChunkTicketLevel::new(0)
        );
        assert_eq!(
            manager.get_level(ChunkPos::new(0, 0)),
            ChunkTicketLevel::new(0)
        );
    }

    #[test]
    fn test_overlapping_propagation() {
        let mut manager = ChunkTicketManager::new();
        manager.add_ticket(
            ChunkPos::new(0, 0),
            ChunkTicket::full_chunks(MAX_VIEW_DISTANCE),
        );
        manager.add_ticket(
            ChunkPos::new(3, 0),
            ChunkTicket::full_chunks(MAX_VIEW_DISTANCE),
        );
        manager.run_all_updates();

        assert_eq!(
            manager.get_level(ChunkPos::new(1, 0)),
            ChunkTicketLevel::new(1)
        );
        assert_eq!(
            manager.get_level(ChunkPos::new(2, 0)),
            ChunkTicketLevel::new(1)
        );
    }

    #[test]
    fn test_remove_ticket() {
        let mut manager = ChunkTicketManager::new();
        let ticket = ChunkTicket::full_chunks(MAX_VIEW_DISTANCE);
        manager.add_ticket(ChunkPos::new(0, 0), ticket);
        manager.add_ticket(ChunkPos::new(5, 0), ticket);
        manager.run_all_updates();

        assert_eq!(
            manager.get_level(ChunkPos::new(0, 0)),
            ChunkTicketLevel::new(0)
        );
        assert_eq!(
            manager.get_level(ChunkPos::new(5, 0)),
            ChunkTicketLevel::new(0)
        );

        assert!(manager.remove_ticket(ChunkPos::new(0, 0), ticket));
        manager.run_all_updates();

        assert_eq!(
            manager.get_level(ChunkPos::new(0, 0)),
            ChunkTicketLevel::new(5)
        );
        assert_eq!(
            manager.get_level(ChunkPos::new(5, 0)),
            ChunkTicketLevel::new(0)
        );
    }

    #[test]
    fn test_remove_all_tickets_at_position() {
        let mut manager = ChunkTicketManager::new();
        let ticket = ChunkTicket::full_chunks(MAX_VIEW_DISTANCE);
        manager.add_ticket(ChunkPos::new(0, 0), ticket);
        manager.run_all_updates();

        manager.remove_ticket(ChunkPos::new(0, 0), ticket);
        manager.run_all_updates();

        assert_eq!(manager.get_level(ChunkPos::new(0, 0)), None);
    }

    #[test]
    fn test_multiple_tickets_same_position_with_removal() {
        let mut manager = ChunkTicketManager::new();
        let level_0 = ChunkTicket::full_chunks(MAX_VIEW_DISTANCE);
        let level_1 = ChunkTicket::loading(ChunkTicketLevel::new(1).expect("test level is valid"));
        let level_2 = ChunkTicket::loading(ChunkTicketLevel::new(2).expect("test level is valid"));
        manager.add_ticket(ChunkPos::new(0, 0), level_0);
        manager.add_ticket(ChunkPos::new(0, 0), level_2);
        manager.add_ticket(ChunkPos::new(0, 0), level_1);
        manager.run_all_updates();

        assert_eq!(
            manager.get_ticket(ChunkPos::new(0, 0)),
            ChunkTicketLevel::new(0)
        );
        assert_eq!(manager.ticket_count(), 3);

        manager.remove_ticket(ChunkPos::new(0, 0), level_0);
        manager.run_all_updates();
        assert_eq!(
            manager.get_ticket(ChunkPos::new(0, 0)),
            ChunkTicketLevel::new(1)
        );

        manager.remove_ticket(ChunkPos::new(0, 0), level_1);
        manager.run_all_updates();
        assert_eq!(
            manager.get_ticket(ChunkPos::new(0, 0)),
            ChunkTicketLevel::new(2)
        );
    }

    #[test]
    fn test_duplicate_tickets_same_level() {
        let mut manager = ChunkTicketManager::new();
        let ticket = ChunkTicket::full_chunks(MAX_VIEW_DISTANCE);
        manager.add_ticket(ChunkPos::new(0, 0), ticket);
        manager.add_ticket(ChunkPos::new(0, 0), ticket);
        manager.run_all_updates();

        assert_eq!(manager.ticket_count(), 2);

        manager.remove_ticket(ChunkPos::new(0, 0), ticket);
        manager.run_all_updates();
        assert_eq!(manager.ticket_count(), 1);
        assert_eq!(
            manager.get_level(ChunkPos::new(0, 0)),
            ChunkTicketLevel::new(0)
        );

        manager.remove_ticket(ChunkPos::new(0, 0), ticket);
        manager.run_all_updates();
        assert_eq!(manager.ticket_count(), 0);
        assert_eq!(manager.get_level(ChunkPos::new(0, 0)), None);
    }

    #[test]
    fn test_no_recalculation_when_clean() {
        let mut manager = ChunkTicketManager::new();
        manager.add_ticket(
            ChunkPos::new(0, 0),
            ChunkTicket::full_chunks(MAX_VIEW_DISTANCE),
        );
        manager.run_all_updates();

        assert!(!manager.is_dirty());
        manager.run_all_updates();
        assert!(!manager.is_dirty());
    }

    #[test]
    fn simulated_ticket_propagates_only_inside_loaded_full_area() {
        let mut manager = ChunkTicketManager::new();
        manager.add_ticket(ChunkPos::new(0, 0), ChunkTicket::simulated_full_chunks(1));
        manager.run_all_updates();

        assert!(is_ticked(manager.get_simulation_level(ChunkPos::new(0, 0))));
        assert!(is_ticked(manager.get_simulation_level(ChunkPos::new(1, 1))));
        assert!(!is_ticked(
            manager.get_simulation_level(ChunkPos::new(2, 0))
        ));
        assert_eq!(
            manager.get_level(ChunkPos::new(1, 1)),
            Some(ChunkTicketLevel::FULL_CHUNK)
        );
    }

    #[test]
    fn player_ticket_caps_simulation_radius_to_load_radius() {
        let mut manager = ChunkTicketManager::new();
        manager.add_ticket(ChunkPos::new(0, 0), ChunkTicket::player(1, 3));
        manager.run_all_updates();

        assert!(is_ticked(manager.get_simulation_level(ChunkPos::new(1, 0))));
        assert!(!is_ticked(
            manager.get_simulation_level(ChunkPos::new(2, 0))
        ));
    }

    #[test]
    fn ticket_level_for_status_allows_requested_status() {
        for index in 0..=ChunkStatus::Full.get_index() {
            let status = ChunkStatus::from_index(index).expect("index is in status range");
            let ticket_level = ticket_level_for_status(status);
            let allowed = generation_status(Some(ticket_level));
            assert!(
                allowed.is_some_and(|allowed| allowed >= status),
                "{status:?} request mapped to level {ticket_level:?}, which allows {allowed:?}"
            );
        }
    }

    #[test]
    fn ticket_level_for_status_creates_required_dependency_holders() {
        for index in 0..=ChunkStatus::Full.get_index() {
            let status = ChunkStatus::from_index(index).expect("index is in status range");
            let ticket_level = ticket_level_for_status(status);
            let propagation_radius = usize::from(ticket_level.distance_to_max());
            let required_radius = GENERATION_PYRAMID
                .get_step_to(status)
                .accumulated_dependencies
                .get_radius();

            assert!(
                propagation_radius >= required_radius,
                "{status:?} request maps to level {ticket_level:?}, propagation radius {propagation_radius}, required radius {required_radius}"
            );
        }
    }
}
