#![expect(missing_docs, reason = "self-explanatory utility types")]

use std::{
    borrow::Cow,
    collections::VecDeque,
    error::Error,
    fmt::{self, Debug, Display, Formatter},
    hash::{Hash, Hasher},
    io::{self, Cursor, Write},
    mem::MaybeUninit,
    str::FromStr,
};

use bitflags::bitflags;
use glam::{DVec3, IVec2, IVec3};
use rustc_hash::FxHashSet;
use serde::{Deserialize, Serialize, de::Error as _};
use simdnbt::owned::{NbtCompound, NbtTag};
use wincode::{SchemaRead, SchemaWrite, config::Config, io::Reader, io::Writer};

use crate::{
    axis::Axis,
    codec::VarInt,
    direction::Direction,
    hash::{ComponentHasher, HashComponent},
    serial::{ReadFrom, WriteTo},
};

/// A placeholder type for unimplemented component values.
/// Unlike `()`, this is a distinct type that can have its own trait implementations.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Todo;

impl WriteTo for Todo {
    fn write(&self, _writer: &mut impl Write) -> io::Result<()> {
        // Placeholder components write nothing
        Ok(())
    }
}

impl ReadFrom for Todo {
    fn read(_data: &mut Cursor<&[u8]>) -> io::Result<Self> {
        // Placeholder components read nothing
        Ok(Todo)
    }
}

impl HashComponent for Todo {
    fn hash_component(&self, hasher: &mut ComponentHasher) {
        // Hash as empty value
        hasher.put_empty();
    }
}

impl simdnbt::ToNbtTag for Todo {
    fn to_nbt_tag(self) -> NbtTag {
        // Placeholder components serialize as empty compound
        NbtTag::Compound(NbtCompound::new())
    }
}

impl simdnbt::FromNbtTag for Todo {
    fn from_nbt_tag(_tag: simdnbt::borrow::NbtTag) -> Option<Self> {
        // Placeholder components always deserialize successfully
        Some(Todo)
    }
}

impl HashComponent for Identifier {
    fn hash_component(&self, hasher: &mut ComponentHasher) {
        // Identifiers are hashed as strings in "namespace:path" format
        hasher.put_string(&self.to_string());
    }
}

impl simdnbt::ToNbtTag for Identifier {
    fn to_nbt_tag(self) -> NbtTag {
        NbtTag::String(self.to_string().into())
    }
}

impl simdnbt::FromNbtTag for Identifier {
    fn from_nbt_tag(tag: simdnbt::borrow::NbtTag) -> Option<Self> {
        let s = tag.string()?.to_str();
        s.parse().ok()
    }
}

/// A raw block state id. Using the registry this id can be derived into a block and it's current properties.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct BlockStateId(pub u16);

impl WriteTo for BlockStateId {
    fn write(&self, writer: &mut impl Write) -> io::Result<()> {
        VarInt(i32::from(self.0)).write(writer)
    }
}

impl ReadFrom for BlockStateId {
    fn read(data: &mut Cursor<&[u8]>) -> io::Result<Self> {
        let id = VarInt::read(data)?.0;
        #[expect(
            clippy::cast_sign_loss,
            reason = "VarInt is validated upstream; block state IDs are non-negative"
        )]
        Ok(Self(id as u16))
    }
}

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
    const SECTION_MASK: i32 = Self::SECTION_SIZE - 1; // 15

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

/// A chunk position in Steel's packed `i64` layout.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, SchemaWrite, SchemaRead)]
pub struct PackedChunkPos(i64);

impl PackedChunkPos {
    /// Creates a packed chunk position from its raw representation.
    #[must_use]
    pub const fn from_raw(raw: i64) -> Self {
        Self(raw)
    }

    /// Returns the raw packed representation.
    #[must_use]
    pub const fn as_raw(self) -> i64 {
        self.0
    }

    /// Converts this packed value into a `ChunkPos`.
    #[must_use]
    pub const fn to_chunk_pos(self) -> ChunkPos {
        ChunkPos(IVec2::new(
            (self.0 & 0xFFFF_FFFF) as i32,
            (self.0 >> 32) as i32,
        ))
    }
}

