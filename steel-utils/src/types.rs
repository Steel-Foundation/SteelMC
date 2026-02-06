#![allow(missing_docs)]

use std::{
    borrow::Cow,
    fmt::{self, Debug, Display, Formatter},
    hash::{Hash, Hasher},
    io::{self, Cursor, Write},
    mem::MaybeUninit,
    str::FromStr,
};

use bitflags::bitflags;
use serde::{Deserialize, Serialize, de::Error as _};
use simdnbt::owned::{NbtCompound, NbtTag};
use wincode::{SchemaRead, SchemaWrite, config::Config, io::Reader, io::Writer};

use crate::{
    codec::VarInt,
    direction::Direction,
    hash::{ComponentHasher, HashComponent},
    math::{Axis, Vector2, Vector3},
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
        #[allow(clippy::cast_sign_loss)]
        Ok(Self(id as u16))
    }
}

/// A chunk position.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct ChunkPos(pub Vector2<i32>);

impl Hash for ChunkPos {
    fn hash<H: Hasher>(&self, state: &mut H) {
        state.write_u64(self.as_i64() as u64);
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
    pub fn neighbors(&self) -> [ChunkPos; 8] {
        Self::OFFSETS.map(|(dx, dy)| ChunkPos::new(self.0.x + dx, self.0.y + dy))
    }

    #[must_use]
    #[inline]
    /// Creates a new `ChunkPos` with the given x and y coordinates.
    pub const fn new(x: i32, y: i32) -> Self {
        Self(Vector2::new(x, y))
    }

    /// Checks if the given chunk coordinates are within valid bounds.
    /// Uses `Mth.absMax(x, z) <= MAX_COORDINATE_VALUE`.
    #[must_use]
    #[inline]
    pub const fn is_valid(x: i32, z: i32) -> bool {
        x.abs().max(z.abs()) <= Self::MAX_COORDINATE_VALUE
    }

    /// Converts the `ChunkPos` to an `i64`.
    #[must_use]
    #[inline]
    pub fn as_i64(&self) -> i64 {
        (i64::from(self.0.x) & 0xFFFF_FFFF) | ((i64::from(self.0.y) & 0xFFFF_FFFF) << 32)
    }

    /// Creates a new `ChunkPos` from an `i64`.
    #[must_use]
    #[inline]
    pub const fn from_i64(value: i64) -> Self {
        Self(Vector2::new(
            (value & 0xFFFF_FFFF) as i32,
            (value >> 32) as i32,
        ))
    }
}

#[allow(missing_docs)]
impl WriteTo for ChunkPos {
    fn write(&self, writer: &mut impl Write) -> io::Result<()> {
        self.0.write(writer)
    }
}

#[allow(missing_docs)]
impl ReadFrom for ChunkPos {
    fn read(data: &mut Cursor<&[u8]>) -> io::Result<Self> {
        Ok(Self(Vector2::<i32>::read(data)?))
    }
}

/// A block position.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct BlockPos(pub Vector3<i32>);

impl BlockPos {
    // Define constants as per the Java logic
    const PACKED_HORIZONTAL_LEN: u32 = 26;
    const PACKED_Y_LEN: u32 = 12;
    const X_OFFSET: u32 = Self::PACKED_HORIZONTAL_LEN + Self::PACKED_Y_LEN; // 38
    const Z_OFFSET: u32 = Self::PACKED_Y_LEN; // 12
    const PACKED_X_MASK: i64 = (1i64 << Self::PACKED_HORIZONTAL_LEN) - 1;
    const PACKED_Y_MASK: i64 = (1i64 << Self::PACKED_Y_LEN) - 1;
    const PACKED_Z_MASK: i64 = (1i64 << Self::PACKED_HORIZONTAL_LEN) - 1;
    pub const ZERO: BlockPos = BlockPos(Vector3::new(0, 0, 0));

    /// Maximum horizontal coordinate value: `(1 << 26) / 2 - 1 = 33554431`
    pub const MAX_HORIZONTAL_COORDINATE: i32 = (1 << Self::PACKED_HORIZONTAL_LEN) / 2 - 1;

    /// Creates a new `BlockPos` from coordinates.
    #[must_use]
    pub const fn new(x: i32, y: i32, z: i32) -> Self {
        Self(Vector3::new(x, y, z))
    }

    /// Converts the `BlockPos` to an `i64`.
    /// Layout: X (26 bits, offset 38) | Z (26 bits, offset 12) | Y (12 bits, offset 0)
    #[must_use]
    pub fn as_i64(&self) -> i64 {
        let x = i64::from(self.0.x);
        let y = i64::from(self.0.y);
        let z = i64::from(self.0.z);
        ((x & Self::PACKED_X_MASK) << Self::X_OFFSET)
            | ((z & Self::PACKED_Z_MASK) << Self::Z_OFFSET)
            | (y & Self::PACKED_Y_MASK)
    }

