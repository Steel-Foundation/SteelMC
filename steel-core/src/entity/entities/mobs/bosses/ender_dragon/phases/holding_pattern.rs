use std::sync::Arc;

use glam::DVec3;
use steel_utils::locks::SyncMutex;

use super::navigation::{is_off_course, navigate_to_next_path_node};
use super::{DragonPhaseInstance, EnderDragonPhase};
use crate::chunk::heightmap::HeightmapType;
use crate::entity::Entity as _;
use crate::entity::ai::path::Path;
use crate::entity::ai::targeting::TargetingConditions;
use crate::entity::entities::mobs::bosses::ender_dragon::{
    EnderDragonEntity, end_podium_location, flight_graph,
};
use crate::world::World;

/// `World::nearest_player` treats a negative limit as unlimited, which is what
/// vanilla's unset `TargetingConditions` range amounts to.
const UNLIMITED_RANGE: f64 = -1.0;
/// Divisor turning the egg-to-player distance into a strafing probability.
const STRAFE_DISTANCE_SCALE: f64 = 512.0;
/// One in this many target picks reverses the circling direction and jumps half a ring.
const REVERSE_DIRECTION_CHANCE: i32 = 8;
const OUTER_RING_NODES: i32 = flight_graph::OUTER_RING_NODES as i32;
/// Mask confining a no-fight target to the eight middle-ring nodes.
const MIDDLE_RING_MASK: i32 = 7;

/// Circling the arena: walks the flight graph a node at a time and, whenever a path runs
/// out, rolls for circling on or handing off to the landing cycle or a strafing run.
pub struct DragonHoldingPatternPhase {
    state: SyncMutex<HoldingPatternState>,
}

struct HoldingPatternState {
    current_path: Option<Path>,
    target_location: Option<DVec3>,
    clockwise: bool,
}

impl DragonHoldingPatternPhase {
    #[must_use]
    pub const fn new() -> Self {
        Self {
            state: SyncMutex::new(HoldingPatternState {
                current_path: None,
                target_location: None,
                clockwise: false,
            }),
        }
    }

    /// Vanilla `NEW_TARGET_TARGETING`, which sets no range at all.
    const fn new_target_targeting() -> TargetingConditions {
        TargetingConditions::for_combat().ignore_line_of_sight()
    }

    /// Whether the current path has been walked out, or `None` when there is none.
    fn path_is_done(&self) -> Option<bool> {
        self.state.lock().current_path.as_ref().map(Path::is_done)
    }

    fn find_new_target(&self, dragon: &EnderDragonEntity, world: &Arc<World>) {
        // Sampled once: `try_hand_off` only switches phase and cannot touch the path.
        let path_is_done = self.path_is_done();

        if path_is_done == Some(true) && Self::try_hand_off(dragon, world) {
            return;
        }

        if path_is_done.is_none_or(|done| done) {
            self.pick_new_path(dragon, world);
        }

        let mut state = self.state.lock();
        let Some(path) = state.current_path.as_mut() else {
            return;
        };
        let Some(target) = navigate_to_next_path_node(path) else {
            return;
        };
        state.target_location = Some(target);
    }

    /// Rolls for leaving the circle; returns whether the dragon switched phase. Runs only
    /// once a path is walked out, so the dragon finishes a hop before peeling off.
    fn try_hand_off(dragon: &EnderDragonEntity, world: &Arc<World>) -> bool {
        let crystals = dragon.alive_crystals().unwrap_or(0);
        let egg = world.heightmap_pos(
            HeightmapType::MotionBlockingNoLeaves,
            end_podium_location(dragon.fight_origin()),
        );

        if rand::random_range(0..crystals + 3) == 0 {
            dragon
                .phase_manager()
                .set_phase(dragon, EnderDragonPhase::LandingApproach);
            return true;
        }

        // Vanilla ranks players by the raw block position but scores the strafing roll
        // from the block's center, so the two really do use different points.
        let targeting = Self::new_target_targeting();
        let Some(player) = world.nearest_player(
            DVec3::new(f64::from(egg.x()), f64::from(egg.y()), f64::from(egg.z())),
            UNLIMITED_RANGE,
            |player| targeting.test(world, Some(dragon), player),
        ) else {
            return false;
        };

        let (cx, cy, cz) = egg.get_center();
        let distance =
            DVec3::new(cx, cy, cz).distance_squared(player.position()) / STRAFE_DISTANCE_SCALE;

        if rand::random_range(0..(distance + 2.0) as i32) == 0
            || rand::random_range(0..crystals + 2) == 0
        {
            Self::strafe_player(dragon);
            return true;
        }

        false
    }

