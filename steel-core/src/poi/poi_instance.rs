#![allow(missing_docs)]

use steel_utils::BlockPos;

#[derive(Debug, Clone)]
pub struct PointOfInterest {
    pub pos: BlockPos,
    pub poi_type_id: usize,
    pub free_tickets: u32,
}

impl PointOfInterest {
    #[must_use]
    pub const fn new(pos: BlockPos, poi_type_id: usize, max_tickets: u32) -> Self {
        Self {
            pos,
            poi_type_id,
            free_tickets: max_tickets,
        }
    }

    pub const fn reserve_ticket(&mut self) -> bool {
        if self.free_tickets > 0 {
            self.free_tickets -= 1;
            true
        } else {
            false
        }
    }

    pub const fn release_ticket(&mut self, max_tickets: u32) -> bool {
        if self.free_tickets < max_tickets {
            self.free_tickets += 1;
            true
        } else {
            false
        }
    }

    #[must_use]
    pub const fn has_space(&self) -> bool {
        self.free_tickets > 0
    }

    #[must_use]
    pub const fn is_occupied(&self, max_tickets: u32) -> bool {
        self.free_tickets == 0 && max_tickets > 0
    }
}
