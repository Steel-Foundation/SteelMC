use std::{
    collections::VecDeque,
    hash::{Hash, Hasher},
    io::{self, Cursor, Write},
};

use glam::{DVec3, IVec2, IVec3};
use rustc_hash::FxHashSet;

use crate::{
    axis::Axis,
    direction::Direction,
    serial::{ReadFrom, WriteTo},
};

use super::{
    identifier::Identifier,
    packed_position::{PackedBlockPos, PackedChunkPos, PackedSectionBlockPos, PackedSectionPos},
};

/// A chunk position.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChunkPos(pub IVec2);

impl Hash for ChunkPos {
    fn hash<H: Hasher>(&self, state: &mut H) {
        state.write_u64(PackedChunkPos::from(*self).as_raw() as u64);
    }
}

impl ChunkPos {
    const OFFSETS: [(i32, i32); 8] = [
        (-1, -1),
        (0, -1),
        (1, -1),
        (-1, 0),
        (1, 0),
        (-1, 1),
        (0, 1),
        (1, 1),
    ];

    /// Safety margin in chunks for world generation dependencies.
    /// Calculated as `(32 + GENERATION_PYRAMID.getStepTo(FULL).accumulatedDependencies().size() + 1) * 2`.
    /// The accumulated dependencies size for FULL is 9 (radius 8 + 1).
    const SAFETY_MARGIN_CHUNKS: i32 = (32 + 12 + 1) * 2;

    /// Maximum valid chunk coordinate value.
    /// Calculated as `SectionPos.blockToSectionCoord(MAX_HORIZONTAL_COORDINATE) - SAFETY_MARGIN_CHUNKS`.
    pub const MAX_COORDINATE_VALUE: i32 =
        SectionPos::block_to_section_coord(BlockPos::MAX_HORIZONTAL_COORDINATE)
            - Self::SAFETY_MARGIN_CHUNKS;

    /// Returns all 8 neighbors of this chunk position.
    #[must_use]
    pub fn neighbors(self) -> [ChunkPos; 8] {
        Self::OFFSETS.map(|(dx, dy)| ChunkPos::new(self.0.x + dx, self.0.y + dy))
    }

    #[must_use]
    #[inline]
    /// Creates a new `ChunkPos` with the given x and y coordinates.
    pub const fn new(x: i32, y: i32) -> Self {
        Self(IVec2::new(x, y))
    }

    /// Creates a `ChunkPos` from a world block position.
    #[must_use]
    pub const fn from_block_pos(pos: BlockPos) -> Self {
        Self::new(
            SectionPos::block_to_section_coord(pos.0.x),
            SectionPos::block_to_section_coord(pos.0.z),
        )
    }

    /// Creates a `ChunkPos` containing the given floating-point world position.
    #[must_use]
    pub fn from_entity_pos(pos: DVec3) -> Self {
        Self::from_block_pos(BlockPos::from(pos))
    }

    /// Checks if the given chunk coordinates are within valid bounds.
    /// Uses `Mth.absMax(x, z) <= MAX_COORDINATE_VALUE`.
    #[must_use]
    #[inline]
    pub const fn is_valid(x: i32, z: i32) -> bool {
        x.abs().max(z.abs()) <= Self::MAX_COORDINATE_VALUE
    }
}

impl WriteTo for ChunkPos {
    fn write(&self, writer: &mut impl Write) -> io::Result<()> {
        self.0.write(writer)
    }
}

impl ReadFrom for ChunkPos {
    fn read(data: &mut Cursor<&[u8]>) -> io::Result<Self> {
        Ok(Self(IVec2::read(data)?))
    }
}

/// A block position.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct BlockPos(pub IVec3);

/// Result of processing a node during bfs
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TraversalNodeStatus {
    /// Count the node and visit its neighbors if depth allows
    Accept,
    /// Do not count the node or visit its neighbors
    Skip,
    /// Stop traversal immediately
    Stop,
}

/// Iterator returned by [`BlockPos::spiral_around`].
#[derive(Clone, Debug)]
pub struct SpiralAround {
    directions: [Direction; 4],
    cursor: BlockPos,
    legs: i32,
    leg: i32,
    leg_size: i32,
    leg_index: i32,
    last: BlockPos,
}

