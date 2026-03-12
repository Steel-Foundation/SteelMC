//! Portal shape detection for validating obsidian frames.

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
    /// The block type of the frame.
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
fn is_empty_interior(world: &World, pos: BlockPos, _config: &PortalFrameConfig) -> bool {
    let block = world.get_block_state(&pos).get_block();
    block == vanilla_blocks::AIR || block == vanilla_blocks::FIRE
}

/// Interior check: air, fire, or existing portal blocks (used when validating an existing portal).
fn is_portal_or_empty_interior(world: &World, pos: BlockPos, config: &PortalFrameConfig) -> bool {
    let block = world.get_block_state(&pos).get_block();
    block == vanilla_blocks::AIR || block == vanilla_blocks::FIRE || block == config.portal
}

/// Interior validator function signature.
type InteriorCheck = fn(&World, BlockPos, &PortalFrameConfig) -> bool;

impl PortalShape {
    /// Tries to find a valid portal shape from a position inside or adjacent to a frame.
    pub fn find_portal_shape(
        world: &World,
        fire_pos: BlockPos,
        config: &PortalFrameConfig,
    ) -> Option<Self> {
        Self::try_axis(world, fire_pos, Axis::X, config, is_empty_interior)
            .or_else(|| Self::try_axis(world, fire_pos, Axis::Z, config, is_empty_interior))
    }

    /// Finds a portal shape on a specific axis, treating existing portal blocks as valid interior.
    /// Used by `update_shape` to check if the portal frame is still complete.
    pub fn find_any_shape(
        world: &World,
        pos: BlockPos,
        axis: Axis,
        config: &PortalFrameConfig,
    ) -> Option<Self> {
        Self::try_axis(world, pos, axis, config, is_portal_or_empty_interior)
    }

    /// Tries to find a valid portal on a single axis.
    /// It loops over the interior (not frame blocks) to determine the portal dimensions.
    fn try_axis(
        world: &World,
        pos: BlockPos,
        axis: Axis,
        config: &PortalFrameConfig,
        interior_check: InteriorCheck,
    ) -> Option<Self> {
        // Width direction: portal axis=X means width along Z, axis=Z means width along X
        let dir: Direction = match axis {
            Axis::X => Direction::East,
            Axis::Z => Direction::North,
            Axis::Y => return None,
        };

        // searches the bottom obsidian
        let mut cur = pos;
        for _ in 0..=config.max_height as i32 {
            let next = BlockPos::new(cur.x(), cur.y() - 1, cur.z());
            if Self::is_frame_block(world, next, config) {
                break;
            }
            cur = next;
        }

        // searches for the left obsidian (-1) because we don't want to be at the obsidian block
        let to_left = Self::get_width(world, cur, dir, config, interior_check);
        cur = cur.relative_n(dir, to_left as i32);

        let width = Self::get_width(world, cur, dir.opposite(), config, interior_check) + 1;
        if width < config.min_width {
            return None;
        }
        let height = Self::get_height(world, cur, dir, config, interior_check);
        if height < config.min_height {
            return None;
        }

        // Validate entire frame
        if !Self::validate_frame(
            world,
            cur,
            width,
            height,
            dir.opposite(),
            config,
            interior_check,
        ) {
            return None;
        }

        Some(Self {
            axis,
            bottom_left: cur,
            width,
            height,
            portal: config.portal,
        })
    }

    /// Returns the width - 1 of the portal interior starting from the given position.
    fn get_width(
        world: &World,
        pos: BlockPos,
        direction: Direction,
        config: &PortalFrameConfig,
        interior_check: InteriorCheck,
    ) -> u32 {
        for i in 1..config.max_width {
            let next = pos.relative_n(direction, i as i32);
            if !interior_check(world, next, config) && Self::is_frame_block(world, next, config) {
                return i - 1;
            }
            if !Self::is_frame_block(world, next.below(), config) {
                return 0;
            }
        }
        0
    }

    fn get_height(
        world: &World,
        pos: BlockPos,
        direction: Direction,
        config: &PortalFrameConfig,
        interior_check: InteriorCheck,
    ) -> u32 {
        let mut cur = pos;
        for i in 1..config.max_height {
            let next = cur.above();
            if !interior_check(world, next, config) && Self::is_frame_block(world, next, config) {
                return i;
            }
            if !Self::is_frame_block(world, next.relative(direction), config) {
                return 0;
            }
            cur = next;
        }
        0
    }

    fn is_frame_block(world: &World, pos: BlockPos, config: &PortalFrameConfig) -> bool {
        world.get_block_state(&pos).get_block() == config.frame
    }

    fn validate_frame(
        world: &World,
        bottom_left: BlockPos,
        width: u32,
        height: u32,
        direction: Direction,
        config: &PortalFrameConfig,
        interior_check: InteriorCheck,
    ) -> bool {
        // Check top frame row
        let top_row = bottom_left.above_n(height as i32);
        for w in 0..width as i32 {
            if !Self::is_frame_block(world, top_row.relative_n(direction, w), config) {
                return false;
            }
        }

        // Check right columns + interior
        for h in 0..height as i32 {
            // Right column
            let height_pos = bottom_left.above_n(h);
            if !Self::is_frame_block(
                world,
                height_pos.relative_n(direction, width as i32),
                config,
            ) {
                return false;
            }

            // Interior blocks
            for w in 0..width as i32 {
                if !interior_check(world, height_pos.relative_n(direction, w), config) {
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
