//! Portal shape detection for validating obsidian frames.

use std::ptr;

use steel_registry::blocks::BlockRef;
use steel_registry::blocks::block_state_ext::BlockStateExt;
use steel_registry::blocks::properties::BlockStateProperties;
use steel_registry::vanilla_blocks;
use steel_utils::math::Axis;
use steel_utils::types::UpdateFlags;
use steel_utils::{BlockPos, Direction};

use crate::world::World;

/// A detected portal shape with axis, position, and dimensions.
pub struct PortalTest {
    /// The axis of the portal (X or Z).
    pub axis: Axis,
    /// Bottom-left corner of the portal interior.
    pub bottom_left: BlockPos,
    /// Width of the interior (2-21).
    pub width: u32,
    /// Height of the interior (3-21).
    pub height: u32,
    /// The block type of the frame.
    pub portal: BlockRef,
}

/// Definition of a portal shape in rectangular form, like the nether portal frame.
pub struct PortalShape {
    /// min size of the portal in x direction
    pub min_x: u32,
    /// max size of the portal in x direction
    pub max_x: u32,
    /// min size of the portal in y direction
    pub min_y: u32,
    /// max size of the portal in y direction
    pub max_y: u32,
    /// The block type of the frame.
    pub frame: BlockRef,
    /// The block type of the portal.
    pub portal: BlockRef,
}

impl PortalTest {
    const MIN_WIDTH: u32 = 2;
    const MAX_WIDTH: u32 = 21;
    const MIN_HEIGHT: u32 = 3;
    const MAX_HEIGHT: u32 = 21;

    /// Tries to find a valid portal shape from a position inside or adjacent to a frame.
    pub fn find_portal_shape(
        world: &World,
        fire_pos: BlockPos,
        shape: &PortalShape,
    ) -> Option<Self> {
        Self::try_axis(world, fire_pos, Axis::X, shape)
            .or_else(|| Self::try_axis(world, fire_pos, Axis::Z, shape))
    }
    /// Tries to find a valid portal
    /// It doesn't loop over the obsidian, it loops over the air in the portal, to get the size of the portal
    fn try_axis(world: &World, pos: BlockPos, axis: Axis, shape: &PortalShape) -> Option<Self> {
        // Width direction: portal axis=X means width along Z, axis=Z means width along X
        let dir: Direction = match axis {
            Axis::X => Direction::East,
            Axis::Z => Direction::North,
            Axis::Y => return None,
        };
        tracing::info!("axis: {:?}", dir);

        // searches the bottom obsidian
        let mut cur = pos;
        for _ in 0..=Self::MAX_HEIGHT as i32 {
            let next = BlockPos::new(cur.x(), cur.y() - 1, cur.z());
            if Self::is_frame_block(world, next, shape) {
                break;
            }
            cur = next;
        }

        // searches for the left obsidian (-1) because we don't want to be at the obsidian block
        let to_left = Self::get_width(world, cur, dir, shape);
        tracing::info!("to_left: {}", to_left);
        cur = cur.relative_n(dir, to_left as i32);

        tracing::info!("left_bottom: {:?}", cur);

        let width = Self::get_width(world, cur, dir.opposite(), shape) + 1;
        tracing::info!("width: {}", width);
        if width < Self::MIN_WIDTH {
            return None;
        }
        let height = Self::get_height(world, cur, dir, shape);
        tracing::info!("height: {}", height);
        if height < Self::MIN_HEIGHT {
            return None;
        }
        // Measure width (walk right from bottom_left)

        // Validate entire frame
        if !Self::validate_frame(world, cur, width, height, dir.opposite(), shape) {
            tracing::info!("invalid frame");
            return None;
        }

        Some(Self {
            axis,
            bottom_left: cur,
            width,
            height,
            portal: shape.portal,
        })
    }

    /// Returns the width - 1 of the portal interior starting from the given position.
    fn get_width(world: &World, pos: BlockPos, direction: Direction, shape: &PortalShape) -> u32 {
        for i in 1..Self::MAX_WIDTH {
            let next = pos.relative_n(direction, i as i32);
            if !Self::is_valid_interior(world, next) && Self::is_frame_block(world, next, shape) {
                return i - 1;
            }
            if !Self::is_frame_block(world, next.below(), shape) {
                return 0;
            }
        }
        0
    }
    fn get_height(world: &World, pos: BlockPos, direction: Direction, shape: &PortalShape) -> u32 {
        let mut cur = pos;
        for i in 1..Self::MAX_HEIGHT {
            let next = cur.above();
            tracing::info!("next: {:?}", next);
            if !Self::is_valid_interior(world, next) && Self::is_frame_block(world, next, shape) {
                return i;
            }
            if !Self::is_frame_block(world, next.relative(direction), shape) {
                return 0;
            }
            cur = next;
        }
        0
    }

    fn is_frame_block(world: &World, pos: BlockPos, shape: &PortalShape) -> bool {
        ptr::eq(world.get_block_state(&pos).get_block(), shape.frame)
    }

    fn is_valid_interior(world: &World, pos: BlockPos) -> bool {
        let block = world.get_block_state(&pos).get_block();
        ptr::eq(block, vanilla_blocks::AIR) || ptr::eq(block, vanilla_blocks::FIRE)
    }

    fn validate_frame(
        world: &World,
        bottom_left: BlockPos,
        width: u32,
        height: u32,
        direction: Direction,
        shape: &PortalShape,
    ) -> bool {
        // Check top frame row
        let top_row = bottom_left.above_n(height as i32);
        for w in 0..width as i32 {
            if !Self::is_frame_block(world, top_row.relative_n(direction, w), shape) {
                return false;
            }
        }

        // Check right columns + interior
        for h in 0..height as i32 {
            // Right column
            let height_pos = bottom_left.above_n(h);
            if !Self::is_frame_block(world, height_pos.relative_n(direction, width as i32), shape) {
                return false;
            }

            // Interior blocks
            for w in 0..width as i32 {
                if !Self::is_valid_interior(world, height_pos.relative_n(direction, w)) {
                    return false;
                }
            }
        }

        true
    }

    /// Fills the interior with nether portal blocks.
    pub fn place_portal_blocks(&self, world: &World) {
        let portal_state = self
            .portal
            .default_state()
            .set_value(&BlockStateProperties::HORIZONTAL_AXIS, self.axis);
        let dir = match self.axis {
            Axis::X => Direction::West,
            Axis::Z => Direction::South,
            Axis::Y => return,
        };
        let flags = UpdateFlags::UPDATE_ALL;
        for w in 0..self.width {
            for h in 0..self.height {
                world.set_block(
                    self.bottom_left.above_n(h as i32).relative_n(dir, w as i32),
                    portal_state,
                    flags,
                );
            }
        }
    }
}
