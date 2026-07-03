//! World portal system for nether/end portals and future portal types.
//!
//! Vanilla commonly calls loaded worlds "dimensions". Steel uses "world" for
//! loaded runtime worlds and reserves "dimension type" for the vanilla registry
//! entry that defines world rules.

use crate::world::World;
use glam::DVec3;
use std::sync::Arc;
use steel_utils::BlockPos;

pub mod portal_shape;

/// Vanilla portal behavior kind tracked by an entity while it is inside a portal.
///
/// Java stores a reference to the `Portal` block behavior object. Steel keeps a
/// compact explicit kind here so entity state does not depend on block behavior
/// object identity.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PortalKind {
    /// Vanilla nether portal block.
    Nether,
    /// Vanilla end portal block.
    End,
    /// Vanilla end gateway block.
    EndGateway,
}

/// Result of advancing an entity's active portal process for one server tick.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PortalProcessResult {
    /// The entity has not reached the portal transition threshold.
    Waiting,
    /// The portal transition threshold was reached this tick.
    Ready,
}

/// Per-entity portal timer state.
///
/// Mirrors vanilla `PortalProcessor`: the active portal kind, the entry block
/// position, the accumulated portal time, and whether the entity touched the
/// portal during the current tick.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PortalProcessor {
    portal: PortalKind,
    entry_position: BlockPos,
    portal_time: i32,
    inside_portal_this_tick: bool,
}

impl PortalProcessor {
    /// Creates a portal process for a freshly entered portal.
    #[must_use]
    pub const fn new(portal: PortalKind, entry_position: BlockPos) -> Self {
        Self {
            portal,
            entry_position,
            portal_time: 0,
            inside_portal_this_tick: true,
        }
    }

    /// Returns the tracked portal kind.
    #[must_use]
    pub const fn portal(self) -> PortalKind {
        self.portal
    }

    /// Returns the portal block position the entity entered from.
    #[must_use]
    pub const fn entry_position(self) -> BlockPos {
        self.entry_position
    }

    /// Returns the accumulated portal time.
    #[must_use]
    pub const fn portal_time(self) -> i32 {
        self.portal_time
    }

    /// Returns whether the entity touched this portal during the current tick.
    #[must_use]
    pub const fn is_inside_portal_this_tick(self) -> bool {
        self.inside_portal_this_tick
    }

    /// Returns true if this process tracks the same portal behavior.
    #[must_use]
    pub fn is_same_portal(self, portal: PortalKind) -> bool {
        self.portal == portal
    }

    /// Marks this process as touched by the entity for the current tick.
    pub fn set_as_inside_portal(&mut self, entry_position: BlockPos) {
        if !self.inside_portal_this_tick {
            self.entry_position = entry_position;
            self.inside_portal_this_tick = true;
        }
    }

    /// Advances vanilla portal timing for one server tick.
    pub fn process_portal_teleportation(
        &mut self,
        allowed_to_teleport: bool,
        transition_time: i32,
    ) -> PortalProcessResult {
        if !self.inside_portal_this_tick {
            self.decay_tick();
            return PortalProcessResult::Waiting;
        }

        self.inside_portal_this_tick = false;
        if !allowed_to_teleport {
            return PortalProcessResult::Waiting;
        }

        let ready = self.portal_time >= transition_time;
        self.portal_time += 1;
        if ready {
            PortalProcessResult::Ready
        } else {
            PortalProcessResult::Waiting
        }
    }

    fn decay_tick(&mut self) {
        self.portal_time = self.portal_time.saturating_sub(4).max(0);
    }

    /// Returns true when vanilla would clear the active portal process.
    #[must_use]
    pub const fn has_expired(self) -> bool {
        self.portal_time <= 0
    }
}

