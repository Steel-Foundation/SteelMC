//! Chunk ticket management for tracking chunk levels and propagation.
#![expect(missing_docs, reason = "internal module; items are self-explanatory")]

use std::mem;

use rustc_hash::{FxBuildHasher, FxHashMap};
use serde::{Deserialize, Serialize};
use smallvec::SmallVec;
use steel_utils::ChunkPos;

use crate::chunk::{chunk_access::ChunkStatus, chunk_pyramid::GENERATION_PYRAMID};

/// The maximum view distance for players.
pub const MAX_VIEW_DISTANCE: u8 = 127;
const RADIUS_AROUND_FULL_CHUNK: u8 = GENERATION_PYRAMID
    .get_step_to(ChunkStatus::Full)
    .accumulated_dependencies
    .get_radius_of(ChunkStatus::Empty) as u8;
const MAX_LEVEL_RAW: u8 = MAX_VIEW_DISTANCE + RADIUS_AROUND_FULL_CHUNK;
pub(crate) const PORTAL_TICKET_RADIUS: u8 = 3;
const PORTAL_TICKET_TIMEOUT_TICKS: i64 = 300;
pub(crate) const ENDER_PEARL_TICKET_TIMEOUT_TICKS: u32 = 40;
const ENDER_PEARL_TICKET_RADIUS: u8 = 2;

/// A chunk ticket level.
///
/// Lower levels are stronger tickets. `MAX_VIEW_DISTANCE` is the boundary where
/// a propagated ticket can still make a chunk full; larger levels only keep
/// dependency chunks loaded far enough for generation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ChunkTicketLevel(u8);

impl ChunkTicketLevel {
    /// The weakest level whose full chunk may tick entities.
    pub const ENTITY_TICKING_CHUNK: Self = Self(MAX_VIEW_DISTANCE - 2);
    /// The weakest level whose full chunk may tick blocks.
    pub const BLOCK_TICKING_CHUNK: Self = Self(MAX_VIEW_DISTANCE - 1);
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

    /// Builds an entity-ticking ticket level from a square radius.
    #[must_use]
    pub const fn for_entity_ticking_radius(radius: u8) -> Self {
        Self(Self::ENTITY_TICKING_CHUNK.0.saturating_sub(radius))
    }

    /// Returns the raw level value used for compact storage.
    #[must_use]
    pub const fn raw(self) -> u8 {
        self.0
    }

    /// Returns vanilla's full-chunk status for this propagated level.
    #[must_use]
    pub const fn full_status(self) -> FullChunkStatus {
        if self.0 <= Self::ENTITY_TICKING_CHUNK.0 {
            FullChunkStatus::EntityTicking
        } else if self.0 <= Self::BLOCK_TICKING_CHUNK.0 {
            FullChunkStatus::BlockTicking
        } else if self.0 <= Self::FULL_CHUNK.0 {
            FullChunkStatus::Full
        } else {
            FullChunkStatus::Inaccessible
        }
    }

    #[must_use]
    pub const fn is_full(self) -> bool {
        self.0 <= Self::FULL_CHUNK.0
    }

    #[must_use]
    pub const fn is_block_ticking(self) -> bool {
        self.0 <= Self::BLOCK_TICKING_CHUNK.0
    }

    #[must_use]
    pub const fn is_entity_ticking(self) -> bool {
        self.0 <= Self::ENTITY_TICKING_CHUNK.0
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
    const fn distance_to_block_ticking(self) -> u8 {
        ChunkTicketLevel::BLOCK_TICKING_CHUNK
            .0
            .saturating_sub(self.0)
    }
}

/// Vanilla full-chunk accessibility and ticking status.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum FullChunkStatus {
    Inaccessible,
    Full,
    BlockTicking,
    EntityTicking,
}

