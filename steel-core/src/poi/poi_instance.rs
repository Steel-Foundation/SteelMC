//! Individual point of interest instance.

use steel_utils::BlockPos;

/// A single point of interest at a specific block position.
///
/// Tracks the POI type and available tickets (e.g., a bed has 1 ticket
/// that gets reserved when a villager claims it).
#[derive(Debug, Clone)]
pub struct PointOfInterest {
    /// The block position of this POI.
    pub pos: BlockPos,
    /// The registry ID of this POI's type.
    pub poi_type_id: usize,
    /// Number of tickets still available for claiming.
    pub free_tickets: u32,
}

impl PointOfInterest {
    /// Creates a new POI with all tickets available.
    #[must_use]
    pub const fn new(pos: BlockPos, poi_type_id: usize, max_tickets: u32) -> Self {
        Self {
            pos,
            poi_type_id,
            free_tickets: max_tickets,
        }
    }

    /// Attempts to reserve a ticket. Returns `true` if successful.
    pub const fn reserve_ticket(&mut self) -> bool {
        if self.free_tickets > 0 {
            self.free_tickets -= 1;
            true
        } else {
            false
        }
    }

    /// Releases a previously reserved ticket. Returns `true` if successful.
    pub const fn release_ticket(&mut self, max_tickets: u32) -> bool {
        if self.free_tickets < max_tickets {
            self.free_tickets += 1;
            true
        } else {
            false
        }
    }

    /// Returns `true` if at least one ticket is available.
    #[must_use]
    pub const fn has_space(&self) -> bool {
        self.free_tickets > 0
    }

    /// Returns `true` if at least one ticket has been reserved.
    ///
    /// Vanilla equivalent: `freeTickets != maxTickets`.
    #[must_use]
    pub const fn is_occupied(&self, max_tickets: u32) -> bool {
        self.free_tickets != max_tickets
    }
}
