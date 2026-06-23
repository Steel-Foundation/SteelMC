//! `VoxelShape` collision operations.
//!
//! Implements vanilla's `Shapes` class methods for AABB-list based collision.

use steel_registry::blocks::properties::Direction;
use steel_registry::blocks::shapes::{
    OffsetVoxelShape, VoxelShape, is_offset_shape_full_block, is_shape_full_block,
};
use steel_utils::{BlockLocalAabb, BlockPos, WorldAabb, axis::Axis};

const COLLISION_EPSILON: f64 = 1.0e-7;

/// Computes the maximum safe movement along an axis for an entity AABB through a list of obstacle shapes.
///
/// This is the core collision function used by vanilla's `Shapes.collide()`.
///
/// # Arguments
/// * `axis` - The axis along which to move (X, Y, or Z)
/// * `entity_aabb` - The entity's current bounding box
/// * `shapes` - List of obstacle shapes (block collision boxes) to test against
/// * `desired_movement` - The desired movement distance along the axis
///
/// # Returns
/// The maximum safe movement that won't cause collision (may be less than `desired_movement`).
/// Returns the input value if no collision occurs.
///
/// # Algorithm
/// For each obstacle AABB, check if the entity AABB (moved by `desired_movement` on the given axis)
/// would intersect on the other two axes. If so, clip the movement to stop at the obstacle's face.
///
/// Matches: `net.minecraft.world.phys.shapes.Shapes.collide(Direction.Axis, AABB, List<AABB>, double)`
#[must_use]
pub fn collide(
    axis: Axis,
    entity_aabb: &WorldAabb,
    shapes: &[WorldAabb],
    desired_movement: f64,
) -> f64 {
    if desired_movement.abs() < COLLISION_EPSILON {
        return 0.0;
    }

    let mut movement = desired_movement;

    for shape in shapes {
        movement = collide_single(axis, entity_aabb, shape, movement);

        if movement.abs() < COLLISION_EPSILON {
            return 0.0;
        }
    }

    movement
}

fn collide_single(
    axis: Axis,
    entity_aabb: &WorldAabb,
    obstacle: &WorldAabb,
    desired_movement: f64,
) -> f64 {
    let (first_cross_axis, second_cross_axis) = cross_axes(axis);
    if !overlaps_for_collision(entity_aabb, obstacle, first_cross_axis)
        || !overlaps_for_collision(entity_aabb, obstacle, second_cross_axis)
    {
        return desired_movement;
    }

    if desired_movement > 0.0 {
        let max_move = obstacle.min(axis) - entity_aabb.max(axis);
        if max_move >= -COLLISION_EPSILON && max_move < desired_movement {
            max_move
        } else {
            desired_movement
        }
    } else {
        let max_move = obstacle.max(axis) - entity_aabb.min(axis);
        if max_move <= COLLISION_EPSILON && max_move > desired_movement {
            max_move
        } else {
            desired_movement
        }
    }
}

const fn cross_axes(axis: Axis) -> (Axis, Axis) {
    match axis {
        Axis::X => (Axis::Y, Axis::Z),
        Axis::Y => (Axis::X, Axis::Z),
        Axis::Z => (Axis::X, Axis::Y),
    }
}

fn overlaps_for_collision(entity_aabb: &WorldAabb, obstacle: &WorldAabb, axis: Axis) -> bool {
    // Vanilla looks up cross-axis cells using min + epsilon and max - epsilon.
    entity_aabb.max(axis) - COLLISION_EPSILON > obstacle.min(axis)
        && entity_aabb.min(axis) + COLLISION_EPSILON < obstacle.max(axis)
}

/// Tests if two shapes have a non-empty intersection (boolean AND operation).
///
/// This is used for "new collision" detection in movement validation.
///
/// # Arguments
/// * `aabb1` - First AABB (typically entity's position after movement)
/// * `aabb2` - Second AABB (typically a block collision shape)
///
/// # Returns
/// `true` if the AABBs intersect (have overlapping volume), `false` otherwise.
///
/// Matches: `Shapes.joinIsNotEmpty(shape1, shape2, BooleanOp.AND)`
#[must_use]
pub fn join_is_not_empty(aabb1: &WorldAabb, aabb2: &WorldAabb) -> bool {
    aabb1.intersects(*aabb2)
}

