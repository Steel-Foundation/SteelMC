use std::mem;

use glam::{DVec3, IVec3};
use rustc_hash::FxHashSet;
use steel_registry::blocks::shapes::VoxelShape;
use steel_utils::{BlockPos, WorldAabb, axis::Axis};

const SMALL_MOVEMENT_EPSILON_SQ: f64 = 9.999_999_4e-11;
const CLIP_EPSILON: f64 = 1.0e-7;
const CORNER_HIT_EPSILON: f64 = 1.0e-5;
const ENTITY_INSIDE_SWEEP_INFLATE_EPSILON: f64 = 1.0e-7;

pub(super) fn for_each_block_intersected_between(
    from: DVec3,
    to: DVec3,
    aabb_at_target: WorldAabb,
    mut visitor: impl FnMut(BlockPos, i32) -> bool,
) -> Option<i32> {
    let mut last_iteration = 0;
    let travel = to - from;
    if travel.length_squared() < SMALL_MOVEMENT_EPSILON_SQ {
        if !for_each_block_in_aabb(aabb_at_target, |pos| {
            last_iteration = 0;
            visitor(pos, 0)
        }) {
            return None;
        }

        return Some(last_iteration + 1);
    }

    let mut visited = FxHashSet::default();
    let aabb_at_start = aabb_at_target.translate(-travel);
    for pos in between_corners_in_direction(aabb_at_start, travel) {
        last_iteration = 0;
        if !visitor(pos, 0) {
            return None;
        }
        visited.insert(pos);
    }

    let iterations = ({
        let mut traced_visitor = |pos, iteration| {
            last_iteration = iteration;
            visitor(pos, iteration)
        };
        add_collisions_along_travel(&mut visited, travel, aabb_at_target, &mut traced_visitor)
    })?;

    for pos in between_corners_in_direction(aabb_at_target, travel) {
        if visited.insert(pos) {
            last_iteration = iterations + 1;
            if !visitor(pos, iterations + 1) {
                return None;
            }
        }
    }

    Some(last_iteration + 1)
}

pub(super) fn collided_with_shape_moving_from(
    entity_box_at_from: WorldAabb,
    from: DVec3,
    to: DVec3,
    block_pos: BlockPos,
    shape: VoxelShape,
) -> bool {
    shape.iter().any(|part| {
        collided_with_aabb_moving_from(entity_box_at_from, from, to, part.at_block(block_pos))
    })
}

pub(super) fn collided_with_aabb_moving_from(
    entity_box_at_from: WorldAabb,
    from: DVec3,
    to: DVec3,
    target_box: WorldAabb,
) -> bool {
    let from_center = center(entity_box_at_from);
    let to_center = from_center + (to - from);
    let inflate_x = entity_box_at_from.width() * 0.5 - ENTITY_INSIDE_SWEEP_INFLATE_EPSILON;
    let inflate_y = entity_box_at_from.height() * 0.5 - ENTITY_INSIDE_SWEEP_INFLATE_EPSILON;
    let inflate_z = entity_box_at_from.depth() * 0.5 - ENTITY_INSIDE_SWEEP_INFLATE_EPSILON;

    let inflated_part = target_box.inflate_xyz(inflate_x, inflate_y, inflate_z);
    contains(inflated_part, from_center)
        || contains(inflated_part, to_center)
        || clip_aabb(inflated_part, from_center, to_center).is_some()
}