impl Iterator for SpiralAround {
    type Item = BlockPos;

    fn next(&mut self) -> Option<Self::Item> {
        let direction_index = (self.leg + 4).rem_euclid(4) as usize;
        self.cursor = self.last.relative(self.directions[direction_index]);
        self.last = self.cursor;

        if self.leg_index >= self.leg_size {
            if self.leg >= self.legs {
                return None;
            }

            self.leg += 1;
            self.leg_index = 0;
            self.leg_size = self.leg / 2 + 1;
        }

        self.leg_index += 1;
        Some(self.cursor)
    }
}

impl From<DVec3> for BlockPos {
    fn from(value: DVec3) -> Self {
        BlockPos(IVec3 {
            x: value.x.floor() as i32,
            y: value.y.floor() as i32,
            z: value.z.floor() as i32,
        })
    }
}

impl BlockPos {
    pub const ZERO: BlockPos = BlockPos(IVec3::new(0, 0, 0));

    /// Maximum horizontal coordinate value: `(1 << 26) / 2 - 1 = 33554431`
    pub const MAX_HORIZONTAL_COORDINATE: i32 = (1 << PackedBlockPos::HORIZONTAL_BITS) / 2 - 1;

    /// Creates a new `BlockPos` from coordinates.
    #[must_use]
    pub const fn new(x: i32, y: i32, z: i32) -> Self {
        Self(IVec3::new(x, y, z))
    }

    /// Returns a new `BlockPos` offset by the given amounts.
    #[must_use]
    pub const fn offset(&self, dx: i32, dy: i32, dz: i32) -> Self {
        Self(IVec3::new(self.0.x + dx, self.0.y + dy, self.0.z + dz))
    }

    /// Returns the x coordinate.
    #[must_use]
    pub const fn x(&self) -> i32 {
        self.0.x
    }

    /// Returns the y coordinate.
    #[must_use]
    pub const fn y(&self) -> i32 {
        self.0.y
    }

    /// Returns the z coordinate.
    #[must_use]
    pub const fn z(&self) -> i32 {
        self.0.z
    }

    /// Returns the position one block above (Y + 1).
    #[must_use]
    pub const fn above(&self) -> Self {
        self.offset(0, 1, 0)
    }

    /// Returns the position `n` blocks above (Y + n).
    #[must_use]
    pub const fn above_n(&self, n: i32) -> Self {
        self.offset(0, n, 0)
    }

    /// Returns the position one block below (Y - 1).
    #[must_use]
    pub const fn below(&self) -> Self {
        self.offset(0, -1, 0)
    }

    /// Returns the position `n` blocks below (Y - n).
    #[must_use]
    pub const fn below_n(&self, n: i32) -> Self {
        self.offset(0, -n, 0)
    }

    /// Returns the position one block to the north (Z - 1).
    #[must_use]
    pub const fn north(&self) -> Self {
        self.offset(0, 0, -1)
    }

    /// Returns the position `n` blocks to the north (Z - n).
    #[must_use]
    pub const fn north_n(&self, n: i32) -> Self {
        self.offset(0, 0, -n)
    }

    /// Returns the position one block to the south (Z + 1).
    #[must_use]
    pub const fn south(&self) -> Self {
        self.offset(0, 0, 1)
    }

    /// Returns the position `n` blocks to the south (Z + n).
    #[must_use]
    pub const fn south_n(&self, n: i32) -> Self {
        self.offset(0, 0, n)
    }

    /// Returns the position one block to the west (X - 1).
    #[must_use]
    pub const fn west(&self) -> Self {
        self.offset(-1, 0, 0)
    }

    /// Returns the position `n` blocks to the west (X - n).
    #[must_use]
    pub const fn west_n(&self, n: i32) -> Self {
        self.offset(-n, 0, 0)
    }

    /// Returns the position one block to the east (X + 1).
    #[must_use]
    pub const fn east(&self) -> Self {
        self.offset(1, 0, 0)
    }

