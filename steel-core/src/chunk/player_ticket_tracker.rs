//! Vanilla-style player ownership for loading and simulation tickets.

use std::collections::hash_map::Entry;

use rustc_hash::{FxHashMap, FxHashSet};
use steel_registry::vanilla_ticket_types::{PLAYER_LOADING, PLAYER_SIMULATION};
use steel_utils::ChunkPos;
use uuid::Uuid;

use super::{
    chunk_ticket::ChunkTicket,
    chunk_ticket_manager::ChunkTicketLevel,
    chunk_ticket_storage::{ChunkTicketStorage, SourceProjectionChanges},
};

#[must_use]
const fn player_loading_ticket() -> ChunkTicket {
    ChunkTicket::new(&PLAYER_LOADING, ChunkTicketLevel::ENTITY_TICKING_CHUNK)
}

#[must_use]
const fn player_simulation_ticket(simulation_distance: u8) -> ChunkTicket {
    ChunkTicket::new(
        &PLAYER_SIMULATION,
        ChunkTicketLevel::for_entity_ticking_radius(simulation_distance),
    )
}

/// Owns player positions and the logical coverage of Vanilla's player tickets.
///
/// Steel materializes the coverage immediately; generation admission remains
/// bounded by its generation scheduler instead of Vanilla's ticket dispatcher.
///
/// The caller holds this tracker and [`ChunkTicketStorage`] under the same
/// ingress lock, so one method call is the atomic player-source mutation.
#[derive(Debug)]
pub(crate) struct PlayerTicketTracker {
    players_by_pos: FxHashMap<ChunkPos, FxHashSet<Uuid>>,
    player_positions: FxHashMap<Uuid, ChunkPos>,
    loading_coverage: FxHashMap<ChunkPos, usize>,
    view_distance: u8,
    simulation_distance: u8,
}

impl PlayerTicketTracker {
    /// Creates an empty tracker with the current global player distances.
    #[must_use]
    pub(crate) fn new(view_distance: u8, simulation_distance: u8) -> Self {
        Self {
            players_by_pos: FxHashMap::default(),
            player_positions: FxHashMap::default(),
            loading_coverage: FxHashMap::default(),
            view_distance,
            simulation_distance,
        }
    }

    /// Adds a player, moving an existing UUID from its previous position.
    pub(crate) fn add_player(
        &mut self,
        storage: &mut ChunkTicketStorage,
        pos: ChunkPos,
        player_id: Uuid,
    ) -> SourceProjectionChanges {
        let mut changes = SourceProjectionChanges::default();

        let mut removed_center = None;
        if let Some(previous_pos) = self.player_positions.get(&player_id).copied() {
            if previous_pos == pos {
                return changes;
            }

            if self.remove_player_from_center(storage, previous_pos, player_id, &mut changes) {
                removed_center = Some(previous_pos);
            }
            self.player_positions.remove(&player_id);
        }

        let players = self.players_by_pos.entry(pos).or_default();
        let was_empty = players.is_empty();
        assert!(
            players.insert(player_id),
            "player position index disagrees with center membership"
        );
        assert!(
            self.player_positions.insert(player_id, pos).is_none(),
            "player position index retained a moved player"
        );

        if was_empty {
            changes
                .merge(storage.add_ticket(pos, player_simulation_ticket(self.simulation_distance)));
        }

        match (removed_center, was_empty) {
            (Some(previous), true) => {
                // Retain coverage in the overlap instead of tearing down and
                // recreating its tickets on every chunk crossing.
                Self::for_each_square_difference(previous, pos, self.view_distance, |covered| {
                    self.decrement_loading_coverage(storage, covered, &mut changes);
                });
                Self::for_each_square_difference(pos, previous, self.view_distance, |covered| {
                    self.increment_loading_coverage(storage, covered, &mut changes);
                });
            }
            (Some(previous), false) => {
                Self::for_each_square_position(previous, self.view_distance, |covered| {
                    self.decrement_loading_coverage(storage, covered, &mut changes);
                });
            }
            (None, true) => {
                Self::for_each_square_position(pos, self.view_distance, |covered| {
                    self.increment_loading_coverage(storage, covered, &mut changes);
                });
            }
            (None, false) => {}
        }

        changes
    }

