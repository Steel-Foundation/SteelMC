//! Chunk ticket management for tracking load levels and propagation.
#![expect(missing_docs, reason = "internal module; items are self-explanatory")]

use std::mem;

use rustc_hash::{FxBuildHasher, FxHashMap};
use steel_utils::ChunkPos;

use crate::chunk::{
    chunk_pyramid::GENERATION_PYRAMID, chunk_ticket_storage::SourceLevelUpdate, status::ChunkStatus,
};

/// The maximum supported view distance for players.
pub const MAX_SUPPORTED_VIEW_DISTANCE: u8 = 128;
const FULL_CHUNK_LEVEL_RAW: u8 = MAX_SUPPORTED_VIEW_DISTANCE + 2;
const RADIUS_AROUND_FULL_CHUNK: u8 = GENERATION_PYRAMID
    .get_step_to(ChunkStatus::Full)
    .accumulated_dependencies
    .get_radius_of(ChunkStatus::Empty) as u8;
const MAX_LEVEL_RAW: u8 = FULL_CHUNK_LEVEL_RAW + RADIUS_AROUND_FULL_CHUNK;

/// A chunk ticket level.
///
/// Lower levels are stronger tickets. `FULL_CHUNK_LEVEL_RAW` is the boundary
/// where a propagated ticket can still make a chunk full; larger levels only
/// keep dependency chunks loaded far enough for generation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ChunkTicketLevel(u8);

impl ChunkTicketLevel {
    /// The strongest possible ticket level.
    pub const STRONGEST: Self = Self(0);
    /// The weakest level whose full chunk may tick entities.
    pub const ENTITY_TICKING_CHUNK: Self = Self(MAX_SUPPORTED_VIEW_DISTANCE);
    /// The weakest level whose full chunk may tick blocks.
    pub const BLOCK_TICKING_CHUNK: Self = Self(MAX_SUPPORTED_VIEW_DISTANCE + 1);
    /// The weakest level that still permits a full chunk.
    pub const FULL_CHUNK: Self = Self(FULL_CHUNK_LEVEL_RAW);
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
        Self(FULL_CHUNK_LEVEL_RAW.saturating_sub(radius))
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

    /// Creates a loading-only player ticket with Vanilla's two-chunk loading moat.
    ///
    /// Loading is entity-ticking through `view_distance`, block-ticking one
    /// chunk farther, and full one chunk beyond that.
    #[must_use]
    pub const fn player_loading(view_distance: u8) -> Self {
        Self::loading(ChunkTicketLevel::for_entity_ticking_radius(view_distance))
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
                let distance = (level.raw() - FULL_CHUNK_LEVEL_RAW) as usize;
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
            FULL_CHUNK_LEVEL_RAW
                + GENERATION_PYRAMID
                    .get_step_to(ChunkStatus::Full)
                    .accumulated_dependencies
                    .get_radius_of(status) as u8,
        )
    }
}

/// A load level change for a chunk position.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LoadLevelChange {
    pub pos: ChunkPos,
    /// `Some(level)` if level changed or added, `None` if removed.
    pub new_level: Option<ChunkTicketLevel>,
}

/// Load-level propagation derived from authoritative ticket sources.
/// Lower levels have higher priority.
#[derive(Debug)]
pub struct LoadTicketManager {
    source_levels: FxHashMap<ChunkPos, ChunkTicketLevel>,
    levels: FxHashMap<ChunkPos, ChunkTicketLevel>,
    dirty: bool,
    /// Tracks changes from the last `run_all_updates()` call.
    changes: Vec<LoadLevelChange>,
}

impl Default for LoadTicketManager {
    fn default() -> Self {
        Self::new()
    }
}

impl LoadTicketManager {
    #[must_use]
    pub fn new() -> Self {
        Self {
            source_levels: FxHashMap::default(),
            levels: FxHashMap::default(),
            dirty: false,
            changes: Vec::new(),
        }
    }

    /// Applies an authoritative load source level update.
    pub(crate) fn apply_source_update(&mut self, update: SourceLevelUpdate) {
        let old_level = self.source_levels.get(&update.pos).copied();
        if old_level == update.level {
            return;
        }

        match update.level {
            Some(level) => {
                self.source_levels.insert(update.pos, level);
            }
            None => {
                self.source_levels.remove(&update.pos);
            }
        }
        self.dirty = true;
    }

    /// Applies a batch of authoritative load source level updates.
    pub(crate) fn apply_source_updates(
        &mut self,
        updates: impl IntoIterator<Item = SourceLevelUpdate>,
    ) {
        for update in updates {
            self.apply_source_update(update);
        }
    }