/// Translates a `VoxelShape` (block-local AABB) to world coordinates.
///
/// # Arguments
/// * `shape` - Block-local AABB (0.0-1.0 space)
/// * `block_pos` - World position of the block
///
/// # Returns
/// World-space AABB at the block position.
#[must_use]
pub fn translate_shape(shape: &BlockLocalAabb, block_pos: BlockPos) -> WorldAabb {
    shape.at_block(block_pos)
}

/// Checks if two voxel shapes fully occlude the face between them.
/// Returns true if fluid/objects cannot pass through the face.
///
/// Direct equivalent of vanilla's `Shapes.mergedFaceOccludes(shape1, shape2, direction)`.
///
/// The algorithm:
/// 1. Fast path: if **either** shape is a full cube → `true` (face fully sealed).
/// 2. For each shape, keep only the face slice that actually touches the shared
///    face boundary (shapes that don't reach the boundary contribute nothing).
/// 3. Project both slices onto a 16×16 rasterisation grid and check if their
///    union covers all 256 pixels.
///
/// Note: vanilla uses exact discrete-voxel arithmetic; the 16×16 rasterisation
/// used here is equivalent for all vanilla block shapes (aligned to 1/16) but
/// may have floating-point rounding for non-standard shapes from future mods.
#[must_use]
pub fn merged_face_occludes(shape1: VoxelShape, shape2: VoxelShape, direction: Direction) -> bool {
    // Fast path — vanilla: if EITHER shape is a full block the face is sealed.
    // (SteelMC previously required BOTH to be full — that was wrong.)
    let is_s1_full = is_shape_full_block(shape1);
    let is_s2_full = is_shape_full_block(shape2);

    if is_s1_full || is_s2_full {
        return true;
    }

    if shape1.is_empty() && shape2.is_empty() {
        return false;
    }

    // Vanilla assigns shape3 / shape4 based on axis direction, then zeroes out
    // any shape that does not actually touch the shared face boundary.
    // We replicate this by passing the expected face to project_shape_onto_grid:
    // shape1 contributes via the face it presents *toward* `direction` (its max face).
    // shape2 contributes via the face it presents *against* `direction` (its min face).
    // project_shape_onto_grid already checks `touches_face` per AABB, which is
    // equivalent to vanilla's per-shape boundary check for single-AABB shapes.

    let mut grid = [false; 256];
    let mut coverage_count = 0;

    // Project shape1 on the face it presents in `direction`
    coverage_count += project_shape_onto_grid(shape1, direction, &mut grid);
    if coverage_count == 256 {
        return true;
    }

    // Project shape2 on the face it presents against `direction`
    coverage_count += project_shape_onto_grid(shape2, direction.opposite(), &mut grid);
    coverage_count == 256
}

/// Checks if two position-offset voxel shapes fully occlude the face between them.
///
/// This is the offset-aware form used by block states whose collision shape
/// depends on `BlockState.getOffset(level, pos)`.
#[must_use]
pub fn merged_offset_face_occludes(
    shape1: OffsetVoxelShape,
    shape2: OffsetVoxelShape,
    direction: Direction,
) -> bool {
    if is_offset_shape_full_block(shape1) || is_offset_shape_full_block(shape2) {
        return true;
    }

    if shape1.is_empty() && shape2.is_empty() {
        return false;
    }

    let mut grid = [false; 256];
    let mut coverage_count = 0;

    coverage_count += project_offset_shape_onto_grid(shape1, direction, &mut grid);
    if coverage_count == 256 {
        return true;
    }

    coverage_count += project_offset_shape_onto_grid(shape2, direction.opposite(), &mut grid);
    coverage_count == 256
}

/// Checks whether two face occlusion shapes fully cover a block face.
#[must_use]
pub fn face_shape_occludes(
    shape1: VoxelShape,
    shape1_face: Direction,
    shape2: VoxelShape,
    shape2_face: Direction,
) -> bool {
    if is_shape_full_block(shape1) || is_shape_full_block(shape2) {
        return true;
    }

    if shape1.is_empty() && shape2.is_empty() {
        return false;
    }

    let mut grid = [false; 256];
    let mut coverage_count = 0;

    coverage_count += project_shape_onto_grid(shape1, shape1_face, &mut grid);
    if coverage_count == 256 {
        return true;
    }

    coverage_count += project_shape_onto_grid(shape2, shape2_face, &mut grid);
    coverage_count == 256
}