impl From<ChunkPos> for PackedChunkPos {
    fn from(pos: ChunkPos) -> Self {
        Self((i64::from(pos.0.x) & 0xFFFF_FFFF) | ((i64::from(pos.0.y) & 0xFFFF_FFFF) << 32))
    }
}

impl From<PackedChunkPos> for ChunkPos {
    fn from(pos: PackedChunkPos) -> Self {
        pos.to_chunk_pos()
    }
}

impl ReadFrom for PackedChunkPos {
    fn read(data: &mut Cursor<&[u8]>) -> io::Result<Self> {
        Ok(Self::from_raw(<i64 as ReadFrom>::read(data)?))
    }
}

impl WriteTo for PackedChunkPos {
    fn write(&self, writer: &mut impl Write) -> io::Result<()> {
        self.0.write(writer)
    }
}

/// A block position in Minecraft's packed protocol `i64` layout.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, SchemaWrite, SchemaRead)]
pub struct PackedBlockPos(i64);

impl PackedBlockPos {
    const HORIZONTAL_BITS: u32 = 26;
    const Y_BITS: u32 = 12;
    const X_OFFSET: u32 = Self::HORIZONTAL_BITS + Self::Y_BITS;
    const Z_OFFSET: u32 = Self::Y_BITS;
    const XZ_MASK: i64 = (1i64 << Self::HORIZONTAL_BITS) - 1;
    const Y_MASK: i64 = (1i64 << Self::Y_BITS) - 1;

    /// Creates a packed block position from its raw representation.
    #[must_use]
    pub const fn from_raw(raw: i64) -> Self {
        Self(raw)
    }

    /// Returns the raw packed representation.
    #[must_use]
    pub const fn as_raw(self) -> i64 {
        self.0
    }

    /// Converts this packed value into a `BlockPos`.
    #[must_use]
    pub const fn to_block_pos(self) -> BlockPos {
        let x = self.0 >> Self::X_OFFSET;
        let y = self.0 & Self::Y_MASK;
        let z = (self.0 >> Self::Z_OFFSET) & Self::XZ_MASK;

        let x = (x << (64 - Self::HORIZONTAL_BITS)) >> (64 - Self::HORIZONTAL_BITS);
        let y = (y << (64 - Self::Y_BITS)) >> (64 - Self::Y_BITS);
        let z = (z << (64 - Self::HORIZONTAL_BITS)) >> (64 - Self::HORIZONTAL_BITS);

        BlockPos(IVec3::new(x as i32, y as i32, z as i32))
    }
}

impl From<BlockPos> for PackedBlockPos {
    fn from(pos: BlockPos) -> Self {
        let x = i64::from(pos.0.x);
        let y = i64::from(pos.0.y);
        let z = i64::from(pos.0.z);
        Self(
            ((x & Self::XZ_MASK) << Self::X_OFFSET)
                | ((z & Self::XZ_MASK) << Self::Z_OFFSET)
                | (y & Self::Y_MASK),
        )
    }
}

impl From<PackedBlockPos> for BlockPos {
    fn from(pos: PackedBlockPos) -> Self {
        pos.to_block_pos()
    }
}

impl ReadFrom for PackedBlockPos {
    fn read(data: &mut Cursor<&[u8]>) -> io::Result<Self> {
        Ok(Self::from_raw(<i64 as ReadFrom>::read(data)?))
    }
}

impl WriteTo for PackedBlockPos {
    fn write(&self, writer: &mut impl Write) -> io::Result<()> {
        self.0.write(writer)
    }
}

/// A section position in Minecraft's packed `i64` layout.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, SchemaWrite, SchemaRead)]
pub struct PackedSectionPos(i64);

impl PackedSectionPos {
    const XZ_BITS: u32 = 22;
    const Y_BITS: u32 = 20;
    const X_OFFSET: u32 = Self::XZ_BITS + Self::Y_BITS;
    const Z_OFFSET: u32 = Self::Y_BITS;
    const XZ_MASK: i64 = (1i64 << Self::XZ_BITS) - 1;
    const Y_MASK: i64 = (1i64 << Self::Y_BITS) - 1;