    /// Removes a player only when the UUID still maps to the supplied position.
    pub(crate) fn remove_player(
        &mut self,
        storage: &mut ChunkTicketStorage,
        pos: ChunkPos,
        player_id: Uuid,
    ) -> SourceProjectionChanges {
        if self.player_positions.get(&player_id).copied() != Some(pos) {
            return SourceProjectionChanges::default();
        }

        let mut changes = SourceProjectionChanges::default();
        if self.remove_player_from_center(storage, pos, player_id, &mut changes) {
            Self::for_each_square_position(pos, self.view_distance, |covered| {
                self.decrement_loading_coverage(storage, covered, &mut changes);
            });
        }
        self.player_positions.remove(&player_id);
        changes
    }

    /// Replaces the simulation ticket at every occupied player center.
    pub(crate) fn set_simulation_distance(
        &mut self,
        storage: &mut ChunkTicketStorage,
        simulation_distance: u8,
    ) -> SourceProjectionChanges {
        if self.simulation_distance == simulation_distance {
            return SourceProjectionChanges::default();
        }

        let old_ticket = player_simulation_ticket(self.simulation_distance);
        let new_ticket = player_simulation_ticket(simulation_distance);
        let occupied_positions: Vec<_> = self.players_by_pos.keys().copied().collect();
        let mut changes = SourceProjectionChanges::default();

        for pos in occupied_positions {
            changes.merge(storage.remove_ticket(pos, old_ticket));
            changes.merge(storage.add_ticket(pos, new_ticket));
        }

        self.simulation_distance = simulation_distance;
        changes
    }

    /// Updates loading coverage for a changed global view distance.
    pub(crate) fn set_view_distance(
        &mut self,
        storage: &mut ChunkTicketStorage,
        view_distance: u8,
    ) -> SourceProjectionChanges {
        if self.view_distance == view_distance {
            return SourceProjectionChanges::default();
        }

        let old_distance = self.view_distance;
        let occupied_positions: Vec<_> = self.players_by_pos.keys().copied().collect();
        let mut changes = SourceProjectionChanges::default();

        for center in occupied_positions {
            if view_distance > old_distance {
                Self::for_each_square_ring(center, old_distance, view_distance, |covered_pos| {
                    self.increment_loading_coverage(storage, covered_pos, &mut changes);
                });
            } else {
                Self::for_each_square_ring(center, view_distance, old_distance, |covered_pos| {
                    self.decrement_loading_coverage(storage, covered_pos, &mut changes);
                });
            }
        }

        self.view_distance = view_distance;
        changes
    }

    fn remove_player_from_center(
        &mut self,
        storage: &mut ChunkTicketStorage,
        pos: ChunkPos,
        player_id: Uuid,
        changes: &mut SourceProjectionChanges,
    ) -> bool {
        let center_became_empty = {
            let Some(players) = self.players_by_pos.get_mut(&pos) else {
                panic!("player position index references a missing center");
            };
            assert!(
                players.remove(&player_id),
                "player position index disagrees with center membership"
            );
            players.is_empty()
        };

        if !center_became_empty {
            return false;
        }

        self.players_by_pos.remove(&pos);
        changes
            .merge(storage.remove_ticket(pos, player_simulation_ticket(self.simulation_distance)));
        true
    }

    fn increment_loading_coverage(
        &mut self,
        storage: &mut ChunkTicketStorage,
        pos: ChunkPos,
        changes: &mut SourceProjectionChanges,
    ) {
        match self.loading_coverage.entry(pos) {
            Entry::Vacant(entry) => {
                entry.insert(1);
                changes.merge(storage.add_ticket(pos, player_loading_ticket()));
            }
            Entry::Occupied(mut entry) => {
                let coverage = entry.get_mut();
                assert_ne!(
                    *coverage,
                    usize::MAX,
                    "player loading coverage refcount exhausted"
                );
                *coverage += 1;
            }
        }
    }

