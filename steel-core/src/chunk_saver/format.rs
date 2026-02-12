//! Data structures for the chunk persistence format.
//!
//! ## Format Overview
//!
//! Region files use a sector-based format with a fixed header for fast random access:
//!
//! ```text
//! ┌─────────────────────────────────────────────────────┐
//! │ Magic (4 bytes): "STLR"                             │
//! │ Version (2 bytes): u16                              │
//! │ Padding (2 bytes): reserved                         │
//! ├─────────────────────────────────────────────────────┤
//! │ Header: 1024 entries × 8 bytes = 8KB                │
//! │   Each entry: offset (u32) + size (u24) + flags (u8)│
//! ├─────────────────────────────────────────────────────┤
//! │ Chunk data in 4KB sectors                           │
//! │   [chunk data padded to 4KB boundary]               │
//! │   [chunk data padded to 4KB boundary]               │
//! │   ...                                               │
//! └─────────────────────────────────────────────────────┘
//! ```
//!
//! ## Design
//!
//! Each chunk stores its own block state and biome palettes, making chunks
//! self-contained and avoiding expensive region-wide table rebuilds.
//!
//! Block data uses power-of-2 bit packing (1, 2, 4, 8, 16 bits) to avoid entries
//! spanning u64 boundaries.

use steel_utils::Identifier;
use wincode::{SchemaRead, SchemaWrite};

use crate::chunk::chunk_access::ChunkStatus;

/// Magic bytes for region file identification: "STLR" (Steel Region)
pub const REGION_MAGIC: [u8; 4] = *b"STLR";

/// Current format version. Increment when making breaking changes.
/// v3: Added entity persistence (`PersistentEntity`).
/// v4: Added scheduled tick persistence (`PersistentTick`).
pub const FORMAT_VERSION: u16 = 4;

/// Number of chunks per region side (32×32 = 1024 chunks per region).
pub const REGION_SIZE: usize = 32;

/// Total chunks in a region.
pub const CHUNKS_PER_REGION: usize = REGION_SIZE * REGION_SIZE;

/// Number of blocks per section side (16×16×16 = 4096 blocks per section).
pub const SECTION_SIZE: usize = 16;

/// Total blocks in a section.
pub const BLOCKS_PER_SECTION: usize = SECTION_SIZE * SECTION_SIZE * SECTION_SIZE;

/// Number of biome cells per section side (4×4×4 = 64 biomes per section).
pub const BIOME_SIZE: usize = 4;

/// Total biome cells in a section.
pub const BIOMES_PER_SECTION: usize = BIOME_SIZE * BIOME_SIZE * BIOME_SIZE;

/// Sector size in bytes (4KB, matches modern disk physical sectors).
pub const SECTOR_SIZE: usize = 4096;

/// Size of the file header (magic + version + padding).
pub const FILE_HEADER_SIZE: usize = 8;

/// Size of the chunk location table (1024 entries × 8 bytes).
pub const CHUNK_TABLE_SIZE: usize = CHUNKS_PER_REGION * 8;

/// Total header size (file header + chunk table).
pub const TOTAL_HEADER_SIZE: usize = FILE_HEADER_SIZE + CHUNK_TABLE_SIZE;

/// First sector where chunk data can be stored.
/// Header takes `ceil(TOTAL_HEADER_SIZE` / `SECTOR_SIZE`) = 3 sectors (8 + 8192 = 8200 bytes).
pub const FIRST_DATA_SECTOR: u32 = 3;

/// Maximum chunk size in bytes (16MB - should be plenty).
pub const MAX_CHUNK_SIZE: usize = 16 * 1024 * 1024;

/// Entry in the chunk location table.
///
/// Layout (8 bytes total):
/// - offset: u32 - sector offset (0 = chunk doesn't exist)
/// - size: u24 - compressed size in bytes
/// - flags: u8 - status and flags
#[derive(Clone, Copy)]
pub struct ChunkEntry {
    /// Sector offset from start of file. 0 means chunk doesn't exist.
    /// Multiply by `SECTOR_SIZE` to get byte offset.
    pub sector_offset: u32,
    /// Size of compressed chunk data in bytes (stored as u24, max ~16MB).
    pub size_bytes: u32,
    /// Chunk status (generation state).
    pub status: ChunkStatus,
}