#[expect(
    clippy::too_many_lines,
    reason = "keeps the vanilla BlockGetter.addCollisionsAlongTravel port auditable"
)]
fn add_collisions_along_travel(
    visited: &mut FxHashSet<BlockPos>,
    travel: DVec3,
    aabb_at_target: WorldAabb,
    visitor: &mut impl FnMut(BlockPos, i32) -> bool,
) -> Option<i32> {
    let box_size = DVec3::new(
        aabb_at_target.width(),
        aabb_at_target.height(),
        aabb_at_target.depth(),
    );
    let corner_dir = get_furthest_corner(travel);
    let to_center = DVec3::new(
        f64::midpoint(aabb_at_target.min(Axis::X), aabb_at_target.max(Axis::X)),
        f64::midpoint(aabb_at_target.min(Axis::Y), aabb_at_target.max(Axis::Y)),
        f64::midpoint(aabb_at_target.min(Axis::Z), aabb_at_target.max(Axis::Z)),
    );
    let to_corner = DVec3::new(
        to_center.x + box_size.x * 0.5 * f64::from(corner_dir.x),
        to_center.y + box_size.y * 0.5 * f64::from(corner_dir.y),
        to_center.z + box_size.z * 0.5 * f64::from(corner_dir.z),
    );
    let from_corner = to_corner - travel;
    let mut corner_block = IVec3::new(
        from_corner.x.floor() as i32,
        from_corner.y.floor() as i32,
        from_corner.z.floor() as i32,
    );
    let sign_x = sign_i32(travel.x);
    let sign_y = sign_i32(travel.y);
    let sign_z = sign_i32(travel.z);
    let t_delta_x = if sign_x == 0 {
        f64::MAX
    } else {
        f64::from(sign_x) / travel.x
    };
    let t_delta_y = if sign_y == 0 {
        f64::MAX
    } else {
        f64::from(sign_y) / travel.y
    };
    let t_delta_z = if sign_z == 0 {
        f64::MAX
    } else {
        f64::from(sign_z) / travel.z
    };
    let mut t_x = t_delta_x
        * if sign_x > 0 {
            1.0 - frac(from_corner.x)
        } else {
            frac(from_corner.x)
        };
    let mut t_y = t_delta_y
        * if sign_y > 0 {
            1.0 - frac(from_corner.y)
        } else {
            frac(from_corner.y)
        };
    let mut t_z = t_delta_z
        * if sign_z > 0 {
            1.0 - frac(from_corner.z)
        } else {
            frac(from_corner.z)
        };
    let mut iterations = 0;

    while t_x <= 1.0 || t_y <= 1.0 || t_z <= 1.0 {
        if t_x < t_y {
            if t_x < t_z {
                corner_block.x += sign_x;
                t_x += t_delta_x;
            } else {
                corner_block.z += sign_z;
                t_z += t_delta_z;
            }
        } else if t_y < t_z {
            corner_block.y += sign_y;
            t_y += t_delta_y;
        } else {
            corner_block.z += sign_z;
            t_z += t_delta_z;
        }

        let block_pos = BlockPos::new(corner_block.x, corner_block.y, corner_block.z);
        if let Some(hit_point) = clip_block(block_pos, from_corner, to_corner) {
            iterations += 1;
            let corner_hit_x = hit_point.x.clamp(
                f64::from(corner_block.x) + CORNER_HIT_EPSILON,
                f64::from(corner_block.x + 1) - CORNER_HIT_EPSILON,
            );
            let corner_hit_y = hit_point.y.clamp(
                f64::from(corner_block.y) + CORNER_HIT_EPSILON,
                f64::from(corner_block.y + 1) - CORNER_HIT_EPSILON,
            );
            let corner_hit_z = hit_point.z.clamp(
                f64::from(corner_block.z) + CORNER_HIT_EPSILON,
                f64::from(corner_block.z + 1) - CORNER_HIT_EPSILON,
            );
            let opposite_corner = IVec3::new(
                (corner_hit_x - box_size.x * f64::from(corner_dir.x)).floor() as i32,
                (corner_hit_y - box_size.y * f64::from(corner_dir.y)).floor() as i32,
                (corner_hit_z - box_size.z * f64::from(corner_dir.z)).floor() as i32,
            );

            for pos in between_corners_in_direction_between(corner_block, opposite_corner, travel) {
                if visited.insert(pos) && !visitor(pos, iterations) {
                    return None;
                }
            }
        }
    }

    Some(iterations)
}

pub(super) fn for_each_block_in_aabb(
    aabb: WorldAabb,
    mut visitor: impl FnMut(BlockPos) -> bool,
) -> bool {
    let min_x = aabb.min(Axis::X).floor() as i32;
    let min_y = aabb.min(Axis::Y).floor() as i32;
    let min_z = aabb.min(Axis::Z).floor() as i32;
    let max_x = aabb.max(Axis::X).floor() as i32;
    let max_y = aabb.max(Axis::Y).floor() as i32;
    let max_z = aabb.max(Axis::Z).floor() as i32;

    for x in min_x..=max_x {
        for y in min_y..=max_y {
            for z in min_z..=max_z {
                if !visitor(BlockPos::new(x, y, z)) {
                    return false;
                }
            }
        }
    }

    true
}

fn between_corners_in_direction(aabb: WorldAabb, direction: DVec3) -> Vec<BlockPos> {
    let first_corner = IVec3::new(
        aabb.min(Axis::X).floor() as i32,
        aabb.min(Axis::Y).floor() as i32,
        aabb.min(Axis::Z).floor() as i32,
    );
    let second_corner = IVec3::new(
        aabb.max(Axis::X).floor() as i32,
        aabb.max(Axis::Y).floor() as i32,
        aabb.max(Axis::Z).floor() as i32,
    );
    between_corners_in_direction_between(first_corner, second_corner, direction)
}

