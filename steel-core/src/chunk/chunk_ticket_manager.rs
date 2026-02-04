//! Chunk ticket management for tracking chunk levels and propagation.
#![allow(missing_docs)]

use std::mem;

use rustc_hash::{FxBuildHasher, FxHashMap};
use smallvec::SmallVec;
use steel_utils::ChunkPos;

use crate::{
    chunk::{chunk_access::ChunkStatus, chunk_pyramid::GENERATION_PYRAMID},
    config::STEEL_CONFIG,
};

/// The maximum view distance for players.
pub const MAX_VIEW_DISTANCE: u8 = 32;
const RADIUS_AROUND_FULL_CHUNK: u8 = GENERATION_PYRAMID
    .get_step_to(ChunkStatus::Full)
    .accumulated_dependencies
    .get_radius_of(ChunkStatus::Empty) as u8;
const MAX_LEVEL: u8 = MAX_VIEW_DISTANCE + RADIUS_AROUND_FULL_CHUNK;

#[must_use]
pub const fn is_full(level: u8) -> bool {
    level <= MAX_VIEW_DISTANCE
}

#[must_use]
pub fn is_ticked(level: u8) -> bool {
    level <= MAX_VIEW_DISTANCE - STEEL_CONFIG.view_distance + STEEL_CONFIG.simulation_distance
}

#[must_use]
pub const fn generation_status(level: Option<u8>) -> Option<ChunkStatus> {
    match level {
        None => None,
        Some(level) => {
            if is_full(level) {
                Some(ChunkStatus::Full)
            } else {
                let distance = (level - MAX_VIEW_DISTANCE) as usize;
                // Fallback to None if distance is out of bounds (simulating Vanilla logic)
                GENERATION_PYRAMID
                    .get_step_to(ChunkStatus::Full)
                    .accumulated_dependencies
                    .get(distance)
            }
        }
    }
}

/// Up to 4 tickets stored inline per position.
type TicketLevels = SmallVec<[u8; 4]>;

/// A level change for a chunk position.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LevelChange {
    pub pos: ChunkPos,
    /// `Some(level)` if level changed or added, `None` if removed.
    pub new_level: Option<u8>,
}

