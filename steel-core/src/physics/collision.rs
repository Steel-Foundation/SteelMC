//! World collision queries for physics simulation.

use std::sync::Arc;

use glam::DVec3;
use steel_registry::{
    blocks::{block_state_ext::BlockStateExt, shapes::VoxelShape},
    vanilla_blocks,
};
use steel_utils::{BlockPos, BlockStateId, WorldAabb};

use crate::physics::shapes::translate_shape;
use crate::world::World;

const BLOCK_COLLISION_EPSILON: f64 = 1.0e-7;

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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct BlockCollisionSearchBounds {
    min_x: i32,
    min_y: i32,
    min_z: i32,
    max_x: i32,
    max_y: i32,
    max_z: i32,
}

impl BlockCollisionSearchBounds {
    fn from_aabb(aabb: &WorldAabb) -> Self {
        Self {
            min_x: (aabb.min_x() - BLOCK_COLLISION_EPSILON).floor() as i32 - 1,
            min_y: (aabb.min_y() - BLOCK_COLLISION_EPSILON).floor() as i32 - 1,
            min_z: (aabb.min_z() - BLOCK_COLLISION_EPSILON).floor() as i32 - 1,
            max_x: (aabb.max_x() + BLOCK_COLLISION_EPSILON).floor() as i32 + 1,
            max_y: (aabb.max_y() + BLOCK_COLLISION_EPSILON).floor() as i32 + 1,
            max_z: (aabb.max_z() + BLOCK_COLLISION_EPSILON).floor() as i32 + 1,
        }
    }

    fn cursor_type(self, x: i32, y: i32, z: i32) -> CollisionCursorType {
        let boundary_axis_count = u8::from(x == self.min_x || x == self.max_x)
            + u8::from(y == self.min_y || y == self.max_y)
            + u8::from(z == self.min_z || z == self.max_z);

        match boundary_axis_count {
            0 => CollisionCursorType::Inside,
            1 => CollisionCursorType::Face,
            2 => CollisionCursorType::Edge,
            _ => CollisionCursorType::Corner,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CollisionCursorType {
    Inside,
    Face,
    Edge,
    Corner,
}

fn should_query_collision_shape(
    block_state: BlockStateId,
    collision_shape: VoxelShape,
    cursor_type: CollisionCursorType,
) -> bool {
    match cursor_type {
        CollisionCursorType::Inside => true,
        CollisionCursorType::Face => {
            block_state.get_block().config.dynamic_shape
                || collision_shape.has_large_collision_shape()
        }
        CollisionCursorType::Edge => block_state.get_block() == &vanilla_blocks::MOVING_PISTON,
        CollisionCursorType::Corner => false,
    }
}

impl<'a> WorldCollisionProvider<'a> {
    /// Creates a new collision provider for the given world.
    pub const fn new(world: &'a Arc<World>) -> Self {
        Self { world }
    }

    /// Finds the block supporting an entity within `aabb`.
    ///
    /// Mirrors vanilla `CollisionGetter.findSupportingBlock`: among colliding
    /// blocks, choose the closest block center to the entity position, then use
    /// vanilla `BlockPos` ordering as a tie-breaker.
    #[must_use]
    #[expect(
        clippy::float_cmp,
        reason = "intentional: vanilla compares equal support distances exactly"
    )]
    pub fn find_supporting_block(
        &self,
        entity_position: DVec3,
        aabb: &WorldAabb,
    ) -> Option<BlockPos> {
        let bounds = BlockCollisionSearchBounds::from_aabb(aabb);

        let mut main_support = None;
        let mut main_support_distance = f64::MAX;

        for y in bounds.min_y..=bounds.max_y {
            for z in bounds.min_z..=bounds.max_z {
                for x in bounds.min_x..=bounds.max_x {
                    let cursor_type = bounds.cursor_type(x, y, z);
                    if cursor_type == CollisionCursorType::Corner {
                        continue;
                    }

                    let block_pos = BlockPos::new(x, y, z);
                    let block_state = self.world.get_block_state(block_pos);
                    if block_state.is_air() {
                        continue;
                    }

                    let collision_shape = block_state.get_collision_shape();
                    if collision_shape.is_empty() {
                        continue;
                    }
                    if !should_query_collision_shape(block_state, collision_shape, cursor_type) {
                        continue;
                    }

                    let supports_entity = collision_shape
                        .into_iter()
                        .map(|shape_aabb| translate_shape(shape_aabb, block_pos))
                        .any(|world_aabb| aabb.intersects(world_aabb));
                    if !supports_entity {
                        continue;
                    }

                    let distance = block_pos_center_distance_sq(block_pos, entity_position);
                    let should_replace = distance < main_support_distance
                        || distance == main_support_distance
                            && main_support
                                .is_none_or(|support| vanilla_block_pos_less(support, block_pos));

                    if should_replace {
                        main_support = Some(block_pos);
                        main_support_distance = distance;
                    }
                }
            }
        }

        main_support
    }
}

fn block_pos_center_distance_sq(pos: BlockPos, point: DVec3) -> f64 {
    let dx = f64::from(pos.x()) + 0.5 - point.x;
    let dy = f64::from(pos.y()) + 0.5 - point.y;
    let dz = f64::from(pos.z()) + 0.5 - point.z;
    dx * dx + dy * dy + dz * dz
}

const fn vanilla_block_pos_less(left: BlockPos, right: BlockPos) -> bool {
    left.y() < right.y()
        || left.y() == right.y()
            && (left.z() < right.z() || left.z() == right.z() && left.x() < right.x())
}

impl CollisionWorld for WorldCollisionProvider<'_> {
    fn get_block_state(&self, pos: BlockPos) -> BlockStateId {
        self.world.get_block_state(pos)
    }