impl FullChunkStatus {
    #[must_use]
    pub const fn is_or_after(self, status: Self) -> bool {
        self as u8 >= status as u8
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

    /// Creates a vanilla simulation ticket whose source level is `FULL - radius`.
    #[must_use]
    pub const fn simulated_full_chunks(radius: u8) -> Self {
        let level = ChunkTicketLevel::for_full_chunk_radius(radius);
        Self {
            load_level: level,
            simulation_level: Some(level),
        }
    }

    /// Creates a ticket with separate full-load and entity-ticking radii.
    #[must_use]
    pub const fn full_chunks_with_entity_ticking(
        load_radius: u8,
        entity_ticking_radius: u8,
    ) -> Self {
        let entity_ticking_radius = if entity_ticking_radius > load_radius {
            load_radius
        } else {
            entity_ticking_radius
        };

        Self {
            load_level: ChunkTicketLevel::for_full_chunk_radius(load_radius),
            simulation_level: Some(ChunkTicketLevel::for_entity_ticking_radius(
                entity_ticking_radius,
            )),
        }
    }

    /// Creates a player ticket, capping simulation to the loaded radius.
    #[must_use]
    pub const fn player(load_radius: u8, simulation_radius: u8) -> Self {
        Self::full_chunks_with_entity_ticking(load_radius, simulation_radius)
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
pub const fn full_status(level: Option<ChunkTicketLevel>) -> FullChunkStatus {
    match level {
        Some(level) => level.full_status(),
        None => FullChunkStatus::Inaccessible,
    }
}

#[must_use]
pub const fn is_block_ticking(level: Option<ChunkTicketLevel>) -> bool {
    match level {
        Some(level) => level.is_block_ticking(),
        None => false,
    }
}

#[must_use]
pub const fn is_entity_ticking(level: Option<ChunkTicketLevel>) -> bool {
    match level {
        Some(level) => level.is_entity_ticking(),
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

/// Persistent chunk ticket saved data.
#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct PersistentChunkTickets {
    #[serde(default)]
    tickets: Vec<PersistentChunkTicket>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
struct PersistentChunkTicket {
    #[serde(rename = "type")]
    kind: PersistentChunkTicketKind,
    chunk_x: i32,
    chunk_z: i32,
    ticks_left: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum PersistentChunkTicketKind {
    Portal,
}

/// Timed chunk tickets owned by vanilla gameplay systems.
#[derive(Debug, Default)]
pub(crate) struct TimedChunkTickets {
    tickets: Vec<TimedChunkTicket>,
}

impl TimedChunkTickets {
    /// Restores timed tickets from persistent saved data.
    pub(crate) fn from_persistent(persistent: PersistentChunkTickets) -> Self {
        let mut timed_tickets = Self::default();
        for ticket in persistent.tickets {
            timed_tickets.add_loaded_persistent_ticket(ticket);
        }
        timed_tickets
    }

    /// Converts active timed tickets to persistent saved data.
    pub(crate) fn to_persistent(&self) -> PersistentChunkTickets {
        PersistentChunkTickets {
            tickets: self
                .tickets
                .iter()
                .copied()
                .filter_map(TimedChunkTicket::to_persistent)
                .collect(),
        }
    }

    /// Inserts restored timed ticket sources into the active ticket manager.
    pub(crate) fn activate_all(&self, ticket_manager: &mut ChunkTicketManager) {
        for ticket in &self.tickets {
            ticket_manager.add_ticket(ticket.pos, ticket.ticket);
        }
    }

    /// Adds or refreshes vanilla's portal ticket.
    pub(crate) fn add_portal_ticket(
        &mut self,
        ticket_manager: &mut ChunkTicketManager,
        pos: ChunkPos,
    ) {
        self.add_or_reset(
            ticket_manager,
            TimedChunkTicketKind::Portal,
            pos,
            portal_ticket(),
            PORTAL_TICKET_TIMEOUT_TICKS,
        );
    }

    /// Adds or refreshes vanilla's in-flight ender pearl ticket.
    pub(crate) fn add_ender_pearl_ticket(
        &mut self,
        ticket_manager: &mut ChunkTicketManager,
        pos: ChunkPos,
    ) {
        self.add_or_reset(
            ticket_manager,
            TimedChunkTicketKind::EnderPearl,
            pos,
            ender_pearl_ticket(),
            i64::from(ENDER_PEARL_TICKET_TIMEOUT_TICKS),
        );
    }

    /// Decrements timed tickets and removes expired sources from the ticket manager.
    pub(crate) fn tick(
        &mut self,
        ticket_manager: &mut ChunkTicketManager,
        mut can_expire: impl FnMut(ChunkPos) -> bool,
    ) {
        let mut index = 0;
        while index < self.tickets.len() {
            let ticket = &mut self.tickets[index];
            if !can_expire(ticket.pos) {
                index += 1;
                continue;
            }

            ticket.ticks_left -= 1;
            if ticket.ticks_left >= 0 {
                index += 1;
                continue;
            }

            let expired = self.tickets.swap_remove(index);
            ticket_manager.remove_ticket(expired.pos, expired.ticket);
        }
    }

    fn add_or_reset(
        &mut self,
        ticket_manager: &mut ChunkTicketManager,
        kind: TimedChunkTicketKind,
        pos: ChunkPos,
        ticket: ChunkTicket,
        ticks_left: i64,
    ) {
        if let Some(existing) = self
            .tickets
            .iter_mut()
            .find(|entry| entry.kind == kind && entry.pos == pos && entry.ticket == ticket)
        {
            existing.ticks_left = ticks_left;
            return;
        }

        self.tickets.push(TimedChunkTicket {
            kind,
            pos,
            ticket,
            ticks_left,
        });
        ticket_manager.add_ticket(pos, ticket);
    }

    fn add_loaded_persistent_ticket(&mut self, persistent: PersistentChunkTicket) {
        match persistent.kind {
            PersistentChunkTicketKind::Portal => {
                self.add_loaded_portal_ticket(
                    ChunkPos::new(persistent.chunk_x, persistent.chunk_z),
                    persistent.ticks_left,
                );
            }
        }
    }

    fn add_loaded_portal_ticket(&mut self, pos: ChunkPos, ticks_left: i64) {
        if let Some(existing) = self.tickets.iter_mut().find(|entry| {
            entry.kind == TimedChunkTicketKind::Portal
                && entry.pos == pos
                && entry.ticket == portal_ticket()
        }) {
            existing.ticks_left = PORTAL_TICKET_TIMEOUT_TICKS;
            return;
        }

        self.tickets.push(TimedChunkTicket::portal(pos, ticks_left));
    }

    #[cfg(test)]
    #[must_use]
    const fn len(&self) -> usize {
        self.tickets.len()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct TimedChunkTicket {
    kind: TimedChunkTicketKind,
    pos: ChunkPos,
    ticket: ChunkTicket,
    ticks_left: i64,
}

impl TimedChunkTicket {
    const fn portal(pos: ChunkPos, ticks_left: i64) -> Self {
        Self {
            kind: TimedChunkTicketKind::Portal,
            pos,
            ticket: portal_ticket(),
            ticks_left,
        }
    }

    const fn to_persistent(self) -> Option<PersistentChunkTicket> {
        match self.kind {
            TimedChunkTicketKind::Portal => Some(PersistentChunkTicket {
                kind: PersistentChunkTicketKind::Portal,
                chunk_x: self.pos.0.x,
                chunk_z: self.pos.0.y,
                ticks_left: self.ticks_left,
            }),
            TimedChunkTicketKind::EnderPearl => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TimedChunkTicketKind {
    Portal,
    EnderPearl,
}

#[must_use]
const fn portal_ticket() -> ChunkTicket {
    ChunkTicket::simulated_full_chunks(PORTAL_TICKET_RADIUS)
}

#[must_use]
const fn ender_pearl_ticket() -> ChunkTicket {
    ChunkTicket::simulated_full_chunks(ENDER_PEARL_TICKET_RADIUS)
}

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

    /// Adds a ticket source. Multiple tickets can exist at the same position.
    pub fn add_ticket(&mut self, pos: ChunkPos, ticket: ChunkTicket) {
        self.tickets.entry(pos).or_default().push(ticket);
        self.dirty = true;
    }

    /// Removes one ticket matching `(pos, ticket)`. Returns true if found.
    pub fn remove_ticket(&mut self, pos: ChunkPos, ticket: ChunkTicket) -> bool {
        if let Some(tickets) = self.tickets.get_mut(&pos)
            && let Some(idx) = tickets.iter().position(|stored| *stored == ticket)
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

    /// Removes all tickets at position. Returns true if any were present.
    pub fn remove_all_tickets_at(&mut self, pos: ChunkPos) -> bool {
        let removed = self.tickets.remove(&pos).is_some();
        if removed {
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

    /// Iterator over the tickets currently held at `pos`.
    pub fn tickets_at(&self, pos: ChunkPos) -> impl Iterator<Item = ChunkTicket> + '_ {
        self.tickets
            .get(&pos)
            .into_iter()
            .flat_map(|tickets| tickets.iter().copied())
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

            let radius = i32::from(simulation_level.distance_to_block_ticking());
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
    fn portal_timed_ticket_loads_simulates_resets_and_expires_like_vanilla() {
        let mut manager = ChunkTicketManager::new();
        let mut timed_tickets = TimedChunkTickets::default();
        let center = ChunkPos::new(0, 0);

        timed_tickets.add_portal_ticket(&mut manager, center);
        timed_tickets.add_portal_ticket(&mut manager, center);
        manager.run_all_updates();

        assert_eq!(timed_tickets.len(), 1);
        assert_eq!(manager.ticket_count(), 1);
        assert!(is_full(
            manager.get_level(center).expect("ticket should load")
        ));
        assert!(is_entity_ticking(manager.get_simulation_level(center)));
        assert!(is_entity_ticking(
            manager.get_simulation_level(ChunkPos::new(1, 0))
        ));
        assert!(is_block_ticking(
            manager.get_simulation_level(ChunkPos::new(2, 0))
        ));
        assert!(!is_entity_ticking(
            manager.get_simulation_level(ChunkPos::new(2, 0))
        ));
        assert_eq!(manager.get_simulation_level(ChunkPos::new(3, 0)), None);
        assert!(is_full(
            manager
                .get_level(ChunkPos::new(3, 0))
                .expect("portal ticket should load the full outer ring")
        ));
        assert!(!manager.get_level(ChunkPos::new(4, 0)).is_some_and(is_full));

        for _ in 0..PORTAL_TICKET_TIMEOUT_TICKS {
            timed_tickets.tick(&mut manager, |_| true);
        }
        manager.run_all_updates();
        assert_eq!(manager.ticket_count(), 1);

        timed_tickets.tick(&mut manager, |_| true);
        manager.run_all_updates();
        assert_eq!(manager.ticket_count(), 0);
        assert_eq!(manager.get_level(center), None);
        assert_eq!(manager.get_simulation_level(center), None);
    }

    #[test]
    fn timed_ticket_does_not_age_until_chunk_can_expire() {
        let mut manager = ChunkTicketManager::new();
        let mut timed_tickets = TimedChunkTickets::default();
        let center = ChunkPos::new(0, 0);

        timed_tickets.add_portal_ticket(&mut manager, center);
        for _ in 0..=PORTAL_TICKET_TIMEOUT_TICKS {
            timed_tickets.tick(&mut manager, |_| false);
        }
        manager.run_all_updates();
        assert_eq!(manager.ticket_count(), 1);

        for _ in 0..=PORTAL_TICKET_TIMEOUT_TICKS {
            timed_tickets.tick(&mut manager, |_| true);
        }
        manager.run_all_updates();
        assert_eq!(manager.ticket_count(), 0);
    }

    #[test]
    fn persistent_portal_ticket_round_trips_remaining_ticks() {
        let persistent = PersistentChunkTickets {
            tickets: vec![PersistentChunkTicket {
                kind: PersistentChunkTicketKind::Portal,
                chunk_x: -4,
                chunk_z: 7,
                ticks_left: 123,
            }],
        };

        let timed_tickets = TimedChunkTickets::from_persistent(persistent);
        let restored = timed_tickets.to_persistent();

        assert_eq!(
            restored,
            PersistentChunkTickets {
                tickets: vec![PersistentChunkTicket {
                    kind: PersistentChunkTicketKind::Portal,
                    chunk_x: -4,
                    chunk_z: 7,
                    ticks_left: 123,
                }],
            }
        );
    }

    #[test]
    fn duplicate_persistent_portal_ticket_resets_timeout_like_vanilla_activation() {
        let persistent = PersistentChunkTickets {
            tickets: vec![
                PersistentChunkTicket {
                    kind: PersistentChunkTicketKind::Portal,
                    chunk_x: 2,
                    chunk_z: 3,
                    ticks_left: 10,
                },
                PersistentChunkTicket {
                    kind: PersistentChunkTicketKind::Portal,
                    chunk_x: 2,
                    chunk_z: 3,
                    ticks_left: 20,
                },
            ],
        };

        let restored = TimedChunkTickets::from_persistent(persistent).to_persistent();

        assert_eq!(
            restored,
            PersistentChunkTickets {
                tickets: vec![PersistentChunkTicket {
                    kind: PersistentChunkTicketKind::Portal,
                    chunk_x: 2,
                    chunk_z: 3,
                    ticks_left: PORTAL_TICKET_TIMEOUT_TICKS,
                }],
            }
        );
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
    fn simulated_full_ticket_propagates_simulation_only_to_block_ticking_area() {
        let mut manager = ChunkTicketManager::new();
        manager.add_ticket(ChunkPos::new(0, 0), ChunkTicket::simulated_full_chunks(1));
        manager.run_all_updates();

        assert!(is_block_ticking(
            manager.get_simulation_level(ChunkPos::new(0, 0))
        ));
        assert!(!is_entity_ticking(
            manager.get_simulation_level(ChunkPos::new(0, 0))
        ));
        assert_eq!(manager.get_simulation_level(ChunkPos::new(1, 1)), None);
        assert!(is_full(
            manager
                .get_level(ChunkPos::new(1, 1))
                .expect("ticket should load the full outer ring")
        ));
        assert_eq!(
            manager.get_level(ChunkPos::new(1, 1)),
            Some(ChunkTicketLevel::FULL_CHUNK)
        );
    }

    #[test]
    fn player_ticket_caps_entity_ticking_radius_to_load_radius() {
        let mut manager = ChunkTicketManager::new();
        let center = ChunkPos::new(0, 0);
        manager.add_ticket(center, ChunkTicket::player(1, 3));
        manager.run_all_updates();

        assert!(is_entity_ticking(manager.get_simulation_level(center)));
        assert!(is_entity_ticking(
            manager.get_simulation_level(ChunkPos::new(1, 0))
        ));
        assert!(is_block_ticking(
            manager.get_simulation_level(ChunkPos::new(2, 0))
        ));
        assert!(!is_entity_ticking(
            manager.get_simulation_level(ChunkPos::new(2, 0))
        ));
        assert_eq!(manager.get_simulation_level(ChunkPos::new(3, 0)), None);
    }

    #[test]
    fn full_chunk_status_matches_vanilla_ticket_thresholds() {
        assert_eq!(
            ChunkTicketLevel::ENTITY_TICKING_CHUNK.full_status(),
            FullChunkStatus::EntityTicking
        );
        assert_eq!(
            ChunkTicketLevel::BLOCK_TICKING_CHUNK.full_status(),
            FullChunkStatus::BlockTicking
        );
        assert_eq!(
            ChunkTicketLevel::FULL_CHUNK.full_status(),
            FullChunkStatus::Full
        );
        assert_eq!(full_status(None), FullChunkStatus::Inaccessible);
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
    fn non_full_ticket_level_maps_to_generation_status() {
        let ticket_level = ticket_level_for_status(ChunkStatus::StructureStarts);

        assert!(!ticket_level.is_full());
        assert!(generation_status(Some(ticket_level)).is_some_and(|status| {
            status >= ChunkStatus::StructureStarts && status != ChunkStatus::Full
        }));
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

    #[test]
    fn ender_pearl_timed_ticket_loads_simulates_resets_and_expires_like_vanilla() {
        let mut manager = ChunkTicketManager::new();
        let mut timed_tickets = TimedChunkTickets::default();
        let center = ChunkPos::new(0, 0);

        timed_tickets.add_ender_pearl_ticket(&mut manager, center);
        timed_tickets.add_ender_pearl_ticket(&mut manager, center);
        manager.run_all_updates();

        assert_eq!(timed_tickets.len(), 1);
        assert_eq!(manager.ticket_count(), 1);
        assert!(is_full(
            manager.get_level(center).expect("ticket should load")
        ));
        assert!(is_full(
            manager
                .get_level(ChunkPos::new(i32::from(ENDER_PEARL_TICKET_RADIUS), 0))
                .expect("ender pearl ticket should load the full outer ring")
        ));
        assert!(
            !manager
                .get_level(ChunkPos::new(i32::from(ENDER_PEARL_TICKET_RADIUS) + 1, 0))
                .is_some_and(is_full)
        );

        for _ in 0..ENDER_PEARL_TICKET_TIMEOUT_TICKS {
            timed_tickets.tick(&mut manager, |_| true);
        }
        manager.run_all_updates();
        assert_eq!(manager.ticket_count(), 1);

        timed_tickets.tick(&mut manager, |_| true);
        manager.run_all_updates();
        assert_eq!(manager.ticket_count(), 0);
        assert_eq!(manager.get_level(center), None);
        assert_eq!(manager.get_simulation_level(center), None);
    }

    #[test]
    fn ender_pearl_timed_ticket_does_not_age_until_chunk_can_expire() {
        let mut manager = ChunkTicketManager::new();
        let mut timed_tickets = TimedChunkTickets::default();
        let center = ChunkPos::new(0, 0);

        timed_tickets.add_ender_pearl_ticket(&mut manager, center);
        for _ in 0..=ENDER_PEARL_TICKET_TIMEOUT_TICKS {
            timed_tickets.tick(&mut manager, |_| false);
        }
        manager.run_all_updates();
        assert_eq!(manager.ticket_count(), 1);

        for _ in 0..=ENDER_PEARL_TICKET_TIMEOUT_TICKS {
            timed_tickets.tick(&mut manager, |_| true);
        }
        manager.run_all_updates();
        assert_eq!(manager.ticket_count(), 0);
    }

    #[test]
    fn ender_pearl_timed_ticket_propagates_like_a_manual_simulated_ticket() {
        let mut timed = ChunkTicketManager::new();
        let mut timed_tickets = TimedChunkTickets::default();
        let mut manual = ChunkTicketManager::new();
        let pos = ChunkPos::new(0, 0);
        timed_tickets.add_ender_pearl_ticket(&mut timed, pos);
        manual.add_ticket(
            pos,
            ChunkTicket::simulated_full_chunks(ENDER_PEARL_TICKET_RADIUS),
        );
        timed.run_all_updates();
        manual.run_all_updates();

        for dx in -i32::from(ENDER_PEARL_TICKET_RADIUS)..=i32::from(ENDER_PEARL_TICKET_RADIUS) {
            for dz in -i32::from(ENDER_PEARL_TICKET_RADIUS)..=i32::from(ENDER_PEARL_TICKET_RADIUS) {
                let p = ChunkPos::new(dx, dz);
                assert_eq!(timed.get_level(p), manual.get_level(p));
                assert_eq!(
                    timed.get_simulation_level(p),
                    manual.get_simulation_level(p)
                );
            }
        }
    }

    #[test]
    fn ender_pearl_timed_ticket_is_not_persisted() {
        let mut manager = ChunkTicketManager::new();
        let mut timed_tickets = TimedChunkTickets::default();

        timed_tickets.add_portal_ticket(&mut manager, ChunkPos::new(0, 0));
        timed_tickets.add_ender_pearl_ticket(&mut manager, ChunkPos::new(1, 0));

        assert_eq!(
            timed_tickets.to_persistent(),
            PersistentChunkTickets {
                tickets: vec![PersistentChunkTicket {
                    kind: PersistentChunkTicketKind::Portal,
                    chunk_x: 0,
                    chunk_z: 0,
                    ticks_left: PORTAL_TICKET_TIMEOUT_TICKS,
                }],
            }
        );
    }
}
