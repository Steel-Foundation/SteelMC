//! Portal shape detection for validating obsidian frames.

use std::sync::Arc;
use steel_registry::blocks::BlockRef;
use steel_registry::blocks::block_state_ext::BlockStateExt;
use steel_registry::blocks::properties::BlockStateProperties;
use steel_registry::vanilla_blocks;
use steel_utils::math::Axis;
use steel_utils::types::UpdateFlags;
use steel_utils::{BlockPos, Direction};

use crate::world::World;

/// A detected portal shape with axis, position, and dimensions.
pub struct PortalShape {
    /// The axis of the portal (X or Z).
    pub axis: Axis,
    /// Bottom-left corner of the portal interior.
    pub bottom_left: BlockPos,
    /// Width of the interior (2-21).
    pub width: u32,
    /// Height of the interior (3-21).
    pub height: u32,
    /// The horizontal direction along which width is measured.
    pub right_dir: Direction,
    /// The block type of the portal.
    pub portal: BlockRef,
}

/// Definition of a portal shape in rectangular form, like the nether portal frame.
pub struct PortalFrameConfig {
    /// min size of the portal in x direction
    pub min_width: u32,
    /// max size of the portal in x direction
    pub max_width: u32,
    /// min size of the portal in y direction
    pub min_height: u32,
    /// max size of the portal in y direction
    pub max_height: u32,
    /// The block type of the frame.
    pub frame: BlockRef,
    /// The block type of the portal.
    pub portal: BlockRef,
}

/// Returns the standard nether portal frame configuration.
#[must_use]
pub fn nether_portal_config() -> PortalFrameConfig {
    PortalFrameConfig {
        min_width: 2,
        max_width: 21,
        min_height: 3,
        max_height: 21,
        frame: vanilla_blocks::OBSIDIAN,
        portal: vanilla_blocks::NETHER_PORTAL,
    }
}

/// Interior check: air or fire only (used when creating a new portal).
fn is_empty_interior(world: &Arc<World>, pos: BlockPos, _config: &PortalFrameConfig) -> bool {
    let block = world.get_block_state(pos).get_block();
    block == vanilla_blocks::AIR || block == vanilla_blocks::FIRE
}

/// Interior check: air, fire, or existing portal blocks (used when validating an existing portal).
fn is_portal_or_empty_interior(
    world: &Arc<World>,
    pos: BlockPos,
    config: &PortalFrameConfig,
) -> bool {
    let block = world.get_block_state(pos).get_block();
    block == vanilla_blocks::AIR || block == vanilla_blocks::FIRE || block == config.portal
}

/// Interior validator function signature.
type InteriorCheck = fn(&Arc<World>, BlockPos, &PortalFrameConfig) -> bool;

impl PortalShape {
    /// Tries to find a valid portal shape from a position inside or adjacent to a frame.
    pub fn find_portal_shape(
        world: &Arc<World>,
        fire_pos: BlockPos,
        config: &PortalFrameConfig,
    ) -> Option<Self> {
        Self::try_axis(world, fire_pos, Axis::X, config, is_empty_interior)
            .or_else(|| Self::try_axis(world, fire_pos, Axis::Z, config, is_empty_interior))
    }

    /// Finds a portal shape on a specific axis, treating existing portal blocks as valid interior.
    /// Used by `update_shape` to check if the portal frame is still complete.
    pub fn find_any_shape(
        world: &Arc<World>,
        pos: BlockPos,
        axis: Axis,
        config: &PortalFrameConfig,
    ) -> Option<Self> {
        Self::try_axis(world, pos, axis, config, is_portal_or_empty_interior)
    }

    /// Tries to find a valid portal on a single axis, matching vanilla's detection algorithm.
    fn try_axis(
        world: &Arc<World>,
        pos: BlockPos,
        axis: Axis,
        config: &PortalFrameConfig,
        interior_check: InteriorCheck,
    ) -> Option<Self> {
        // Vanilla: rightDir is WEST for X-axis, SOUTH for Z-axis
        let right_dir: Direction = match axis {
            Axis::X => Direction::West,
            Axis::Z => Direction::South,
            Axis::Y => return None,
        };

        let bottom_left =
            Self::calculate_bottom_left(world, pos, right_dir, config, interior_check)?;

        let width = Self::calculate_width(world, bottom_left, right_dir, config, interior_check);
        if width == 0 {
            return None;
        }

        let height = Self::calculate_height(
            world, bottom_left, width, right_dir, config, interior_check,
        );
        if height < config.min_height {
            return None;
        }

        if !Self::has_top_frame(world, bottom_left, height, width, right_dir, config) {
            return None;
        }

        Some(Self {
            axis,
            bottom_left,
            width,
            height,
            right_dir,
            portal: config.portal,
        })
    }