fn project_shape_onto_grid(shape: VoxelShape, face: Direction, grid: &mut [bool; 256]) -> usize {
    let mut added_coverage = 0;

    for aabb in shape {
        let touches_face = match face {
            Direction::Down => aabb.min_y() <= 1.0e-5,
            Direction::Up => aabb.max_y() >= 1.0 - 1.0e-5,
            Direction::North => aabb.min_z() <= 1.0e-5,
            Direction::South => aabb.max_z() >= 1.0 - 1.0e-5,
            Direction::West => aabb.min_x() <= 1.0e-5,
            Direction::East => aabb.max_x() >= 1.0 - 1.0e-5,
        };

        if !touches_face {
            continue;
        }

        let (min_u, max_u, min_v, max_v) = match face {
            Direction::Down | Direction::Up => {
                (aabb.min_x(), aabb.max_x(), aabb.min_z(), aabb.max_z())
            }
            Direction::North | Direction::South => {
                (aabb.min_x(), aabb.max_x(), aabb.min_y(), aabb.max_y())
            }
            Direction::West | Direction::East => {
                (aabb.min_z(), aabb.max_z(), aabb.min_y(), aabb.max_y())
            }
        };

        let u_start = ((min_u * 16.0).round() as i32).clamp(0, 16) as usize;
        let u_end = ((max_u * 16.0).round() as i32).clamp(0, 16) as usize;
        let v_start = ((min_v * 16.0).round() as i32).clamp(0, 16) as usize;
        let v_end = ((max_v * 16.0).round() as i32).clamp(0, 16) as usize;

        for u in u_start..u_end {
            for v in v_start..v_end {
                let idx = u * 16 + v;
                if !grid[idx] {
                    grid[idx] = true;
                    added_coverage += 1;
                }
            }
        }
    }

    added_coverage
}

fn project_offset_shape_onto_grid(
    shape: OffsetVoxelShape,
    face: Direction,
    grid: &mut [bool; 256],
) -> usize {
    let mut added_coverage = 0;

    for aabb in shape.iter() {
        let touches_face = match face {
            Direction::Down => aabb.min_y() <= 1.0e-5,
            Direction::Up => aabb.max_y() >= 1.0 - 1.0e-5,
            Direction::North => aabb.min_z() <= 1.0e-5,
            Direction::South => aabb.max_z() >= 1.0 - 1.0e-5,
            Direction::West => aabb.min_x() <= 1.0e-5,
            Direction::East => aabb.max_x() >= 1.0 - 1.0e-5,
        };

        if !touches_face {
            continue;
        }

        let (min_u, max_u, min_v, max_v) = match face {
            Direction::Down | Direction::Up => {
                (aabb.min_x(), aabb.max_x(), aabb.min_z(), aabb.max_z())
            }
            Direction::North | Direction::South => {
                (aabb.min_x(), aabb.max_x(), aabb.min_y(), aabb.max_y())
            }
            Direction::West | Direction::East => {
                (aabb.min_z(), aabb.max_z(), aabb.min_y(), aabb.max_y())
            }
        };

        let u_start = ((min_u * 16.0).round() as i32).clamp(0, 16) as usize;
        let u_end = ((max_u * 16.0).round() as i32).clamp(0, 16) as usize;
        let v_start = ((min_v * 16.0).round() as i32).clamp(0, 16) as usize;
        let v_end = ((max_v * 16.0).round() as i32).clamp(0, 16) as usize;

        for u in u_start..u_end {
            for v in v_start..v_end {
                let idx = u * 16 + v;
                if !grid[idx] {
                    grid[idx] = true;
                    added_coverage += 1;
                }
            }
        }
    }

    added_coverage
}

#[cfg(test)]
#[expect(clippy::float_cmp, reason = "exact match against vanilla test vectors")]
mod tests {
    use super::*;