    /// Returns the position `n` blocks to the east (X + n).
    #[must_use]
    pub const fn east_n(&self, n: i32) -> Self {
        self.offset(n, 0, 0)
    }

    /// Returns the position offset by one block in the given direction.
    #[must_use]
    pub fn relative(self, direction: Direction) -> Self {
        Self(self.0 + direction.offset_vec())
    }

    /// Does a breadth-first traversal of all block pos from `start_pos`
    #[must_use]
    pub fn breadth_first_traversal<NP, P>(
        start_pos: Self,
        max_depth: i32,
        max_count: i32,
        mut neighbor_provider: NP,
        mut node_processor: P,
    ) -> i32
    where
        NP: FnMut(Self, &mut dyn FnMut(Self)),
        P: FnMut(Self) -> TraversalNodeStatus,
    {
        let mut nodes = VecDeque::from([(start_pos, 0)]);
        let mut visited = FxHashSet::default();
        let mut count = 0;

        while let Some((current_pos, depth)) = nodes.pop_front() {
            if !visited.insert(current_pos) {
                continue;
            }

            let next = node_processor(current_pos);
            if next == TraversalNodeStatus::Skip {
                continue;
            }

            if next == TraversalNodeStatus::Stop {
                break;
            }

            count += 1;
            if count >= max_count {
                return count;
            }

            if depth < max_depth {
                let next_depth = depth + 1;
                neighbor_provider(current_pos, &mut |pos| nodes.push_back((pos, next_depth)));
            }
        }

        count
    }

    /// Returns the position offset by `n` blocks in the given direction.
    #[must_use]
    pub fn relative_n(&self, direction: Direction, n: i32) -> Self {
        if n == 0 {
            *self
        } else {
            Self(self.0 + direction.offset_vec() * n)
        }
    }

    /// Returns vanilla `BlockPos.spiralAround`.
    ///
    /// # Panics
    ///
    /// Panics if `radius` is negative or if both directions are on the same axis.
    #[must_use]
    pub fn spiral_around(
        center: Self,
        radius: i32,
        first_direction: Direction,
        second_direction: Direction,
    ) -> SpiralAround {
        assert!(radius >= 0, "spiral radius must be non-negative");
        assert!(
            first_direction.get_axis() != second_direction.get_axis(),
            "spiral directions cannot be on the same axis"
        );

        let cursor = center.relative(second_direction);
        SpiralAround {
            directions: [
                first_direction,
                second_direction,
                first_direction.opposite(),
                second_direction.opposite(),
            ],
            cursor,
            legs: 4 * radius,
            leg: -1,
            leg_size: 0,
            leg_index: 0,
            last: cursor,
        }
    }

    /// Returns the position offset by `n` blocks along the given axis.
    #[must_use]
    pub const fn relative_axis(&self, axis: Axis, n: i32) -> Self {
        if n == 0 {
            *self
        } else {
            match axis {
                Axis::X => self.offset(n, 0, 0),
                Axis::Y => self.offset(0, n, 0),
                Axis::Z => self.offset(0, 0, n),
            }
        }
    }

    /// Returns a new position with the same X and Z but the given Y.
    #[must_use]
    pub const fn at_y(&self, y: i32) -> Self {
        Self::new(self.0.x, y, self.0.z)
    }

    /// Returns a new position with all coordinates multiplied by the given factor.
    #[must_use]
    pub const fn multiply(&self, factor: i32) -> Self {
        if factor == 1 {
            *self
        } else if factor == 0 {
            Self::ZERO
        } else {
            Self::new(self.0.x * factor, self.0.y * factor, self.0.z * factor)
        }
    }

    /// Returns the center of this block as a floating-point position.
    #[must_use]
    pub fn get_center(&self) -> (f64, f64, f64) {
        (
            f64::from(self.0.x) + 0.5,
            f64::from(self.0.y) + 0.5,
            f64::from(self.0.z) + 0.5,
        )
    }

    /// Returns the bottom center of this block (center of the bottom face).
    #[must_use]
    pub fn get_bottom_center(&self) -> (f64, f64, f64) {
        (
            f64::from(self.0.x) + 0.5,
            f64::from(self.0.y),
            f64::from(self.0.z) + 0.5,
        )
    }

