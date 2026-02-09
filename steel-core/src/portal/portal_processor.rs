//! Per-entity state for standing inside a portal.

use steel_utils::BlockPos;

/// Tracks how long an entity has been standing in a portal.
pub struct PortalProcessor {
    /// The position of the portal block the entity is standing in.
    pub portal_pos: BlockPos,
    /// How many ticks the entity has been in the portal.
    pub portal_time: i32,
    /// How many ticks required to trigger the teleport.
    pub transition_time: i32,
}

impl PortalProcessor {
    /// Creates a new portal processor for the given portal position.
    #[must_use]
    pub const fn new(portal_pos: BlockPos, transition_time: i32) -> Self {
        Self {
            portal_pos,
            portal_time: 0,
            transition_time,
        }
    }

    /// Increments the portal time and returns true if the transition should trigger.
    pub const fn tick(&mut self) -> bool {
        self.portal_time += 1;
        self.portal_time >= self.transition_time
    }

    /// Decays the portal time (when the entity leaves the portal).
    /// Returns true if the processor should be removed (time reached 0).
    pub fn decay(&mut self) -> bool {
        self.portal_time = (self.portal_time - 4).max(0);
        self.portal_time == 0
    }
}
