//! Structure start and reference types for chunk-level structure tracking.
//!
//! In vanilla, chunks store two maps:
//! - `structureStarts`: structures originating in this chunk
//! - `structuresReferences`: references to structures from nearby chunks
//!
//! Structure generation is not yet implemented. These types support
//! persistence, the proto-chunk → level-chunk data flow, and future generation.
//! The structure key is `Identifier` until a structure registry is added.

use rustc_hash::FxHashMap;

use steel_utils::{BoundingBox, ChunkPos, Direction, Identifier};

/// A structure start placed in a chunk.
///
/// Corresponds to vanilla's `StructureStart`. A start is "valid" if it has
/// at least one piece; invalid starts are not stored (they correspond to
/// vanilla's `INVALID_START` sentinel).
#[derive(Debug, Clone)]
pub struct StructureStart {
    /// The structure type identifier (e.g., `minecraft:village`).
    pub structure: Identifier,
    /// The chunk where this structure originates.
    pub chunk_pos: ChunkPos,
    /// How many neighboring chunks reference this start.
    pub references: i32,
    /// The pieces composing this structure.
    pub pieces: Vec<StructurePiece>,
}

/// A single piece of a structure.
///
/// Corresponds to vanilla's `StructurePiece`. Type-specific data is stored
/// as an NBT blob since there are 56+ piece types in vanilla.
#[derive(Debug, Clone)]
pub struct StructurePiece {
    /// Piece type identifier (e.g., `minecraft:jigsaw`).
    pub piece_type: Identifier,
    /// World-space bounding box of this piece.
    pub bounding_box: BoundingBox,
    /// Generation depth (distance from start piece in the piece tree).
    pub gen_depth: i32,
    /// Horizontal orientation of this piece (`None` for unoriented pieces).
    /// Only horizontal directions (North/South/East/West) are used.
    pub orientation: Option<Direction>,
    /// Type-specific NBT data (simdnbt binary format).
    pub nbt_data: Vec<u8>,
}

/// Map of structure starts keyed by structure identifier.
pub type StructureStartMap = FxHashMap<Identifier, StructureStart>;

/// Map of structure references keyed by structure identifier.
/// Values are the chunk positions of origin chunks that contain the structure start.
pub type StructureReferenceMap = FxHashMap<Identifier, Vec<ChunkPos>>;
