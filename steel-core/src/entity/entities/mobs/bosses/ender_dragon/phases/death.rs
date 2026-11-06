use std::sync::Arc;

use glam::DVec3;
use steel_utils::BlockPos;
use steel_utils::locks::SyncMutex;

use super::navigation::is_off_course;
use super::{DragonPhaseInstance, EnderDragonPhase};
use crate::chunk::heightmap::HeightmapType;
use crate::entity::Entity as _;
use crate::entity::LivingEntity as _;
use crate::entity::entities::mobs::bosses::ender_dragon::{EnderDragonEntity, end_podium_location};
use crate::world::World;

/// How fast the dragon makes its final flight, well above any other phase.
const DEATH_FLY_SPEED: f32 = 3.0;

/// Flying to the podium to die. The phase *keeps the dragon alive*: health is held at
/// 1.0 each tick until arrival, then dropped to 0.0, which is what lets `tick_death`
/// (health-based via `is_dead_or_dying`) start counting.
pub struct DragonDeathPhase {
    /// Latched on the first tick after `begin`, because resolving it needs the world.
    target_location: SyncMutex<Option<DVec3>>,
}

impl DragonDeathPhase {
    #[must_use]
    pub const fn new() -> Self {
        Self {
            target_location: SyncMutex::new(None),
        }
    }

    /// Resolves the point above the exit portal the dragon dies on, or `None` while the
    /// podium's chunk is unloaded. [`World::heightmap_pos`] would return the world floor
    /// instead, and the latched target would send the dragon down to y 0.
    fn podium_target(dragon: &EnderDragonEntity, world: &World) -> Option<DVec3> {
        // `MotionBlocking`, unlike every other phase's `MotionBlockingNoLeaves`, as in vanilla.
        let podium = end_podium_location(dragon.fight_origin());
        let y = world.height_at(HeightmapType::MotionBlocking, podium.x(), podium.z())?;
        let (x, y, z) = BlockPos::new(podium.x(), y, podium.z()).get_bottom_center();
        Some(DVec3::new(x, y, z))
    }
}

impl Default for DragonDeathPhase {
    fn default() -> Self {
        Self::new()
    }
}

impl DragonPhaseInstance for DragonDeathPhase {
    fn phase(&self) -> EnderDragonPhase {
        EnderDragonPhase::Dying
    }

    fn do_server_tick(&self, dragon: &EnderDragonEntity, world: &Arc<World>) {
        // Not `get_or_insert_with`: an unresolved target must not be latched.
        let target = {
            let mut latched = self.target_location.lock();
            if latched.is_none() {
                *latched = Self::podium_target(dragon, world);
            }
            match *latched {
                Some(target) => target,
                None => return,
            }
        };

        let arrived = is_off_course(dragon, target.distance_squared(dragon.position()));

        dragon.set_health(if arrived { 0.0 } else { 1.0 });
    }

    fn begin(&self, _dragon: &EnderDragonEntity) {
        // Vanilla also resets `time` here, which only the client's particle tick reads.
        *self.target_location.lock() = None;
    }

    fn fly_speed(&self) -> f32 {
        DEATH_FLY_SPEED
    }

    fn fly_target_location(&self) -> Option<DVec3> {
        *self.target_location.lock()
    }
}