fn between_corners_in_direction_between(
    first_corner: IVec3,
    second_corner: IVec3,
    direction: DVec3,
) -> Vec<BlockPos> {
    let min_corner = first_corner.min(second_corner);
    let max_corner = first_corner.max(second_corner);
    let diff = max_corner - min_corner;
    let start = IVec3::new(
        if direction.x >= 0.0 {
            min_corner.x
        } else {
            max_corner.x
        },
        if direction.y >= 0.0 {
            min_corner.y
        } else {
            max_corner.y
        },
        if direction.z >= 0.0 {
            min_corner.z
        } else {
            max_corner.z
        },
    );
    let axes = axis_step_order(direction);
    let first_axis = axes[0];
    let second_axis = axes[1];
    let third_axis = axes[2];
    let first_step = axis_step(first_axis, direction);
    let second_step = axis_step(second_axis, direction);
    let third_step = axis_step(third_axis, direction);
    let first_max = axis_value(diff, first_axis);
    let second_max = axis_value(diff, second_axis);
    let third_max = axis_value(diff, third_axis);
    let mut positions = Vec::new();

    for first_index in 0..=first_max {
        for second_index in 0..=second_max {
            for third_index in 0..=third_max {
                let position = start
                    + first_step * first_index
                    + second_step * second_index
                    + third_step * third_index;
                positions.push(BlockPos::new(position.x, position.y, position.z));
            }
        }
    }

    positions
}

fn clip_block(pos: BlockPos, from: DVec3, to: DVec3) -> Option<DVec3> {
    let min = DVec3::new(f64::from(pos.x()), f64::from(pos.y()), f64::from(pos.z()));
    let max = min + DVec3::ONE;
    let direction = to - from;
    let mut t_min = 0.0;
    let mut t_max = 1.0;

    for axis in [Axis::X, Axis::Y, Axis::Z] {
        let start = component(from, axis);
        let delta = component(direction, axis);
        let axis_min = component(min, axis);
        let axis_max = component(max, axis);
        if delta.abs() < CLIP_EPSILON {
            if start < axis_min || start > axis_max {
                return None;
            }
            continue;
        }

        let inv_delta = 1.0 / delta;
        let mut low = (axis_min - start) * inv_delta;
        let mut high = (axis_max - start) * inv_delta;
        if low > high {
            mem::swap(&mut low, &mut high);
        }

        if low > t_min {
            t_min = low;
        }
        if high < t_max {
            t_max = high;
        }
        if t_min > t_max {
            return None;
        }
    }

    Some(from + direction * t_min)
}

fn clip_aabb(aabb: WorldAabb, from: DVec3, to: DVec3) -> Option<DVec3> {
    let direction = to - from;
    let mut t_min = 0.0;
    let mut t_max = 1.0;

    for axis in [Axis::X, Axis::Y, Axis::Z] {
        let start = component(from, axis);
        let delta = component(direction, axis);
        let axis_min = aabb.min(axis);
        let axis_max = aabb.max(axis);
        if delta.abs() < CLIP_EPSILON {
            if start < axis_min || start > axis_max {
                return None;
            }
            continue;
        }

        let inv_delta = 1.0 / delta;
        let mut low = (axis_min - start) * inv_delta;
        let mut high = (axis_max - start) * inv_delta;
        if low > high {
            mem::swap(&mut low, &mut high);
        }

        if low > t_min {
            t_min = low;
        }
        if high < t_max {
            t_max = high;
        }
        if t_min > t_max {
            return None;
        }
    }

    Some(from + direction * t_min)
}

fn contains(aabb: WorldAabb, point: DVec3) -> bool {
    point.x >= aabb.min(Axis::X)
        && point.x < aabb.max(Axis::X)
        && point.y >= aabb.min(Axis::Y)
        && point.y < aabb.max(Axis::Y)
        && point.z >= aabb.min(Axis::Z)
        && point.z < aabb.max(Axis::Z)
}

fn center(aabb: WorldAabb) -> DVec3 {
    DVec3::new(
        f64::midpoint(aabb.min(Axis::X), aabb.max(Axis::X)),
        f64::midpoint(aabb.min(Axis::Y), aabb.max(Axis::Y)),
        f64::midpoint(aabb.min(Axis::Z), aabb.max(Axis::Z)),
    )
}

fn get_furthest_corner(direction: DVec3) -> IVec3 {
    let x_dot = direction.x.abs();
    let y_dot = direction.y.abs();
    let z_dot = direction.z.abs();
    let x_sign = if direction.x >= 0.0 { 1 } else { -1 };
    let y_sign = if direction.y >= 0.0 { 1 } else { -1 };
    let z_sign = if direction.z >= 0.0 { 1 } else { -1 };
    if x_dot <= y_dot && x_dot <= z_dot {
        IVec3::new(-x_sign, -z_sign, y_sign)
    } else if y_dot <= z_dot {
        IVec3::new(z_sign, -y_sign, -x_sign)
    } else {
        IVec3::new(-y_sign, x_sign, -z_sign)
    }
}