    /// Returns the number of valid interior blocks in `direction` from `pos`, matching vanilla's
    /// `getDistanceUntilEdgeAboveFrame`. Each position must pass `interior_check` and have a
    /// frame block below it.
    fn get_distance_until_edge(
        world: &Arc<World>,
        pos: BlockPos,
        direction: Direction,
        config: &PortalFrameConfig,
        interior_check: InteriorCheck,
    ) -> u32 {
        for i in 0..config.max_width {
            let next = pos.relative_n(direction, i as i32);
            if !interior_check(world, next, config)
                || !Self::is_frame_block(world, next.below(), config)
            {
                return i;
            }
        }
        config.max_width
    }

    /// Finds the bottom-left corner of the portal interior.
    fn calculate_bottom_left(
        world: &Arc<World>,
        pos: BlockPos,
        right_dir: Direction,
        config: &PortalFrameConfig,
        interior_check: InteriorCheck,
    ) -> Option<BlockPos> {
        // Scan down to find the floor
        let mut cur = pos;
        for _ in 0..=config.max_height {
            let next = cur.below();
            if Self::is_frame_block(world, next, config) {
                break;
            }
            if !interior_check(world, next, config) {
                return None;
            }
            cur = next;
        }

        // Scan in opposite of right_dir to find the left edge
        let left_dir = right_dir.opposite();
        let dist = Self::get_distance_until_edge(world, cur, left_dir, config, interior_check);
        if dist == 0 {
            return None;
        }
        Some(cur.relative_n(left_dir, (dist - 1) as i32))
    }

    /// Calculates the width of the portal interior from the bottom-left corner.
    fn calculate_width(
        world: &Arc<World>,
        bottom_left: BlockPos,
        right_dir: Direction,
        config: &PortalFrameConfig,
        interior_check: InteriorCheck,
    ) -> u32 {
        let dist = Self::get_distance_until_edge(world, bottom_left, right_dir, config, interior_check);
        if dist < config.min_width || dist > config.max_width {
            return 0;
        }
        dist
    }

    /// Calculates the height while validating side columns and interior.
    fn calculate_height(
        world: &Arc<World>,
        bottom_left: BlockPos,
        width: u32,
        right_dir: Direction,
        config: &PortalFrameConfig,
        interior_check: InteriorCheck,
    ) -> u32 {
        let mut height = 0;
        'outer: for h in 0..config.max_height {
            let row_start = bottom_left.above_n(h as i32);

            // Check left frame column (one block left of bottom_left)
            if !Self::is_frame_block(world, row_start.relative(right_dir.opposite()), config) {
                break;
            }
            // Check right frame column (one block past the width)
            if !Self::is_frame_block(world, row_start.relative_n(right_dir, width as i32), config) {
                break;
            }

            // Check interior
            for w in 0..width {
                let interior_pos = row_start.relative_n(right_dir, w as i32);
                if !interior_check(world, interior_pos, config) {
                    break 'outer;
                }
            }
            height = h + 1;
        }
        height
    }

    /// Checks that the top frame row is complete.
    fn has_top_frame(
        world: &Arc<World>,
        bottom_left: BlockPos,
        height: u32,
        width: u32,
        right_dir: Direction,
        config: &PortalFrameConfig,
    ) -> bool {
        let top_row = bottom_left.above_n(height as i32);
        for w in 0..width {
            if !Self::is_frame_block(world, top_row.relative_n(right_dir, w as i32), config) {
                return false;
            }
        }
        true
    }

    fn is_frame_block(world: &Arc<World>, pos: BlockPos, config: &PortalFrameConfig) -> bool {
        world.get_block_state(pos).get_block() == config.frame
    }

    /// Fills the interior with nether portal blocks.
    /// Vanilla uses flag 18 (`UPDATE_CLIENTS` | `UPDATE_KNOWN_SHAPE`) to avoid redundant neighbor
    /// updates during bulk placement.
    pub fn place_portal_blocks(&self, world: &Arc<World>) {
        let portal_state = self
            .portal
            .default_state()
            .set_value(&BlockStateProperties::HORIZONTAL_AXIS, self.axis);
        let flags = UpdateFlags::UPDATE_CLIENTS.union(UpdateFlags::UPDATE_KNOWN_SHAPE);
        for w in 0..self.width {
            for h in 0..self.height {
                world.set_block(
                    self.bottom_left
                        .above_n(h as i32)
                        .relative_n(self.right_dir, w as i32),
                    portal_state,
                    flags,
                );
            }
        }
    }
}