    fn strafe_player(dragon: &EnderDragonEntity) {
        // TODO: Pass the player as the strafe target once `DragonStrafePlayerPhase` is
        // ported; until then that phase has no fly target and the dragon holds position.
        dragon
            .phase_manager()
            .set_phase(dragon, EnderDragonPhase::StrafePlayer);
    }

    /// Picks the next graph node to circle toward and paths to it. Does nothing (keeping
    /// the old path, retrying next tick) while the arena's chunks are unloaded. Also
    /// warms the flight graph before the state lock is taken.
    fn pick_new_path(&self, dragon: &EnderDragonEntity, world: &Arc<World>) {
        let Some(current_node) = dragon.find_closest_node(world) else {
            return;
        };

        let mut state = self.state.lock();
        let mut target_node = current_node as i32;
        if rand::random_range(0..REVERSE_DIRECTION_CHANCE) == 0 {
            state.clockwise = !state.clockwise;
            target_node += 6;
        }
        target_node += if state.clockwise { 1 } else { -1 };

        target_node = match dragon.alive_crystals() {
            // Vanilla's test is `aliveCrystals() >= 0`, which is always true, so the
            // crystal count never matters here, only whether a fight exists at all.
            Some(alive) if alive >= 0 => target_node.rem_euclid(OUTER_RING_NODES),
            // `&` on a possibly negative operand gives the same two's-complement result
            // as Java's.
            _ => ((target_node - OUTER_RING_NODES) & MIDDLE_RING_MASK) + OUTER_RING_NODES,
        };

        state.current_path = dragon.find_path(world, current_node, target_node as usize, None);
        if let Some(path) = state.current_path.as_mut() {
            path.advance();
        }
    }
}

impl Default for DragonHoldingPatternPhase {
    fn default() -> Self {
        Self::new()
    }
}

impl DragonPhaseInstance for DragonHoldingPatternPhase {
    fn phase(&self) -> EnderDragonPhase {
        EnderDragonPhase::HoldingPattern
    }

    fn do_server_tick(&self, dragon: &EnderDragonEntity, world: &Arc<World>) {
        let distance = self
            .state
            .lock()
            .target_location
            .map_or(0.0, |target| target.distance_squared(dragon.position()));

        // A missing target reads as distance zero, so `begin` forces a pick next tick.
        // The collision flags are always false under `no_physics`, as in vanilla.
        if is_off_course(dragon, distance) {
            self.find_new_target(dragon, world);
        }
    }

    fn begin(&self, _dragon: &EnderDragonEntity) {
        // `clockwise` survives, as in vanilla, so a dragon back from the landing cycle
        // keeps circling the same way.
        let mut state = self.state.lock();
        state.current_path = None;
        state.target_location = None;
    }

    fn fly_target_location(&self) -> Option<DVec3> {
        self.state.lock().target_location
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Weak;

    use super::*;
    use crate::entity::entities::mobs::bosses::ender_dragon::tests::{build_dragon, in_phase};
    use crate::test_support::test_world;

    /// Enough rolls that a 1-in-3 branch is overwhelmingly likely to have been taken.
    const ROLLS: usize = 200;

    fn holding_dragon() -> Arc<EnderDragonEntity> {
        in_phase(
            build_dragon(DVec3::ZERO, Weak::new()),
            EnderDragonPhase::HoldingPattern,
        )
    }

    // Driven directly because it is unreachable in play until `EnderDragonFight` is
    // ported. The roll is random, so this asserts the invariant, not an outcome.
    #[test]
    fn a_hand_off_either_lands_or_leaves_the_phase_untouched() {
        let world = test_world();
        let mut handed_off = 0_usize;

        for _ in 0..ROLLS {
            let dragon = holding_dragon();

            if DragonHoldingPatternPhase::try_hand_off(&dragon, world) {
                handed_off += 1;
                assert_eq!(
                    dragon.phase_manager().current_phase(),
                    EnderDragonPhase::LandingApproach,
                    "a hand-off must leave the dragon on the landing approach"
                );
            } else {
                assert_eq!(
                    dragon.phase_manager().current_phase(),
                    EnderDragonPhase::HoldingPattern,
                    "declining to hand off must not move the dragon"
                );
            }
        }

        assert!(
            handed_off > 0,
            "the 1-in-3 landing roll never fired across {ROLLS} attempts"
        );
        assert!(
            handed_off < ROLLS,
            "the landing roll fired every time, so the roll is not random"
        );
    }

    #[test]
    fn an_empty_world_never_strafes() {
        let world = test_world();

        for _ in 0..ROLLS {
            let dragon = holding_dragon();
            DragonHoldingPatternPhase::try_hand_off(&dragon, world);

            assert_ne!(
                dragon.phase_manager().current_phase(),
                EnderDragonPhase::StrafePlayer,
                "there is no player to strafe"
            );
        }
    }
}