    /// Creates a packed section position from its raw representation.
    #[must_use]
    pub const fn from_raw(raw: i64) -> Self {
        Self(raw)
    }

    /// Returns the raw packed representation.
    #[must_use]
    pub const fn as_raw(self) -> i64 {
        self.0
    }

    /// Converts this packed value into a `SectionPos`.
    #[must_use]
    pub const fn to_section_pos(self) -> SectionPos {
        let x = self.0 >> Self::X_OFFSET;
        let z = (self.0 >> Self::Z_OFFSET) & Self::XZ_MASK;
        let y = self.0 & Self::Y_MASK;

        let x = (x << (64 - Self::XZ_BITS)) >> (64 - Self::XZ_BITS);
        let y = (y << (64 - Self::Y_BITS)) >> (64 - Self::Y_BITS);
        let z = (z << (64 - Self::XZ_BITS)) >> (64 - Self::XZ_BITS);

        SectionPos(IVec3::new(x as i32, y as i32, z as i32))
    }
}

impl From<SectionPos> for PackedSectionPos {
    fn from(pos: SectionPos) -> Self {
        let x = i64::from(pos.0.x);
        let y = i64::from(pos.0.y);
        let z = i64::from(pos.0.z);
        Self(
            ((x & Self::XZ_MASK) << Self::X_OFFSET)
                | ((z & Self::XZ_MASK) << Self::Z_OFFSET)
                | (y & Self::Y_MASK),
        )
    }
}

impl From<PackedSectionPos> for SectionPos {
    fn from(pos: PackedSectionPos) -> Self {
        pos.to_section_pos()
    }
}

impl ReadFrom for PackedSectionPos {
    fn read(data: &mut Cursor<&[u8]>) -> io::Result<Self> {
        Ok(Self::from_raw(<i64 as ReadFrom>::read(data)?))
    }
}

impl WriteTo for PackedSectionPos {
    fn write(&self, writer: &mut impl Write) -> io::Result<()> {
        self.0.write(writer)
    }
}

/// A block's X/Z position packed relative to its containing chunk.
///
/// Layout: `(x << 4) | z`, with each coordinate using 4 bits.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, SchemaWrite, SchemaRead)]
pub struct PackedChunkLocalXZ(u8);

impl PackedChunkLocalXZ {
    const COORD_MASK: u8 = 0x0f;

    /// Packs an absolute block position by masking X and Z to chunk-local range.
    #[must_use]
    pub const fn from_block_pos(pos: BlockPos) -> Self {
        Self::from_local_unchecked(
            (pos.0.x & SectionPos::SECTION_MASK) as u8,
            (pos.0.z & SectionPos::SECTION_MASK) as u8,
        )
    }

    /// Packs validated chunk-local X/Z coordinates.
    #[must_use]
    pub const fn from_local_xz(x: u8, z: u8) -> Option<Self> {
        if x < 16 && z < 16 {
            Some(Self::from_local_unchecked(x, z))
        } else {
            None
        }
    }

    /// Rebuilds a packed chunk-local X/Z position from its raw representation.
    #[must_use]
    pub const fn from_raw(raw: u8) -> Self {
        Self(raw)
    }

    /// Returns the raw packed representation.
    #[must_use]
    pub const fn as_u8(self) -> u8 {
        self.0
    }

    /// Returns the chunk-local X coordinate.
    #[must_use]
    pub const fn x(self) -> u8 {
        (self.0 >> 4) & Self::COORD_MASK
    }

    /// Returns the chunk-local Z coordinate.
    #[must_use]
    pub const fn z(self) -> u8 {
        self.0 & Self::COORD_MASK
    }

    const fn from_local_unchecked(x: u8, z: u8) -> Self {
        Self((x << 4) | z)
    }
}

impl From<BlockPos> for PackedChunkLocalXZ {
    fn from(pos: BlockPos) -> Self {
        Self::from_block_pos(pos)
    }
}

impl ReadFrom for PackedChunkLocalXZ {
    fn read(data: &mut Cursor<&[u8]>) -> io::Result<Self> {
        Ok(Self::from_raw(<u8 as ReadFrom>::read(data)?))
    }
}