    #[test]
    fn test_collide_no_obstacle() {
        let entity = WorldAabb::new(0.0, 0.0, 0.0, 1.0, 1.0, 1.0);

        let result = collide(Axis::X, &entity, &[], 5.0);
        assert_eq!(result, 5.0, "Should move full distance with no obstacles");
    }

    #[test]
    fn test_collide_with_obstacle() {
        let entity = WorldAabb::new(0.0, 0.0, 0.0, 1.0, 1.0, 1.0);

        // Obstacle at x=2, blocking positive X movement
        let obstacle = WorldAabb::new(2.0, 0.0, 0.0, 3.0, 1.0, 1.0);

        let result = collide(Axis::X, &entity, &[obstacle], 5.0);
        assert_eq!(
            result, 1.0,
            "Should stop at obstacle face (2.0 - 1.0 = 1.0)"
        );
    }

    #[test]
    fn test_collide_no_overlap_on_other_axes() {
        let entity = WorldAabb::new(0.0, 0.0, 0.0, 1.0, 1.0, 1.0);

        // Obstacle at x=2 but y=5 (no Y overlap)
        let obstacle = WorldAabb::new(2.0, 5.0, 0.0, 3.0, 6.0, 1.0);

        let result = collide(Axis::X, &entity, &[obstacle], 5.0);
        assert_eq!(result, 5.0, "Should ignore obstacle with no Y overlap");
    }

    #[test]
    fn collide_ignores_cross_axis_overlap_below_vanilla_epsilon() {
        let entity = WorldAabb::new(0.0, 0.0, 0.0, 1.0, 1.0, 1.0);
        let obstacle = WorldAabb::new(2.0, 1.0 - 0.5e-7, 0.0, 3.0, 2.0, 1.0);

        let result = collide(Axis::X, &entity, &[obstacle], 5.0);
        assert_eq!(result, 5.0);
    }

    #[test]
    fn collide_keeps_cross_axis_overlap_above_vanilla_epsilon() {
        let entity = WorldAabb::new(0.0, 0.0, 0.0, 1.0, 1.0, 1.0);
        let obstacle = WorldAabb::new(2.0, 1.0 - 2.0e-7, 0.0, 3.0, 2.0, 1.0);

        let result = collide(Axis::X, &entity, &[obstacle], 5.0);
        assert_eq!(result, 1.0);
    }

    #[test]
    fn test_join_is_not_empty_intersecting() {
        let aabb1 = WorldAabb::new(0.0, 0.0, 0.0, 2.0, 2.0, 2.0);
        let aabb2 = WorldAabb::new(1.0, 1.0, 1.0, 3.0, 3.0, 3.0);

        assert!(
            join_is_not_empty(&aabb1, &aabb2),
            "Overlapping AABBs should intersect"
        );
    }

    #[test]
    fn test_join_is_not_empty_non_intersecting() {
        let aabb1 = WorldAabb::new(0.0, 0.0, 0.0, 1.0, 1.0, 1.0);
        let aabb2 = WorldAabb::new(2.0, 2.0, 2.0, 3.0, 3.0, 3.0);

        assert!(
            !join_is_not_empty(&aabb1, &aabb2),
            "Separate AABBs should not intersect"
        );
    }

    #[test]
    fn test_translate_shape() {
        let shape = BlockLocalAabb::new(0.0, 0.0, 0.0, 1.0, 0.5, 1.0); // Half slab
        let block_pos = BlockPos::new(10, 64, -5);

        let result = translate_shape(&shape, block_pos);

        assert_eq!(result.min_x(), 10.0);
        assert_eq!(result.min_y(), 64.0);
        assert_eq!(result.min_z(), -5.0);
        assert_eq!(result.max_x(), 11.0);
        assert_eq!(result.max_y(), 64.5);
        assert_eq!(result.max_z(), -4.0);
    }

    #[test]
    fn merged_offset_face_occludes_respects_shape_offset() {
        let shifted_up =
            OffsetVoxelShape::new(VoxelShape::FULL_BLOCK, glam::DVec3::new(0.0, 0.25, 0.0));
        let empty = OffsetVoxelShape::without_offset(VoxelShape::EMPTY);

        assert!(!merged_offset_face_occludes(
            shifted_up,
            empty,
            Direction::Down
        ));
    }
}
