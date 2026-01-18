//! World collision queries for physics simulation.

use steel_registry::blocks::block_state_ext::BlockStateExt;
use steel_registry::blocks::shapes::AABBd;
use steel_utils::math::Vector3;
use steel_utils::{BlockPos, BlockStateId};

use crate::physics::shapes::translate_shape;
use crate::world::World;

/// Trait for querying collision shapes from the world.
///
/// This abstraction allows testing physics without a full world instance.
pub trait CollisionWorld {
    /// Gets the block state at the given position.
    fn get_block_state(&self, pos: &BlockPos) -> BlockStateId;

    /// Queries all block collision shapes that intersect with the given AABB.
    ///
    /// Returns a list of world-space AABBs representing solid block collisions.
    fn get_block_collisions(&self, aabb: &AABBd) -> Vec<AABBd>;

    /// Gets collision shapes needed for pre-move checks (sneak edge prevention).
    ///
    /// # Arguments
    /// * `aabb` - The entity's bounding box after intended movement
    /// * `old_bottom_center` - The entity's bottom-center position before movement
    ///
    /// # Returns
    /// Collision shapes at the blocks beneath the old position (for sneak checks).
    ///
    /// Matches vanilla's logic in `ServerGamePacketListenerImpl.handleMovePlayer()` where
    /// it checks blocks at the old Y position to detect edge cases.
    fn get_pre_move_collisions(&self, aabb: &AABBd, old_bottom_center: Vector3<f64>) -> Vec<AABBd>;
}

/// Implements `CollisionWorld` for the Steel World struct.
pub struct WorldCollisionProvider<'a> {
    world: &'a World,
}

impl<'a> WorldCollisionProvider<'a> {
    /// Creates a new collision provider for the given world.
    pub fn new(world: &'a World) -> Self {
        Self { world }
    }
}

impl CollisionWorld for WorldCollisionProvider<'_> {
    fn get_block_state(&self, pos: &BlockPos) -> BlockStateId {
        self.world.get_block_state(pos)
    }

    fn get_block_collisions(&self, aabb: &AABBd) -> Vec<AABBd> {
        let mut collisions = Vec::new();

        // Calculate block bounds from AABB (vanilla uses BlockPos.betweenClosed)
        let min_x = aabb.min_x.floor() as i32;
        let min_y = aabb.min_y.floor() as i32;
        let min_z = aabb.min_z.floor() as i32;
        let max_x = aabb.max_x.ceil() as i32;
        let max_y = aabb.max_y.ceil() as i32;
        let max_z = aabb.max_z.ceil() as i32;

        // Iterate over all blocks that could intersect
        for y in min_y..=max_y {
            for z in min_z..=max_z {
                for x in min_x..=max_x {
                    let block_pos = BlockPos::new(x, y, z);
                    let block_state = self.world.get_block_state(&block_pos);

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

                    // Translate each AABB in the shape to world coordinates
                    let block_pos_vec = Vector3::new(x, y, z);
                    for shape_aabb in collision_shape {
                        let world_aabb = translate_shape(shape_aabb, block_pos_vec);

                        // Only include if it actually intersects our query AABB
                        if intersects_aabb(aabb, &world_aabb) {
                            collisions.push(world_aabb);
                        }
                    }
                }
            }
        }

        collisions
    }

    fn get_pre_move_collisions(&self, aabb: &AABBd, old_bottom_center: Vector3<f64>) -> Vec<AABBd> {
        let mut collisions = Vec::new();

        // Check blocks at the old Y position (for sneak edge detection)
        // We check a small area around the old position to catch edge cases
        let check_min_x = (old_bottom_center.x - 0.3).floor() as i32;
        let check_max_x = (old_bottom_center.x + 0.3).ceil() as i32;
        let check_min_z = (old_bottom_center.z - 0.3).floor() as i32;
        let check_max_z = (old_bottom_center.z + 0.3).ceil() as i32;
        let check_y = old_bottom_center.y.floor() as i32;

        for z in check_min_z..=check_max_z {
            for x in check_min_x..=check_max_x {
                let block_pos = BlockPos::new(x, check_y - 1, z); // Check block below feet
                let block_state = self.world.get_block_state(&block_pos);

                if block_state.is_air() {
                    continue;
                }

                let collision_shape = block_state.get_collision_shape();
                if collision_shape.is_empty() {
                    continue;
                }

                let block_pos_vec = Vector3::new(x, check_y - 1, z);
                for shape_aabb in collision_shape {
                    let world_aabb = translate_shape(shape_aabb, block_pos_vec);
                    if intersects_aabb(aabb, &world_aabb) {
                        collisions.push(world_aabb);
                    }
                }
            }
        }

        collisions
    }
}

/// Helper function to check if two AABBs intersect.
#[inline]
fn intersects_aabb(a: &AABBd, b: &AABBd) -> bool {
    a.max_x > b.min_x
        && a.min_x < b.max_x
        && a.max_y > b.min_y
        && a.min_y < b.max_y
        && a.max_z > b.min_z
        && a.min_z < b.max_z
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_intersects_aabb() {
        let aabb1 = AABBd {
            min_x: 0.0,
            min_y: 0.0,
            min_z: 0.0,
            max_x: 2.0,
            max_y: 2.0,
            max_z: 2.0,
        };

        let aabb2 = AABBd {
            min_x: 1.0,
            min_y: 1.0,
            min_z: 1.0,
            max_x: 3.0,
            max_y: 3.0,
            max_z: 3.0,
        };

        assert!(intersects_aabb(&aabb1, &aabb2));

        let aabb3 = AABBd {
            min_x: 5.0,
            min_y: 5.0,
            min_z: 5.0,
            max_x: 6.0,
            max_y: 6.0,
            max_z: 6.0,
        };

        assert!(!intersects_aabb(&aabb1, &aabb3));
    }
}