pub(super) fn axis_step_order(movement: DVec3) -> [Axis; 3] {
    if movement.x.abs() < movement.z.abs() {
        [Axis::Y, Axis::Z, Axis::X]
    } else {
        [Axis::Y, Axis::X, Axis::Z]
    }
}

fn axis_step(axis: Axis, direction: DVec3) -> IVec3 {
    let sign = if component(direction, axis) >= 0.0 {
        1
    } else {
        -1
    };
    match axis {
        Axis::X => IVec3::new(sign, 0, 0),
        Axis::Y => IVec3::new(0, sign, 0),
        Axis::Z => IVec3::new(0, 0, sign),
    }
}

const fn axis_value(vector: IVec3, axis: Axis) -> i32 {
    match axis {
        Axis::X => vector.x,
        Axis::Y => vector.y,
        Axis::Z => vector.z,
    }
}

pub(super) const fn component(vector: DVec3, axis: Axis) -> f64 {
    match axis {
        Axis::X => vector.x,
        Axis::Y => vector.y,
        Axis::Z => vector.z,
    }
}

fn frac(value: f64) -> f64 {
    value - value.floor()
}

fn sign_i32(value: f64) -> i32 {
    if value > 0.0 {
        1
    } else if value < 0.0 {
        -1
    } else {
        0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn visited_positions(from: DVec3, to: DVec3, aabb_at_target: WorldAabb) -> Vec<BlockPos> {
        let mut positions = Vec::new();
        assert!(
            for_each_block_intersected_between(from, to, aabb_at_target, |pos, _iteration| {
                positions.push(pos);
                true
            })
            .is_some()
        );
        positions
    }

    #[test]
    fn entity_inside_shape_uses_swept_entity_center_against_inflated_shape() {
        let entity_box = WorldAabb::entity_box(0.5, 0.0, 0.5, 0.3, 1.8);
        let from = DVec3::new(0.5, 0.0, 0.5);
        let to = DVec3::new(2.5, 0.0, 2.5);

        assert!(collided_with_shape_moving_from(
            entity_box,
            from,
            to,
            BlockPos::new(2, 0, 2),
            VoxelShape::FULL_BLOCK,
        ));
        assert!(!collided_with_shape_moving_from(
            entity_box,
            from,
            to,
            BlockPos::new(2, 0, 0),
            VoxelShape::FULL_BLOCK,
        ));
    }

    #[test]
    fn stationary_entity_inside_shape_uses_current_entity_box() {
        let entity_box = WorldAabb::entity_box(0.5, 0.0, 0.5, 0.3, 1.8);
        let position = DVec3::new(0.5, 0.0, 0.5);

        assert!(collided_with_shape_moving_from(
            entity_box,
            position,
            position,
            BlockPos::new(0, 0, 0),
            VoxelShape::FULL_BLOCK,
        ));
        assert!(!collided_with_shape_moving_from(
            entity_box,
            position,
            position,
            BlockPos::new(2, 0, 0),
            VoxelShape::FULL_BLOCK,
        ));
    }

    #[test]
    fn stationary_trace_visits_target_aabb_blocks() {
        let positions = visited_positions(
            DVec3::new(0.5, 64.0, 0.5),
            DVec3::new(0.5, 64.0, 0.5),
            WorldAabb::new(0.2, 64.0, 0.2, 1.2, 64.9, 0.9),
        );

        assert_eq!(
            positions,
            vec![BlockPos::new(0, 64, 0), BlockPos::new(1, 64, 0)]
        );
    }

    #[test]
    fn horizontal_trace_includes_blocks_between_start_and_target() {
        let positions = visited_positions(
            DVec3::new(0.5, 64.0, 0.5),
            DVec3::new(2.5, 64.0, 0.5),
            WorldAabb::new(2.2, 64.0, 0.2, 2.8, 64.9, 0.8),
        );

        assert!(positions.contains(&BlockPos::new(0, 64, 0)));
        assert!(positions.contains(&BlockPos::new(1, 64, 0)));
        assert!(positions.contains(&BlockPos::new(2, 64, 0)));
    }

    #[test]
    fn visitor_can_stop_trace() {
        let mut visited = Vec::new();
        let completed = for_each_block_intersected_between(
            DVec3::new(0.5, 64.0, 0.5),
            DVec3::new(2.5, 64.0, 0.5),
            WorldAabb::new(2.2, 64.0, 0.2, 2.8, 64.9, 0.8),
            |pos, _iteration| {
                visited.push(pos);
                false
            },
        );

        assert!(completed.is_none());
        assert_eq!(visited.len(), 1);
    }
}
