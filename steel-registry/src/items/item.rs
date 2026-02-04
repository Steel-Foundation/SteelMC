//! Item-related types.
//!
//! Dynamic item behavior has been moved to `steel-core::behavior`.
//! This file contains data structures that are needed by other crates.

use std::io::{self, Cursor};

use steel_utils::BlockPos;
use steel_utils::math::Vector3;
use steel_utils::serial::ReadFrom;

use crate::blocks::properties::Direction;

/// Result of a ray cast hitting a block.
///
/// This is kept in steel-registry because it's used by steel-protocol
/// for packet deserialization.
#[derive(Debug, Clone)]
pub struct BlockHitResult {
    /// The exact location where the ray hit the block.
    pub location: Vector3<f64>,
    /// The face of the block that was hit.
    pub direction: Direction,
    /// The position of the block that was hit.
    pub block_pos: BlockPos,
    /// Whether this is a miss (no block hit).
    pub miss: bool,
    /// Whether the hit location is inside the block.
    pub inside: bool,
    /// Whether the world border was hit.
    pub world_border_hit: bool,
}

impl ReadFrom for BlockHitResult {
    fn read(data: &mut Cursor<&[u8]>) -> io::Result<Self> {
        let block_pos = BlockPos::read(data)?;
        let direction = Direction::read(data)?;
        // Click coordinates are relative to the block position (0.0 to 1.0 range)
        let click_x = f32::read(data)?;
        let click_y = f32::read(data)?;
        let click_z = f32::read(data)?;
        let inside = bool::read(data)?;
        let world_border_hit = bool::read(data)?;

        // Convert to absolute world coordinates by adding block position
        // (matching Java's FriendlyByteBuf.readBlockHitResult)
        let location = Vector3::new(
            f64::from(block_pos.x()) + f64::from(click_x),
            f64::from(block_pos.y()) + f64::from(click_y),
            f64::from(block_pos.z()) + f64::from(click_z),
        );

        Ok(BlockHitResult {
            location,
            direction,
            block_pos,
            miss: false,
            inside,
            world_border_hit,
        })
    }
}
