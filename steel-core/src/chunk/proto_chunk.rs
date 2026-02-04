//! A proto chunk is a chunk that is still being generated.
use std::sync::atomic::{AtomicBool, Ordering};

use rustc_hash::FxHashMap;

use crossbeam::atomic::AtomicCell;
use steel_registry::{REGISTRY, blocks::block_state_ext::BlockStateExt, vanilla_blocks};
use steel_utils::{BlockPos, BlockStateId, ChunkPos, locks::SyncRwLock, types::UpdateFlags};

use crate::chunk::{
    chunk_access::ChunkStatus,
    heightmap::{Heightmap, HeightmapType, prime_heightmaps},
    section::Sections,
};

/// A chunk that is still being generated.
#[derive(Debug)]
pub struct ProtoChunk {
    /// The sections of the chunk.
    pub sections: Sections,
    /// The position of the chunk.
    pub pos: ChunkPos,
    /// Whether the chunk has been modified since last save.
    /// Proto chunks start dirty since they're being generated.
    pub dirty: AtomicBool,
    /// Current generation status of this chunk. Every time a chunk is loaded it goes thru all stages.
    /// If you want the real status use the chunkholder status
    status: AtomicCell<ChunkStatus>,
    /// Heightmaps (lazily initialized based on status).
    pub heightmaps: SyncRwLock<FxHashMap<HeightmapType, Heightmap>>,
    /// The minimum Y coordinate of the world this chunk belongs to.
    min_y: i32,
    /// The total height of the world.
    height: i32,
}

impl ProtoChunk {
    /// Creates a new proto chunk at the given position with empty sections.
    #[must_use]
    pub fn new(sections: Sections, pos: ChunkPos, min_y: i32, height: i32) -> Self {
        Self {
            sections,
            pos,
            dirty: AtomicBool::new(true), // New chunks are always dirty
            status: AtomicCell::new(ChunkStatus::Empty),
            heightmaps: SyncRwLock::new(FxHashMap::default()),
            min_y,
            height,
        }
    }

    /// Creates a proto chunk that was loaded from disk.
    #[must_use]
    pub fn from_disk(
        sections: Sections,
        pos: ChunkPos,
        status: ChunkStatus,
        min_y: i32,
        height: i32,
    ) -> Self {
        Self {
            sections,
            pos,
            dirty: AtomicBool::new(false),
            status: AtomicCell::new(status),
            //TODO: Save heigtmaps on disk
            heightmaps: SyncRwLock::new(FxHashMap::default()),
            min_y,
            height,
        }
    }

    /// Returns the minimum Y coordinate of the world.
    #[must_use]
    pub const fn min_y(&self) -> i32 {
        self.min_y
    }

    /// Returns the total height of the world.
    #[must_use]
    pub const fn height(&self) -> i32 {
        self.height
    }

    /// Gets the current generation status of this chunk.
    #[must_use]
    pub fn status(&self) -> ChunkStatus {
        self.status.load()
    }

    /// Sets the generation status of this chunk.
    pub fn set_status(&self, status: ChunkStatus) {
        self.status.store(status);
    }

    /// Gets the section index for a given Y coordinate.
    #[must_use]
    const fn get_section_index(&self, y: i32) -> usize {
        ((y - self.min_y) / 16) as usize
    }

    /// Marks the chunk as unsaved.
    fn mark_unsaved(&self) {
        self.dirty.store(true, Ordering::Release);
    }

    /// Sets a block state at the given position.
    ///
    /// Returns the old block state at the position, or `VOID_AIR` if out of bounds.
    pub fn set_block_state(
        &self,
        pos: BlockPos,
        state: BlockStateId,
        _flags: UpdateFlags,
    ) -> Option<BlockStateId> {
        let y = pos.0.y;

        if y < self.min_y || y >= self.min_y + self.height {
            return Some(
                REGISTRY
                    .blocks
                    .get_default_state_id(vanilla_blocks::VOID_AIR),
            );
        }

        let section_index = self.get_section_index(y);
        let section = &self.sections.sections[section_index];

        let was_empty = section.read().states.has_only_air();
        if was_empty && state.is_air() {
            return Some(state);
        }

        let local_x = (pos.0.x & 15) as usize;
        let local_y = (y & 15) as usize;
        let local_z = (pos.0.z & 15) as usize;

        let old_state = section.write().states.set(local_x, local_y, local_z, state);

        if old_state == state {
            return None;
        }

        let heightmap_types = self.status().heightmaps_after();

        let min_y = self.min_y;
        let height = self.height;
        let sections = &self.sections;
        let mut heightmaps = self.heightmaps.write();

        prime_heightmaps(
            &mut heightmaps,
            heightmap_types,
            min_y,
            height,
            |lx, scan_y, lz| {
                let scan_section_index = ((scan_y - min_y) / 16) as usize;
                let scan_local_y = ((scan_y - min_y) % 16) as usize;
                sections.sections[scan_section_index]
                    .read()
                    .states
                    .get(lx, scan_local_y, lz)
            },
        );

        for &hm_type in heightmap_types {
            if let Some(heightmap) = heightmaps.get_mut(&hm_type) {
                heightmap.update(local_x, y, local_z, state, |lx, scan_y, lz| {
                    let scan_section_index = ((scan_y - min_y) / 16) as usize;
                    let scan_local_y = ((scan_y - min_y) % 16) as usize;
                    sections.sections[scan_section_index]
                        .read()
                        .states
                        .get(lx, scan_local_y, lz)
                });
            }
        }

        self.mark_unsaved();
        Some(old_state)
    }

    /// Gets a block state at the given position.
    #[must_use]
    pub fn get_block_state(&self, pos: BlockPos) -> BlockStateId {
        let y = pos.0.y;

        // Check bounds
        if y < self.min_y || y >= self.min_y + self.height {
            // Out of bounds - return default air
            return REGISTRY.blocks.get_default_state_id(vanilla_blocks::AIR);
        }

        let section_index = self.get_section_index(y);
        let section = &self.sections.sections[section_index];

        // Optimization: if section is empty, return air
        if section.read().states.has_only_air() {
            return REGISTRY.blocks.get_default_state_id(vanilla_blocks::AIR);
        }

        let local_x = (pos.0.x & 15) as usize;
        let local_y = (y & 15) as usize;
        let local_z = (pos.0.z & 15) as usize;

        section.read().states.get(local_x, local_y, local_z)
    }
}
