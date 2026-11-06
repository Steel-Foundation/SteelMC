//! Path following shared by the dragon's navigating phases, each of which repeats
//! `navigateToNextPathNode` in vanilla. Takeoff's extra `advance()` stays at its call site.

use glam::DVec3;

use crate::entity::Entity as _;
use crate::entity::ai::path::Path;
use crate::entity::entities::mobs::bosses::ender_dragon::EnderDragonEntity;

/// Bounds on the squared distance to a fly target that count as on course.
const MIN_TARGET_DISTANCE_SQR: f64 = 100.0;
const MAX_TARGET_DISTANCE_SQR: f64 = 22_500.0;

/// How far above a node the dragon may aim.
const NODE_HEIGHT_SPREAD: f32 = 20.0;

/// Whether the dragon is too close to, too far from, or blocked short of a target
/// `distance_sqr` away. The holding pattern re-targets then; the death phase treats it
/// as arrival.
pub fn is_off_course(dragon: &EnderDragonEntity, distance_sqr: f64) -> bool {
    distance_sqr < MIN_TARGET_DISTANCE_SQR
        || distance_sqr > MAX_TARGET_DISTANCE_SQR
        || dragon.horizontal_collision()
        || dragon.vertical_collision()
}

/// Advances `path` by one node and returns the point to steer at, or `None` if the path
/// is already finished (vanilla's "leave the target where it is" case).
pub fn navigate_to_next_path_node(path: &mut Path) -> Option<DVec3> {
    let node = path.next_node_pos()?;
    path.advance();

    // Vanilla's `do/while` retry for a height below the node can never fire. The sum is
    // `int + float`, so it is `f32` arithmetic widened only on assignment.
    let y = node.y() as f32 + rand::random::<f32>() * NODE_HEIGHT_SPREAD;

    Some(DVec3::new(
        f64::from(node.x()),
        f64::from(y),
        f64::from(node.z()),
    ))
}
