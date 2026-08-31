//! Authoritative chunk ticket source storage.
use rustc_hash::{FxHashMap, FxHashSet};
use serde::{Deserialize, Serialize};
use smallvec::SmallVec;
use steel_utils::ChunkPos;
use uuid::Uuid;

use super::chunk_ticket_manager::{ChunkTicket, ChunkTicketLevel};

pub(crate) const PORTAL_TICKET_RADIUS: u8 = 3;
const PORTAL_TICKET_TIMEOUT_TICKS: i64 = 300;
pub(crate) const ENDER_PEARL_TICKET_TIMEOUT_TICKS: u32 = 40;
const ENDER_PEARL_TICKET_RADIUS: u8 = 2;

type StoredTickets = SmallVec<[StoredChunkTicket; 4]>;

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

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum PersistentChunkTicketKind {
    Portal,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct StoredChunkTicket {
    ticket: ChunkTicket,
    lifetime: TicketLifetime,
}

impl StoredChunkTicket {
    const fn untimed(ticket: ChunkTicket) -> Self {
        Self {
            ticket,
            lifetime: TicketLifetime::Untimed,
        }
    }

    const fn timed(
        ticket: ChunkTicket,
        kind: TimedChunkTicketKind,
        ticks_left: i64,
        generation: u64,
    ) -> Self {
        Self {
            ticket,
            lifetime: TicketLifetime::Timed {
                kind,
                ticks_left,
                generation,
            },
        }
    }

    const fn to_persistent(self, pos: ChunkPos) -> Option<PersistentChunkTicket> {
        match self.lifetime {
            TicketLifetime::Timed {
                kind: TimedChunkTicketKind::Portal,
                ticks_left,
                ..
            } => Some(PersistentChunkTicket {
                kind: PersistentChunkTicketKind::Portal,
                chunk_x: pos.0.x,
                chunk_z: pos.0.y,
                ticks_left,
            }),
            TicketLifetime::Untimed
            | TicketLifetime::Timed {
                kind: TimedChunkTicketKind::EnderPearl,
                ..
            } => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TicketLifetime {
    Untimed,
    Timed {
        kind: TimedChunkTicketKind,
        ticks_left: i64,
        generation: u64,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TimedChunkTicketKind {
    Portal,
    EnderPearl,
}

/// One timed entry observed for the current world-tick expiration pass.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct TimedTicketExpiration {
    pos: ChunkPos,
    kind: TimedChunkTicketKind,
    generation: u64,
}

impl TimedTicketExpiration {
    #[must_use]
    pub(crate) const fn pos(self) -> ChunkPos {
        self.pos
    }

    #[must_use]
    pub(crate) const fn can_expire_if_unloaded(self) -> bool {
        matches!(self.kind, TimedChunkTicketKind::EnderPearl)
    }
}

#[must_use]
const fn portal_ticket() -> ChunkTicket {
    ChunkTicket::simulated_full_chunks(PORTAL_TICKET_RADIUS)
}

#[must_use]
const fn ender_pearl_ticket() -> ChunkTicket {
    ChunkTicket::simulated_full_chunks(ENDER_PEARL_TICKET_RADIUS)
}

/// One ticket source mutation submitted by gameplay.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ChunkTicketOperation {
    Add {
        pos: ChunkPos,
        ticket: ChunkTicket,
    },
    /// Removes one matching untimed ticket submitted through `Add`.
    ///
    /// Timed portal and ender-pearl entries have separate ownership and expire
    /// only through the timed-ticket path.
    Remove {
        pos: ChunkPos,
        ticket: ChunkTicket,
    },
    AddPlayer {
        pos: ChunkPos,
        player_id: Uuid,
        load_level: ChunkTicketLevel,
    },
    RemovePlayer {
        pos: ChunkPos,
        player_id: Uuid,
    },
}

impl ChunkTicketOperation {
    #[must_use]
    pub(crate) const fn affects_simulation(self) -> bool {
        match self {
            Self::Add { ticket, .. } | Self::Remove { ticket, .. } => {
                ticket.simulation_level().is_some()
            }
            Self::AddPlayer { .. } | Self::RemovePlayer { .. } => true,
        }
    }
}

/// One materialized source level for a propagation domain.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct SourceLevelUpdate {
    pub(crate) pos: ChunkPos,
    pub(crate) level: Option<ChunkTicketLevel>,
}

/// Source positions dirtied by one or more storage mutations.
///
/// Domain flags describe submitted work, including idempotent or missing
/// removals. Dirty positions are only present when stored source data changed
/// and may repeat when multiple operations are combined.
#[must_use = "dirty source positions must be forwarded to the propagation domains"]
#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct SourceProjectionChanges {
    pub(crate) load_positions: SmallVec<[ChunkPos; 2]>,
    pub(crate) simulation_positions: SmallVec<[ChunkPos; 2]>,
    pub(crate) load_domain_affected: bool,
    pub(crate) simulation_domain_affected: bool,
}

impl SourceProjectionChanges {
    fn for_operation(operation: ChunkTicketOperation) -> Self {
        Self {
            load_domain_affected: true,
            simulation_domain_affected: operation.affects_simulation(),
            ..Self::default()
        }
    }

    fn mark_ticket_membership(&mut self, pos: ChunkPos, ticket: ChunkTicket) {
        self.load_positions.push(pos);
        if ticket.simulation_level().is_some() {
            self.simulation_positions.push(pos);
        }
    }

    fn append(&mut self, mut other: Self) {
        self.load_positions.append(&mut other.load_positions);
        self.simulation_positions
            .append(&mut other.simulation_positions);
        self.load_domain_affected |= other.load_domain_affected;
        self.simulation_domain_affected |= other.simulation_domain_affected;
    }
}

/// Owns ticket multiplicity, timed tickets, and player source membership once.
#[derive(Debug)]
pub(crate) struct ChunkTicketStorage {
    tickets: FxHashMap<ChunkPos, StoredTickets>,
    timed_positions: FxHashSet<ChunkPos>,
    players_by_pos: FxHashMap<ChunkPos, FxHashMap<Uuid, ChunkTicketLevel>>,
    player_positions: FxHashMap<Uuid, ChunkPos>,
    last_projected_simulation_distance: u8,
    timed_generation: u64,
}

impl ChunkTicketStorage {
    #[must_use]
    pub(crate) fn new(simulation_distance: u8) -> Self {
        Self {
            tickets: FxHashMap::default(),
            timed_positions: FxHashSet::default(),
            players_by_pos: FxHashMap::default(),
            player_positions: FxHashMap::default(),
            last_projected_simulation_distance: simulation_distance,
            timed_generation: 0,
        }
    }

    /// Restores Vanilla's persistent timed ticket sources.
    #[must_use]
    pub(crate) fn from_persistent(
        persistent: PersistentChunkTickets,
        simulation_distance: u8,
    ) -> Self {
        let mut storage = Self::new(simulation_distance);
        for ticket in persistent.tickets {
            storage.add_loaded_persistent_ticket(ticket);
        }
        storage
    }

    /// Applies one source mutation exactly once.
    pub(crate) fn apply(&mut self, operation: ChunkTicketOperation) -> SourceProjectionChanges {
        let mut changes = SourceProjectionChanges::for_operation(operation);
        match operation {
            ChunkTicketOperation::Add { pos, ticket } => {
                self.tickets
                    .entry(pos)
                    .or_default()
                    .push(StoredChunkTicket::untimed(ticket));
                changes.mark_ticket_membership(pos, ticket);
            }
            ChunkTicketOperation::Remove { pos, ticket } => {
                if self.remove_untimed_ticket(pos, ticket) {
                    changes.mark_ticket_membership(pos, ticket);
                }
            }
            ChunkTicketOperation::AddPlayer {
                pos,
                player_id,
                load_level,
            } => self.add_player(pos, player_id, load_level, &mut changes),
            ChunkTicketOperation::RemovePlayer { pos, player_id } => {
                self.remove_player(pos, player_id, &mut changes);
            }
        }
        changes
    }

    /// Applies source mutations in iterator order without duplicating ownership.
    pub(crate) fn apply_operations(
        &mut self,
        operations: impl IntoIterator<Item = ChunkTicketOperation>,
    ) -> SourceProjectionChanges {
        let mut combined = SourceProjectionChanges::default();
        for operation in operations {
            combined.append(self.apply(operation));
        }
        combined
    }

    /// Returns the effective loading source at `pos`.
    #[must_use]
    pub(crate) fn load_source_level(&self, pos: ChunkPos) -> Option<ChunkTicketLevel> {
        let ticket_level = self
            .tickets
            .get(&pos)
            .and_then(|tickets| tickets.iter().map(|entry| entry.ticket.load_level()).min());
        let player_level = self
            .players_by_pos
            .get(&pos)
            .and_then(|players| players.values().copied().min());

        ticket_level.into_iter().chain(player_level).min()
    }

    /// Returns the effective simulation source at `pos`.
    #[must_use]
    pub(crate) fn simulation_source_level(&self, pos: ChunkPos) -> Option<ChunkTicketLevel> {
        let ticket_level = self.tickets.get(&pos).and_then(|tickets| {
            tickets
                .iter()
                .filter_map(|entry| entry.ticket.simulation_level())
                .filter(|level| level.is_block_ticking())
                .min()
        });
        let player_level = self.players_by_pos.contains_key(&pos).then(|| {
            ChunkTicketLevel::for_entity_ticking_radius(self.last_projected_simulation_distance)
        });

        ticket_level.into_iter().chain(player_level).min()
    }

    #[must_use]
    pub(crate) fn load_source_update(&self, pos: ChunkPos) -> SourceLevelUpdate {
        SourceLevelUpdate {
            pos,
            level: self.load_source_level(pos),
        }
    }

    #[must_use]
    pub(crate) fn simulation_source_update(&self, pos: ChunkPos) -> SourceLevelUpdate {
        SourceLevelUpdate {
            pos,
            level: self.simulation_source_level(pos),
        }
    }

    /// Enumerates the initial loading sources for propagation tracker seeding.
    #[must_use]
    pub(crate) fn initial_load_sources(&self) -> Vec<SourceLevelUpdate> {
        self.initial_sources(Self::load_source_update)
    }

    /// Enumerates the initial simulation sources for propagation tracker seeding.
    #[must_use]
    pub(crate) fn initial_simulation_sources(&self) -> Vec<SourceLevelUpdate> {
        self.initial_sources(Self::simulation_source_update)
    }

    fn initial_sources(
        &self,
        source_update: fn(&Self, ChunkPos) -> SourceLevelUpdate,
    ) -> Vec<SourceLevelUpdate> {
        let mut positions = FxHashSet::default();
        positions.extend(self.tickets.keys().copied());
        positions.extend(self.players_by_pos.keys().copied());

        let mut sources: Vec<_> = positions
            .into_iter()
            .map(|pos| source_update(self, pos))
            .filter(|update| update.level.is_some())
            .collect();
        sources.sort_unstable_by_key(|update| (update.pos.0.x, update.pos.0.y));
        sources
    }

    /// Adds or refreshes Vanilla's post-portal ticket.
    pub(crate) fn add_or_refresh_portal_ticket(
        &mut self,
        pos: ChunkPos,
    ) -> SourceProjectionChanges {
        self.add_or_refresh_timed_ticket(
            pos,
            TimedChunkTicketKind::Portal,
            portal_ticket(),
            PORTAL_TICKET_TIMEOUT_TICKS,
        )
    }

    /// Adds or refreshes Vanilla's in-flight ender pearl ticket.
    pub(crate) fn add_or_refresh_ender_pearl_ticket(
        &mut self,
        pos: ChunkPos,
    ) -> SourceProjectionChanges {
        self.add_or_refresh_timed_ticket(
            pos,
            TimedChunkTicketKind::EnderPearl,
            ender_pearl_ticket(),
            i64::from(ENDER_PEARL_TICKET_TIMEOUT_TICKS),
        )
    }

    /// Snapshots the exact timed entries eligible for this world-tick pass.
    #[must_use]
    pub(crate) fn timed_ticket_expirations(&self) -> Vec<TimedTicketExpiration> {
        let mut expirations = Vec::with_capacity(self.timed_positions.len());
        for &pos in &self.timed_positions {
            let Some(tickets) = self.tickets.get(&pos) else {
                panic!("timed position index references a missing ticket list");
            };
            expirations.extend(tickets.iter().filter_map(|entry| match entry.lifetime {
                TicketLifetime::Timed {
                    kind, generation, ..
                } => Some(TimedTicketExpiration {
                    pos,
                    kind,
                    generation,
                }),
                TicketLifetime::Untimed => None,
            }));
        }
        expirations.sort_unstable_by_key(|expiration| {
            (
                expiration.pos.0.x,
                expiration.pos.0.y,
                matches!(expiration.kind, TimedChunkTicketKind::EnderPearl),
            )
        });
        expirations
    }

    /// Ages unchanged timed entries selected by the current world-tick pass.
    pub(crate) fn tick_timed_tickets(
        &mut self,
        expirations: &[TimedTicketExpiration],
    ) -> SourceProjectionChanges {
        let mut changes = SourceProjectionChanges::default();

        for &expiration in expirations {
            let Some(tickets) = self.tickets.get_mut(&expiration.pos) else {
                continue;
            };
            let Some(index) = tickets.iter().position(|entry| {
                matches!(
                    entry.lifetime,
                    TicketLifetime::Timed {
                        kind,
                        generation,
                        ..
                    } if kind == expiration.kind && generation == expiration.generation
                )
            }) else {
                continue;
            };
            let TicketLifetime::Timed { ticks_left, .. } = &mut tickets[index].lifetime else {
                unreachable!("the selected ticket was checked as timed");
            };
            *ticks_left -= 1;
            if *ticks_left >= 0 {
                continue;
            }

            let expired = tickets.swap_remove(index);
            let remove_position = tickets.is_empty();
            let has_timed_ticket = tickets
                .iter()
                .any(|entry| matches!(entry.lifetime, TicketLifetime::Timed { .. }));
            if remove_position {
                self.tickets.remove(&expiration.pos);
            }
            if !has_timed_ticket {
                self.timed_positions.remove(&expiration.pos);
            }
            changes.load_positions.push(expiration.pos);
            changes.load_domain_affected = true;
            if expired.ticket.simulation_level().is_some() {
                changes.simulation_positions.push(expiration.pos);
                changes.simulation_domain_affected = true;
            }
        }

        changes
    }

    /// Converts active persistent timed tickets to saved data.
    #[must_use]
    pub(crate) fn to_persistent(&self) -> PersistentChunkTickets {
        let mut tickets = Vec::new();
        for &pos in &self.timed_positions {
            let Some(entries) = self.tickets.get(&pos) else {
                panic!("timed position index references a missing ticket list");
            };
            tickets.extend(
                entries
                    .iter()
                    .filter_map(|ticket| ticket.to_persistent(pos)),
            );
        }
        tickets.sort_unstable_by_key(|ticket| (ticket.chunk_x, ticket.chunk_z, ticket.kind));
        PersistentChunkTickets { tickets }
    }

    /// Reprojects every occupied player source when simulation distance changes.
    pub(crate) fn set_simulation_distance(
        &mut self,
        simulation_distance: u8,
    ) -> SourceProjectionChanges {
        if self.last_projected_simulation_distance == simulation_distance {
            return SourceProjectionChanges::default();
        }

        self.last_projected_simulation_distance = simulation_distance;
        SourceProjectionChanges {
            simulation_positions: self.players_by_pos.keys().copied().collect(),
            simulation_domain_affected: true,
            ..SourceProjectionChanges::default()
        }
    }

    fn remove_untimed_ticket(&mut self, pos: ChunkPos, ticket: ChunkTicket) -> bool {
        let Some(tickets) = self.tickets.get_mut(&pos) else {
            return false;
        };
        let Some(index) = tickets.iter().position(|stored| {
            stored.ticket == ticket && matches!(stored.lifetime, TicketLifetime::Untimed)
        }) else {
            return false;
        };

        tickets.swap_remove(index);
        if tickets.is_empty() {
            self.tickets.remove(&pos);
        }
        true
    }

    fn add_player(
        &mut self,
        pos: ChunkPos,
        player_id: Uuid,
        load_level: ChunkTicketLevel,
        changes: &mut SourceProjectionChanges,
    ) {
        if let Some(current_pos) = self.player_positions.get(&player_id).copied() {
            if current_pos == pos {
                let Some(players) = self.players_by_pos.get_mut(&pos) else {
                    panic!("player position index references a missing source");
                };
                let Some(current_level) = players.get_mut(&player_id) else {
                    panic!("player position index disagrees with source membership");
                };
                if *current_level != load_level {
                    *current_level = load_level;
                    changes.load_positions.push(pos);
                }
                return;
            }

            self.remove_player_membership(current_pos, player_id);
            changes.load_positions.push(current_pos);
            changes.simulation_positions.push(current_pos);
        }

        let replaced = self
            .players_by_pos
            .entry(pos)
            .or_default()
            .insert(player_id, load_level);
        assert!(
            replaced.is_none(),
            "player position index disagrees with source membership"
        );
        self.player_positions.insert(player_id, pos);
        changes.load_positions.push(pos);
        changes.simulation_positions.push(pos);
    }

    fn remove_player(
        &mut self,
        pos: ChunkPos,
        player_id: Uuid,
        changes: &mut SourceProjectionChanges,
    ) {
        if self.player_positions.get(&player_id).copied() != Some(pos) {
            return;
        }

        self.remove_player_membership(pos, player_id);
        self.player_positions.remove(&player_id);
        changes.load_positions.push(pos);
        changes.simulation_positions.push(pos);
    }

    fn remove_player_membership(&mut self, pos: ChunkPos, player_id: Uuid) {
        let Some(players) = self.players_by_pos.get_mut(&pos) else {
            panic!("player position index references a missing source");
        };
        assert!(
            players.remove(&player_id).is_some(),
            "player position index disagrees with source membership"
        );
        if players.is_empty() {
            self.players_by_pos.remove(&pos);
        }
    }

    fn add_or_refresh_timed_ticket(
        &mut self,
        pos: ChunkPos,
        kind: TimedChunkTicketKind,
        ticket: ChunkTicket,
        ticks_left: i64,
    ) -> SourceProjectionChanges {
        let generation = self.allocate_timed_generation();
        let tickets = self.tickets.entry(pos).or_default();
        if let Some((existing_ticks_left, existing_generation)) =
            Self::timed_state_mut(tickets, ticket, kind)
        {
            *existing_ticks_left = ticks_left;
            *existing_generation = generation;
            return SourceProjectionChanges::default();
        }

        tickets.push(StoredChunkTicket::timed(
            ticket, kind, ticks_left, generation,
        ));
        self.timed_positions.insert(pos);
        let mut changes = SourceProjectionChanges {
            load_domain_affected: true,
            simulation_domain_affected: ticket.simulation_level().is_some(),
            ..SourceProjectionChanges::default()
        };
        changes.mark_ticket_membership(pos, ticket);
        changes
    }

    fn add_loaded_persistent_ticket(&mut self, persistent: PersistentChunkTicket) {
        match persistent.kind {
            PersistentChunkTicketKind::Portal => {
                let pos = ChunkPos::new(persistent.chunk_x, persistent.chunk_z);
                let generation = self.allocate_timed_generation();
                let tickets = self.tickets.entry(pos).or_default();
                if let Some((existing_ticks_left, existing_generation)) =
                    Self::timed_state_mut(tickets, portal_ticket(), TimedChunkTicketKind::Portal)
                {
                    *existing_ticks_left = PORTAL_TICKET_TIMEOUT_TICKS;
                    *existing_generation = generation;
                } else {
                    tickets.push(StoredChunkTicket::timed(
                        portal_ticket(),
                        TimedChunkTicketKind::Portal,
                        persistent.ticks_left,
                        generation,
                    ));
                    self.timed_positions.insert(pos);
                }
            }
        }
    }

    fn timed_state_mut(
        tickets: &mut StoredTickets,
        ticket: ChunkTicket,
        kind: TimedChunkTicketKind,
    ) -> Option<(&mut i64, &mut u64)> {
        tickets.iter_mut().find_map(|entry| {
            if entry.ticket != ticket {
                return None;
            }
            match &mut entry.lifetime {
                TicketLifetime::Timed {
                    kind: stored_kind,
                    ticks_left,
                    generation,
                } if *stored_kind == kind => Some((ticks_left, generation)),
                TicketLifetime::Untimed | TicketLifetime::Timed { .. } => None,
            }
        })
    }

    fn allocate_timed_generation(&mut self) -> u64 {
        assert_ne!(
            self.timed_generation,
            u64::MAX,
            "timed ticket generation exhausted"
        );
        self.timed_generation += 1;
        self.timed_generation
    }

    #[cfg(test)]
    fn untimed_ticket_count(&self) -> usize {
        self.tickets
            .values()
            .flatten()
            .filter(|entry| matches!(entry.lifetime, TicketLifetime::Untimed))
            .count()
    }

    #[cfg(test)]
    fn timed_ticket_count(&self) -> usize {
        self.tickets
            .values()
            .flatten()
            .filter(|entry| matches!(entry.lifetime, TicketLifetime::Timed { .. }))
            .count()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn player_id(value: u128) -> Uuid {
        Uuid::from_u128(value)
    }

    #[test]
    fn untimed_ticket_multiplicity_and_domain_minima_are_independent() {
        let mut storage = ChunkTicketStorage::new(5);
        let pos = ChunkPos::new(2, -3);
        let simulated = ChunkTicket::simulated_full_chunks(2);
        let stronger_loading = ChunkTicket::full_chunks(4);

        let _ = storage.apply_operations([
            ChunkTicketOperation::Add {
                pos,
                ticket: simulated,
            },
            ChunkTicketOperation::Add {
                pos,
                ticket: simulated,
            },
            ChunkTicketOperation::Add {
                pos,
                ticket: stronger_loading,
            },
        ]);

        assert_eq!(storage.untimed_ticket_count(), 3);
        assert_eq!(
            storage.load_source_level(pos),
            Some(stronger_loading.load_level())
        );
        assert_eq!(
            storage.simulation_source_level(pos),
            simulated.simulation_level()
        );

        let _ = storage.apply(ChunkTicketOperation::Remove {
            pos,
            ticket: simulated,
        });
        assert_eq!(storage.untimed_ticket_count(), 2);
        assert_eq!(
            storage.simulation_source_level(pos),
            simulated.simulation_level()
        );
    }

    #[test]
    fn removing_strongest_ticket_reveals_the_weaker_source() {
        let mut storage = ChunkTicketStorage::new(5);
        let pos = ChunkPos::new(2, -3);
        let weaker = ChunkTicket::full_chunks(1);
        let stronger = ChunkTicket::full_chunks(4);
        let _ = storage.apply_operations([
            ChunkTicketOperation::Add {
                pos,
                ticket: weaker,
            },
            ChunkTicketOperation::Add {
                pos,
                ticket: stronger,
            },
        ]);

        assert_eq!(storage.load_source_level(pos), Some(stronger.load_level()));

        let _ = storage.apply(ChunkTicketOperation::Remove {
            pos,
            ticket: stronger,
        });

        assert_eq!(storage.load_source_level(pos), Some(weaker.load_level()));
    }

    #[test]
    fn equal_untimed_ticket_survives_timed_expiry_and_index_cleans_up() {
        let mut storage = ChunkTicketStorage::new(5);
        let pos = ChunkPos::new(0, 0);
        let ticket = portal_ticket();

        let _ = storage.add_or_refresh_portal_ticket(pos);
        let expirations = storage.timed_ticket_expirations();
        assert_eq!(expirations.len(), 1);
        assert_eq!(expirations[0].pos(), pos);
        let missing_remove = storage.apply(ChunkTicketOperation::Remove { pos, ticket });

        assert!(missing_remove.load_domain_affected);
        assert!(missing_remove.simulation_domain_affected);
        assert!(missing_remove.load_positions.is_empty());
        assert_eq!(storage.timed_ticket_count(), 1);
        assert_eq!(storage.load_source_level(pos), Some(ticket.load_level()));

        let _ = storage.apply(ChunkTicketOperation::Add { pos, ticket });
        for _ in 0..=PORTAL_TICKET_TIMEOUT_TICKS {
            let _ = storage.tick_timed_tickets(&expirations);
        }

        assert_eq!(storage.timed_ticket_count(), 0);
        assert_eq!(storage.timed_ticket_expirations(), []);
        assert_eq!(storage.untimed_ticket_count(), 1);
        assert_eq!(storage.load_source_level(pos), Some(ticket.load_level()));
    }

    #[test]
    fn refreshed_timed_ticket_rejects_a_stale_expiration_snapshot() {
        let mut storage = ChunkTicketStorage::new(5);
        let pos = ChunkPos::new(0, 0);
        let _ = storage.add_or_refresh_portal_ticket(pos);
        let stale_expirations = storage.timed_ticket_expirations();

        let _ = storage.add_or_refresh_portal_ticket(pos);
        let _ = storage.tick_timed_tickets(&stale_expirations);

        assert_eq!(
            storage.to_persistent().tickets[0].ticks_left,
            PORTAL_TICKET_TIMEOUT_TICKS
        );
    }

    #[test]
    fn player_moves_are_canonical_and_stale_removals_do_not_remove_new_sources() {
        let mut storage = ChunkTicketStorage::new(4);
        let old_pos = ChunkPos::new(0, 0);
        let new_pos = ChunkPos::new(1, 0);
        let first = player_id(1);
        let second = player_id(2);
        let load_level = ChunkTicket::player_loading(8).load_level();

        let _ = storage.apply(ChunkTicketOperation::AddPlayer {
            pos: old_pos,
            player_id: first,
            load_level,
        });
        let _ = storage.apply(ChunkTicketOperation::AddPlayer {
            pos: old_pos,
            player_id: second,
            load_level,
        });
        let moved = storage.apply(ChunkTicketOperation::AddPlayer {
            pos: new_pos,
            player_id: first,
            load_level,
        });

        assert_eq!(moved.load_positions.as_slice(), &[old_pos, new_pos]);
        assert_eq!(moved.simulation_positions.as_slice(), &[old_pos, new_pos]);
        assert!(storage.simulation_source_level(old_pos).is_some());
        assert!(storage.simulation_source_level(new_pos).is_some());

        let stale = storage.apply(ChunkTicketOperation::RemovePlayer {
            pos: old_pos,
            player_id: first,
        });
        assert!(stale.load_positions.is_empty());
        assert!(stale.simulation_positions.is_empty());
        assert!(storage.simulation_source_level(new_pos).is_some());

        let _ = storage.apply(ChunkTicketOperation::RemovePlayer {
            pos: old_pos,
            player_id: second,
        });
        assert_eq!(storage.simulation_source_level(old_pos), None);
    }

    #[test]
    fn simulation_distance_reprojects_only_occupied_player_positions() {
        let mut storage = ChunkTicketStorage::new(2);
        let first_pos = ChunkPos::new(-2, 4);
        let second_pos = ChunkPos::new(7, 1);
        let load_level = ChunkTicket::player_loading(8).load_level();
        let _ = storage.apply_operations([
            ChunkTicketOperation::AddPlayer {
                pos: first_pos,
                player_id: player_id(1),
                load_level,
            },
            ChunkTicketOperation::AddPlayer {
                pos: second_pos,
                player_id: player_id(2),
                load_level,
            },
        ]);

        let old_level = storage.simulation_source_level(first_pos);
        let changes = storage.set_simulation_distance(6);
        let mut positions = changes.simulation_positions.into_vec();
        positions.sort_unstable_by_key(|pos| (pos.0.x, pos.0.y));

        assert!(!changes.load_domain_affected);
        assert!(changes.simulation_domain_affected);
        assert_eq!(positions, vec![first_pos, second_pos]);
        assert_ne!(storage.simulation_source_level(first_pos), old_level);
        assert!(
            storage
                .set_simulation_distance(6)
                .simulation_positions
                .is_empty()
        );
    }

    #[test]
    fn timed_tickets_refresh_wait_for_expiration_and_persist_only_portals() {
        let mut storage = ChunkTicketStorage::new(5);
        let portal_pos = ChunkPos::new(-4, 7);
        let pearl_pos = ChunkPos::new(3, 9);

        assert!(
            !storage
                .add_or_refresh_portal_ticket(portal_pos)
                .load_positions
                .is_empty()
        );
        assert!(
            storage
                .add_or_refresh_portal_ticket(portal_pos)
                .load_positions
                .is_empty()
        );
        let _ = storage.add_or_refresh_ender_pearl_ticket(pearl_pos);
        let expirations = storage.timed_ticket_expirations();
        assert_eq!(
            expirations
                .iter()
                .map(|expiration| expiration.pos())
                .collect::<Vec<_>>(),
            vec![portal_pos, pearl_pos]
        );
        let pearl_expirations: Vec<_> = expirations
            .into_iter()
            .filter(|expiration| expiration.can_expire_if_unloaded())
            .collect();

        for _ in 0..=PORTAL_TICKET_TIMEOUT_TICKS {
            let _ = storage.tick_timed_tickets(&pearl_expirations);
        }
        assert_eq!(storage.timed_ticket_count(), 1);
        assert_eq!(storage.load_source_level(pearl_pos), None);
        assert_eq!(
            storage.load_source_level(portal_pos),
            Some(portal_ticket().load_level())
        );

        let persistent = storage.to_persistent();
        assert_eq!(
            persistent,
            PersistentChunkTickets {
                tickets: vec![PersistentChunkTicket {
                    kind: PersistentChunkTicketKind::Portal,
                    chunk_x: -4,
                    chunk_z: 7,
                    ticks_left: PORTAL_TICKET_TIMEOUT_TICKS,
                }],
            }
        );

        let portal_expirations = storage.timed_ticket_expirations();
        for _ in 0..PORTAL_TICKET_TIMEOUT_TICKS {
            let _ = storage.tick_timed_tickets(&portal_expirations);
        }
        assert!(storage.load_source_level(portal_pos).is_some());
        let expired = storage.tick_timed_tickets(&portal_expirations);
        assert_eq!(expired.load_positions.as_slice(), &[portal_pos]);
        assert_eq!(storage.load_source_level(portal_pos), None);
    }

    #[test]
    fn persistent_duplicate_portal_resets_to_vanilla_timeout() {
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

        let restored = ChunkTicketStorage::from_persistent(persistent, 5);

        assert_eq!(restored.timed_ticket_count(), 1);
        let expirations = restored.timed_ticket_expirations();
        assert_eq!(expirations.len(), 1);
        assert_eq!(expirations[0].pos(), ChunkPos::new(2, 3));
        assert_eq!(
            restored.to_persistent(),
            PersistentChunkTickets {
                tickets: vec![PersistentChunkTicket {
                    kind: PersistentChunkTicketKind::Portal,
                    chunk_x: 2,
                    chunk_z: 3,
                    ticks_left: PORTAL_TICKET_TIMEOUT_TICKS,
                }],
            }
        );
        assert_eq!(restored.initial_load_sources().len(), 1);
        assert_eq!(restored.initial_simulation_sources().len(), 1);
    }

    #[test]
    fn persistent_portal_preserves_remaining_ticks() {
        let persistent = PersistentChunkTickets {
            tickets: vec![PersistentChunkTicket {
                kind: PersistentChunkTicketKind::Portal,
                chunk_x: -8,
                chunk_z: 12,
                ticks_left: 123,
            }],
        };

        let restored = ChunkTicketStorage::from_persistent(persistent.clone(), 5);

        assert_eq!(restored.to_persistent(), persistent);
    }
}