    fn decrement_loading_coverage(
        &mut self,
        storage: &mut ChunkTicketStorage,
        pos: ChunkPos,
        changes: &mut SourceProjectionChanges,
    ) {
        let Entry::Occupied(mut entry) = self.loading_coverage.entry(pos) else {
            panic!("player loading coverage is missing a covered position");
        };

        match *entry.get() {
            0 => panic!("player loading coverage contains a zero refcount"),
            1 => {
                entry.remove();
                changes.merge(storage.remove_ticket(pos, player_loading_ticket()));
            }
            _ => *entry.get_mut() -= 1,
        }
    }

    fn for_each_square_position(center: ChunkPos, distance: u8, mut visit: impl FnMut(ChunkPos)) {
        let distance = i32::from(distance);
        for offset_x in -distance..=distance {
            for offset_z in -distance..=distance {
                visit(ChunkPos::new(
                    center.0.x.wrapping_add(offset_x),
                    center.0.y.wrapping_add(offset_z),
                ));
            }
        }
    }

    fn for_each_square_difference(
        center: ChunkPos,
        other: ChunkPos,
        distance: u8,
        mut visit: impl FnMut(ChunkPos),
    ) {
        // Relative wrapping coordinates preserve the square iterator's behavior
        // at i32 boundaries. Widen before computing the overlap bounds.
        let delta_x = i64::from(other.0.x.wrapping_sub(center.0.x));
        let delta_z = i64::from(other.0.y.wrapping_sub(center.0.y));
        let radius = i64::from(distance);
        let overlap_min_z = (-radius).max(delta_z - radius);
        let overlap_max_z = radius.min(delta_z + radius);

        for offset_x in -radius..=radius {
            let mut visit_z = |offset_z: i64| {
                visit(ChunkPos::new(
                    center.0.x.wrapping_add(offset_x as i32),
                    center.0.y.wrapping_add(offset_z as i32),
                ));
            };
            if offset_x < delta_x - radius
                || offset_x > delta_x + radius
                || overlap_min_z > overlap_max_z
            {
                for offset_z in -radius..=radius {
                    visit_z(offset_z);
                }
                continue;
            }
            for offset_z in -radius..overlap_min_z {
                visit_z(offset_z);
            }
            for offset_z in overlap_max_z + 1..=radius {
                visit_z(offset_z);
            }
        }
    }

