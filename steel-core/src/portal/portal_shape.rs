//! Portal shape detection for validating obsidian frames.

use glam::DVec3;
use std::sync::Arc;
use steel_math::inverse_lerp;
use steel_registry::blocks::BlockRef;
use steel_registry::blocks::block_state_ext::BlockStateExt;
use steel_registry::blocks::properties::BlockStateProperties;
use steel_registry::entity_type::EntityDimensions;
use steel_registry::vanilla_block_tags::BlockTag;
use steel_registry::vanilla_blocks;
use steel_utils::axis::Axis;
use steel_utils::block_util::FoundRectangle;
use steel_utils::types::UpdateFlags;
use steel_utils::{BlockPos, Direction, WorldAabb};

use crate::entity::Entity;
use crate::physics::WorldCollisionProvider;
use crate::world::{LevelReader, World};

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
    /// Number of portal blocks found in the interior.
    /// Used by `is_complete` to verify the portal is fully filled.
    num_portal_blocks: u32,
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
        frame: &vanilla_blocks::OBSIDIAN,
        portal: &vanilla_blocks::NETHER_PORTAL,
    }
}

/// Matches vanilla's `PortalShape.isEmpty`: any air variant, any block in the `fire` tag,
/// or the portal block itself.
fn is_empty(world: &dyn LevelReader, pos: BlockPos, config: &PortalFrameConfig) -> bool {
    let state = world.get_block_state(pos);
    if state.is_air() {
        return true;
    }
    let block = state.get_block();
    block.has_tag(&BlockTag::FIRE) || block == config.portal
}

const fn block_pos_axis(pos: BlockPos, axis: Axis) -> i32 {
    match axis {
        Axis::X => pos.x(),
        Axis::Y => pos.y(),
        Axis::Z => pos.z(),
    }
}

const fn vec_axis(pos: DVec3, axis: Axis) -> f64 {
    match axis {
        Axis::X => pos.x,
        Axis::Y => pos.y,
        Axis::Z => pos.z,
    }
}

impl PortalShape {
    /// Finds an empty portal frame starting with X axis preferred.
    /// Matches vanilla's `findEmptyPortalShape` as called from `BaseFireBlock.onPlace`.
    pub fn find_empty_portal_shape(
        world: &dyn LevelReader,
        fire_pos: BlockPos,
        config: &PortalFrameConfig,
    ) -> Option<Self> {
        Self::find_empty_portal_shape_with_axis(world, fire_pos, Axis::X, config)
    }

    /// Finds an empty portal frame trying `preferred_axis` first, then the other.
    /// Matches vanilla's `findPortalShape` with the empty-portal predicate.
    pub fn find_empty_portal_shape_with_axis(
        world: &dyn LevelReader,
        fire_pos: BlockPos,
        preferred_axis: Axis,
        config: &PortalFrameConfig,
    ) -> Option<Self> {
        let other_axis = if preferred_axis == Axis::X {
            Axis::Z
        } else {
            Axis::X
        };
        Self::try_axis(world, fire_pos, preferred_axis, config)
            .filter(|s| s.num_portal_blocks == 0)
            .or_else(|| {
                Self::try_axis(world, fire_pos, other_axis, config)
                    .filter(|s| s.num_portal_blocks == 0)
            })
    }

    /// Finds a portal shape on a specific axis.
    /// Used by `update_shape` to check if the portal is still complete.
    pub fn find_any_shape(
        world: &dyn LevelReader,
        pos: BlockPos,
        axis: Axis,
        config: &PortalFrameConfig,
    ) -> Option<Self> {
        Self::try_axis(world, pos, axis, config)
    }