impl WriteTo for PackedChunkLocalXZ {
    fn write(&self, writer: &mut impl Write) -> io::Result<()> {
        self.0.write(writer)
    }
}

/// A block position packed relative to its containing 16x16x16 section.
///
/// Layout: `(x << 8) | (z << 4) | y`, with each coordinate using 4 bits.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, SchemaWrite, SchemaRead)]
pub struct PackedSectionBlockPos(u16);

impl PackedSectionBlockPos {
    const COORD_MASK: u16 = 0x0f;
    const RAW_MASK: u16 = 0x0fff;

    /// Packs an absolute block position by masking each coordinate to section-local range.
    #[must_use]
    #[inline]
    pub const fn from_block_pos(pos: BlockPos) -> Self {
        Self::from_local_unchecked(
            (pos.0.x & SectionPos::SECTION_MASK) as u8,
            (pos.0.y & SectionPos::SECTION_MASK) as u8,
            (pos.0.z & SectionPos::SECTION_MASK) as u8,
        )
    }

    /// Packs validated section-local coordinates.
    #[must_use]
    pub const fn from_local_xyz(x: u8, y: u8, z: u8) -> Option<Self> {
        if x < 16 && y < 16 && z < 16 {
            Some(Self::from_local_unchecked(x, y, z))
        } else {
            None
        }
    }

    /// Rebuilds a packed section block position from its raw representation.
    #[must_use]
    pub const fn from_raw(raw: u16) -> Option<Self> {
        if raw & !Self::RAW_MASK == 0 {
            Some(Self(raw))
        } else {
            None
        }
    }

    /// Returns the raw packed representation.
    #[must_use]
    pub const fn as_u16(self) -> u16 {
        self.0
    }

    /// Returns the section-local X coordinate.
    #[must_use]
    pub const fn x(self) -> u8 {
        ((self.0 >> 8) & Self::COORD_MASK) as u8
    }

    /// Returns the section-local Y coordinate.
    #[must_use]
    pub const fn y(self) -> u8 {
        (self.0 & Self::COORD_MASK) as u8
    }

    /// Returns the section-local Z coordinate.
    #[must_use]
    pub const fn z(self) -> u8 {
        ((self.0 >> 4) & Self::COORD_MASK) as u8
    }

    /// Converts this section-relative position to an absolute block position.
    #[must_use]
    pub const fn to_block_pos(self, section_pos: SectionPos) -> BlockPos {
        section_pos.relative_to_block_pos(self)
    }

    const fn from_local_unchecked(x: u8, y: u8, z: u8) -> Self {
        Self(((x as u16) << 8) | ((z as u16) << 4) | y as u16)
    }
}

impl From<BlockPos> for PackedSectionBlockPos {
    fn from(pos: BlockPos) -> Self {
        Self::from_block_pos(pos)
    }
}

impl TryFrom<u16> for PackedSectionBlockPos {
    type Error = InvalidPackedSectionBlockPos;

    fn try_from(raw: u16) -> Result<Self, Self::Error> {
        Self::from_raw(raw).ok_or(InvalidPackedSectionBlockPos { raw })
    }
}

/// Error returned when a raw section-relative block position uses reserved bits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InvalidPackedSectionBlockPos {
    raw: u16,
}

impl InvalidPackedSectionBlockPos {
    /// Returns the invalid raw value.
    #[must_use]
    pub const fn raw(self) -> u16 {
        self.raw
    }
}

impl Display for InvalidPackedSectionBlockPos {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "packed section block position {:#06x} uses reserved bits",
            self.raw
        )
    }
}

impl Error for InvalidPackedSectionBlockPos {}

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

/// The game type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[expect(missing_docs, reason = "variant names are self-explanatory")]
pub enum GameType {
    Survival = 0,
    Creative = 1,
    Adventure = 2,
    Spectator = 3,
}

impl GameType {
    /// Returns the name of the game type.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            GameType::Survival => "survival",
            GameType::Creative => "creative",
            GameType::Adventure => "adventure",
            GameType::Spectator => "spectator",
        }
    }
}