    /// Creates a `BlockPos` containing the given floating-point coordinates.
    #[must_use]
    pub const fn containing(x: f64, y: f64, z: f64) -> Self {
        Self::new(x.floor() as i32, y.floor() as i32, z.floor() as i32)
    }

    /// Returns the minimum coordinates of two positions.
    #[must_use]
    pub const fn min(a: BlockPos, b: BlockPos) -> Self {
        Self::new(a.0.x.min(b.0.x), a.0.y.min(b.0.y), a.0.z.min(b.0.z))
    }

    /// Returns the maximum coordinates of two positions.
    #[must_use]
    pub const fn max(a: BlockPos, b: BlockPos) -> Self {
        Self::new(a.0.x.max(b.0.x), a.0.y.max(b.0.y), a.0.z.max(b.0.z))
    }

    /// Returns positions in vanilla `BlockPos.withinManhattan` order.
    #[must_use]
    pub const fn within_manhattan(
        self,
        reach_x: i32,
        reach_y: i32,
        reach_z: i32,
    ) -> BlockPosWithinManhattan {
        BlockPosWithinManhattan {
            origin: self,
            reach_x,
            reach_y,
            reach_z,
            max_depth: reach_x + reach_y + reach_z,
            current_depth: 0,
            max_x: 0,
            max_y: 0,
            x: 0,
            y: 0,
            pending_z_mirror: None,
            done: false,
        }
    }

    /// Returns vanilla `BlockPos.findClosestMatch`.
    #[must_use]
    pub fn find_closest_match(
        self,
        horizontal_search_radius: i32,
        vertical_search_radius: i32,
        mut predicate: impl FnMut(BlockPos) -> bool,
    ) -> Option<BlockPos> {
        self.within_manhattan(
            horizontal_search_radius,
            vertical_search_radius,
            horizontal_search_radius,
        )
        .find(|pos| predicate(*pos))
    }
}

/// Iterator returned by [`BlockPos::within_manhattan`].
#[derive(Debug, Clone)]
pub struct BlockPosWithinManhattan {
    origin: BlockPos,
    reach_x: i32,
    reach_y: i32,
    reach_z: i32,
    max_depth: i32,
    current_depth: i32,
    max_x: i32,
    max_y: i32,
    x: i32,
    y: i32,
    pending_z_mirror: Option<BlockPos>,
    done: bool,
}

impl Iterator for BlockPosWithinManhattan {
    type Item = BlockPos;

    fn next(&mut self) -> Option<Self::Item> {
        if let Some(pos) = self.pending_z_mirror.take() {
            return Some(pos);
        }
        if self.done {
            return None;
        }

        loop {
            if self.y > self.max_y {
                self.x += 1;
                if self.x > self.max_x {
                    self.current_depth += 1;
                    if self.current_depth > self.max_depth {
                        self.done = true;
                        return None;
                    }

                    self.max_x = self.reach_x.min(self.current_depth);
                    self.x = -self.max_x;
                }

                self.max_y = self.reach_y.min(self.current_depth - self.x.abs());
                self.y = -self.max_y;
            }

            let x = self.x;
            let y = self.y;
            let z = self.current_depth - x.abs() - y.abs();
            self.y += 1;
            if z > self.reach_z {
                continue;
            }

            let pos = self.origin.offset(x, y, z);
            if z != 0 {
                self.pending_z_mirror = Some(self.origin.offset(x, y, -z));
            }
            return Some(pos);
        }
    }
}

impl ReadFrom for BlockPos {
    fn read(data: &mut Cursor<&[u8]>) -> io::Result<Self> {
        let packed = <i64 as ReadFrom>::read(data)?;
        Ok(PackedBlockPos::from_raw(packed).into())
    }
}

/// A position tied to a dimension key.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct GlobalPos {
    /// Dimension containing the block position.
    pub dimension: Identifier,
    /// Block position within the dimension.
    pub pos: BlockPos,
}

impl GlobalPos {
    /// Creates a new global position.
    #[must_use]
    pub const fn new(dimension: Identifier, pos: BlockPos) -> Self {
        Self { dimension, pos }
    }
}