    /// Propagates all tickets. Only runs if dirty.
    /// Returns a slice of changes (added/updated/removed levels).
    pub fn run_all_updates(&mut self) -> &[LoadLevelChange] {
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

        for (&source_pos, &source_level) in &self.source_levels {
            Self::propagate_source(&mut self.levels, source_pos, source_level);
        }

        // Find changed/added levels
        for (&pos, &new_level) in &self.levels {
            match old_levels.get(&pos) {
                Some(&old_level) if old_level == new_level => {} // No change
                _ => self.changes.push(LoadLevelChange {
                    pos,
                    new_level: Some(new_level),
                }),
            }
        }

        // Find removed levels
        for &pos in old_levels.keys() {
            if !self.levels.contains_key(&pos) {
                self.changes.push(LoadLevelChange {
                    pos,
                    new_level: None,
                });
            }
        }

        &self.changes
    }

    fn propagate_source(
        levels: &mut FxHashMap<ChunkPos, ChunkTicketLevel>,
        source_pos: ChunkPos,
        source_level: ChunkTicketLevel,
    ) {
        let radius = i32::from(source_level.distance_to_max());
        let source_x = source_pos.0.x;
        let source_z = source_pos.0.y;

        for dz in -radius..=radius {
            for dx in -radius..=radius {
                let distance = dx.abs().max(dz.abs()) as u8;
                let Some(level) = source_level.with_distance(distance) else {
                    continue;
                };

                let pos = ChunkPos::new(source_x + dx, source_z + dz);
                levels
                    .entry(pos)
                    .and_modify(|current| *current = (*current).min(level))
                    .or_insert(level);
            }
        }
    }

    /// Takes the change list produced by the last propagation pass.
    pub(crate) fn take_changes(&mut self) -> Vec<LoadLevelChange> {
        mem::take(&mut self.changes)
    }

    /// Returns a drained change buffer for reuse by the next propagation pass.
    pub(crate) fn recycle_changes(&mut self, mut changes: Vec<LoadLevelChange>) {
        debug_assert_eq!(self.changes, []);
        changes.clear();
        self.changes = changes;
    }

    /// Returns the propagated level at position. Call `run_all_updates()` first.
    #[must_use]
    pub fn get_level(&self, pos: ChunkPos) -> Option<ChunkTicketLevel> {
        self.levels.get(&pos).copied()
    }

    #[cfg(test)]
    #[must_use]
    const fn is_dirty(&self) -> bool {
        self.dirty
    }

    #[expect(dead_code, reason = "utility method for tests and future use")]
    fn clear(&mut self) {
        self.source_levels.clear();
        self.levels.clear();
        self.dirty = false;
        self.changes.clear();
    }

    pub fn iter_levels(&self) -> impl Iterator<Item = (ChunkPos, ChunkTicketLevel)> + '_ {
        self.levels.iter().map(|(&pos, &level)| (pos, level))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn source(pos: ChunkPos, level: Option<ChunkTicketLevel>) -> SourceLevelUpdate {
        SourceLevelUpdate { pos, level }
    }

    #[test]
    fn source_updates_are_deferred_and_idempotent() {
        let mut manager = LoadTicketManager::new();
        let center = ChunkPos::new(0, 0);
        let level = ChunkTicketLevel::FULL_CHUNK;

        manager.apply_source_update(source(center, Some(level)));

        assert!(manager.is_dirty());
        assert_eq!(manager.get_level(center), None);

        manager.run_all_updates();

        assert!(!manager.is_dirty());
        assert_eq!(manager.get_level(center), Some(level));

        manager.apply_source_update(source(center, Some(level)));

        assert!(!manager.is_dirty());
    }

    #[test]
    fn source_level_propagates_by_chebyshev_distance() {
        let mut manager = LoadTicketManager::new();
        let center = ChunkPos::new(0, 0);
        manager.apply_source_update(source(center, Some(ChunkTicketLevel::FULL_CHUNK)));
        manager.run_all_updates();

        assert_eq!(
            manager.get_level(center),
            Some(ChunkTicketLevel::FULL_CHUNK)
        );
        assert_eq!(
            manager.get_level(ChunkPos::new(-1, -1)),
            ChunkTicketLevel::new(ChunkTicketLevel::FULL_CHUNK.raw() + 1)
        );
        assert_eq!(
            manager.get_level(ChunkPos::new(2, 0)),
            ChunkTicketLevel::new(ChunkTicketLevel::FULL_CHUNK.raw() + 2)
        );
    }