/// Chunk ticket propagation.
/// Lower levels = higher priority. Multiple tickets per position supported.
#[derive(Debug)]
pub struct ChunkTicketManager {
    tickets: FxHashMap<ChunkPos, TicketLevels>,
    levels: FxHashMap<ChunkPos, u8>,
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
            dirty: false,
            changes: Vec::new(),
        }
    }

    /// Adds a ticket. Multiple tickets can exist at the same position.
    pub fn add_ticket(&mut self, pos: ChunkPos, level: u8) {
        if level > MAX_LEVEL {
            return;
        }
        self.tickets.entry(pos).or_default().push(level);
        self.dirty = true;
    }

    /// Removes one ticket matching (pos, level). Returns true if found.
    pub fn remove_ticket(&mut self, pos: ChunkPos, level: u8) -> bool {
        if let Some(levels) = self.tickets.get_mut(&pos)
            && let Some(idx) = levels.iter().position(|&l| l == level)
        {
            levels.swap_remove(idx);
            self.dirty = true;
            if levels.is_empty() {
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
    pub fn get_ticket(&self, pos: ChunkPos) -> Option<u8> {
        self.tickets.get(&pos).and_then(|l| l.iter().min().copied())
    }

    #[must_use]
    pub fn get_tickets_at(&self, pos: ChunkPos) -> Option<&[u8]> {
        self.tickets.get(&pos).map(smallvec::SmallVec::as_slice)
    }

    /// Iterator over (position, `min_level`) for all ticket sources.
    pub fn tickets(&self) -> impl Iterator<Item = (ChunkPos, u8)> + '_ {
        self.tickets
            .iter()
            .filter_map(|(&pos, levels)| levels.iter().min().map(|&level| (pos, level)))
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

        self.dirty = false;

        // Propagate each ticket source
        for (&source_pos, levels) in &self.tickets {
            let Some(&source_level) = levels.iter().min() else {
                continue;
            };

            let radius = i32::from(MAX_LEVEL - source_level);
            let sx = source_pos.0.x;
            let sy = source_pos.0.y;

            for dy in -radius..=radius {
                for dx in -radius..=radius {
                    let distance = dx.abs().max(dy.abs()) as u8;
                    let level = source_level + distance;

                    let pos = ChunkPos::new(sx + dx, sy + dy);
                    self.levels
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
                }),
            }
        }

        // Find removed levels
        for &pos in old_levels.keys() {
            if !self.levels.contains_key(&pos) {
                self.changes.push(LevelChange {
                    pos,
                    new_level: None,
                });
            }
        }

        &self.changes
    }

    /// Returns the propagated level at position. Call `run_all_updates()` first.
    #[must_use]
    pub fn get_level(&self, pos: ChunkPos) -> Option<u8> {
        self.levels.get(&pos).copied()
    }

    #[allow(dead_code)]
    #[must_use]
    const fn is_dirty(&self) -> bool {
        self.dirty
    }

    #[allow(dead_code)]
    fn clear(&mut self) {
        self.tickets.clear();
        self.levels.clear();
        self.dirty = false;
        self.changes.clear();
    }

    pub fn iter_levels(&self) -> impl Iterator<Item = (ChunkPos, u8)> + '_ {
        self.levels.iter().map(|(&pos, &level)| (pos, level))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_single_ticket_propagation() {
        let mut manager = ChunkTicketManager::new();
        manager.add_ticket(ChunkPos::new(0, 0), 0);
        manager.run_all_updates();

        assert_eq!(manager.get_level(ChunkPos::new(0, 0)), Some(0));
        assert_eq!(manager.get_level(ChunkPos::new(-1, -1)), Some(1));
        assert_eq!(manager.get_level(ChunkPos::new(0, -1)), Some(1));
        assert_eq!(manager.get_level(ChunkPos::new(1, 0)), Some(1));
        assert_eq!(manager.get_level(ChunkPos::new(-2, -2)), Some(2));
    }

    #[test]
    fn test_deferred_updates() {
        let mut manager = ChunkTicketManager::new();
        manager.add_ticket(ChunkPos::new(0, 0), 0);

        assert!(manager.is_dirty());
        assert_eq!(manager.get_level(ChunkPos::new(0, 0)), None);

        manager.run_all_updates();
        assert!(!manager.is_dirty());
        assert_eq!(manager.get_level(ChunkPos::new(0, 0)), Some(0));
    }

    #[test]
    fn test_multiple_tickets_same_position() {
        let mut manager = ChunkTicketManager::new();
        manager.add_ticket(ChunkPos::new(0, 0), 2);
        manager.add_ticket(ChunkPos::new(0, 0), 0);
        manager.add_ticket(ChunkPos::new(0, 0), 1);
        manager.run_all_updates();

        assert_eq!(manager.get_ticket(ChunkPos::new(0, 0)), Some(0));
        assert_eq!(manager.get_level(ChunkPos::new(0, 0)), Some(0));
    }

    #[test]
    fn test_overlapping_propagation() {
        let mut manager = ChunkTicketManager::new();
        manager.add_ticket(ChunkPos::new(0, 0), 0);
        manager.add_ticket(ChunkPos::new(3, 0), 0);
        manager.run_all_updates();

        assert_eq!(manager.get_level(ChunkPos::new(1, 0)), Some(1));
        assert_eq!(manager.get_level(ChunkPos::new(2, 0)), Some(1));
    }

    #[test]
    fn test_remove_ticket() {
        let mut manager = ChunkTicketManager::new();
        manager.add_ticket(ChunkPos::new(0, 0), 0);
        manager.add_ticket(ChunkPos::new(5, 0), 0);
        manager.run_all_updates();

        assert_eq!(manager.get_level(ChunkPos::new(0, 0)), Some(0));
        assert_eq!(manager.get_level(ChunkPos::new(5, 0)), Some(0));

        assert!(manager.remove_ticket(ChunkPos::new(0, 0), 0));
        manager.run_all_updates();

        assert_eq!(manager.get_level(ChunkPos::new(0, 0)), Some(5));
        assert_eq!(manager.get_level(ChunkPos::new(5, 0)), Some(0));
    }

    #[test]
    fn test_remove_all_tickets_at_position() {
        let mut manager = ChunkTicketManager::new();
        manager.add_ticket(ChunkPos::new(0, 0), 0);
        manager.run_all_updates();

        manager.remove_ticket(ChunkPos::new(0, 0), 0);
        manager.run_all_updates();

        assert_eq!(manager.get_level(ChunkPos::new(0, 0)), None);
    }

    #[test]
    fn test_multiple_tickets_same_position_with_removal() {
        let mut manager = ChunkTicketManager::new();
        manager.add_ticket(ChunkPos::new(0, 0), 0);
        manager.add_ticket(ChunkPos::new(0, 0), 2);
        manager.add_ticket(ChunkPos::new(0, 0), 1);
        manager.run_all_updates();

        assert_eq!(manager.get_ticket(ChunkPos::new(0, 0)), Some(0));
        assert_eq!(manager.ticket_count(), 3);

        manager.remove_ticket(ChunkPos::new(0, 0), 0);
        manager.run_all_updates();
        assert_eq!(manager.get_ticket(ChunkPos::new(0, 0)), Some(1));

        manager.remove_ticket(ChunkPos::new(0, 0), 1);
        manager.run_all_updates();
        assert_eq!(manager.get_ticket(ChunkPos::new(0, 0)), Some(2));
    }

    #[test]
    fn test_duplicate_tickets_same_level() {
        let mut manager = ChunkTicketManager::new();
        manager.add_ticket(ChunkPos::new(0, 0), 0);
        manager.add_ticket(ChunkPos::new(0, 0), 0);
        manager.run_all_updates();

        assert_eq!(manager.ticket_count(), 2);

        manager.remove_ticket(ChunkPos::new(0, 0), 0);
        manager.run_all_updates();
        assert_eq!(manager.ticket_count(), 1);
        assert_eq!(manager.get_level(ChunkPos::new(0, 0)), Some(0));

        manager.remove_ticket(ChunkPos::new(0, 0), 0);
        manager.run_all_updates();
        assert_eq!(manager.ticket_count(), 0);
        assert_eq!(manager.get_level(ChunkPos::new(0, 0)), None);
    }

    #[test]
    fn test_no_recalculation_when_clean() {
        let mut manager = ChunkTicketManager::new();
        manager.add_ticket(ChunkPos::new(0, 0), 0);
        manager.run_all_updates();

        assert!(!manager.is_dirty());
        manager.run_all_updates();
        assert!(!manager.is_dirty());
    }
}
