//! A typed chunk ticket matching Vanilla's `Ticket` value.

use std::ptr;

use steel_registry::ticket_type::TicketTypeRef;

use super::chunk_ticket_manager::ChunkTicketLevel;

/// One ticket type and level, with its remaining timeout.
#[derive(Debug, Clone, Copy)]
pub(crate) struct ChunkTicket {
    ticket_type: TicketTypeRef,
    ticket_level: ChunkTicketLevel,
    ticks_left: i64,
}

impl ChunkTicket {
    /// Creates a ticket with the type's full timeout.
    #[must_use]
    pub(crate) const fn new(ticket_type: TicketTypeRef, ticket_level: ChunkTicketLevel) -> Self {
        Self::from_saved(ticket_type, ticket_level, ticket_type.timeout())
    }

    /// Restores a ticket's serialized remaining timeout.
    #[must_use]
    pub(crate) const fn from_saved(
        ticket_type: TicketTypeRef,
        ticket_level: ChunkTicketLevel,
        ticks_left: i64,
    ) -> Self {
        Self {
            ticket_type,
            ticket_level,
            ticks_left,
        }
    }

    /// Creates a ticket that makes chunks full within `radius`.
    #[must_use]
    pub(crate) const fn for_full_chunk_radius(ticket_type: TicketTypeRef, radius: u8) -> Self {
        Self::new(ticket_type, ChunkTicketLevel::for_full_chunk_radius(radius))
    }

    #[must_use]
    pub(crate) const fn ticket_type(&self) -> TicketTypeRef {
        self.ticket_type
    }

    #[must_use]
    pub(crate) const fn ticket_level(&self) -> ChunkTicketLevel {
        self.ticket_level
    }

    #[must_use]
    pub(crate) const fn ticks_left(&self) -> i64 {
        self.ticks_left
    }

    /// Returns this ticket's contribution to loading propagation.
    #[must_use]
    pub(crate) const fn loading_level(&self) -> Option<ChunkTicketLevel> {
        if self.ticket_type.does_load() {
            Some(self.ticket_level)
        } else {
            None
        }
    }

    /// Returns this ticket's contribution to simulation propagation.
    #[must_use]
    pub(crate) const fn simulation_level(&self) -> Option<ChunkTicketLevel> {
        if self.ticket_type.does_simulate() {
            Some(self.ticket_level)
        } else {
            None
        }
    }

    /// Restores the type's full timeout.
    pub(crate) const fn reset_ticks_left(&mut self) {
        self.ticks_left = self.ticket_type.timeout();
    }

    /// Ages a timed ticket by one tick using Java `long` overflow behavior.
    pub(crate) const fn decrease_ticks_left(&mut self) {
        if self.ticket_type.has_timeout() {
            self.ticks_left = self.ticks_left.wrapping_sub(1);
        }
    }

    #[must_use]
    pub(crate) const fn is_timed_out(&self) -> bool {
        self.ticket_type.has_timeout() && self.ticks_left < 0
    }
}

impl PartialEq for ChunkTicket {
    fn eq(&self, other: &Self) -> bool {
        ptr::eq(self.ticket_type, other.ticket_type) && self.ticket_level == other.ticket_level
    }
}

impl Eq for ChunkTicket {}

#[cfg(test)]
mod tests {
    use steel_registry::vanilla_ticket_types::{DRAGON, PLAYER_LOADING, PORTAL};

    use super::*;

    #[test]
    fn identity_is_type_pointer_and_level_not_timeout_state() {
        let mut first = ChunkTicket::new(&PORTAL, ChunkTicketLevel::FULL_CHUNK);
        let second = first;

        first.decrease_ticks_left();

        assert_eq!(first, second);
        assert_ne!(
            first,
            ChunkTicket::new(&DRAGON, ChunkTicketLevel::FULL_CHUNK)
        );
        assert_ne!(
            first,
            ChunkTicket::new(&PORTAL, ChunkTicketLevel::BLOCK_TICKING_CHUNK)
        );
    }

    #[test]
    fn timeout_aging_matches_java_long_behavior() {
        let mut timed = ChunkTicket::new(&PORTAL, ChunkTicketLevel::FULL_CHUNK);
        assert_eq!(timed.ticks_left(), PORTAL.timeout());

        timed.ticks_left = 0;
        timed.decrease_ticks_left();
        assert!(timed.is_timed_out());
        timed.reset_ticks_left();
        assert_eq!(timed.ticks_left(), PORTAL.timeout());

        timed.ticks_left = i64::MIN;
        timed.decrease_ticks_left();
        assert_eq!(timed.ticks_left(), i64::MAX);
        assert!(!timed.is_timed_out());

        let mut untimed = ChunkTicket::new(&PLAYER_LOADING, ChunkTicketLevel::FULL_CHUNK);
        untimed.decrease_ticks_left();
        assert_eq!(untimed.ticks_left(), 0);
        assert!(!untimed.is_timed_out());
    }
}