    #[test]
    fn overlapping_sources_keep_the_strongest_propagated_level() {
        let mut manager = LoadTicketManager::new();
        let source_level = ChunkTicketLevel::FULL_CHUNK;
        manager.apply_source_updates([
            source(ChunkPos::new(0, 0), Some(source_level)),
            source(ChunkPos::new(3, 0), Some(source_level)),
        ]);
        manager.run_all_updates();

        let adjacent_level =
            ChunkTicketLevel::new(source_level.raw() + 1).expect("adjacent level is valid");
        assert_eq!(manager.get_level(ChunkPos::new(1, 0)), Some(adjacent_level));
        assert_eq!(manager.get_level(ChunkPos::new(2, 0)), Some(adjacent_level));
    }

    #[test]
    fn changing_a_source_level_rebuilds_its_propagation() {
        let mut manager = LoadTicketManager::new();
        let center = ChunkPos::new(0, 0);
        manager.apply_source_update(source(center, Some(ChunkTicketLevel::FULL_CHUNK)));
        manager.run_all_updates();

        manager.apply_source_update(source(center, Some(ChunkTicketLevel::ENTITY_TICKING_CHUNK)));
        manager.run_all_updates();

        assert_eq!(
            manager.get_level(center),
            Some(ChunkTicketLevel::ENTITY_TICKING_CHUNK)
        );
        assert_eq!(
            manager.get_level(ChunkPos::new(2, 0)),
            Some(ChunkTicketLevel::FULL_CHUNK)
        );
    }

    #[test]
    fn removing_a_source_reveals_overlapping_propagation() {
        let mut manager = LoadTicketManager::new();
        let first = ChunkPos::new(0, 0);
        let second = ChunkPos::new(5, 0);
        let source_level = ChunkTicketLevel::FULL_CHUNK;
        manager.apply_source_updates([
            source(first, Some(source_level)),
            source(second, Some(source_level)),
        ]);
        manager.run_all_updates();

        manager.apply_source_update(source(first, None));
        manager.run_all_updates();

        assert_eq!(
            manager.get_level(first),
            ChunkTicketLevel::new(source_level.raw() + 5)
        );
        assert_eq!(manager.get_level(second), Some(source_level));
    }

    #[test]
    fn removing_the_last_source_reports_removed_levels() {
        let mut manager = LoadTicketManager::new();
        let center = ChunkPos::new(0, 0);
        manager.apply_source_update(source(center, Some(ChunkTicketLevel::MAX)));
        manager.run_all_updates();
        manager.take_changes();

        manager.apply_source_update(source(center, None));
        manager.run_all_updates();

        assert_eq!(
            manager.take_changes(),
            vec![LoadLevelChange {
                pos: center,
                new_level: None,
            }]
        );
        assert_eq!(manager.get_level(center), None);
    }

    #[test]
    fn no_recalculation_occurs_without_source_changes() {
        let mut manager = LoadTicketManager::new();
        manager.apply_source_update(source(
            ChunkPos::new(0, 0),
            Some(ChunkTicketLevel::FULL_CHUNK),
        ));
        manager.run_all_updates();

        assert!(!manager.is_dirty());
        assert_eq!(manager.run_all_updates(), []);
        assert!(!manager.is_dirty());
    }

    #[test]
    fn full_chunk_source_keeps_the_loading_moat() {
        let mut manager = LoadTicketManager::new();
        let center = ChunkPos::new(0, 0);
        let source_level = ChunkTicket::player_loading(1).load_level();
        manager.apply_source_update(source(center, Some(source_level)));
        manager.run_all_updates();

        assert!(is_entity_ticking(manager.get_level(center)));
        assert_eq!(
            manager.get_level(ChunkPos::new(1, 0)),
            Some(ChunkTicketLevel::ENTITY_TICKING_CHUNK)
        );
        assert_eq!(
            manager.get_level(ChunkPos::new(2, 0)),
            Some(ChunkTicketLevel::BLOCK_TICKING_CHUNK)
        );
        assert_eq!(
            manager.get_level(ChunkPos::new(3, 0)),
            Some(ChunkTicketLevel::FULL_CHUNK)
        );
    }

    #[test]
    fn maximum_player_view_distance_fits_ticket_level() {
        let ticket = ChunkTicket::player_loading(MAX_SUPPORTED_VIEW_DISTANCE);

        assert_eq!(ChunkTicketLevel::ENTITY_TICKING_CHUNK.raw(), 128);
        assert_eq!(ChunkTicketLevel::BLOCK_TICKING_CHUNK.raw(), 129);
        assert_eq!(ChunkTicketLevel::FULL_CHUNK.raw(), 130);
        assert_eq!(ticket.load_level().raw(), 0);
        assert_eq!(ticket.simulation_level(), None);
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
}
