use std::sync::Arc;

use glam::DVec3;
use steel_utils::locks::SyncMutex;

use super::navigation::{
    MAX_TARGET_DISTANCE_SQR, MIN_TARGET_DISTANCE_SQR, navigate_to_next_path_node,
};
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
/// [`flight_graph::OUTER_RING_NODES`], typed for the target-index arithmetic.
const OUTER_RING_NODES: i32 = flight_graph::OUTER_RING_NODES as i32;
/// Mask confining a no-fight target to the eight middle-ring nodes.
const MIDDLE_RING_MASK: i32 = 7;

/// Circling the arena. Mirrors vanilla `DragonHoldingPatternPhase`.
///
/// This is the dragon's default flying behaviour: it walks the flight graph one node
/// at a time and, whenever a path runs out, rolls for whether to keep circling or hand
/// off to the landing cycle or a strafing run.
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

    /// Mirrors vanilla `findNewTarget`.
    fn find_new_target(&self, dragon: &EnderDragonEntity, world: &Arc<World>) {
        // Sampled once because `try_hand_off` only switches phase; it cannot touch the
        // path, which is why it takes no `self`.
        let path_is_done = self.path_is_done();

        if path_is_done == Some(true) && Self::try_hand_off(dragon, world) {
            return;
        }

        // Vanilla's `currentPath == null || currentPath.isDone()`.
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

    /// Rolls for leaving the circle, and returns whether the dragon switched phase.
    ///
    /// Vanilla runs this only once a path has been walked to its end, so the dragon
    /// commits to a full hop between nodes before it can peel off.
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

    /// Mirrors vanilla `strafePlayer`.
    fn strafe_player(dragon: &EnderDragonEntity) {
        // TODO: Hand the player over as the strafe target once `DragonStrafePlayerPhase`
        // is ported. Until then that phase reports no fly target, so the dragon holds
        // position rather than making the run.
        dragon
            .phase_manager()
            .set_phase(dragon, EnderDragonPhase::StrafePlayer);
    }

    /// Picks the next graph node to circle towards and paths to it.
    ///
    /// Does nothing while the flight graph cannot be built, which happens when the
    /// arena's chunks are not loaded yet. The dragon keeps whatever path it already had
    /// and this runs again next tick, rather than circling a graph pinned to the world
    /// floor. Note this call also warms the graph before the state lock below is taken.
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
            // Confines a fightless dragon to the eight middle-ring nodes. The mask runs
            // on a possibly negative operand, which is deliberate: Rust's `&` gives the
            // same two's-complement result Java's does.
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

        // A missing target reads as distance zero, which is what makes `begin` force a
        // pick on the very next tick. The two collision flags are permanently false
        // under `no_physics` (in vanilla as well), so they are carried for shape
        // rather than effect.
        if distance < MIN_TARGET_DISTANCE_SQR
            || distance > MAX_TARGET_DISTANCE_SQR
            || dragon.horizontal_collision()
            || dragon.vertical_collision()
        {
            self.find_new_target(dragon, world);
        }
    }

    fn begin(&self, _dragon: &EnderDragonEntity) {
        // Vanilla clears only the path and the target. `clockwise` deliberately
        // survives, so a dragon that leaves for the landing cycle and comes back keeps
        // circling the same way round.
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
    use crate::entity::entities::mobs::bosses::ender_dragon::tests::build_dragon;
    use crate::test_support::test_world;

    /// Enough rolls that a 1-in-3 branch is overwhelmingly likely to have been taken.
    const ROLLS: usize = 200;

    /// A world-less dragon already circling, which is the state `try_hand_off` runs in.
    fn holding_dragon() -> EnderDragonEntity {
        let dragon = build_dragon(DVec3::ZERO, Weak::new());
        dragon
            .phase_manager()
            .set_phase(&dragon, EnderDragonPhase::HoldingPattern);
        dragon
    }

    /// `try_hand_off` is the branchiest code in the phase and is unreachable in play
    /// today, because only `EnderDragonFight` puts a dragon into the holding pattern and
    /// that fight is not ported. Driving it directly is the only coverage available.
    ///
    /// The roll is random, so the assertion is on the invariant rather than an outcome:
    /// a hand-off always lands on `LandingApproach`, and staying put always leaves the
    /// phase alone.
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

    /// With nobody in the world, `nearest_player` finds no one and the strafe branch is
    /// unreachable, so the only two outcomes are landing or staying put.
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
