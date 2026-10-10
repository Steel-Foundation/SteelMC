//! Chunk generation status.

use wincode::{SchemaRead, SchemaWrite};

use super::heightmap::HeightmapType;

/// The status of a chunk.
#[derive(Debug, PartialEq, Eq, PartialOrd, Ord, Clone, Copy, SchemaWrite, SchemaRead)]
pub enum ChunkStatus {
    /// The chunk is empty.
    Empty,
    /// The chunk is being processed for structure starts.
    StructureStarts,
    /// The chunk is being processed for structure references.
    StructureReferences,
    /// The chunk is being processed for biomes.
    Biomes,
    /// The chunk terrain is being generated.
    Terrain,
    /// The chunk is being processed for features.
    Features,
    /// The chunk is being initialized for light.
    InitializeLight,
    /// The chunk is being lit.
    Light,
    /// The chunk is being spawned.
    Spawn,
    /// The chunk is fully generated.
    Full,
}

impl ChunkStatus {
    /// Gets the index of the status.
    #[must_use]
    pub const fn get_index(self) -> usize {
        self as usize
    }

    /// Gets the status from an index.
    #[must_use]
    pub const fn from_index(index: usize) -> Option<Self> {
        match index {
            0 => Some(Self::Empty),
            1 => Some(Self::StructureStarts),
            2 => Some(Self::StructureReferences),
            3 => Some(Self::Biomes),
            4 => Some(Self::Terrain),
            5 => Some(Self::Features),
            6 => Some(Self::InitializeLight),
            7 => Some(Self::Light),
            8 => Some(Self::Spawn),
            9 => Some(Self::Full),
            _ => None,
        }
    }

    /// Gets the next status in the generation order.
    #[must_use]
    pub const fn next(self) -> Option<Self> {
        match self {
            Self::Empty => Some(Self::StructureStarts),
            Self::StructureStarts => Some(Self::StructureReferences),
            Self::StructureReferences => Some(Self::Biomes),
            Self::Biomes => Some(Self::Terrain),
            Self::Terrain => Some(Self::Features),
            Self::Features => Some(Self::InitializeLight),
            Self::InitializeLight => Some(Self::Light),
            Self::Light => Some(Self::Spawn),
            Self::Spawn => Some(Self::Full),
            Self::Full => None,
        }
    }

    /// Gets the parent status in the generation order.
    #[must_use]
    pub const fn parent(self) -> Option<Self> {
        match self {
            Self::Empty => None,
            Self::StructureStarts => Some(Self::Empty),
            Self::StructureReferences => Some(Self::StructureStarts),
            Self::Biomes => Some(Self::StructureReferences),
            Self::Terrain => Some(Self::Biomes),
            Self::Features => Some(Self::Terrain),
            Self::InitializeLight => Some(Self::Features),
            Self::Light => Some(Self::InitializeLight),
            Self::Spawn => Some(Self::Light),
            Self::Full => Some(Self::Spawn),
        }
    }

    /// Returns the heightmap types that should be updated at this status.
    ///
    /// Before Terrain, worldgen heightmaps are used.
    /// At Terrain and after, final heightmaps are used.
    #[must_use]
    pub const fn heightmaps_after(self) -> &'static [HeightmapType] {
        match self {
            Self::Empty | Self::StructureStarts | Self::StructureReferences | Self::Biomes => {
                HeightmapType::worldgen_types()
            }
            Self::Terrain
            | Self::Features
            | Self::InitializeLight
            | Self::Light
            | Self::Spawn
            | Self::Full => HeightmapType::final_types(),
        }
    }
}
