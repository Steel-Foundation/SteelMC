//! This module contains the `ChunkAccess` enum, which is used to access chunks in different states.
use std::sync::atomic::Ordering;
use steel_utils::{BlockPos, BlockStateId, ChunkPos, types::UpdateFlags};
use wincode::{SchemaRead, SchemaWrite};

use crate::chunk::{
    heightmap::HeightmapType, level_chunk::LevelChunk, proto_chunk::ProtoChunk, section::Sections,
};

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
    /// The chunk is being processed for noise.
    Noise,
    /// The chunk is being processed for surfaces.
    Surface,
    /// The chunk is being processed for carvers.
    Carvers,
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
            4 => Some(Self::Noise),
            5 => Some(Self::Surface),
            6 => Some(Self::Carvers),
            7 => Some(Self::Features),
            8 => Some(Self::InitializeLight),
            9 => Some(Self::Light),
            10 => Some(Self::Spawn),
            11 => Some(Self::Full),
            _ => None,
        }
    }
}

impl ChunkStatus {
    /// Gets the next status in the generation order.
    /// # Panics
    /// This function will panic if the chunk is at the Full status.
    #[must_use]
    pub const fn next(self) -> Option<Self> {
        match self {
            Self::Empty => Some(Self::StructureStarts),
            Self::StructureStarts => Some(Self::StructureReferences),
            Self::StructureReferences => Some(Self::Biomes),
            Self::Biomes => Some(Self::Noise),
            Self::Noise => Some(Self::Surface),
            Self::Surface => Some(Self::Carvers),
            Self::Carvers => Some(Self::Features),
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
            Self::Noise => Some(Self::Biomes),
            Self::Surface => Some(Self::Noise),
            Self::Carvers => Some(Self::Surface),
            Self::Features => Some(Self::Carvers),
            Self::InitializeLight => Some(Self::Features),
            Self::Light => Some(Self::InitializeLight),
            Self::Spawn => Some(Self::Light),
            Self::Full => Some(Self::Spawn),
        }
    }

    /// Returns the heightmap types that should be updated at this status.
    ///
    /// Before CARVERS status, worldgen heightmaps are used.
    /// At CARVERS and after, final heightmaps are used.
    #[must_use]
    pub const fn heightmaps_after(self) -> &'static [HeightmapType] {
        match self {
            // Before CARVERS: use worldgen heightmaps
            Self::Empty
            | Self::StructureStarts
            | Self::StructureReferences
            | Self::Biomes
            | Self::Noise
            | Self::Surface => HeightmapType::worldgen_types(),
            // CARVERS and after: use final heightmaps
            Self::Carvers
            | Self::Features
            | Self::InitializeLight
            | Self::Light
            | Self::Spawn
            | Self::Full => HeightmapType::final_types(),
        }
    }
}

/// An enum that allows access to a chunk in different states.
pub enum ChunkAccess {
    /// A fully generated chunk.
    Full(LevelChunk),
    /// A chunk that is still being generated.
    Proto(ProtoChunk),
    /// To get a chunk accses non-internally you need to use the methods on chunk holder.
    /// Which prohibits you from getting an unloaded chunk.
    // Therefore this can be seen as a placeholder that will panic if you somehow get it
    Unloaded,
}

impl ChunkAccess {
    /// Gets a block at a relative position in the chunk.
    #[must_use]
    pub fn get_relative_block(
        &self,
        relative_x: usize,
        relative_y: usize,
        relative_z: usize,
    ) -> Option<BlockStateId> {
        let sections = match self {
            Self::Full(chunk) => &chunk.sections,
            Self::Proto(proto_chunk) => &proto_chunk.sections,
            Self::Unloaded => unreachable!(),
        };

        sections.get_relative_block(relative_x, relative_y, relative_z)
    }

