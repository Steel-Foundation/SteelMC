//! Points of interest tracked by the world.

use steel_registry::poi::PoiTypeRef;
use steel_utils::BlockPos;

/// One POI in the world, with the tickets villagers and other mobs can claim.
#[derive(Debug, Clone)]
pub struct PointOfInterest {
    /// World position of the POI block.
    pub pos: BlockPos,
    /// Type definition; its `ticket_count` is the most tickets this POI can offer.
    pub poi_type: PoiTypeRef,
    /// Tickets left before every claim slot is taken.
    pub free_tickets: u32,
}

impl PointOfInterest {
    /// Creates a POI with every claim slot free.
    #[must_use]
    pub const fn new(pos: BlockPos, poi_type: PoiTypeRef) -> Self {
        Self {
            pos,
            poi_type,
            free_tickets: poi_type.ticket_count,
        }
    }

    /// Claims a ticket if one is free; reports whether it worked.
    pub const fn reserve_ticket(&mut self) -> bool {
        if self.free_tickets > 0 {
            self.free_tickets -= 1;
            true
        } else {
            false
        }
    }

    /// Gives back a claimed ticket; reports whether there was room to do so.
    pub const fn release_ticket(&mut self) -> bool {
        if self.free_tickets < self.poi_type.ticket_count {
            self.free_tickets += 1;
            true
        } else {
            false
        }
    }

    /// Whether at least one claim slot is still open.
    #[must_use]
    pub const fn has_space(&self) -> bool {
        self.free_tickets > 0
    }

    /// Whether any ticket has been claimed (vanilla: `freeTickets != maxTickets`).
    #[must_use]
    pub const fn is_occupied(&self) -> bool {
        self.free_tickets != self.poi_type.ticket_count
    }
}