    fn get_block_collisions(&self, aabb: &WorldAabb) -> Vec<WorldAabb> {
        let mut collisions = Vec::new();

        let bounds = BlockCollisionSearchBounds::from_aabb(aabb);

        for y in bounds.min_y..=bounds.max_y {
            for z in bounds.min_z..=bounds.max_z {
                for x in bounds.min_x..=bounds.max_x {
                    let cursor_type = bounds.cursor_type(x, y, z);
                    if cursor_type == CollisionCursorType::Corner {
                        continue;
                    }

                    let block_pos = BlockPos::new(x, y, z);
                    let block_state = self.world.get_block_state(block_pos);

                    if block_state.is_air() {
                        continue;
                    }

                    let collision_shape = block_state.get_collision_shape();

                    if collision_shape.is_empty() {
                        continue;
                    }
                    if !should_query_collision_shape(block_state, collision_shape, cursor_type) {
                        continue;
                    }

                    for shape_aabb in collision_shape {
                        let world_aabb = translate_shape(shape_aabb, block_pos);

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
    use steel_registry::test_support;
    use steel_utils::BlockLocalAabb;

    const LARGE_COLLISION_SHAPE: &[BlockLocalAabb] =
        &[BlockLocalAabb::new(-0.25, 0.0, 0.0, 1.0, 1.0, 1.0)];

    #[test]
    fn test_intersects_aabb() {
        let aabb1 = WorldAabb::new(0.0, 0.0, 0.0, 2.0, 2.0, 2.0);
        let aabb2 = WorldAabb::new(1.0, 1.0, 1.0, 3.0, 3.0, 3.0);

        assert!(aabb1.intersects(aabb2));

        let aabb3 = WorldAabb::new(5.0, 5.0, 5.0, 6.0, 6.0, 6.0);

        assert!(!aabb1.intersects(aabb3));
    }

    #[test]
    fn supporting_block_tie_breaker_matches_vanilla_ordering() {
        assert!(vanilla_block_pos_less(
            BlockPos::new(0, 0, 0),
            BlockPos::new(0, 1, 0)
        ));
        assert!(vanilla_block_pos_less(
            BlockPos::new(0, 1, 0),
            BlockPos::new(0, 1, 1)
        ));
        assert!(vanilla_block_pos_less(
            BlockPos::new(0, 1, 1),
            BlockPos::new(1, 1, 1)
        ));
        assert!(!vanilla_block_pos_less(
            BlockPos::new(1, 1, 1),
            BlockPos::new(0, 1, 1)
        ));
    }

    #[test]
    fn supporting_block_distance_uses_block_center() {
        let distance =
            block_pos_center_distance_sq(BlockPos::new(1, 2, 3), DVec3::new(1.5, 1.5, 5.5));

        assert!((distance - 5.0).abs() < f64::EPSILON);
    }

    #[test]
    fn block_collision_search_bounds_match_vanilla_epsilon_range() {
        let bounds =
            BlockCollisionSearchBounds::from_aabb(&WorldAabb::new(0.0, 0.25, 0.0, 1.0, 1.0, 1.0));

        assert_eq!(bounds.min_x, -2);
        assert_eq!(bounds.max_x, 2);
        assert_eq!(bounds.min_y, -1);
        assert_eq!(bounds.max_y, 2);
        assert_eq!(bounds.min_z, -2);
        assert_eq!(bounds.max_z, 2);
    }

    #[test]
    fn collision_cursor_type_matches_vanilla_boundary_count() {
        let bounds = BlockCollisionSearchBounds::from_aabb(&WorldAabb::new(
            0.25, 0.25, 0.25, 0.75, 0.75, 0.75,
        ));

        assert_eq!(bounds.cursor_type(0, 0, 0), CollisionCursorType::Inside);
        assert_eq!(
            bounds.cursor_type(bounds.min_x, 0, 0),
            CollisionCursorType::Face
        );
        assert_eq!(
            bounds.cursor_type(bounds.min_x, bounds.min_y, 0),
            CollisionCursorType::Edge
        );
        assert_eq!(
            bounds.cursor_type(bounds.min_x, bounds.min_y, bounds.min_z),
            CollisionCursorType::Corner
        );
    }

    #[test]
    fn collision_shape_filter_matches_vanilla_cursor_rules() {
        test_support::init_test_registry();

        let stone = vanilla_blocks::STONE.default_state();
        let moving_piston = vanilla_blocks::MOVING_PISTON.default_state();
        let large_shape = VoxelShape::from_boxes(LARGE_COLLISION_SHAPE);

        assert!(should_query_collision_shape(
            stone,
            VoxelShape::FULL_BLOCK,
            CollisionCursorType::Inside
        ));
        assert!(!should_query_collision_shape(
            stone,
            VoxelShape::FULL_BLOCK,
            CollisionCursorType::Face
        ));
        assert!(should_query_collision_shape(
            stone,
            large_shape,
            CollisionCursorType::Face
        ));
        assert!(!should_query_collision_shape(
            stone,
            large_shape,
            CollisionCursorType::Edge
        ));
        assert!(should_query_collision_shape(
            moving_piston,
            VoxelShape::FULL_BLOCK,
            CollisionCursorType::Edge
        ));
        assert!(!should_query_collision_shape(
            moving_piston,
            large_shape,
            CollisionCursorType::Corner
        ));
    }
}
