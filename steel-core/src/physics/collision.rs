//! World collision queries for physics simulation.

use std::sync::Arc;

use glam::DVec3;
use steel_registry::blocks::block_state_ext::BlockStateExt;
use steel_utils::{BlockPos, BlockStateId, WorldAabb};

use crate::physics::shapes::translate_shape;
use crate::world::World;

/// Trait for querying collision shapes from the world.
///
/// This abstraction allows testing physics without a full world instance.
pub trait CollisionWorld {
    /// Gets the block state at the given position.
    fn get_block_state(&self, pos: BlockPos) -> BlockStateId;

    /// Queries all block collision shapes that intersect with the given AABB.
    ///
    /// Returns a list of world-space AABBs representing solid block collisions.
    fn get_block_collisions(&self, aabb: &WorldAabb) -> Vec<WorldAabb>;

    /// Gets collision shapes for vanilla pre-move checks.
    ///
    /// # Arguments
    /// * `aabb` - The entity's bounding box after intended movement
    /// * `old_bottom_center` - The entity's bottom-center position before movement
    ///
    /// # Returns
    /// Collision shapes intersecting the target box.
    ///
    /// Vanilla uses the old bottom-center Y as collision context. Steel block
    /// collision shapes are not context-sensitive yet, so this currently matches
    /// the block-collision portion of that query.
    fn get_pre_move_collisions(&self, aabb: &WorldAabb, old_bottom_center: DVec3)
    -> Vec<WorldAabb>;
}

/// Implements `CollisionWorld` for the Steel World struct.
pub struct WorldCollisionProvider<'a> {
    world: &'a Arc<World>,
}

impl<'a> WorldCollisionProvider<'a> {
    /// Creates a new collision provider for the given world.
    pub const fn new(world: &'a Arc<World>) -> Self {
        Self { world }
    }
}

impl CollisionWorld for WorldCollisionProvider<'_> {
    fn get_block_state(&self, pos: BlockPos) -> BlockStateId {
        self.world.get_block_state(pos)
    }

    fn get_block_collisions(&self, aabb: &WorldAabb) -> Vec<WorldAabb> {
        let mut collisions = Vec::new();

        // Calculate block bounds from AABB (vanilla uses BlockPos.betweenClosed)
        let min_x = aabb.min_x().floor() as i32;
        let min_y = aabb.min_y().floor() as i32;
        let min_z = aabb.min_z().floor() as i32;
        let max_x = aabb.max_x().ceil() as i32;
        let max_y = aabb.max_y().ceil() as i32;
        let max_z = aabb.max_z().ceil() as i32;

        // Iterate over all blocks that could intersect
        for y in min_y..=max_y {
            for z in min_z..=max_z {
                for x in min_x..=max_x {
                    let block_pos = BlockPos::new(x, y, z);
                    let block_state = self.world.get_block_state(block_pos);

                    // Skip air blocks
                    if block_state.is_air() {
                        continue;
                    }

                    // Get collision shape for this block
                    let collision_shape = block_state.get_collision_shape();

                    // Skip blocks with no collision
                    if collision_shape.is_empty() {
                        continue;
                    }

                    for shape_aabb in collision_shape {
                        let world_aabb = translate_shape(shape_aabb, block_pos);

                        // Only include if it actually intersects our query AABB
                        if aabb.intersects(world_aabb) {
                            collisions.push(world_aabb);
                        }
                    }
                }
            }
        }

        collisions
    }

    fn get_pre_move_collisions(
        &self,
        aabb: &WorldAabb,
        _old_bottom_center: DVec3,
    ) -> Vec<WorldAabb> {
        self.get_block_collisions(aabb)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_intersects_aabb() {
        let aabb1 = WorldAabb::new(0.0, 0.0, 0.0, 2.0, 2.0, 2.0);
        let aabb2 = WorldAabb::new(1.0, 1.0, 1.0, 3.0, 3.0, 3.0);

        assert!(aabb1.intersects(aabb2));

        let aabb3 = WorldAabb::new(5.0, 5.0, 5.0, 6.0, 6.0, 6.0);

        assert!(!aabb1.intersects(aabb3));
    }
}