/// Describes a teleport transition to another loaded world.
///
/// Vanilla names loaded worlds "dimensions" in packets and saves. Steel uses
/// "world" for runtime loaded world instances, reserving "dimension type" for
/// the vanilla registry entry that defines height, skylight, ceiling, etc.
pub struct TeleportTransition {
    /// The target world to teleport into.
    pub target_world: Arc<World>,
    /// The position in the target world.
    pub position: DVec3,
    /// The rotation (yaw, pitch) in the target world.
    pub rotation: (f32, f32),
    /// Portal cooldown in ticks (prevents immediate re-entry).
    pub portal_cooldown: i32,
}

/// A queued request to move an entity between loaded worlds.
///
/// Vanilla calls these world changes "dimension changes". Steel keeps the
/// runtime API named after loaded worlds to avoid confusing worlds with vanilla
/// dimension types.
pub enum WorldChangeRequest {
    /// Pre-computed transition (players after chunk pre-warming).
    Computed(TeleportTransition),
    /// Command-driven world change to the target world's spawn.
    WorldSpawn {
        /// The target world to teleport into.
        target_world: Arc<World>,
    },
    /// Portal position — server computes destination at processing time.
    /// TODO: implement portal destination calculation (`nether_portal::calculate_destination`)
    Portal {
        /// The world the entity is currently in.
        source_world: Arc<World>,
        /// The portal block position.
        portal_pos: BlockPos,
    },
}

#[cfg(test)]
mod tests {
    use steel_utils::BlockPos;

    use super::{PortalKind, PortalProcessResult, PortalProcessor};

    #[test]
    fn portal_processor_reaches_transition_after_vanilla_threshold() {
        let mut processor = PortalProcessor::new(PortalKind::Nether, BlockPos::new(1, 64, 1));

        assert_eq!(
            processor.process_portal_teleportation(true, 2),
            PortalProcessResult::Waiting
        );
        processor.set_as_inside_portal(BlockPos::new(1, 64, 1));
        assert_eq!(
            processor.process_portal_teleportation(true, 2),
            PortalProcessResult::Waiting
        );
        processor.set_as_inside_portal(BlockPos::new(1, 64, 1));
        assert_eq!(
            processor.process_portal_teleportation(true, 2),
            PortalProcessResult::Ready
        );
        assert_eq!(processor.portal_time(), 3);
    }

    #[test]
    fn portal_processor_does_not_increment_when_teleport_is_disallowed() {
        let mut processor = PortalProcessor::new(PortalKind::End, BlockPos::new(0, 80, 0));

        assert_eq!(
            processor.process_portal_teleportation(false, 0),
            PortalProcessResult::Waiting
        );

        assert_eq!(processor.portal_time(), 0);
        assert!(!processor.is_inside_portal_this_tick());
    }

    #[test]
    fn portal_processor_decays_when_entity_leaves_portal() {
        let mut processor = PortalProcessor::new(PortalKind::EndGateway, BlockPos::new(3, 70, 4));
        for _ in 0..5 {
            processor.set_as_inside_portal(BlockPos::new(3, 70, 4));
            processor.process_portal_teleportation(true, 20);
        }

        assert_eq!(processor.portal_time(), 5);
        processor.process_portal_teleportation(true, 20);
        assert_eq!(processor.portal_time(), 1);
        processor.process_portal_teleportation(true, 20);
        assert_eq!(processor.portal_time(), 0);
        assert!(processor.has_expired());
    }

    #[test]
    fn portal_processor_updates_entry_position_only_after_tick_is_consumed() {
        let mut processor = PortalProcessor::new(PortalKind::Nether, BlockPos::new(1, 64, 1));

        processor.set_as_inside_portal(BlockPos::new(2, 64, 2));
        assert_eq!(processor.entry_position(), BlockPos::new(1, 64, 1));

        processor.process_portal_teleportation(true, 80);
        processor.set_as_inside_portal(BlockPos::new(2, 64, 2));
        assert_eq!(processor.entry_position(), BlockPos::new(2, 64, 2));
    }
}
