use std::{
    error::Error,
    fmt::{self, Display, Formatter},
    io::{self, Cursor, Write},
};

use glam::{IVec2, IVec3};
use wincode::{SchemaRead, SchemaWrite};

use crate::serial::{ReadFrom, WriteTo};

use super::position::{BlockPos, ChunkPos, SectionPos};

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
    pub(super) const HORIZONTAL_BITS: u32 = 26;
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
