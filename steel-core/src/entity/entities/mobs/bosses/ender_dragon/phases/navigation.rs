//! Path following shared by the dragon's navigating phases.
//!
//! Vanilla repeats `navigateToNextPathNode` verbatim in `DragonHoldingPatternPhase`,
//! `DragonLandingApproachPhase`, `DragonStrafePlayerPhase` and `DragonTakeoffPhase`.
//! Takeoff additionally calls `advance()` once before navigating, which stays at that
//! phase's own call site rather than becoming a parameter here.

use glam::DVec3;

use crate::entity::ai::path::Path;

/// Bounds on the squared distance to a fly target that count as still being on course.
///
/// The holding pattern re-targets once it leaves them; the death phase treats leaving
/// them as having arrived at the podium.
pub const MIN_TARGET_DISTANCE_SQR: f64 = 100.0;
pub const MAX_TARGET_DISTANCE_SQR: f64 = 22_500.0;

/// How far above a node the dragon may aim.
const NODE_HEIGHT_SPREAD: f32 = 20.0;

/// Advances `path` by one node and returns the point to steer at.
///
/// Mirrors vanilla `navigateToNextPathNode`. Returns `None` when the path is already
/// finished, which is vanilla's "leave the target where it is" case; vanilla needs an
/// explicit `isDone` guard there only because its `getNextNodePos` would throw.
pub fn navigate_to_next_path_node(path: &mut Path) -> Option<DVec3> {
    let node = path.next_node_pos()?;
    path.advance();

    // Vanilla wraps this in a `do/while` that retries while the height lands below the
    // node, which can never happen because `nextFloat` is not negative. It also
    // computes the sum as `int + float`, so the height is `f32` arithmetic that only
    // widens when it is assigned.
    let y = node.y() as f32 + rand::random::<f32>() * NODE_HEIGHT_SPREAD;

    Some(DVec3::new(
        f64::from(node.x()),
        f64::from(y),
        f64::from(node.z()),
    ))
}