    fn for_each_square_ring(
        center: ChunkPos,
        inner_distance: u8,
        outer_distance: u8,
        mut visit: impl FnMut(ChunkPos),
    ) {
        debug_assert!(inner_distance < outer_distance);
        let inner_distance = i32::from(inner_distance);
        let outer_distance = i32::from(outer_distance);

        for offset_x in -outer_distance..=outer_distance {
            for offset_z in -outer_distance..=outer_distance {
                if offset_x.abs().max(offset_z.abs()) <= inner_distance {
                    continue;
                }

                visit(ChunkPos::new(
                    center.0.x.wrapping_add(offset_x),
                    center.0.y.wrapping_add(offset_z),
                ));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIRST_PLAYER: Uuid = Uuid::from_u128(1);
    const SECOND_PLAYER: Uuid = Uuid::from_u128(2);

    #[test]
    fn movement_changes_only_leaving_and_entering_loading_strips() {
        let mut storage = ChunkTicketStorage::new();
        let mut tracker = PlayerTicketTracker::new(128, 128);
        let _ = tracker.add_player(&mut storage, ChunkPos::new(0, 0), FIRST_PLAYER);

        let changes = tracker.add_player(&mut storage, ChunkPos::new(1, 0), FIRST_PLAYER);

        assert_eq!(changes.load_positions.len(), 2 * 257);
        assert!(
            changes
                .load_positions
                .iter()
                .all(|pos| { pos.0.x == -128 || pos.0.x == 129 })
        );
        assert_eq!(tracker.loading_coverage.len(), 257 * 257);
    }

    #[test]
    fn moving_shared_centers_and_wrapping_teleports_match_remove_then_add() {
        for distance in [0, 2, 32, 128] {
            let mut storage = ChunkTicketStorage::new();
            let mut tracker = PlayerTicketTracker::new(distance, distance);
            let mut reference_storage = ChunkTicketStorage::new();
            let mut reference = PlayerTicketTracker::new(distance, distance);
            let moves = [
                (FIRST_PLAYER, ChunkPos::new(0, 0)),
                (SECOND_PLAYER, ChunkPos::new(0, 0)),
                (FIRST_PLAYER, ChunkPos::new(1, -1)),
                (SECOND_PLAYER, ChunkPos::new(1, -1)),
                (FIRST_PLAYER, ChunkPos::new(-2, 3)),
                (FIRST_PLAYER, ChunkPos::new(i32::MAX, i32::MIN)),
                (FIRST_PLAYER, ChunkPos::new(i32::MIN, i32::MAX)),
                (SECOND_PLAYER, ChunkPos::new(i32::MIN, i32::MAX)),
                (SECOND_PLAYER, ChunkPos::new(i32::MIN + 1, i32::MAX - 1)),
            ];
            for (player, pos) in moves {
                if let Some(previous) = reference.player_positions.get(&player).copied() {
                    let _ = reference.remove_player(&mut reference_storage, previous, player);
                }
                let _ = reference.add_player(&mut reference_storage, pos, player);
                let changes = tracker.add_player(&mut storage, pos, player);

                assert_eq!(tracker.player_positions, reference.player_positions);
                assert_eq!(tracker.players_by_pos, reference.players_by_pos);
                assert_eq!(tracker.loading_coverage, reference.loading_coverage);
                for &covered in reference
                    .loading_coverage
                    .keys()
                    .chain(&changes.load_positions)
                {
                    assert_eq!(
                        storage.load_source_level(covered),
                        reference_storage.load_source_level(covered)
                    );
                }
                for &center in reference
                    .players_by_pos
                    .keys()
                    .chain(&changes.simulation_positions)
                {
                    assert_eq!(
                        storage.simulation_source_level(center),
                        reference_storage.simulation_source_level(center)
                    );
                }
            }
        }
    }

    #[test]
    fn colocated_players_share_center_and_loading_tickets() {
        let center = ChunkPos::new(4, -2);
        let mut storage = ChunkTicketStorage::new();
        let mut tracker = PlayerTicketTracker::new(1, 3);

        let first_add = tracker.add_player(&mut storage, center, FIRST_PLAYER);
        let second_add = tracker.add_player(&mut storage, center, SECOND_PLAYER);
        let first_remove = tracker.remove_player(&mut storage, center, FIRST_PLAYER);

        assert_eq!(first_add.load_positions.len(), 9);
        assert_eq!(first_add.simulation_positions, vec![center]);
        assert_eq!(second_add, SourceProjectionChanges::default());
        assert_eq!(first_remove, SourceProjectionChanges::default());
        assert_eq!(
            tracker.players_by_pos.get(&center).map(FxHashSet::len),
            Some(1)
        );

        let last_remove = tracker.remove_player(&mut storage, center, SECOND_PLAYER);
        assert_eq!(last_remove.load_positions.len(), 9);
        assert_eq!(last_remove.simulation_positions, vec![center]);
        assert!(tracker.players_by_pos.is_empty());
        assert!(tracker.loading_coverage.is_empty());
        assert_eq!(storage.load_source_level(center), None);
        assert_eq!(storage.simulation_source_level(center), None);
    }

    #[test]
    fn overlapping_views_keep_loading_tickets_until_last_center_leaves() {
        let first_center = ChunkPos::new(0, 0);
        let second_center = ChunkPos::new(1, 0);
        let shared_pos = ChunkPos::new(0, 1);
        let first_only_pos = ChunkPos::new(-1, 0);
        let mut storage = ChunkTicketStorage::new();
        let mut tracker = PlayerTicketTracker::new(1, 2);

        let _ = tracker.add_player(&mut storage, first_center, FIRST_PLAYER);
        let second_add = tracker.add_player(&mut storage, second_center, SECOND_PLAYER);

        assert_eq!(second_add.load_positions.len(), 3);
        assert_eq!(tracker.loading_coverage.get(&shared_pos), Some(&2));

        let first_remove = tracker.remove_player(&mut storage, first_center, FIRST_PLAYER);
        assert_eq!(first_remove.load_positions.len(), 3);
        assert_eq!(tracker.loading_coverage.get(&shared_pos), Some(&1));
        assert_eq!(
            storage.load_source_level(shared_pos),
            Some(ChunkTicketLevel::ENTITY_TICKING_CHUNK)
        );
        assert_eq!(storage.load_source_level(first_only_pos), None);
    }

    #[test]
    fn movement_is_atomic_and_stale_removal_cannot_remove_new_position() {
        let old_pos = ChunkPos::new(-7, 5);
        let new_pos = ChunkPos::new(9, 3);
        let mut storage = ChunkTicketStorage::new();
        let mut tracker = PlayerTicketTracker::new(0, 4);

        let _ = tracker.add_player(&mut storage, old_pos, FIRST_PLAYER);
        let movement = tracker.add_player(&mut storage, new_pos, FIRST_PLAYER);
        let duplicate_add = tracker.add_player(&mut storage, new_pos, FIRST_PLAYER);
        let stale_remove = tracker.remove_player(&mut storage, old_pos, FIRST_PLAYER);

        assert_eq!(movement.load_positions, vec![old_pos, new_pos]);
        assert_eq!(movement.simulation_positions, vec![old_pos, new_pos]);
        assert_eq!(duplicate_add, SourceProjectionChanges::default());
        assert_eq!(stale_remove, SourceProjectionChanges::default());
        assert_eq!(tracker.player_positions.get(&FIRST_PLAYER), Some(&new_pos));
        assert_eq!(storage.load_source_level(old_pos), None);
        assert_eq!(storage.simulation_source_level(old_pos), None);
        assert_eq!(
            storage.load_source_level(new_pos),
            Some(ChunkTicketLevel::ENTITY_TICKING_CHUNK)
        );
        assert_eq!(
            storage.simulation_source_level(new_pos),
            Some(ChunkTicketLevel::for_entity_ticking_radius(4))
        );
    }

    #[test]
    fn distance_changes_replace_simulation_tickets_and_adjust_only_view_ring() {
        let center = ChunkPos::new(2, 8);
        let mut storage = ChunkTicketStorage::new();
        let mut tracker = PlayerTicketTracker::new(0, 2);
        let _ = tracker.add_player(&mut storage, center, FIRST_PLAYER);

        let simulation_changes = tracker.set_simulation_distance(&mut storage, 5);
        assert_eq!(simulation_changes.load_positions, Vec::new());
        assert_eq!(
            simulation_changes.simulation_positions,
            vec![center, center]
        );
        assert_eq!(
            storage.simulation_source_level(center),
            Some(ChunkTicketLevel::for_entity_ticking_radius(5))
        );

        let expanded = tracker.set_view_distance(&mut storage, 2);
        assert_eq!(expanded.load_positions.len(), 24);
        assert_eq!(tracker.loading_coverage.len(), 25);

        let shrunk = tracker.set_view_distance(&mut storage, 1);
        assert_eq!(shrunk.load_positions.len(), 16);
        assert_eq!(tracker.loading_coverage.len(), 9);
        assert_eq!(storage.load_source_level(ChunkPos::new(4, 8)), None);
        assert_eq!(
            storage.load_source_level(ChunkPos::new(3, 9)),
            Some(ChunkTicketLevel::ENTITY_TICKING_CHUNK)
        );
    }
}