impl ChunkEntry {
    /// Creates a new chunk entry.
    #[must_use]
    pub const fn new(sector_offset: u32, size_bytes: u32, status: ChunkStatus) -> Self {
        Self {
            sector_offset,
            size_bytes,
            status,
        }
    }

    /// Returns true if this entry represents an existing chunk.
    #[must_use]
    pub const fn exists(&self) -> bool {
        self.sector_offset != 0
    }

    /// Creates an empty/non-existent chunk entry.
    #[must_use]
    pub const fn empty() -> Self {
        Self {
            sector_offset: 0,
            size_bytes: 0,
            status: ChunkStatus::Empty,
        }
    }

    /// Calculates the number of sectors this chunk occupies.
    #[must_use]
    pub const fn sector_count(&self) -> u32 {
        if self.size_bytes == 0 {
            0
        } else {
            (self.size_bytes as usize).div_ceil(SECTOR_SIZE) as u32
        }
    }

    /// Serializes to 8 bytes: [offset: 4][size: 3][flags: 1]
    #[must_use]
    pub const fn to_bytes(&self) -> [u8; 8] {
        let offset_bytes = self.sector_offset.to_le_bytes();
        let size_bytes = self.size_bytes.to_le_bytes();
        let flags = self.status.get_index() as u8;
        [
            offset_bytes[0],
            offset_bytes[1],
            offset_bytes[2],
            offset_bytes[3],
            size_bytes[0],
            size_bytes[1],
            size_bytes[2],
            flags,
        ]
    }

    /// Deserializes from 8 bytes.
    #[must_use]
    pub fn from_bytes(bytes: [u8; 8]) -> Self {
        let sector_offset = u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
        let size_bytes = u32::from_le_bytes([bytes[4], bytes[5], bytes[6], 0]);
        let status = ChunkStatus::from_index(bytes[7] as usize).unwrap_or(ChunkStatus::Empty);
        Self {
            sector_offset,
            size_bytes,
            status,
        }
    }
}

impl Default for ChunkEntry {
    fn default() -> Self {
        Self::empty()
    }
}

/// Region header containing chunk location table.
pub struct RegionHeader {
    /// Chunk entries (1024 = 32×32).
    pub entries: Box<[ChunkEntry; CHUNKS_PER_REGION]>,
}

impl RegionHeader {
    /// Creates an empty header with no chunks.
    #[must_use]
    pub fn new() -> Self {
        Self {
            entries: Box::new([ChunkEntry::default(); CHUNKS_PER_REGION]),
        }
    }

    /// Gets the local index for a chunk position within this region.
    #[must_use]
    pub const fn chunk_index(local_x: usize, local_z: usize) -> usize {
        debug_assert!(local_x < REGION_SIZE);
        debug_assert!(local_z < REGION_SIZE);
        local_z * REGION_SIZE + local_x
    }

    /// Converts a chunk index back to local coordinates.
    #[must_use]
    pub const fn index_to_local(index: usize) -> (usize, usize) {
        debug_assert!(index < CHUNKS_PER_REGION);
        (index % REGION_SIZE, index / REGION_SIZE)
    }