impl ReadFrom for GameType {
    fn read(data: &mut Cursor<&[u8]>) -> io::Result<Self> {
        let value = VarInt::read(data)?.0;
        match value {
            0 => Ok(GameType::Survival),
            1 => Ok(GameType::Creative),
            2 => Ok(GameType::Adventure),
            3 => Ok(GameType::Spectator),
            _ => Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "Invalid GameType",
            )),
        }
    }
}

impl From<GameType> for i8 {
    fn from(value: GameType) -> Self {
        value as i8
    }
}

impl From<GameType> for i32 {
    fn from(value: GameType) -> Self {
        value as i32
    }
}

impl From<GameType> for f32 {
    fn from(value: GameType) -> Self {
        f32::from(value as i8)
    }
}

impl From<i8> for GameType {
    fn from(value: i8) -> Self {
        match value {
            1 => GameType::Creative,
            2 => GameType::Adventure,
            3 => GameType::Spectator,
            _ => GameType::Survival,
        }
    }
}

impl From<i32> for GameType {
    fn from(value: i32) -> Self {
        match value {
            1 => GameType::Creative,
            2 => GameType::Adventure,
            3 => GameType::Spectator,
            _ => GameType::Survival,
        }
    }
}

impl From<f32> for GameType {
    fn from(value: f32) -> Self {
        match value {
            1. => GameType::Creative,
            2. => GameType::Adventure,
            3. => GameType::Spectator,
            _ => GameType::Survival,
        }
    }
}

/// World difficulty level.
///
/// Controls starvation damage thresholds, mob spawning behavior,
/// and various other gameplay tweaks.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum Difficulty {
    /// No hostile mobs, no starvation, health regenerates quickly.
    Peaceful = 0,
    /// Hostile mobs deal less damage, starvation stops at 10 HP.
    Easy = 1,
    /// Default difficulty, starvation stops at 1 HP.
    #[default]
    Normal = 2,
    /// Hostile mobs deal more damage, starvation can kill.
    Hard = 3,
}

#[expect(clippy::match_same_arms, reason = "cause it looks better")]
impl From<u8> for Difficulty {
    fn from(value: u8) -> Self {
        match value {
            0 => Difficulty::Peaceful,
            1 => Difficulty::Easy,
            2 => Difficulty::Normal,
            3 => Difficulty::Hard,
            _ => Difficulty::Normal,
        }
    }
}

impl From<Difficulty> for u8 {
    fn from(value: Difficulty) -> Self {
        value as u8
    }
}

impl ReadFrom for Difficulty {
    fn read(data: &mut Cursor<&[u8]>) -> io::Result<Self> {
        let value = <u8 as ReadFrom>::read(data)?;
        match value {
            0 => Ok(Difficulty::Peaceful),
            1 => Ok(Difficulty::Easy),
            2 => Ok(Difficulty::Normal),
            3 => Ok(Difficulty::Hard),
            _ => Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("Invalid Difficulty: {value}"),
            )),
        }
    }
}

impl WriteTo for Difficulty {
    fn write(&self, writer: &mut impl Write) -> io::Result<()> {
        (*self as u8).write(writer)
    }
}

impl Serialize for Difficulty {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_u8(*self as u8)
    }
}

impl<'de> Deserialize<'de> for Difficulty {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let id = u8::deserialize(deserializer)?;
        Ok(Self::from(id))
    }
}

/// An identifier used by Minecraft.
#[derive(Clone, PartialEq, Eq, Hash, Default)]
pub struct Identifier {
    /// The namespace of the identifier.
    pub namespace: Cow<'static, str>,
    /// The path of the identifier.
    pub path: Cow<'static, str>,
}

impl Debug for Identifier {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.write_str(&format!("{}:{}", self.namespace, self.path))
    }
}

impl Identifier {
    /// The vanilla namespace.
    pub const VANILLA_NAMESPACE: &'static str = "minecraft";