    /// Creates a `BlockPos` from an `i64`.
    /// Layout: X (26 bits, offset 38) | Z (26 bits, offset 12) | Y (12 bits, offset 0)
    #[must_use]
    pub const fn from_i64(value: i64) -> Self {
        let x = value >> Self::X_OFFSET;
        let y = value & Self::PACKED_Y_MASK;
        let z = (value >> Self::Z_OFFSET) & Self::PACKED_Z_MASK;

        // Sign extend the values
        let x = (x << (64 - Self::PACKED_HORIZONTAL_LEN)) >> (64 - Self::PACKED_HORIZONTAL_LEN);
        let y = (y << (64 - Self::PACKED_Y_LEN)) >> (64 - Self::PACKED_Y_LEN);
        let z = (z << (64 - Self::PACKED_HORIZONTAL_LEN)) >> (64 - Self::PACKED_HORIZONTAL_LEN);

        Self(Vector3::new(x as i32, y as i32, z as i32))
    }

    /// Returns a new `BlockPos` offset by the given amounts.
    #[must_use]
    pub const fn offset(&self, dx: i32, dy: i32, dz: i32) -> Self {
        Self(Vector3::new(self.0.x + dx, self.0.y + dy, self.0.z + dz))
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
    pub const fn relative(&self, direction: Direction) -> Self {
        let (dx, dy, dz) = direction.offset();
        self.offset(dx, dy, dz)
    }

    /// Returns the position offset by `n` blocks in the given direction.
    #[must_use]
    pub const fn relative_n(&self, direction: Direction, n: i32) -> Self {
        if n == 0 {
            *self
        } else {
            let (dx, dy, dz) = direction.offset();
            self.offset(dx * n, dy * n, dz * n)
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
    pub const fn min(a: &BlockPos, b: &BlockPos) -> Self {
        Self::new(a.0.x.min(b.0.x), a.0.y.min(b.0.y), a.0.z.min(b.0.z))
    }

    /// Returns the maximum coordinates of two positions.
    #[must_use]
    pub const fn max(a: &BlockPos, b: &BlockPos) -> Self {
        Self::new(a.0.x.max(b.0.x), a.0.y.max(b.0.y), a.0.z.max(b.0.z))
    }
}

impl ReadFrom for BlockPos {
    fn read(data: &mut Cursor<&[u8]>) -> io::Result<Self> {
        let packed = <i64 as ReadFrom>::read(data)?;
        Ok(Self::from_i64(packed))
    }
}

/// A chunk section position (16x16x16 region).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SectionPos(pub Vector3<i32>);

impl SectionPos {
    const SECTION_BITS: i32 = 4;
    const SECTION_SIZE: i32 = 1 << Self::SECTION_BITS; // 16
    const SECTION_MASK: i32 = Self::SECTION_SIZE - 1; // 15

    /// Creates a new `SectionPos` from section coordinates.
    #[must_use]
    pub const fn new(x: i32, y: i32, z: i32) -> Self {
        Self(Vector3::new(x, y, z))
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

    /// Extracts the section-relative X coordinate from a packed position.
    #[must_use]
    pub const fn section_relative_x(packed: i16) -> i32 {
        ((packed as i32) >> 8) & Self::SECTION_MASK
    }

    /// Extracts the section-relative Y coordinate from a packed position.
    #[must_use]
    pub const fn section_relative_y(packed: i16) -> i32 {
        (packed as i32) & Self::SECTION_MASK
    }

    /// Extracts the section-relative Z coordinate from a packed position.
    #[must_use]
    pub const fn section_relative_z(packed: i16) -> i32 {
        ((packed as i32) >> 4) & Self::SECTION_MASK
    }

    /// Converts section-relative coordinates to an absolute block X coordinate.
    #[must_use]
    pub const fn relative_to_block_x(&self, relative_x: i16) -> i32 {
        (self.0.x << Self::SECTION_BITS) + Self::section_relative_x(relative_x)
    }

    /// Converts section-relative coordinates to an absolute block Y coordinate.
    #[must_use]
    pub const fn relative_to_block_y(&self, relative_y: i16) -> i32 {
        (self.0.y << Self::SECTION_BITS) + Self::section_relative_y(relative_y)
    }

    /// Converts section-relative coordinates to an absolute block Z coordinate.
    #[must_use]
    pub const fn relative_to_block_z(&self, relative_z: i16) -> i32 {
        (self.0.z << Self::SECTION_BITS) + Self::section_relative_z(relative_z)
    }

    /// Packs the section position into an i64.
    /// Format: (x << 42) | (z << 20) | y
    #[must_use]
    pub fn as_i64(&self) -> i64 {
        let x = i64::from(self.0.x);
        let y = i64::from(self.0.y);
        let z = i64::from(self.0.z);

        ((x & 0x3F_FFFF) << 42) | ((z & 0x3F_FFFF) << 20) | (y & 0xF_FFFF)
    }

    /// Unpacks a section position from an i64.
    /// Format: (x << 42) | (z << 20) | y
    #[must_use]
    pub const fn from_i64(value: i64) -> Self {
        let x = value >> 42;
        let z = (value >> 20) & 0x3F_FFFF;
        let y = value & 0xF_FFFF;

        // Sign extend
        let x = (x << 42) >> 42;
        let y = (y << 44) >> 44;
        let z = (z << 42) >> 42;

        Self(Vector3::new(x as i32, y as i32, z as i32))
    }

    /// Packs a block position into a section-relative short.
    /// Format: (x << 8) | (z << 4) | y (each coordinate masked to 4 bits)
    #[must_use]
    #[inline]
    pub const fn section_relative_pos(pos: &BlockPos) -> i16 {
        let x = pos.0.x & Self::SECTION_MASK;
        let y = pos.0.y & Self::SECTION_MASK;
        let z = pos.0.z & Self::SECTION_MASK;
        ((x << 8) | (z << 4) | y) as i16
    }

    /// Converts a section-relative packed position back to a block position.
    #[must_use]
    pub const fn relative_to_block_pos(&self, relative: i16) -> BlockPos {
        BlockPos(Vector3::new(
            self.relative_to_block_x(relative),
            self.relative_to_block_y(relative),
            self.relative_to_block_z(relative),
        ))
    }
}

impl ReadFrom for SectionPos {
    fn read(data: &mut Cursor<&[u8]>) -> io::Result<Self> {
        let packed = <i64 as ReadFrom>::read(data)?;
        Ok(Self::from_i64(packed))
    }
}

impl WriteTo for SectionPos {
    fn write(&self, writer: &mut impl Write) -> io::Result<()> {
        self.as_i64().write(writer)
    }
}

/// The game type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[allow(missing_docs)]
pub enum GameType {
    Survival = 0,
    Creative = 1,
    Adventure = 2,
    Spectator = 3,
}

impl GameType {
    /// Returns the name of the game type.
    #[must_use]
    pub const fn name(&self) -> &'static str {
        match self {
            GameType::Survival => "survival",
            GameType::Creative => "creative",
            GameType::Adventure => "adventure",
            GameType::Spectator => "spectator",
        }
    }
}

#[allow(missing_docs)]
impl From<GameType> for i8 {
    fn from(value: GameType) -> Self {
        value as i8
    }
}

#[allow(missing_docs)]
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

#[allow(missing_docs)]
impl Display for Identifier {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}", self.namespace, self.path)
    }
}

#[allow(missing_docs)]
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
#[allow(missing_docs)]
impl Serialize for Identifier {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}

#[allow(missing_docs)]
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