    /// Serializes the header to bytes.
    #[must_use]
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(CHUNK_TABLE_SIZE);
        for entry in self.entries.iter() {
            bytes.extend_from_slice(&entry.to_bytes());
        }
        bytes
    }

    /// Deserializes the header from bytes.
    ///
    /// # Panics
    /// Panics if bytes length is not exactly `CHUNK_TABLE_SIZE`.
    #[must_use]
    pub fn from_bytes(bytes: &[u8]) -> Self {
        assert_eq!(bytes.len(), CHUNK_TABLE_SIZE);
        let mut entries = Box::new([ChunkEntry::default(); CHUNKS_PER_REGION]);
        for (i, chunk) in bytes.chunks_exact(8).enumerate() {
            entries[i] = ChunkEntry::from_bytes(chunk.try_into().expect("chunk entry is 8 bytes"));
        }
        Self { entries }
    }

    /// Finds a contiguous range of free sectors for allocation.
    ///
    /// Returns the starting sector offset, or `None` if no suitable range exists.
    #[must_use]
    pub fn find_free_sectors(&self, sectors_needed: u32, file_sectors: u32) -> u32 {
        if sectors_needed == 0 {
            return FIRST_DATA_SECTOR;
        }

        // Build a list of (start, end) ranges for used sectors
        let mut used_ranges: Vec<(u32, u32)> = self
            .entries
            .iter()
            .filter(|e| e.exists())
            .map(|e| (e.sector_offset, e.sector_offset + e.sector_count()))
            .collect();
        used_ranges.sort_by_key(|r| r.0);

        // Try to find a gap between used ranges
        let mut current_sector = FIRST_DATA_SECTOR;
        for (start, end) in used_ranges {
            if start >= current_sector + sectors_needed {
                // Found a gap
                return current_sector;
            }
            current_sector = current_sector.max(end);
        }

        // No gap found, append at the end
        current_sector.max(file_sectors)
    }
}

impl Default for RegionHeader {
    fn default() -> Self {
        Self::new()
    }
}

/// A block state with its identifier and properties.
#[derive(SchemaWrite, SchemaRead, Clone, PartialEq, Eq, Hash, Debug)]
pub struct PersistentBlockState {
    /// Block identifier (e.g., "`minecraft:oak_stairs`").
    pub name: Identifier,
    /// Block properties as key-value pairs (e.g., [("facing", "north")]).
    pub properties: Vec<(String, String)>,
}

/// A persistent chunk containing sections and metadata.
///
/// Each chunk stores its own block state and biome palettes, making it
/// self-contained. Sections reference indices into these chunk-level palettes.
#[derive(SchemaWrite, SchemaRead)]
pub struct PersistentChunk {
    /// Unix timestamp of last modification.
    pub last_modified: u32,
    /// Block states used in this chunk. Sections reference indices into this.
    pub block_states: Vec<PersistentBlockState>,
    /// Biomes used in this chunk. Sections reference indices into this.
    pub biomes: Vec<Identifier>,
    /// Vertical sections (typically 24 for -64 to 319).
    pub sections: Vec<PersistentSection>,
    /// Block entities (chests, signs, etc.).
    pub block_entities: Vec<PersistentBlockEntity>,
    /// Entities in this chunk (excludes players and non-serializable types).
    pub entities: Vec<PersistentEntity>,
    /// Scheduled block ticks pending in this chunk.
    pub block_ticks: Vec<PersistentTick>,
    /// Scheduled fluid ticks pending in this chunk.
    pub fluid_ticks: Vec<PersistentTick>,
}

/// A 16×16×16 section of a chunk.
#[derive(SchemaWrite, SchemaRead)]
pub enum PersistentSection {
    /// All blocks are the same type.
    Homogeneous {
        /// Index into chunk's `block_states` palette.
        block_state: u16,
        /// Biome data for this section.
        biomes: PersistentBiomeData,
    },
    /// Multiple block types present.
    Heterogeneous {
        /// Section-local palette: indices into chunk's `block_states` palette.
        palette: Vec<u16>,
        /// Bits per entry (1, 2, 4, 8, or 16).
        bits_per_entry: u8,
        /// Packed block indices into section-local palette. 4096 entries.
        block_data: Box<[u64]>,
        /// Biome data for this section.
        biomes: PersistentBiomeData,
    },
}

