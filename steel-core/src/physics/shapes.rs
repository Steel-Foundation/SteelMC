//! `VoxelShape` collision operations.
//!
//! Implements vanilla's `Shapes` class methods for AABB-list based collision.
//! Uses the existing `VoxelShape` type (slice of AABBs) from steel-registry.

use steel_registry::blocks::properties::Direction;
use steel_registry::blocks::shapes::{AABB, AABBd, VoxelShape, is_shape_full_block};
use steel_utils::math::{Axis, Vector3};

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
pub fn collide(axis: Axis, entity_aabb: &AABBd, shapes: &[AABBd], desired_movement: f64) -> f64 {
    if desired_movement.abs() < 1.0e-7 {
        return 0.0;
    }

    let mut movement = desired_movement;

    for shape in shapes {
        movement = collide_single(axis, entity_aabb, shape, movement);

        // Early exit if movement is completely blocked
        if movement.abs() < 1.0e-7 {
            return 0.0;
        }
    }

    movement
}

/// Collides entity AABB against a single obstacle shape along the given axis.
///
/// DEBUG: Added extensive logging to trace collision issues.
fn collide_single(axis: Axis, entity_aabb: &AABBd, obstacle: &AABBd, desired_movement: f64) -> f64 {
    match axis {
        Axis::X => {
            // Check if entity and obstacle overlap on Y and Z axes
            if entity_aabb.max_y <= obstacle.min_y || entity_aabb.min_y >= obstacle.max_y {
                return desired_movement;
            }
            if entity_aabb.max_z <= obstacle.min_z || entity_aabb.min_z >= obstacle.max_z {
                return desired_movement;
            }

            // Calculate max movement before hitting obstacle
            if desired_movement > 0.0 {
                // Moving in positive X direction
                let max_move = obstacle.min_x - entity_aabb.max_x;
                // Only apply collision if obstacle is actually blocking (vanilla: newDistance >= -1.0E-7)
                if max_move >= -1.0e-7 && max_move < desired_movement {
                    max_move
                } else {
                    desired_movement
                }
            } else {
                // Moving in negative X direction
                let max_move = obstacle.max_x - entity_aabb.min_x;
                // Only apply collision if obstacle is actually blocking (vanilla: newDistance <= 1.0E-7)
                if max_move <= 1.0e-7 && max_move > desired_movement {
                    max_move
                } else {
                    desired_movement
                }
            }
        }
        Axis::Y => {
            // Check if entity and obstacle overlap on X and Z axes
            if entity_aabb.max_x <= obstacle.min_x || entity_aabb.min_x >= obstacle.max_x {
                return desired_movement;
            }
            if entity_aabb.max_z <= obstacle.min_z || entity_aabb.min_z >= obstacle.max_z {
                return desired_movement;
            }

            // Calculate max movement before hitting obstacle
            if desired_movement > 0.0 {
                // Moving in positive Y direction
                let max_move = obstacle.min_y - entity_aabb.max_y;
                // Only apply collision if obstacle is actually blocking (vanilla: newDistance >= -1.0E-7)

                if max_move >= -1.0e-7 && max_move < desired_movement {
                    max_move
                } else {
                    desired_movement
                }
            } else {
                // Moving in negative Y direction
                let max_move = obstacle.max_y - entity_aabb.min_y;
                // Only apply collision if obstacle is actually blocking (vanilla: newDistance <= 1.0E-7)

                if max_move <= 1.0e-7 && max_move > desired_movement {
                    max_move
                } else {
                    desired_movement
                }
            }
        }
        Axis::Z => {
            // Check if entity and obstacle overlap on X and Y axes
            if entity_aabb.max_x <= obstacle.min_x || entity_aabb.min_x >= obstacle.max_x {
                return desired_movement;
            }
            if entity_aabb.max_y <= obstacle.min_y || entity_aabb.min_y >= obstacle.max_y {
                return desired_movement;
            }

            // Calculate max movement before hitting obstacle
            if desired_movement > 0.0 {
                // Moving in positive Z direction
                let max_move = obstacle.min_z - entity_aabb.max_z;
                // Only apply collision if obstacle is actually blocking (vanilla: newDistance >= -1.0E-7)
                if max_move >= -1.0e-7 && max_move < desired_movement {
                    max_move
                } else {
                    desired_movement
                }
            } else {
                // Moving in negative Z direction
                let max_move = obstacle.max_z - entity_aabb.min_z;
                // Only apply collision if obstacle is actually blocking (vanilla: newDistance <= 1.0E-7)
                if max_move <= 1.0e-7 && max_move > desired_movement {
                    max_move
                } else {
                    desired_movement
                }
            }
        }
    }
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
pub fn join_is_not_empty(aabb1: &AABBd, aabb2: &AABBd) -> bool {
    aabb1.max_x > aabb2.min_x
        && aabb1.min_x < aabb2.max_x
        && aabb1.max_y > aabb2.min_y
        && aabb1.min_y < aabb2.max_y
        && aabb1.max_z > aabb2.min_z
        && aabb1.min_z < aabb2.max_z
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
pub fn translate_shape(shape: &AABB, block_pos: Vector3<i32>) -> AABBd {
    let bx = f64::from(block_pos.x);
    let by = f64::from(block_pos.y);
    let bz = f64::from(block_pos.z);

    AABBd {
        min_x: bx + f64::from(shape.min_x),
        min_y: by + f64::from(shape.min_y),
        min_z: bz + f64::from(shape.min_z),
        max_x: bx + f64::from(shape.max_x),
        max_y: by + f64::from(shape.max_y),
        max_z: bz + f64::from(shape.max_z),
    }
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

fn project_shape_onto_grid(shape: VoxelShape, face: Direction, grid: &mut [bool; 256]) -> usize {
    let mut added_coverage = 0;

    for aabb in shape {
        let touches_face = match face {
            Direction::Down => aabb.min_y <= 1.0e-5,
            Direction::Up => aabb.max_y >= 1.0 - 1.0e-5,
            Direction::North => aabb.min_z <= 1.0e-5,
            Direction::South => aabb.max_z >= 1.0 - 1.0e-5,
            Direction::West => aabb.min_x <= 1.0e-5,
            Direction::East => aabb.max_x >= 1.0 - 1.0e-5,
        };

        if !touches_face {
            continue;
        }

        let (min_u, max_u, min_v, max_v) = match face {
            Direction::Down | Direction::Up => (aabb.min_x, aabb.max_x, aabb.min_z, aabb.max_z),
            Direction::North | Direction::South => (aabb.min_x, aabb.max_x, aabb.min_y, aabb.max_y),
            Direction::West | Direction::East => (aabb.min_z, aabb.max_z, aabb.min_y, aabb.max_y),
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
#[allow(clippy::float_cmp)]
mod tests {
    use super::*;

    #[test]
    fn test_collide_no_obstacle() {
        let entity = AABBd {
            min_x: 0.0,
            min_y: 0.0,
            min_z: 0.0,
            max_x: 1.0,
            max_y: 1.0,
            max_z: 1.0,
        };

        let result = collide(Axis::X, &entity, &[], 5.0);
        assert_eq!(result, 5.0, "Should move full distance with no obstacles");
    }

    #[test]
    fn test_collide_with_obstacle() {
        let entity = AABBd {
            min_x: 0.0,
            min_y: 0.0,
            min_z: 0.0,
            max_x: 1.0,
            max_y: 1.0,
            max_z: 1.0,
        };

        // Obstacle at x=2, blocking positive X movement
        let obstacle = AABBd {
            min_x: 2.0,
            min_y: 0.0,
            min_z: 0.0,
            max_x: 3.0,
            max_y: 1.0,
            max_z: 1.0,
        };

        let result = collide(Axis::X, &entity, &[obstacle], 5.0);
        assert_eq!(
            result, 1.0,
            "Should stop at obstacle face (2.0 - 1.0 = 1.0)"
        );
    }

    #[test]
    fn test_collide_no_overlap_on_other_axes() {
        let entity = AABBd {
            min_x: 0.0,
            min_y: 0.0,
            min_z: 0.0,
            max_x: 1.0,
            max_y: 1.0,
            max_z: 1.0,
        };

        // Obstacle at x=2 but y=5 (no Y overlap)
        let obstacle = AABBd {
            min_x: 2.0,
            min_y: 5.0,
            min_z: 0.0,
            max_x: 3.0,
            max_y: 6.0,
            max_z: 1.0,
        };

        let result = collide(Axis::X, &entity, &[obstacle], 5.0);
        assert_eq!(result, 5.0, "Should ignore obstacle with no Y overlap");
    }

    #[test]
    fn test_join_is_not_empty_intersecting() {
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

        assert!(
            join_is_not_empty(&aabb1, &aabb2),
            "Overlapping AABBs should intersect"
        );
    }

    #[test]
    fn test_join_is_not_empty_non_intersecting() {
        let aabb1 = AABBd {
            min_x: 0.0,
            min_y: 0.0,
            min_z: 0.0,
            max_x: 1.0,
            max_y: 1.0,
            max_z: 1.0,
        };

        let aabb2 = AABBd {
            min_x: 2.0,
            min_y: 2.0,
            min_z: 2.0,
            max_x: 3.0,
            max_y: 3.0,
            max_z: 3.0,
        };

        assert!(
            !join_is_not_empty(&aabb1, &aabb2),
            "Separate AABBs should not intersect"
        );
    }

    #[test]
    fn test_translate_shape() {
        let shape = AABB::new(0.0, 0.0, 0.0, 1.0, 0.5, 1.0); // Half slab
        let block_pos = Vector3::new(10, 64, -5);

        let result = translate_shape(&shape, block_pos);

        assert_eq!(result.min_x, 10.0);
        assert_eq!(result.min_y, 64.0);
        assert_eq!(result.min_z, -5.0);
        assert_eq!(result.max_x, 11.0);
        assert_eq!(result.max_y, 64.5);
        assert_eq!(result.max_z, -4.0);
    }
}