    /// Creates a new `Identifier` with the given namespace and path.
    #[must_use]
    pub fn new(
        namespace: impl Into<Cow<'static, str>>,
        path: impl Into<Cow<'static, str>>,
    ) -> Self {
        Identifier {
            namespace: namespace.into(),
            path: path.into(),
        }
    }
    #[must_use]
    pub const fn new_static(namespace: &'static str, path: &'static str) -> Self {
        Identifier {
            namespace: Cow::Borrowed(namespace),
            path: Cow::Borrowed(path),
        }
    }

    /// Creates a new `Identifier` with the vanilla namespace.
    #[must_use]
    pub const fn vanilla(path: String) -> Self {
        Identifier {
            namespace: Cow::Borrowed(Self::VANILLA_NAMESPACE),
            path: Cow::Owned(path),
        }
    }

    /// Creates a new `Identifier` with the vanilla namespace and a static path.
    #[must_use]
    pub const fn vanilla_static(path: &'static str) -> Self {
        Identifier {
            namespace: Cow::Borrowed(Self::VANILLA_NAMESPACE),
            path: Cow::Borrowed(path),
        }
    }

    /// Returns whether the character is a valid namespace character.
    #[must_use]
    pub const fn valid_namespace_char(char: char) -> bool {
        char == '_'
            || char == '-'
            || char.is_ascii_lowercase()
            || char.is_ascii_digit()
            || char == '.'
    }

    /// Returns whether the character is a valid path character.
    #[must_use]
    pub const fn valid_char(char: char) -> bool {
        Self::valid_namespace_char(char) || char == '/'
    }

    /// Returns whether the namespace is valid.
    pub fn validate_namespace(namespace: &str) -> bool {
        namespace.chars().all(Self::valid_namespace_char)
    }

    /// Returns whether the path is valid.
    pub fn validate_path(path: &str) -> bool {
        path.chars().all(Self::valid_char)
    }

    /// Returns whether the namespace and path are valid.
    #[must_use]
    pub fn validate(namespace: &str, path: &str) -> bool {
        Self::validate_namespace(namespace) && Self::validate_path(path)
    }
}

impl Display for Identifier {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}", self.namespace, self.path)
    }
}

impl FromStr for Identifier {
    type Err = &'static str;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let parts: Vec<&str> = s.split(':').collect();
        if parts.len() != 2 {
            return Err("Invalid resource location");
        }

        if !Identifier::validate_namespace(parts[0]) {
            return Err("Invalid namespace");
        }

        if !Identifier::validate_path(parts[1]) {
            return Err("Invalid path");
        }

        Ok(Identifier {
            namespace: Cow::Owned(parts[0].to_string()),
            path: Cow::Owned(parts[1].to_string()),
        })
    }
}
impl Serialize for Identifier {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}

impl<'de> Deserialize<'de> for Identifier {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        Identifier::from_str(&s).map_err(D::Error::custom)
    }
}

// SAFETY: This implementation delegates to the `str` and `String` implementations
// which are already safe, and the Identifier type has the same serialized representation
// as a String (length-prefixed UTF-8 bytes). The size_of method returns exactly the
// number of bytes that write will produce.
unsafe impl<C: Config> SchemaWrite<C> for Identifier {
    type Src = Identifier;

    fn size_of(src: &Self::Src) -> wincode::WriteResult<usize> {
        <str as SchemaWrite<C>>::size_of(&src.to_string())
    }

    fn write(writer: impl Writer, src: &Self::Src) -> wincode::WriteResult<()> {
        <str as SchemaWrite<C>>::write(writer, &src.to_string())
    }
}

// SAFETY: This implementation delegates to the `String` implementation which is
// already safe, and then validates the result as a valid Identifier. The read
// method initializes `dst` if and only if it returns Ok(()).
unsafe impl<'de, C: Config> SchemaRead<'de, C> for Identifier {
    type Dst = Identifier;

    fn read(reader: impl Reader<'de>, dst: &mut MaybeUninit<Self::Dst>) -> wincode::ReadResult<()> {
        let mut s = MaybeUninit::<String>::uninit();
        <String as SchemaRead<'de, C>>::read(reader, &mut s)?;

        // SAFETY: String::read succeeded, so s is initialized
        let s = unsafe { s.assume_init() };

        dst.write(Identifier::from_str(&s).map_err(wincode::ReadError::Custom)?);
        Ok(())
    }
}