impl ReadFrom for GlobalPos {
    fn read(data: &mut Cursor<&[u8]>) -> io::Result<Self> {
        Ok(Self {
            dimension: <Identifier as ReadFrom>::read(data)?,
            pos: BlockPos::read(data)?,
        })
    }
}

impl WriteTo for GlobalPos {
    fn write(&self, writer: &mut impl Write) -> io::Result<()> {
        self.dimension.write(writer)?;
        self.pos.write(writer)
    }
}

/// A chunk section position (16x16x16 region).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SectionPos(pub IVec3);

impl SectionPos {
    const SECTION_BITS: i32 = 4;
    const SECTION_SIZE: i32 = 1 << Self::SECTION_BITS; // 16
    pub(super) const SECTION_MASK: i32 = Self::SECTION_SIZE - 1; // 15

    /// Creates a new `SectionPos` from section coordinates.
    #[must_use]
    pub const fn new(x: i32, y: i32, z: i32) -> Self {
        Self(IVec3::new(x, y, z))
    }

    /// Converts a block coordinate to a section coordinate.
    #[must_use]
    #[inline]
    pub const fn block_to_section_coord(block_coord: i32) -> i32 {
        block_coord >> Self::SECTION_BITS
    }

    /// Creates a `SectionPos` from a `BlockPos`.
    #[must_use]
    pub const fn from_block_pos(pos: BlockPos) -> Self {
        Self::new(
            Self::block_to_section_coord(pos.0.x),
            Self::block_to_section_coord(pos.0.y),
            Self::block_to_section_coord(pos.0.z),
        )
    }

    /// Creates a `SectionPos` containing the given floating-point world position.
    #[must_use]
    pub fn from_entity_pos(pos: DVec3) -> Self {
        Self::from_block_pos(BlockPos::from(pos))
    }

    /// Gets the X coordinate.
    #[must_use]
    pub const fn x(&self) -> i32 {
        self.0.x
    }

    /// Gets the Y coordinate.
    #[must_use]
    pub const fn y(&self) -> i32 {
        self.0.y
    }

    /// Gets the Z coordinate.
    #[must_use]
    pub const fn z(&self) -> i32 {
        self.0.z
    }

    /// Converts section-relative coordinates to an absolute block X coordinate.
    #[must_use]
    pub const fn relative_to_block_x(&self, relative: PackedSectionBlockPos) -> i32 {
        (self.0.x << Self::SECTION_BITS) + relative.x() as i32
    }

    /// Converts section-relative coordinates to an absolute block Y coordinate.
    #[must_use]
    pub const fn relative_to_block_y(&self, relative: PackedSectionBlockPos) -> i32 {
        (self.0.y << Self::SECTION_BITS) + relative.y() as i32
    }

    /// Converts section-relative coordinates to an absolute block Z coordinate.
    #[must_use]
    pub const fn relative_to_block_z(&self, relative: PackedSectionBlockPos) -> i32 {
        (self.0.z << Self::SECTION_BITS) + relative.z() as i32
    }

    /// Packs a block position into a section-relative offset.
    /// Format: (x << 8) | (z << 4) | y (each coordinate masked to 4 bits)
    #[must_use]
    #[inline]
    pub const fn section_relative_pos(pos: BlockPos) -> PackedSectionBlockPos {
        PackedSectionBlockPos::from_block_pos(pos)
    }

    /// Converts a section-relative packed position back to a block position.
    #[must_use]
    pub const fn relative_to_block_pos(&self, relative: PackedSectionBlockPos) -> BlockPos {
        BlockPos(IVec3::new(
            self.relative_to_block_x(relative),
            self.relative_to_block_y(relative),
            self.relative_to_block_z(relative),
        ))
    }
}

impl ReadFrom for SectionPos {
    fn read(data: &mut Cursor<&[u8]>) -> io::Result<Self> {
        Ok(<PackedSectionPos as ReadFrom>::read(data)?.into())
    }
}

impl WriteTo for SectionPos {
    fn write(&self, writer: &mut impl Write) -> io::Result<()> {
        PackedSectionPos::from(*self).write(writer)
    }
}
