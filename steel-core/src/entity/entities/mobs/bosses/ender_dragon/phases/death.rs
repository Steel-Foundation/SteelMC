use std::sync::Arc;

use glam::DVec3;
use steel_utils::BlockPos;
use steel_utils::locks::SyncMutex;

use super::navigation::{MAX_TARGET_DISTANCE_SQR, MIN_TARGET_DISTANCE_SQR};
use super::{DragonPhaseInstance, EnderDragonPhase};
use crate::chunk::heightmap::HeightmapType;
use crate::entity::Entity as _;
use crate::entity::LivingEntity as _;
use crate::entity::entities::mobs::bosses::ender_dragon::{EnderDragonEntity, end_podium_location};
use crate::world::World;

/// How fast the dragon makes its final flight, well above any other phase.
const DEATH_FLY_SPEED: f32 = 3.0;

/// Flying to the podium to die. Mirrors vanilla `DragonDeathPhase`.
///
/// The phase does not kill the dragon; it *keeps it alive*. Health is re-asserted at
/// 1.0 every tick while the podium is still out of reach, and dropped to 0.0 on
/// arrival, and since `is_dead_or_dying` is health-based, that drop is what finally
/// lets `tick_death` start counting.
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

    /// Resolves the point above the exit portal that the dragon dies on.
    ///
    /// `None` while the podium's chunk is not loaded. The result is latched for the
    /// whole death flight, and [`World::heightmap_pos`] would answer with the world
    /// floor rather than failing, which in the End would send the dragon down to y 0 to
    /// die instead of onto the portal.
    fn podium_target(dragon: &EnderDragonEntity, world: &World) -> Option<DVec3> {
        // `MotionBlocking` is the odd one out: every other dragon phase resolves ground
        // level with `MotionBlockingNoLeaves`. Vanilla really does differ here.
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
        // Not `get_or_insert_with`: a target resolved off a cold chunk must not be
        // latched, so leave the slot empty and resolve again next tick.
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

        let distance = target.distance_squared(dragon.position());
        let arrived = distance < MIN_TARGET_DISTANCE_SQR
            || distance > MAX_TARGET_DISTANCE_SQR
            || dragon.horizontal_collision()
            || dragon.vertical_collision();

        dragon.set_health(if arrived { 0.0 } else { 1.0 });
    }

    fn begin(&self, _dragon: &EnderDragonEntity) {
        // Vanilla also resets a `time` counter here, but it is read only by the client
        // tick that paints explosion particles, so it is write-only server-side.
        *self.target_location.lock() = None;
    }

    fn fly_speed(&self) -> f32 {
        DEATH_FLY_SPEED
    }

    fn fly_target_location(&self) -> Option<DVec3> {
        *self.target_location.lock()
    }
}