    /// Tries to find a valid portal on a single axis, matching vanilla's detection algorithm.
    fn try_axis(
        world: &dyn LevelReader,
        pos: BlockPos,
        axis: Axis,
        config: &PortalFrameConfig,
    ) -> Option<Self> {
        // Vanilla: rightDir is WEST for X-axis, SOUTH for Z-axis
        let right_dir: Direction = match axis {
            Axis::X => Direction::West,
            Axis::Z => Direction::South,
            Axis::Y => return None,
        };

        let bottom_left = Self::calculate_bottom_left(world, pos, right_dir, config)?;

        let width = Self::calculate_width(world, bottom_left, right_dir, config);
        if width == 0 {
            return None;
        }

        let mut num_portal_blocks = 0;
        let height = Self::calculate_height(
            world,
            bottom_left,
            width,
            right_dir,
            config,
            &mut num_portal_blocks,
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
            num_portal_blocks,
        })
    }

    /// Returns the number of valid interior blocks in `direction` from `pos`, matching vanilla's
    /// `getDistanceUntilEdgeAboveFrame`. Each position must be empty and have a frame block
    /// below it. Returns 0 if the terminating block is not a frame block.
    fn get_distance_until_edge(
        world: &dyn LevelReader,
        pos: BlockPos,
        direction: Direction,
        config: &PortalFrameConfig,
    ) -> u32 {
        for i in 0..=config.max_width {
            let next = pos.relative_n(direction, i as i32);
            if !is_empty(world, next, config) {
                // Edge must be a frame block, otherwise the interior is unbounded
                return if Self::is_frame_block(world, next, config) {
                    i
                } else {
                    0
                };
            }
            if !Self::is_frame_block(world, next.below(), config) {
                return 0;
            }
        }
        0
    }

    /// Finds the bottom-left corner of the portal interior.
    fn calculate_bottom_left(
        world: &dyn LevelReader,
        pos: BlockPos,
        right_dir: Direction,
        config: &PortalFrameConfig,
    ) -> Option<BlockPos> {
        // Scan down to find the lowest empty block above frame
        let mut cur = pos;
        for _ in 0..config.max_height {
            let next = cur.below();
            if !is_empty(world, next, config) {
                break;
            }
            cur = next;
        }

        // Scan in opposite of right_dir to find the left edge
        let left_dir = right_dir.opposite();
        let dist = Self::get_distance_until_edge(world, cur, left_dir, config);
        if dist == 0 {
            return None;
        }
        Some(cur.relative_n(left_dir, (dist - 1) as i32))
    }

    /// Calculates the width of the portal interior from the bottom-left corner.
    fn calculate_width(
        world: &dyn LevelReader,
        bottom_left: BlockPos,
        right_dir: Direction,
        config: &PortalFrameConfig,
    ) -> u32 {
        let dist = Self::get_distance_until_edge(world, bottom_left, right_dir, config);
        if dist < config.min_width || dist > config.max_width {
            return 0;
        }
        dist
    }

    /// Calculates the height while validating side columns and interior.
    /// Also counts portal blocks in the interior via `portal_block_count`.
    ///
    /// Matches vanilla's `getDistanceUntilTop`: always uses `isEmpty` (air/fire/portal)
    /// for interior validation regardless of the outer interior check strategy.
    fn calculate_height(
        world: &dyn LevelReader,
        bottom_left: BlockPos,
        width: u32,
        right_dir: Direction,
        config: &PortalFrameConfig,
        portal_block_count: &mut u32,
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

            // Check interior and count portal blocks
            for w in 0..width {
                let interior_pos = row_start.relative_n(right_dir, w as i32);
                if !is_empty(world, interior_pos, config) {
                    break 'outer;
                }
                if world.get_block_state(interior_pos).get_block() == config.portal {
                    *portal_block_count += 1;
                }
            }
            height = h + 1;
        }
        height
    }

    /// Checks that the top frame row is complete.
    fn has_top_frame(
        world: &dyn LevelReader,
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

    fn is_frame_block(world: &dyn LevelReader, pos: BlockPos, config: &PortalFrameConfig) -> bool {
        world.get_block_state(pos).get_block() == config.frame
    }

    /// Returns `true` if the portal interior is entirely filled with portal blocks.
    /// Matches vanilla's `PortalShape.isComplete()`.
    #[must_use]
    pub const fn is_complete(&self) -> bool {
        self.num_portal_blocks == self.width * self.height
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

    /// Returns vanilla `PortalShape.getRelativePosition`.
    #[must_use]
    pub fn get_relative_position(
        largest_rectangle_around: FoundRectangle,
        axis: Axis,
        position: DVec3,
        dimensions: EntityDimensions,
    ) -> DVec3 {
        let width = f64::from(largest_rectangle_around.axis1_size) - f64::from(dimensions.width);
        let height = f64::from(largest_rectangle_around.axis2_size) - f64::from(dimensions.height);
        let bottom_min = largest_rectangle_around.min_corner;
        let relative_right = if width > 0.0 {
            let bottom_start =
                f64::from(block_pos_axis(bottom_min, axis)) + f64::from(dimensions.width) / 2.0;
            inverse_lerp(vec_axis(position, axis) - bottom_start, 0.0, width).clamp(0.0, 1.0)
        } else {
            0.5
        };

        let relative_up = if height > 0.0 {
            inverse_lerp(
                vec_axis(position, Axis::Y) - f64::from(block_pos_axis(bottom_min, Axis::Y)),
                0.0,
                height,
            )
            .clamp(0.0, 1.0)
        } else {
            0.0
        };

        let forward_axis = if axis == Axis::X { Axis::Z } else { Axis::X };
        let relative_forward = vec_axis(position, forward_axis)
            - (f64::from(block_pos_axis(bottom_min, forward_axis)) + 0.5);
        DVec3::new(relative_right, relative_up, relative_forward)
    }

    /// Returns vanilla `PortalShape.findCollisionFreePosition`.
    #[must_use]
    pub fn find_collision_free_position(
        bottom_center: DVec3,
        world: &Arc<World>,
        entity: &dyn Entity,
        dimensions: EntityDimensions,
    ) -> DVec3 {
        if dimensions.width > 4.0 || dimensions.height > 4.0 {
            return bottom_center;
        }

        let width = f64::from(dimensions.width);
        let height = f64::from(dimensions.height);
        let half_height = height / 2.0;
        let center = bottom_center + DVec3::new(0.0, half_height, 0.0);
        let allowed_centers = [WorldAabb::of_size(center, width, 0.0, width)
            .expand_towards(DVec3::Y)
            .inflate(1.0E-6)];

        WorldCollisionProvider::for_entity(world, entity)
            .find_free_position(&allowed_centers, center, width, height, width)
            .map_or(bottom_center, |pos| pos - DVec3::new(0.0, half_height, 0.0))
    }
}

#[cfg(test)]
mod tests {
    use glam::DVec3;
    use steel_registry::entity_type::EntityDimensions;
    use steel_utils::BlockPos;
    use steel_utils::axis::Axis;
    use steel_utils::block_util::FoundRectangle;

    use super::PortalShape;

    #[test]
    fn relative_portal_position_matches_vanilla_axis_math() {
        let rectangle = FoundRectangle {
            min_corner: BlockPos::new(10, 64, 20),
            axis1_size: 4,
            axis2_size: 5,
        };
        let position = DVec3::new(12.0, 66.0, 20.75);
        let dimensions = EntityDimensions::new(1.0, 2.0, 1.62);

        let relative = PortalShape::get_relative_position(rectangle, Axis::X, position, dimensions);

        assert!((relative.x - 0.5).abs() < f64::EPSILON);
        assert!((relative.y - (2.0 / 3.0)).abs() < f64::EPSILON);
        assert!((relative.z - 0.25).abs() < f64::EPSILON);
    }

    #[test]
    fn relative_portal_position_clamps_right_and_up_offsets() {
        let rectangle = FoundRectangle {
            min_corner: BlockPos::new(10, 64, 20),
            axis1_size: 4,
            axis2_size: 5,
        };
        let position = DVec3::new(8.0, 80.0, 20.5);
        let dimensions = EntityDimensions::new(1.0, 2.0, 1.62);

        let relative = PortalShape::get_relative_position(rectangle, Axis::X, position, dimensions);

        assert!((relative.x - 0.0).abs() < f64::EPSILON);
        assert!((relative.y - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn relative_portal_position_uses_vanilla_fallbacks_when_entity_fills_rectangle() {
        let rectangle = FoundRectangle {
            min_corner: BlockPos::new(10, 64, 20),
            axis1_size: 2,
            axis2_size: 3,
        };
        let position = DVec3::new(11.0, 65.0, 20.25);
        let dimensions = EntityDimensions::new(3.0, 4.0, 1.62);

        let relative = PortalShape::get_relative_position(rectangle, Axis::Z, position, dimensions);

        assert!((relative.x - 0.5).abs() < f64::EPSILON);
        assert!((relative.y - 0.0).abs() < f64::EPSILON);
        assert!((relative.z - 0.5).abs() < f64::EPSILON);
    }
}
