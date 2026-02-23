//! Finds existing portals or creates new ones at the target destination.

use crate::behavior::BlockStateBehaviorExt;
use crate::world::World;
use std::cmp;
use std::cmp::max;
use steel_registry::blocks::block_state_ext::BlockStateExt;
use steel_registry::blocks::properties::BlockStateProperties;
use steel_registry::vanilla_blocks;
use steel_utils::math::Axis;
use steel_utils::math::vector3::AxisDirection;
use steel_utils::types::UpdateFlags;
use steel_utils::{BlockPos, Direction};

/// Finds or creates nether portals at the destination.
pub struct NetherPortalForcer;

impl NetherPortalForcer {
    const FRAME_WIDTH_START: i32 = -1;
    const FRAME_WIDTH_END: i32 = 3;
    const FRAME_HEIGHT_START: i32 = -1;
    const FRAME_HEIGHT_END: i32 = 4;
    const NETHER_PORTAL_RADIUS: i32 = 16;

    /// Searches for an existing nether portal near the target position.
    pub fn find_portal(_world: &World, _target: BlockPos, _search_radius: i32) -> Option<BlockPos> {
        // TODO: POI system which will follow
        None
    }

    /// Creates a new portal at the target position.
    pub fn create_portal(world: &World, target: BlockPos, axis: Axis) -> BlockPos {
        let direction = Direction::get_from_axis(&axis, AxisDirection::Positive);
        let max_placeable_y = cmp::min(
            world.get_max_y(),
            world.get_min_y() + world.dimension.logical_height - 1,
        );
        let mut sqrt_distance_full = -1;
        let mut sqrt_distance_partial = -1;
        let mut full_position: Option<BlockPos> = None;
        let mut partial_position: Option<BlockPos> = None;
        for col in target.spiral_around(
            Self::NETHER_PORTAL_RADIUS,
            Direction::East,
            Direction::South,
        ) {
            //TODO: worldborder check
            let col = col.relative(direction.opposite());
            let height = cmp::min(max_placeable_y, world.get_height());
            // Bug 1 fix: iterate downward
            for y in (world.get_min_y()..=height).rev() {
                // Bug 4 fix: use col.at_y(y) so col is never mutated
                let pos = col.at_y(y);
                if Self::can_replace_block(world, &pos) {
                    let empty_y = y;
                    // Bug 3 fix: track last replaceable y with a separate counter
                    let mut bottom_y = y;
                    while bottom_y > world.get_min_y()
                        && Self::can_replace_block(world, &col.at_y(bottom_y - 1))
                    {
                        bottom_y -= 1;
                    }
                    let column = col.at_y(bottom_y);
                    if bottom_y + 4 <= max_placeable_y {
                        let delta = empty_y - bottom_y;
                        if delta <= 0 || delta >= 3 {
                            if Self::can_host_frame(world, &column, direction, 0) {
                                let dis = target.distance_squared(&column);
                                if Self::can_host_frame(world, &column, direction, 1)
                                    && Self::can_host_frame(world, &column, direction, -1)
                                    && (sqrt_distance_full == -1 || sqrt_distance_full > dis)
                                {
                                    sqrt_distance_full = dis;
                                    full_position = Some(column);
                                }
                                if sqrt_distance_full == -1
                                    && (sqrt_distance_partial == -1 || sqrt_distance_partial > dis)
                                {
                                    sqrt_distance_partial = dis;
                                    partial_position = Some(column);
                                }
                            }
                        }
                    }
                }
            }
        }
        if full_position.is_none() {
            full_position = partial_position;
        }

        let obsidian = vanilla_blocks::OBSIDIAN.default_state();
        let air = vanilla_blocks::AIR.default_state();

        // Bug 5 fix: compute fallback position when no suitable location was found
        if full_position.is_none() {
            let min_start_y = max(world.get_min_y() + 1, 70);
            let max_start_y = max_placeable_y - 9;
            if max_start_y < min_start_y {
                return target; // dimension too small — give up
            }
            let dir_off = direction.offset();
            let cw = direction.rotate_y_clockwise();
            let cw_off = cw.offset();
            let fallback = BlockPos::new(
                target.x() - dir_off.0,
                target.y().clamp(min_start_y, max_start_y),
                target.z() - dir_off.2,
            );
            // Clear terrain: obsidian floor (-1) + air (0..2) in a 2×3 area
            for box_off in -1..2_i32 {
                for width in 0..2_i32 {
                    for height in -1_i32..3 {
                        let block_state = if height < 0 { obsidian } else { air };
                        let pos = fallback.offset(
                            width * dir_off.0 + box_off * cw_off.0,
                            height,
                            width * dir_off.2 + box_off * cw_off.2,
                        );
                        world.set_block(pos, block_state, UpdateFlags::UPDATE_ALL);
                    }
                }
            }
            full_position = Some(fallback);
        }

        if let Some(pos) = full_position {
            tracing::warn!(
                "Created nether portal at x: {}, y: {}, z: {}",
                pos.x(),
                pos.y(),
                pos.z()
            );
        }

        let portal_block = vanilla_blocks::NETHER_PORTAL
            .default_state()
            .set_value(&BlockStateProperties::HORIZONTAL_AXIS, Axis::Z);
        let air = vanilla_blocks::AIR.default_state();
        let flags = UpdateFlags::UPDATE_ALL;
        let pos = full_position.unwrap();
        let base_x = pos.x();
        let base_y = pos.y();
        let base_z = pos.z();

        // Build frame: 4 wide (x) x 5 tall (y), portal faces along Z
        for x_off in 0..4 {
            for y_off in 0..5 {
                let pos = BlockPos::new(base_x + x_off, base_y + y_off, base_z);
                let is_frame = x_off == 0 || x_off == 3 || y_off == 0 || y_off == 4;
                if is_frame {
                    world.set_block(pos, obsidian, flags);
                } else {
                    world.set_block(pos, portal_block, flags);
                }
            }
        }

        // Clear air around portal to prevent suffocation
        for x_off in 0..4 {
            for y_off in 1..4 {
                for z_off in [-1, 1] {
                    let pos = BlockPos::new(base_x + x_off, base_y + y_off, base_z + z_off);
                    world.set_block(pos, air, flags);
                }
            }
        }

        // Return the bottom-left interior position
        BlockPos::new(base_x + 1, base_y + 1, base_z)
    }

    fn can_replace_block(world: &World, pos: &BlockPos) -> bool {
        let state = world.get_block_state(pos);
        state.get_fluid_state().is_empty() && state.is_replaceable()
    }

    fn can_host_frame(world: &World, pos: &BlockPos, direction: Direction, offset: i32) -> bool {
        let rotated = direction.rotate_y_clockwise();
        for width in Self::FRAME_WIDTH_START..Self::FRAME_WIDTH_END {
            for height in Self::FRAME_HEIGHT_START..Self::FRAME_HEIGHT_END {
                let prope_pos = pos.offset(
                    direction.offset().0 * width + rotated.offset().0 * offset,
                    height,
                    direction.offset().2 * width + rotated.offset().2 * offset,
                );
                if height < 0 && !world.get_block_state(&prope_pos).is_solid() {
                    return false;
                }
                if height >= 0 && !Self::can_replace_block(world, &prope_pos) {
                    return false;
                }
            }
        }
        true
    }
}
