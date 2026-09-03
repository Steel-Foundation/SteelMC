//! Vanilla walk path-type classification.

mod collision;
pub mod node_evaluator;
mod path_evaluator;
mod settings;
pub mod swim_node_evaluator;

pub use collision::WalkNodeCollision;
pub use node_evaluator::{WalkNeighbors, WalkNodeEvaluator};
pub use path_evaluator::WalkPathEvaluator;
use path_evaluator::does_block_have_partial_collision;
pub use settings::MobPathSettings;
pub use swim_node_evaluator::SwimNodeEvaluator;

use crate::behavior::{BLOCK_BEHAVIORS, BlockCollisionContext, BlockStateBehaviorExt as _};
use crate::entity::Mob;
use crate::entity::ai::node::Node;
use crate::entity::ai::node::NodeStore;
use crate::entity::ai::path::PathfindingContext;
use crate::entity::ai::path::{PathComputationType, PathType, PathTypeSet, PathfindingMalus};
use crate::fluid::FluidStateExt as _;
use crate::world::LevelReader;
use steel_math::fast_floor;
use steel_registry::blocks::block_state_ext::BlockStateExt as _;
use steel_registry::blocks::properties::BlockStateProperties;
use steel_registry::fluid::FluidState;
use steel_registry::vanilla_block_tags::BlockTag;
use steel_registry::vanilla_blocks;
use steel_utils::{BlockPos, Direction, WorldAabb, axis::Axis};

/// The evaluator surface consumed by the A* [`crate::entity::ai::pathfinder::PathFinder`].
///
/// Vanilla's `NodeEvaluator` hierarchy (walk `WalkNodeEvaluator` vs aquatic
/// `SwimNodeEvaluator`) maps onto this trait.
pub trait NodeEvaluator {
    fn reset_search_state(&mut self);

    fn node(&self, hash: i32) -> Option<&Node>;

    fn node_mut(&mut self, hash: i32) -> Option<&mut Node>;

    fn nodes_mut(&mut self) -> &mut crate::entity::ai::node::NodeStore;

    /// Returns the hash of the search start node.
    fn get_start(&mut self, context: &mut PathfindingContext<'_>) -> i32;

    /// Returns the hashes of the reachable neighbor nodes of `pos_hash`.
    fn get_neighbors(
        &mut self,
        context: &mut PathfindingContext<'_>,
        collision: &mut impl WalkNodeCollision,
        pos_hash: i32,
    ) -> WalkNeighbors;
}
