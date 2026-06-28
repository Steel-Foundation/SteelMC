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

use glam::IVec3;
use steel_utils::{BoundingBox, Identifier, PackedChunkPos};
use wincode::{SchemaRead, SchemaWrite};

use crate::chunk::chunk_access::ChunkStatus;

/// Magic bytes for region file identification: "STLR" (Steel Region)
pub const REGION_MAGIC: [u8; 4] = *b"STLR";

/// Current format version. Increment when making breaking changes.
/// v3: Added entity persistence (`PersistentEntity`).
/// v4: Added scheduled tick persistence (`PersistentTick`).
/// v5: Added heightmap persistence (`PersistentHeightmap`).
/// v6: Added structure start and structure reference persistence.
/// v7: Added POI persistence (`PersistentPoi`).
/// v8: Added typed jigsaw piece-state persistence.
/// v9: Added proto chunk carving mask persistence and typed packed chunk references.
/// v10: Added template piece clip and postprocess persistence.
/// v11: Added template piece placement adjustment persistence.
/// v12: Added igloo template marker, placement adjustment, and postprocess persistence.
/// v13: Split template processor persistence and added ruined-portal processors.
/// v14: Added buried treasure procedural piece persistence.
/// v15: Added procedural structure-piece payload persistence.
/// v16: Added entity fall distance persistence.
/// v17: Added entity `NoGravity` persistence.
/// v18: Added entity `Invulnerable` persistence.
/// v19: Added shared entity save-data persistence.
/// v20: Added chunk-owned light section persistence.
pub const FORMAT_VERSION: u16 = 20;

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
    pub const fn to_bytes(self) -> [u8; 8] {
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
        for (i, &chunk) in bytes.as_chunks::<8>().0.iter().enumerate() {
            entries[i] = ChunkEntry::from_bytes(chunk);
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
    pub properties: Vec<(&'static str, &'static str)>,
}

/// A heightmap stored with a chunk.
///
/// Height values are stored relative to `min_y` (same as the runtime `Heightmap`).
/// Type discriminants: 0=WorldSurface, 1=MotionBlocking, 2=MotionBlockingNoLeaves, 3=OceanFloor.
#[derive(SchemaWrite, SchemaRead)]
pub struct PersistentHeightmap {
    /// Heightmap type discriminant.
    pub heightmap_type: u8,
    /// 256 height values (one per column), stored relative to `min_y`.
    pub data: Vec<u16>,
}

/// Chunk-owned light data stored with a chunk.
#[derive(SchemaWrite, SchemaRead, Default)]
pub struct PersistentLightData {
    /// Block-light sections indexed by light-section index.
    pub block: Vec<PersistentLightSection>,
    /// Sky-light sections indexed by light-section index.
    pub sky: Vec<PersistentLightSection>,
}

/// One persisted chunk-owned light section.
#[derive(SchemaWrite, SchemaRead)]
pub enum PersistentLightSection {
    /// Present zero-filled section without backing bytes.
    Uninitialized {
        /// Index in the chunk's padded light-section array.
        section_index: u32,
    },
    /// Visible initialized light bytes.
    Initialized {
        /// Index in the chunk's padded light-section array.
        section_index: u32,
        /// Packed 4-bit light values.
        data: Vec<u8>,
    },
    /// Initialized bytes hidden from vanilla packet conversion.
    Internal {
        /// Index in the chunk's padded light-section array.
        section_index: u32,
        /// Packed 4-bit light values.
        data: Vec<u8>,
    },
}

impl PersistentLightSection {
    /// Returns the padded light-section index.
    #[must_use]
    pub const fn section_index(&self) -> u32 {
        match self {
            Self::Uninitialized { section_index }
            | Self::Initialized { section_index, .. }
            | Self::Internal { section_index, .. } => *section_index,
        }
    }
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
    /// Final heightmaps for full chunks (empty for proto chunks).
    pub heightmaps: Vec<PersistentHeightmap>,
    /// Chunk-owned light sections.
    pub light: PersistentLightData,
    /// Proto chunk carving mask as Steel's packed bitset layout.
    pub carving_mask: Option<Vec<u64>>,
    /// Proto chunk postprocessing offsets grouped by section index.
    pub postprocessing: Vec<Vec<u16>>,
    /// Structure starts originating in this chunk.
    pub structure_starts: Vec<PersistentStructureStart>,
    /// References to structures from nearby origin chunks.
    pub structure_references: Vec<PersistentStructureReference>,
    /// POI occupancy data (ticket state for beds, workstations, etc.).
    pub pois: Vec<PersistentPoi>,
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
#[derive(Debug, Clone, SchemaWrite, SchemaRead)]
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
    /// Accumulated vanilla fall distance.
    pub fall_distance: f64,
    /// Vanilla `remainingFireTicks`.
    pub remaining_fire_ticks: i32,
    /// Synchronized vanilla `TicksFrozen`.
    pub ticks_frozen: i32,
    /// Vanilla `isInPowderSnow`.
    pub is_in_powder_snow: bool,
    /// Vanilla `wasInPowderSnow`.
    pub was_in_powder_snow: bool,
    /// Vanilla `hasVisualFire`.
    pub has_visual_fire: bool,
    /// Whether entity is on ground.
    pub on_ground: bool,
    /// Shared vanilla `NoGravity` flag.
    pub no_gravity: bool,
    /// Shared vanilla `Invulnerable` flag.
    pub invulnerable: bool,
    /// Synchronized vanilla `Air` value.
    pub air_supply: i32,
    /// Vanilla dimension-change portal cooldown.
    pub portal_cooldown: i32,
    /// Optional vanilla custom name stored as a root compound containing `CustomName`.
    pub custom_name_nbt: Vec<u8>,
    /// Synchronized vanilla custom-name visibility flag.
    pub custom_name_visible: bool,
    /// Synchronized vanilla silent flag.
    pub silent: bool,
    /// Server-owned vanilla glowing tag.
    pub glowing: bool,
    /// Vanilla scoreboard tags.
    pub tags: Vec<String>,
    /// Vanilla custom data compound.
    pub custom_data_nbt: Vec<u8>,
    /// Type-specific NBT data from `save_additional`.
    pub nbt_data: Vec<u8>,
    /// Direct passengers nested under this entity.
    pub passengers: Vec<PersistentEntity>,
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

/// A structure start stored with a chunk.
///
/// Only valid structure starts (those with at least one piece) are stored.
/// Vanilla's `INVALID_START` sentinel is represented by absence from the vec.
#[derive(SchemaWrite, SchemaRead)]
pub struct PersistentStructureStart {
    /// Structure type identifier (e.g., "minecraft:village").
    pub structure: Identifier,
    /// Origin chunk X coordinate.
    pub chunk_x: i32,
    /// Origin chunk Z coordinate.
    pub chunk_z: i32,
    /// Number of chunks referencing this structure start.
    pub references: i32,
    /// The pieces composing this structure.
    pub pieces: Vec<PersistentStructurePiece>,
}

/// A structure bounding box stored as six scalar coordinates.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, SchemaWrite, SchemaRead)]
pub struct PersistentBoundingBox {
    /// Minimum X coordinate.
    pub min_x: i32,
    /// Minimum Y coordinate.
    pub min_y: i32,
    /// Minimum Z coordinate.
    pub min_z: i32,
    /// Maximum X coordinate.
    pub max_x: i32,
    /// Maximum Y coordinate.
    pub max_y: i32,
    /// Maximum Z coordinate.
    pub max_z: i32,
}

impl PersistentBoundingBox {
    /// Converts a runtime bounding box to its persistent representation.
    #[must_use]
    pub const fn from_bounding_box(bounding_box: BoundingBox) -> Self {
        Self {
            min_x: bounding_box.min_x(),
            min_y: bounding_box.min_y(),
            min_z: bounding_box.min_z(),
            max_x: bounding_box.max_x(),
            max_y: bounding_box.max_y(),
            max_z: bounding_box.max_z(),
        }
    }

    /// Converts this persistent representation to a runtime bounding box.
    #[must_use]
    pub const fn to_bounding_box(self) -> BoundingBox {
        BoundingBox::new(
            IVec3::new(self.min_x, self.min_y, self.min_z),
            IVec3::new(self.max_x, self.max_y, self.max_z),
        )
    }
}

/// A structure piece stored with a chunk.
///
/// Common fields are stored directly; type-specific placement data is in
/// `payload`.
#[derive(SchemaWrite, SchemaRead)]
pub struct PersistentStructurePiece {
    /// Piece type identifier (e.g., "minecraft:jigsaw").
    pub piece_type: Identifier,
    /// Bounding box of this piece in world coordinates.
    pub bounding_box: PersistentBoundingBox,
    /// Generation depth in the piece tree.
    pub gen_depth: i32,
    /// 2D direction orientation (-1 = none, 0-3 = south/west/north/east).
    pub orientation: i8,
    /// Type-specific structure piece placement data.
    pub payload: PersistentStructurePiecePayload,
    /// Offset from piece minY to terrain ground level.
    pub ground_level_delta: i32,
    /// Projection mode: -1 = none, 0 = rigid, 1 = terrain matching.
    pub projection: i8,
    /// Jigsaw junctions used by terrain adaptation.
    pub junctions: Vec<PersistentJigsawJunction>,
}

/// Persisted type-specific structure piece placement data.
#[derive(SchemaWrite, SchemaRead)]
pub enum PersistentStructurePiecePayload {
    /// Jigsaw pool piece payload.
    Jigsaw(PersistentJigsawPieceData),
    /// Template-backed non-jigsaw payload.
    Template(PersistentTemplatePieceData),
    /// Procedural family payload.
    Procedural(PersistentProceduralPieceData),
}

/// Steel-native persistent state for a jigsaw pool piece.
#[derive(SchemaWrite, SchemaRead)]
pub struct PersistentJigsawPieceData {
    /// Selected pool element.
    pub pool_element: PersistentPoolElement,
    /// World-space template origin.
    pub position: [i32; 3],
    /// Rotation: 0=none, `1=clockwise_90`, `2=clockwise_180`, `3=counterclockwise_90`.
    pub rotation: i8,
    /// Liquid settings: `0=apply_waterlogging`, `1=ignore_waterlogging`.
    pub liquid_settings: i8,
}

/// Persisted template-backed non-jigsaw piece data.
#[derive(SchemaWrite, SchemaRead)]
pub struct PersistentTemplatePieceData {
    /// Structure template identifier.
    pub template_id: Identifier,
    /// World-space template origin.
    pub template_position: [i32; 3],
    /// Rotation: 0=none, `1=clockwise_90`, `2=clockwise_180`, `3=counterclockwise_90`.
    pub rotation: i8,
    /// Mirror: `0=none`, `1=front_back`, `2=left_right`.
    pub mirror: i8,
    /// Rotation pivot in template-local block coordinates.
    pub rotation_pivot: [i32; 3],
    /// Early block-ignore processor: `0=none`, `1=structure_block`, `2=structure_and_air`.
    pub block_ignore: i8,
    /// Late block-ignore processor: `0=none`, `1=structure_block`, `2=structure_and_air`.
    pub late_block_ignore: i8,
    /// Processors applied during block placement.
    pub processors: PersistentTemplateProcessorList,
    /// Liquid settings: `0=apply_waterlogging`, `1=ignore_waterlogging`.
    pub liquid_settings: i8,
    /// Marker handling:
    /// `0=ignore`, `1=data_markers`, `2=shipwreck`, `3=igloo`,
    /// `4=ocean_ruin_small`, `5=ocean_ruin_large`, `6=end_city`, `7=woodland_mansion`.
    pub marker_handling: i8,
    /// Family-specific position adjustment before template block placement.
    pub placement_adjustment: PersistentTemplatePlacementAdjustment,
    /// Placement clip: `0=center_chunk`, `1=center_chunk_expanded_to_template`,
    /// `2=center_chunk_contains_template_center_expanded_to_template`.
    pub placement_clip: i8,
    /// Postprocess: `0=none`, `1=nether_fossil`, `2=igloo_top`, `3=ruined_portal`.
    pub post_process: i8,
}

/// Persisted processors for template-backed non-jigsaw pieces.
#[derive(SchemaWrite, SchemaRead)]
pub enum PersistentTemplateProcessorList {
    /// Direct empty processor list.
    Empty,
    /// Registry-backed processor list.
    Registry(Identifier),
    /// Vanilla's hardcoded ocean-ruin processor sequence.
    OceanRuin {
        /// Ocean ruin biome temperature: 0=warm, 1=cold.
        biome_temp: i8,
        /// Block-rot integrity.
        integrity: f32,
    },
    /// Vanilla's hardcoded ruined-portal processor sequence.
    RuinedPortal {
        /// Ruined-portal vertical placement.
        vertical_placement: i8,
        /// Whether cold lava/aging behavior is active.
        cold: bool,
        /// Block age processor mossiness.
        mossiness: f32,
        /// Whether structure air is preserved.
        air_pocket: bool,
        /// Whether netherrack can grow leaves.
        overgrown: bool,
        /// Whether vines can be added.
        vines: bool,
        /// Whether blackstone replacement is active.
        replace_with_blackstone: bool,
    },
}

/// Persisted template position adjustment.
#[derive(SchemaWrite, SchemaRead)]
pub enum PersistentTemplatePlacementAdjustment {
    /// Place at the persisted template position.
    None,
    /// Shipwreck height adjustment state.
    Shipwreck {
        /// Whether this is the beached shipwreck variant.
        is_beached: bool,
        /// Vanilla `height_adjusted` flag.
        height_adjusted: bool,
    },
    /// Igloo per-placement height adjustment.
    Igloo {
        /// Vanilla template offset for this igloo piece.
        template_offset: [i32; 3],
    },
    /// Ocean ruin terrain height adjustment.
    OceanRuin,
}

/// Persisted procedural piece data.
#[derive(SchemaWrite, SchemaRead)]
pub enum PersistentProceduralPieceData {
    /// Procedural family whose placement state has not been captured yet.
    Unimplemented,
    /// Buried treasure chest placement.
    BuriedTreasure,
    /// Desert pyramid piece payload.
    DesertPyramid(PersistentDesertPyramidPieceData),
    /// Jungle temple piece payload.
    JungleTemple(PersistentJungleTemplePieceData),
    /// Mineshaft room/corridor/crossing/stairs payload.
    Mineshaft(PersistentMineshaftPieceData),
    /// Nether fortress bridge/castle payload.
    NetherFortress(PersistentNetherFortressPieceData),
    /// Ocean monument building payload.
    OceanMonument(PersistentOceanMonumentPieceData),
    /// Stronghold recursive piece payload.
    Stronghold(PersistentStrongholdPieceData),
    /// Swamp hut piece payload.
    SwampHut(PersistentSwampHutPieceData),
}

/// Persisted stronghold door variant.
#[derive(SchemaWrite, SchemaRead)]
pub enum PersistentStrongholdSmallDoorType {
    /// Three-block cave-air opening.
    Opening,
    /// Oak door framed by stone bricks.
    WoodDoor,
    /// Iron-bar grate opening.
    Grates,
    /// Iron door with stone buttons.
    IronDoor,
}

/// Persisted piece-specific stronghold data.
#[derive(SchemaWrite, SchemaRead)]
pub enum PersistentStrongholdPieceData {
    /// Straight corridor with optional side exits.
    Straight {
        /// Vanilla `entryDoor`.
        entry_door: PersistentStrongholdSmallDoorType,
        /// Vanilla `leftChild`.
        left_child: bool,
        /// Vanilla `rightChild`.
        right_child: bool,
    },
    /// Prison hall.
    PrisonHall {
        /// Vanilla `entryDoor`.
        entry_door: PersistentStrongholdSmallDoorType,
    },
    /// Left turn.
    LeftTurn {
        /// Vanilla `entryDoor`.
        entry_door: PersistentStrongholdSmallDoorType,
    },
    /// Right turn.
    RightTurn {
        /// Vanilla `entryDoor`.
        entry_door: PersistentStrongholdSmallDoorType,
    },
    /// Room crossing with one of five vanilla decorations.
    RoomCrossing {
        /// Vanilla `entryDoor`.
        entry_door: PersistentStrongholdSmallDoorType,
        /// Vanilla `type`.
        crossing_type: i32,
    },
    /// Straight stair corridor.
    StraightStairsDown {
        /// Vanilla `entryDoor`.
        entry_door: PersistentStrongholdSmallDoorType,
    },
    /// Descending stairs, including the source/start piece.
    StairsDown {
        /// Vanilla `entryDoor`.
        entry_door: PersistentStrongholdSmallDoorType,
        /// Vanilla `isSource`.
        is_source: bool,
    },
    /// Five-way crossing with low/high side exits.
    FiveCrossing {
        /// Vanilla `entryDoor`.
        entry_door: PersistentStrongholdSmallDoorType,
        /// Vanilla `leftLow`.
        left_low: bool,
        /// Vanilla `leftHigh`.
        left_high: bool,
        /// Vanilla `rightLow`.
        right_low: bool,
        /// Vanilla `rightHigh`.
        right_high: bool,
    },
    /// Corridor containing a loot chest.
    ChestCorridor {
        /// Vanilla `entryDoor`.
        entry_door: PersistentStrongholdSmallDoorType,
        /// Vanilla `hasPlacedChest`.
        has_placed_chest: bool,
    },
    /// Library room.
    Library {
        /// Vanilla `entryDoor`.
        entry_door: PersistentStrongholdSmallDoorType,
        /// Vanilla `isTall`.
        is_tall: bool,
    },
    /// End portal room.
    PortalRoom {
        /// Vanilla `hasPlacedSpawner`.
        has_placed_spawner: bool,
    },
    /// Collision filler corridor.
    FillerCorridor {
        /// Vanilla `steps`.
        steps: i32,
    },
}

/// Persisted piece-specific nether fortress data.
#[derive(SchemaWrite, SchemaRead)]
pub enum PersistentNetherFortressPieceData {
    /// Bridge crossing piece.
    BridgeCrossing,
    /// Dead-end bridge filler piece.
    BridgeEndFiller {
        /// Vanilla `BridgeEndFiller.selfSeed`.
        self_seed: i32,
    },
    /// Straight bridge segment.
    BridgeStraight,
    /// Castle corridor stair segment.
    CastleCorridorStairs,
    /// Castle corridor T balcony segment.
    CastleCorridorTBalcony,
    /// Castle entrance room.
    CastleEntrance,
    /// Small castle corridor crossing.
    CastleSmallCorridorCrossing,
    /// Small castle corridor left turn.
    CastleSmallCorridorLeftTurn {
        /// Vanilla `isNeedingChest`.
        is_needing_chest: bool,
    },
    /// Small straight castle corridor.
    CastleSmallCorridor,
    /// Small castle corridor right turn.
    CastleSmallCorridorRightTurn {
        /// Vanilla `isNeedingChest`.
        is_needing_chest: bool,
    },
    /// Nether-wart stair room.
    CastleStalkRoom,
    /// Blaze-spawner throne room.
    MonsterThrone {
        /// Vanilla `hasPlacedSpawner`.
        has_placed_spawner: bool,
    },
    /// Bridge room crossing.
    RoomCrossing,
    /// Bridge stair room.
    StairsRoom,
}

/// Persisted desert pyramid piece payload.
#[derive(SchemaWrite, SchemaRead)]
pub struct PersistentDesertPyramidPieceData {
    /// Vanilla `ScatteredFeaturePiece.heightPosition`; -1 means not height-adjusted yet.
    pub height_position: i32,
    /// Chest placement flags ordered by `Direction.get2DDataValue`.
    pub has_placed_chest: [bool; 4],
}

/// Persisted jungle temple piece payload.
#[derive(SchemaWrite, SchemaRead)]
pub struct PersistentJungleTemplePieceData {
    /// Vanilla `ScatteredFeaturePiece.heightPosition`; -1 means not height-adjusted yet.
    pub height_position: i32,
    /// Whether the main chest has already been placed.
    pub placed_main_chest: bool,
    /// Whether the hidden chest has already been placed.
    pub placed_hidden_chest: bool,
    /// Whether the first arrow-dispenser trap has already been placed.
    pub placed_trap1: bool,
    /// Whether the second arrow-dispenser trap has already been placed.
    pub placed_trap2: bool,
}

/// Persisted mineshaft piece payload.
#[derive(SchemaWrite, SchemaRead)]
pub struct PersistentMineshaftPieceData {
    /// Mineshaft type: 0=normal, 1=mesa.
    pub mineshaft_type: i8,
    /// Piece-specific mineshaft data.
    pub kind: PersistentMineshaftPieceKind,
}

/// Persisted piece-specific mineshaft data.
#[derive(SchemaWrite, SchemaRead)]
pub enum PersistentMineshaftPieceKind {
    /// Start room.
    Room {
        /// Child entrance boxes.
        child_entrance_boxes: Vec<PersistentBoundingBox>,
    },
    /// Horizontal corridor.
    Corridor {
        /// Whether rails can generate through this corridor.
        has_rails: bool,
        /// Whether this is a cobweb-heavy cave-spider corridor.
        spider_corridor: bool,
        /// Whether the cave-spider spawner has already been placed.
        has_placed_spider: bool,
        /// Number of five-block corridor sections.
        num_sections: i32,
    },
    /// Corridor crossing.
    Crossing {
        /// Direction: 0=south, 1=west, 2=north, 3=east.
        direction: i8,
        /// Whether the crossing has the upper floor.
        is_two_floored: bool,
    },
    /// Stair segment.
    Stairs,
}

/// Persisted swamp hut piece payload.
#[derive(SchemaWrite, SchemaRead)]
pub struct PersistentSwampHutPieceData {
    /// Vanilla `ScatteredFeaturePiece.heightPosition`; -1 means not height-adjusted yet.
    pub height_position: i32,
    /// Whether the structure witch has already been spawned.
    pub spawned_witch: bool,
    /// Whether the structure black cat has already been spawned.
    pub spawned_cat: bool,
}

/// Persisted ocean monument building payload.
#[derive(SchemaWrite, SchemaRead)]
pub struct PersistentOceanMonumentPieceData {
    /// Internal child pieces generated by vanilla `MonumentBuilding`.
    pub child_pieces: Vec<PersistentOceanMonumentChildPiece>,
}

/// Persisted internal ocean monument child piece.
#[derive(SchemaWrite, SchemaRead)]
pub struct PersistentOceanMonumentChildPiece {
    /// World-space child bounding box after building-relative offset.
    pub bounding_box: PersistentBoundingBox,
    /// Child piece variant and variant-specific placement state.
    pub kind: PersistentOceanMonumentChildPieceKind,
}

/// Persisted ocean monument child piece variant.
#[derive(SchemaWrite, SchemaRead)]
pub enum PersistentOceanMonumentChildPieceKind {
    /// `OceanMonumentEntryRoom`.
    EntryRoom {
        /// Source room snapshot.
        room: PersistentOceanMonumentRoomData,
    },
    /// `OceanMonumentCoreRoom`.
    CoreRoom,
    /// `OceanMonumentDoubleXRoom`.
    DoubleXRoom {
        /// Western room snapshot.
        west: PersistentOceanMonumentRoomData,
        /// Eastern room snapshot.
        east: PersistentOceanMonumentRoomData,
    },
    /// `OceanMonumentDoubleXYRoom`.
    DoubleXYRoom {
        /// Lower western room.
        west: PersistentOceanMonumentRoomData,
        /// Lower eastern room.
        east: PersistentOceanMonumentRoomData,
        /// Upper western room.
        west_up: PersistentOceanMonumentRoomData,
        /// Upper eastern room.
        east_up: PersistentOceanMonumentRoomData,
    },
    /// `OceanMonumentDoubleYRoom`.
    DoubleYRoom {
        /// Lower room.
        room: PersistentOceanMonumentRoomData,
        /// Upper room.
        above: PersistentOceanMonumentRoomData,
    },
    /// `OceanMonumentDoubleYZRoom`.
    DoubleYZRoom {
        /// Southern lower room.
        south: PersistentOceanMonumentRoomData,
        /// Northern lower room.
        north: PersistentOceanMonumentRoomData,
        /// Southern upper room.
        south_up: PersistentOceanMonumentRoomData,
        /// Northern upper room.
        north_up: PersistentOceanMonumentRoomData,
    },
    /// `OceanMonumentDoubleZRoom`.
    DoubleZRoom {
        /// Southern room.
        south: PersistentOceanMonumentRoomData,
        /// Northern room.
        north: PersistentOceanMonumentRoomData,
    },
    /// `OceanMonumentSimpleRoom`.
    SimpleRoom {
        /// Room snapshot.
        room: PersistentOceanMonumentRoomData,
        /// Vanilla `mainDesign`.
        main_design: i32,
    },
    /// `OceanMonumentSimpleTopRoom`.
    SimpleTopRoom {
        /// Room snapshot.
        room: PersistentOceanMonumentRoomData,
    },
    /// `OceanMonumentWingRoom`.
    WingRoom {
        /// Vanilla `mainDesign`.
        main_design: i32,
    },
    /// `OceanMonumentPenthouse`.
    Penthouse,
}

/// Persisted ocean monument room snapshot.
#[derive(SchemaWrite, SchemaRead)]
pub struct PersistentOceanMonumentRoomData {
    /// Vanilla room index.
    pub index: i32,
    /// Vanilla `hasOpening`, ordered by `Direction.get3DDataValue`.
    pub has_opening: [bool; 6],
    /// Whether `connections[UP] != null`.
    pub has_up_connection: bool,
}

/// Persisted pool element selected during jigsaw assembly.
#[derive(SchemaWrite, SchemaRead)]
pub enum PersistentPoolElement {
    /// Single structure template piece.
    Single {
        /// Template location.
        location: Identifier,
        /// Processors applied during block placement.
        processors: PersistentProcessorList,
        /// Projection mode: 0 = rigid, 1 = terrain matching.
        projection: i8,
    },
    /// Legacy single piece.
    LegacySingle {
        /// Template location.
        location: Identifier,
        /// Processors applied during block placement.
        processors: PersistentProcessorList,
        /// Projection mode: 0 = rigid, 1 = terrain matching.
        projection: i8,
    },
    /// Empty placeholder element.
    Empty,
    /// Placed feature element.
    Feature {
        /// Feature identifier.
        feature: Identifier,
        /// Projection mode: 0 = rigid, 1 = terrain matching.
        projection: i8,
    },
    /// Group of sub-elements.
    List {
        /// Sub-elements.
        elements: Vec<PersistentPoolElement>,
        /// Projection mode: 0 = rigid, 1 = terrain matching.
        projection: i8,
    },
}

/// Persisted processor list holder for single pool elements.
#[derive(SchemaWrite, SchemaRead)]
pub enum PersistentProcessorList {
    /// Direct empty processor list.
    Empty,
    /// Registry-backed processor list.
    Registry(Identifier),
}

/// A persisted jigsaw junction used by Beardifier terrain adaptation.
#[derive(SchemaWrite, SchemaRead)]
pub struct PersistentJigsawJunction {
    /// World X.
    pub source_x: i32,
    /// Ground-adjusted Y.
    pub source_ground_y: i32,
    /// World Z.
    pub source_z: i32,
    /// Y delta between source and target.
    pub delta_y: i32,
    /// Destination projection: 0 = rigid, 1 = terrain matching.
    pub dest_projection: i8,
}

/// A structure reference entry stored with a chunk.
///
/// References point to structure starts in nearby origin chunks.
#[derive(SchemaWrite, SchemaRead)]
pub struct PersistentStructureReference {
    /// Structure type identifier.
    pub structure: Identifier,
    /// Packed chunk positions of origin chunks.
    pub references: Vec<PackedChunkPos>,
}

/// A point of interest's occupancy state stored with a chunk.
///
/// Only the position and remaining free tickets are persisted — the POI type
/// is derived from the block state on load via `scan_and_populate`.
#[derive(SchemaWrite, SchemaRead)]
pub struct PersistentPoi {
    /// Relative X position within chunk (0-15).
    pub x: u8,
    /// Absolute Y position (world height).
    pub y: i16,
    /// Relative Z position within chunk (0-15).
    pub z: u8,
    /// Number of tickets still available for claiming.
    pub free_tickets: u32,
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
    pub fn filename(self) -> String {
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
