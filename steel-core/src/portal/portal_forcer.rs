//! Finds existing portals or creates new ones at the target destination.

use std::ptr;

use steel_registry::blocks::block_state_ext::BlockStateExt;
use steel_registry::blocks::properties::BlockStateProperties;
use steel_registry::vanilla_blocks;
use steel_utils::BlockPos;
use steel_utils::math::Axis;
use steel_utils::types::UpdateFlags;

use crate::world::World;

/// Finds or creates nether portals at the destination.
pub struct PortalForcer;

impl PortalForcer {
    /// Finds an existing portal or creates a new one.
    pub fn find_or_create(world: &World, target: BlockPos, search_radius: i32) -> BlockPos {
        Self::find_portal(world, target, search_radius)
            .unwrap_or_else(|| Self::create_portal(world, target))
    }

    /// Searches for an existing nether portal near the target position.
    fn find_portal(world: &World, target: BlockPos, search_radius: i32) -> Option<BlockPos> {
        let mut best_pos: Option<BlockPos> = None;
        let mut best_distance_sq = i64::MAX;

        let min_y = world.dimension.min_y;
        let max_y = min_y + world.dimension.logical_height - 1;

        for dx in -search_radius..=search_radius {
            for dz in -search_radius..=search_radius {
                for y in min_y..=max_y {
                    let check_pos = BlockPos::new(target.x() + dx, y, target.z() + dz);
                    let state = world.get_block_state(&check_pos);
                    let block = state.get_block();

                    if !ptr::eq(block, vanilla_blocks::NETHER_PORTAL) {
                        continue;
                    }

                    let dist_sq = i64::from(dx) * i64::from(dx) + i64::from(dz) * i64::from(dz);
                    if dist_sq < best_distance_sq {
                        best_distance_sq = dist_sq;
                        best_pos = Some(check_pos);
                    }
                }
            }
        }

        // Walk down to the bottom of the portal column
        best_pos.map(|pos| {
            let mut bottom = pos;
            while ptr::eq(
                world
                    .get_block_state(&BlockPos::new(bottom.x(), bottom.y() - 1, bottom.z()))
                    .get_block(),
                vanilla_blocks::NETHER_PORTAL,
            ) {
                bottom = BlockPos::new(bottom.x(), bottom.y() - 1, bottom.z());
            }
            bottom
        })
    }

    /// Creates a new portal at the target position.
    fn create_portal(world: &World, target: BlockPos) -> BlockPos {
        let min_y = world.dimension.min_y;
        let max_y = min_y + world.dimension.logical_height - 1;

        let portal_y = Self::find_valid_y(world, target.x(), target.z(), min_y, max_y);

        let obsidian = vanilla_blocks::OBSIDIAN.default_state();
        let portal_block = vanilla_blocks::NETHER_PORTAL
            .default_state()
            .set_value(&BlockStateProperties::HORIZONTAL_AXIS, Axis::Z);
        let air = vanilla_blocks::AIR.default_state();
        let flags = UpdateFlags::UPDATE_ALL;

        let base_x = target.x();
        let base_y = portal_y;
        let base_z = target.z();

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

    /// Finds a valid Y position for portal creation.
    fn find_valid_y(world: &World, x: i32, z: i32, min_y: i32, max_y: i32) -> i32 {
        let scan_top = (max_y - 5).max(min_y);
        for y in (min_y..=scan_top).rev() {
            let ground = BlockPos::new(x, y, z);
            let state = world.get_block_state(&ground);
            // Check if this is solid ground
            if !state.is_air() && !ptr::eq(state.get_block(), vanilla_blocks::LAVA) {
                // Verify 5 blocks of air above
                let mut has_space = true;
                for dy in 1..=5 {
                    if !world.get_block_state(&BlockPos::new(x, y + dy, z)).is_air() {
                        has_space = false;
                        break;
                    }
                }
                if has_space {
                    return y;
                }
            }
        }
        // Fallback: build at mid-height
        i32::midpoint(min_y, max_y)
    }
}