    /// Sets a block at a relative position in the chunk.
    /// Automatically marks the chunk as dirty.
    pub fn set_relative_block(
        &self,
        relative_x: usize,
        relative_y: usize,
        relative_z: usize,
        value: BlockStateId,
    ) {
        match self {
            Self::Full(chunk) => {
                chunk
                    .sections
                    .set_relative_block(relative_x, relative_y, relative_z, value);
                chunk.dirty.store(true, Ordering::Release);
            }
            Self::Proto(proto_chunk) => {
                proto_chunk
                    .sections
                    .set_relative_block(relative_x, relative_y, relative_z, value);
                proto_chunk.dirty.store(true, Ordering::Release);
            }
            Self::Unloaded => unreachable!(),
        }
    }

    /// Returns whether the chunk has been modified since last save.
    #[must_use]
    pub fn is_dirty(&self) -> bool {
        match self {
            Self::Full(chunk) => chunk.dirty.load(Ordering::Acquire),
            Self::Proto(proto_chunk) => proto_chunk.dirty.load(Ordering::Acquire),
            Self::Unloaded => unreachable!(),
        }
    }

    /// Marks the chunk as dirty (modified).
    pub fn mark_dirty(&self) {
        match self {
            Self::Full(chunk) => chunk.dirty.store(true, Ordering::Release),
            Self::Proto(proto_chunk) => proto_chunk.dirty.store(true, Ordering::Release),
            Self::Unloaded => unreachable!(),
        }
    }

    /// Clears the dirty flag (called after saving).
    pub fn clear_dirty(&self) {
        match self {
            Self::Full(chunk) => chunk.dirty.store(false, Ordering::Release),
            Self::Proto(proto_chunk) => proto_chunk.dirty.store(false, Ordering::Release),
            Self::Unloaded => unreachable!(),
        }
    }

    /// Returns the chunk position.
    #[must_use]
    pub const fn pos(&self) -> ChunkPos {
        match self {
            Self::Full(chunk) => chunk.pos,
            Self::Proto(proto_chunk) => proto_chunk.pos,
            Self::Unloaded => unreachable!(),
        }
    }

    /// Returns a reference to the sections.
    #[must_use]
    pub const fn sections(&self) -> &Sections {
        match self {
            Self::Full(chunk) => &chunk.sections,
            Self::Proto(proto_chunk) => &proto_chunk.sections,
            Self::Unloaded => unreachable!(),
        }
    }

    /// Sets a block state at the given position.
    ///
    /// Returns the old block state, or `None` if nothing changed.
    pub fn set_block_state(
        &self,
        pos: BlockPos,
        state: BlockStateId,
        flags: UpdateFlags,
    ) -> Option<BlockStateId> {
        match self {
            Self::Full(chunk) => chunk.set_block_state(pos, state, flags),
            Self::Proto(proto_chunk) => proto_chunk.set_block_state(pos, state, flags),
            Self::Unloaded => unreachable!(),
        }
    }

    /// Gets a block state at the given position.
    #[must_use]
    pub fn get_block_state(&self, pos: BlockPos) -> BlockStateId {
        match self {
            Self::Full(chunk) => chunk.get_block_state(pos),
            Self::Proto(proto_chunk) => proto_chunk.get_block_state(pos),
            Self::Unloaded => unreachable!(),
        }
    }

    /// Returns a reference to the `LevelChunk` if this is a full chunk.
    #[must_use]
    pub const fn as_full(&self) -> Option<&LevelChunk> {
        match self {
            Self::Full(chunk) => Some(chunk),
            Self::Proto(_) => None,
            Self::Unloaded => unreachable!(),
        }
    }

    /// Ticks the chunk if it's a full chunk.
    ///
    /// # Arguments
    /// * `random_tick_speed` - Number of random blocks to tick per section per tick
    /// * `tick_count` - Current server tick count (for entity sync timing)
    pub fn tick(&self, random_tick_speed: u32, tick_count: i32) {
        if let Self::Full(chunk) = self {
            chunk.tick(random_tick_speed, tick_count);
        }
    }
}