    fn write(writer: &mut impl Writer, src: &Self::Src) -> wincode::WriteResult<()> {
        <str as SchemaWrite<C>>::write(writer, &src.to_string())
    }
}

// SAFETY: This implementation delegates to the `String` implementation which is
// already safe, and then validates the result as a valid Identifier. The read
// method initializes `dst` if and only if it returns Ok(()).
unsafe impl<'de, C: Config> SchemaRead<'de, C> for Identifier {
    type Dst = Identifier;

    fn read(
        reader: &mut impl Reader<'de>,
        dst: &mut MaybeUninit<Self::Dst>,
    ) -> wincode::ReadResult<()> {
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
            BlockPos(Vector3::new(0, -61, -2)),
            BlockPos(Vector3::new(0, 0, 0)),
            BlockPos(Vector3::new(100, 64, -100)),
            BlockPos(Vector3::new(-1000, -64, 1000)),
            BlockPos(Vector3::new(33_554_431, 2047, 33_554_431)), // Max positive values
            BlockPos(Vector3::new(-33_554_432, -2048, -33_554_432)), // Max negative values
        ];

        for pos in positions {
            let encoded = pos.as_i64();
            let decoded = BlockPos::from_i64(encoded);
            assert_eq!(
                pos, decoded,
                "Roundtrip failed for {pos:?}: encoded={encoded}, decoded={decoded:?}"
            );
        }
    }

    #[test]
    fn test_block_pos_specific_case() {
        // Test the specific case from the bug report
        let pos = BlockPos(Vector3::new(0, -61, -2));
        let encoded = pos.as_i64();
        let decoded = BlockPos::from_i64(encoded);
        assert_eq!(pos, decoded, "Position 0, -61, -2 failed roundtrip");
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
