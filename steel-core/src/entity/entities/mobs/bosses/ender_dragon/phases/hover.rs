use glam::DVec3;
use std::sync::Arc;
use steel_utils::locks::SyncMutex;

use super::{DragonPhaseInstance, EnderDragonPhase};
use crate::entity::Entity as _;
use crate::entity::entities::mobs::bosses::ender_dragon::EnderDragonEntity;
use crate::world::World;

/// Holds position. Mirrors vanilla `DragonHoverPhase`.
///
/// This is the dragon's starting phase, and the one it sits in when nothing else has
/// claimed it, so a dragon summoned outside a fight simply hangs in the air.
pub struct DragonHoverPhase {
    /// Where to hold. Latched on the first tick after `begin`, so the dragon holds
    /// wherever it happened to be when it entered the phase.
    target_location: SyncMutex<Option<DVec3>>,
}

impl DragonHoverPhase {
    #[must_use]
    pub const fn new() -> Self {
        Self {
            target_location: SyncMutex::new(None),
        }
    }
}

impl Default for DragonHoverPhase {
    fn default() -> Self {
        Self::new()
    }
}

impl DragonPhaseInstance for DragonHoverPhase {
    fn phase(&self) -> EnderDragonPhase {
        EnderDragonPhase::Hovering
    }

    /// Vanilla reports hovering as sitting.
    ///
    /// This is not just bookkeeping: it selects the perched wing-beat rate and the
    /// lowered head offset, so a hovering dragon animates like a perched one.
    fn is_sitting(&self) -> bool {
        true
    }

    fn do_server_tick(&self, dragon: &EnderDragonEntity, _world: &Arc<World>) {
        let mut target = self.target_location.lock();
        if target.is_none() {
            *target = Some(dragon.position());
        }
    }

    fn begin(&self, _dragon: &EnderDragonEntity) {
        *self.target_location.lock() = None;
    }

    fn fly_speed(&self) -> f32 {
        1.0
    }

    fn fly_target_location(&self) -> Option<DVec3> {
        *self.target_location.lock()
    }
}