/// Represents the hand used for an interaction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InteractionHand {
    /// The main hand.
    MainHand,
    /// The off hand.
    OffHand,
}

impl ReadFrom for InteractionHand {
    fn read(data: &mut Cursor<&[u8]>) -> io::Result<Self> {
        let id = VarInt::read(data)?.0;
        match id {
            0 => Ok(InteractionHand::MainHand),
            1 => Ok(InteractionHand::OffHand),
            _ => Err(io::Error::other("Invalid InteractionHand id")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_block_pos_roundtrip() {
        let positions = vec![
            BlockPos(IVec3::new(0, -61, -2)),
            BlockPos(IVec3::new(0, 0, 0)),
            BlockPos(IVec3::new(100, 64, -100)),
            BlockPos(IVec3::new(-1000, -64, 1000)),
            BlockPos(IVec3::new(33_554_431, 2047, 33_554_431)), // Max positive values
            BlockPos(IVec3::new(-33_554_432, -2048, -33_554_432)), // Max negative values
        ];

        for pos in positions {
            let encoded = PackedBlockPos::from(pos);
            let decoded = encoded.to_block_pos();
            assert_eq!(
                pos, decoded,
                "Roundtrip failed for {pos:?}: encoded={encoded:?}, decoded={decoded:?}"
            );
        }
    }

    #[test]
    fn test_block_pos_specific_case() {
        // Test the specific case from the bug report
        let pos = BlockPos(IVec3::new(0, -61, -2));
        let encoded = PackedBlockPos::from(pos);
        let decoded = encoded.to_block_pos();
        assert_eq!(pos, decoded, "Position 0, -61, -2 failed roundtrip");
    }

    #[test]
    fn block_pos_within_manhattan_starts_in_vanilla_order() {
        let positions: Vec<_> = BlockPos::new(10, 20, 30)
            .within_manhattan(1, 1, 1)
            .take(7)
            .collect();

        assert_eq!(
            positions,
            [
                BlockPos::new(10, 20, 30),
                BlockPos::new(9, 20, 30),
                BlockPos::new(10, 19, 30),
                BlockPos::new(10, 20, 31),
                BlockPos::new(10, 20, 29),
                BlockPos::new(10, 21, 30),
                BlockPos::new(11, 20, 30),
            ]
        );
    }

    #[test]
    fn block_pos_spiral_around_radius_zero_returns_center() {
        let center = BlockPos::new(10, 20, 30);

        assert_eq!(
            BlockPos::spiral_around(center, 0, Direction::East, Direction::South)
                .collect::<Vec<_>>(),
            [center]
        );
    }

    #[test]
    fn block_pos_spiral_around_matches_vanilla_order() {
        let center = BlockPos::new(10, 20, 30);

        assert_eq!(
            BlockPos::spiral_around(center, 1, Direction::East, Direction::South)
                .collect::<Vec<_>>(),
            [
                center,
                center.east(),
                center.east().south(),
                center.south(),
                center.west().south(),
                center.west(),
                center.west().north(),
                center.north(),
                center.east().north(),
            ]
        );
    }

    #[test]
    fn block_pos_find_closest_match_uses_vanilla_order() {
        let origin = BlockPos::new(10, 20, 30);

        let found =
            origin.find_closest_match(1, 1, |pos| pos == origin.south() || pos == origin.west());

        assert_eq!(found, Some(origin.west()));
    }

    #[test]
    fn packed_chunk_local_xz_masks_absolute_coordinates() {
        let packed = PackedChunkLocalXZ::from_block_pos(BlockPos::new(17, 64, 18));

        assert_eq!(packed.as_u8(), 0x12);
        assert_eq!(packed.x(), 1);
        assert_eq!(packed.z(), 2);
    }

    #[test]
    fn packed_chunk_local_xz_rejects_invalid_local_coordinates() {
        assert!(PackedChunkLocalXZ::from_local_xz(15, 15).is_some());
        assert!(PackedChunkLocalXZ::from_local_xz(16, 0).is_none());
        assert!(PackedChunkLocalXZ::from_local_xz(0, 16).is_none());
    }

    #[test]
    fn entity_positions_floor_before_chunk_and_section_conversion() {
        let pos = DVec3::new(-4352.5, -16.5, -4405.5);

        assert_eq!(BlockPos::from(pos), BlockPos::new(-4353, -17, -4406));
        assert_eq!(ChunkPos::from_entity_pos(pos), ChunkPos::new(-273, -276));
        assert_eq!(
            SectionPos::from_entity_pos(pos),
            SectionPos::new(-273, -2, -276)
        );
    }

    #[test]
    fn packed_section_block_pos_masks_absolute_coordinates() {
        let packed = PackedSectionBlockPos::from_block_pos(BlockPos::new(17, -1, 18));

        assert_eq!(packed.as_u16(), 0x12f);
        assert_eq!(packed.x(), 1);
        assert_eq!(packed.y(), 15);
        assert_eq!(packed.z(), 2);
    }

    #[test]
    fn packed_section_block_pos_rejects_invalid_raw_bits() {
        assert!(PackedSectionBlockPos::from_raw(0x0fff).is_some());
        assert!(PackedSectionBlockPos::from_raw(0x1000).is_none());
    }

    #[test]
    fn packed_section_block_pos_rejects_invalid_local_coordinates() {
        assert!(PackedSectionBlockPos::from_local_xyz(15, 15, 15).is_some());
        assert!(PackedSectionBlockPos::from_local_xyz(16, 0, 0).is_none());
        assert!(PackedSectionBlockPos::from_local_xyz(0, 16, 0).is_none());
        assert!(PackedSectionBlockPos::from_local_xyz(0, 0, 16).is_none());
    }

    #[test]
    fn packed_section_block_pos_converts_to_absolute_block_pos() {
        let section = SectionPos::new(2, -4, -3);
        let Some(packed) = PackedSectionBlockPos::from_local_xyz(1, 15, 2) else {
            panic!("valid local packed section block position was rejected");
        };

        assert_eq!(packed.to_block_pos(section), BlockPos::new(33, -49, -46));
        assert_eq!(
            section.relative_to_block_pos(packed),
            BlockPos::new(33, -49, -46)
        );
    }

    #[test]
    fn packed_position_newtypes_roundtrip() {
        let chunk = ChunkPos::new(-12, 34);
        assert_eq!(PackedChunkPos::from(chunk).to_chunk_pos(), chunk);

        let block = BlockPos::new(-1024, 64, 2048);
        assert_eq!(PackedBlockPos::from(block).to_block_pos(), block);

        let section = SectionPos::new(-8, -4, 12);
        assert_eq!(PackedSectionPos::from(section).to_section_pos(), section);
    }
}

/// Flags that control how a block update is processed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct UpdateFlags(u16);

bitflags! {
    impl UpdateFlags: u16 {
        const UPDATE_NEIGHBORS = 1;
        const UPDATE_CLIENTS = 1 << 1;
        const UPDATE_INVISIBLE = 1 << 2;
        const UPDATE_IMMEDIATE = 1 << 3;
        const UPDATE_KNOWN_SHAPE = 1 << 4;
        const UPDATE_SUPPRESS_DROPS = 1 << 5;
        const UPDATE_MOVE_BY_PISTON = 1 << 6;
        const UPDATE_SKIP_SHAPE_UPDATE_ON_WIRE = 1 << 7;
        const UPDATE_SKIP_BLOCK_ENTITY_SIDEEFFECTS = 1 << 8;
        const UPDATE_SKIP_ON_PLACE = 1 << 9;

        const UPDATE_NONE = Self::UPDATE_INVISIBLE.bits() | Self::UPDATE_SKIP_BLOCK_ENTITY_SIDEEFFECTS.bits();
        const UPDATE_ALL = Self::UPDATE_NEIGHBORS.bits() | Self::UPDATE_CLIENTS.bits();
        const UPDATE_ALL_IMMEDIATE = Self::UPDATE_ALL.bits() | Self::UPDATE_IMMEDIATE.bits();
    }
}