/// Biome data for a section (4×4×4 = 64 cells).
#[derive(SchemaWrite, SchemaRead)]
pub enum PersistentBiomeData {
    /// All 64 biome cells are the same.
    Homogeneous {
        /// Index into chunk's `biomes` palette.
        biome: u16,
    },
    /// Multiple biomes present.
    Heterogeneous {
        /// Section-local palette: indices into chunk's `biomes` palette.
        palette: Vec<u16>,
        /// Bits per entry (1, 2, 4, or 8).
        bits_per_entry: u8,
        /// Packed biome indices into section-local palette. 64 entries.
        biome_data: Box<[u64]>,
    },
}

/// A block entity (tile entity) stored with a chunk.
///
/// Block entities are serialized with their type and NBT data.
/// The NBT data is stored as raw bytes (simdnbt binary format).
#[derive(SchemaWrite, SchemaRead)]
pub struct PersistentBlockEntity {
    /// Relative X position within chunk (0-15).
    pub x: u8,
    /// Absolute Y position (world height).
    pub y: i16,
    /// Relative Z position within chunk (0-15).
    pub z: u8,
    /// Block entity type identifier (e.g., "minecraft:chest").
    pub entity_type: Identifier,
    /// Serialized NBT data (simdnbt binary format).
    /// Contains the block entity's custom data from `save_additional`.
    pub nbt_data: Vec<u8>,
}

/// An entity stored with a chunk.
///
/// Unlike vanilla which stores entities in separate region files,
/// Steel stores entities inline with chunk data for simplicity.
/// Base entity fields are stored directly; type-specific data is in `nbt_data`.
#[derive(SchemaWrite, SchemaRead)]
pub struct PersistentEntity {
    /// Entity type identifier (e.g., "minecraft:item").
    pub entity_type: Identifier,
    /// Persistent UUID (16 bytes).
    pub uuid: [u8; 16],
    /// Position (x, y, z) in absolute world coordinates.
    pub pos: [f64; 3],
    /// Velocity (x, y, z) in blocks per tick.
    pub motion: [f64; 3],
    /// Rotation (yaw, pitch) in degrees.
    pub rotation: [f32; 2],
    /// Whether entity is on ground.
    pub on_ground: bool,
    /// Type-specific NBT data from `save_additional`.
    pub nbt_data: Vec<u8>,
}

/// A scheduled tick stored with a chunk.
///
/// Stores the tick's position relative to the chunk, its remaining delay,
/// priority, ordering, and the block/fluid identifier.
#[derive(SchemaWrite, SchemaRead)]
pub struct PersistentTick {
    /// Relative X position within chunk (0-15).
    pub x: u8,
    /// Absolute Y position (world height).
    pub y: i16,
    /// Relative Z position within chunk (0-15).
    pub z: u8,
    /// Remaining delay in game ticks until this tick fires.
    pub delay: i32,
    /// Tick priority as `i8` (maps to `TickPriority` enum, -3 to 3).
    pub priority: i8,
    /// Sub-tick ordering value for stable sort within same priority.
    pub sub_tick_order: i64,
    /// Block or fluid identifier (e.g., "`minecraft:stone_button`").
    pub tick_type: Identifier,
}

/// Position of a region in region coordinates.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct RegionPos {
    /// Region X coordinate (`chunk_x` / 32).
    pub x: i32,
    /// Region Z coordinate (`chunk_z` / 32).
    pub z: i32,
}

impl RegionPos {
    /// Creates a new region position.
    #[must_use]
    pub const fn new(x: i32, z: i32) -> Self {
        Self { x, z }
    }

    /// Converts a chunk position to a region position.
    #[must_use]
    pub const fn from_chunk(chunk_x: i32, chunk_z: i32) -> Self {
        Self {
            x: chunk_x.div_euclid(REGION_SIZE as i32),
            z: chunk_z.div_euclid(REGION_SIZE as i32),
        }
    }

    /// Gets the local chunk coordinates within this region for a global chunk position.
    #[must_use]
    pub const fn local_chunk_pos(chunk_x: i32, chunk_z: i32) -> (usize, usize) {
        (
            chunk_x.rem_euclid(REGION_SIZE as i32) as usize,
            chunk_z.rem_euclid(REGION_SIZE as i32) as usize,
        )
    }

    /// Returns the filename for this region (e.g., "r.0.-1.srg").
    #[must_use]
    pub fn filename(&self) -> String {
        format!("r.{}.{}.srg", self.x, self.z)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_region_pos_from_chunk() {
        // Positive chunks
        assert_eq!(RegionPos::from_chunk(0, 0), RegionPos::new(0, 0));
        assert_eq!(RegionPos::from_chunk(31, 31), RegionPos::new(0, 0));
        assert_eq!(RegionPos::from_chunk(32, 32), RegionPos::new(1, 1));

        // Negative chunks
        assert_eq!(RegionPos::from_chunk(-1, -1), RegionPos::new(-1, -1));
        assert_eq!(RegionPos::from_chunk(-32, -32), RegionPos::new(-1, -1));
        assert_eq!(RegionPos::from_chunk(-33, -33), RegionPos::new(-2, -2));
    }

    #[test]
    fn test_local_chunk_pos() {
        assert_eq!(RegionPos::local_chunk_pos(0, 0), (0, 0));
        assert_eq!(RegionPos::local_chunk_pos(31, 31), (31, 31));
        assert_eq!(RegionPos::local_chunk_pos(32, 32), (0, 0));
        assert_eq!(RegionPos::local_chunk_pos(-1, -1), (31, 31));
        assert_eq!(RegionPos::local_chunk_pos(-32, -32), (0, 0));
    }

    #[test]
    fn test_chunk_index() {
        assert_eq!(RegionHeader::chunk_index(0, 0), 0);
        assert_eq!(RegionHeader::chunk_index(31, 0), 31);
        assert_eq!(RegionHeader::chunk_index(0, 1), 32);
        assert_eq!(RegionHeader::chunk_index(31, 31), 1023);
    }

    #[test]
    fn test_chunk_entry_roundtrip() {
        let entry = ChunkEntry::new(42, 12345, ChunkStatus::Full);
        let bytes = entry.to_bytes();
        let decoded = ChunkEntry::from_bytes(bytes);
        assert_eq!(entry.sector_offset, decoded.sector_offset);
        assert_eq!(entry.size_bytes, decoded.size_bytes);
        assert_eq!(entry.status, decoded.status);
    }

    #[test]
    fn test_chunk_entry_empty() {
        let entry = ChunkEntry::default();
        assert!(!entry.exists());
        assert_eq!(entry.sector_count(), 0);
    }

    #[test]
    fn test_sector_count() {
        // Empty
        assert_eq!(ChunkEntry::new(1, 0, ChunkStatus::Full).sector_count(), 0);
        // Exactly one sector
        assert_eq!(
            ChunkEntry::new(1, 4096, ChunkStatus::Full).sector_count(),
            1
        );
        // Just over one sector
        assert_eq!(
            ChunkEntry::new(1, 4097, ChunkStatus::Full).sector_count(),
            2
        );
        // Multiple sectors
        assert_eq!(
            ChunkEntry::new(1, 12000, ChunkStatus::Full).sector_count(),
            3
        );
    }

    #[test]
    fn test_find_free_sectors_empty() {
        let header = RegionHeader::new();
        // Should return first data sector
        assert_eq!(header.find_free_sectors(1, 3), FIRST_DATA_SECTOR);
    }

    #[test]
    fn test_find_free_sectors_gap() {
        let mut header = RegionHeader::new();
        // Chunk at sector 3-4 (2 sectors)
        header.entries[0] = ChunkEntry::new(3, 8000, ChunkStatus::Full);
        // Chunk at sector 10-11 (2 sectors)
        header.entries[1] = ChunkEntry::new(10, 8000, ChunkStatus::Full);

        // Should find gap at sector 5-9
        assert_eq!(header.find_free_sectors(3, 12), 5);
        // Needs more than gap, append at end
        assert_eq!(header.find_free_sectors(6, 12), 12);
    }
}
