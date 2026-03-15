//! This module contains the `Sections` and `ChunkSection` structs.
use std::{fmt::Debug, io::Cursor};

use steel_registry::REGISTRY;
use steel_registry::blocks::block_state_ext::BlockStateExt;
use steel_registry::vanilla_biomes;
use steel_utils::{BlockStateId, locks::SyncRwLock, serial::WriteTo};

use crate::behavior::{BLOCK_BEHAVIORS, BlockBehaviorRegistry};
use crate::chunk::paletted_container::{BiomePalette, BlockPalette};

/// A wrapper around a chunk section.
#[derive(Debug)]
pub struct SectionHolder {
    /// The chunk section data (requires lock to access).
    pub section: SyncRwLock<ChunkSection>,
}

impl SectionHolder {
    /// Creates a new section holder.
    #[must_use]
    pub const fn new(section: ChunkSection) -> Self {
        Self {
            section: SyncRwLock::new(section),
        }
    }

    /// Returns true if this section contains any randomly-ticking blocks.
    ///
    /// # Safety
    /// This performs an unsynchronized read of the ticking block count.
    /// This is safe because:
    /// - `ticking_block_count` is a `u16` which has atomic reads on all supported platforms
    /// - A stale/torn read is acceptable here (worst case: we acquire an unnecessary lock)
    #[inline]
    #[must_use]
    pub fn is_randomly_ticking(&self) -> bool {
        unsafe { (*self.section.data_ptr()).ticking_block_count > 0 }
    }

    /// Acquires a read lock on the section.
    #[inline]
    pub fn read(&self) -> parking_lot::RwLockReadGuard<'_, ChunkSection> {
        self.section.read()
    }

    /// Acquires a write lock on the section.
    #[inline]
    pub fn write(&self) -> parking_lot::RwLockWriteGuard<'_, ChunkSection> {
        self.section.write()
    }
}

/// A collection of chunk sections.
#[derive(Debug)]
pub struct Sections {
    /// The sections in the collection.
    pub sections: Box<[SectionHolder]>,
}

impl Sections {
    /// Creates a new `Sections` from a box of owned `ChunkSection`s.
    #[must_use]
    pub fn from_owned(sections: Box<[ChunkSection]>) -> Self {
        let holders: Box<[SectionHolder]> = sections
            .into_vec()
            .into_iter()
            .map(SectionHolder::new)
            .collect();
        Self { sections: holders }
    }

    /// Gets a block at a relative position in the chunk.
    #[must_use]
    pub fn get_relative_block(
        &self,
        relative_x: usize,
        relative_y: usize,
        relative_z: usize,
    ) -> Option<BlockStateId> {
        debug_assert!(relative_x < BlockPalette::SIZE);
        debug_assert!(relative_z < BlockPalette::SIZE);

        let section_index = relative_y / BlockPalette::SIZE;
        let relative_y = relative_y % BlockPalette::SIZE;
        self.sections.get(section_index).map(|section| {
            section
                .read()
                .states
                .get(relative_x, relative_y, relative_z)
        })
    }

    /// Reads an entire column at `(x, z)` across all sections into a caller-owned buffer.
    ///
    /// Holds each section's read lock once for 16 Y reads instead of acquiring
    /// a lock per block. Indexed by `relative_y` (0 = chunk min-y).
    /// The buffer is resized if needed and reused across calls to avoid allocation.
    pub fn read_column_into(&self, x: usize, z: usize, buf: &mut Vec<BlockStateId>) {
        debug_assert!(x < BlockPalette::SIZE);
        debug_assert!(z < BlockPalette::SIZE);

        let total = self.sections.len() * 16;
        buf.clear();
        buf.resize(total, BlockStateId(0));
        for (i, holder) in self.sections.iter().enumerate() {
            let guard = holder.read();
            let base = i * 16;
            for ly in 0..16 {
                buf[base + ly] = guard.states.get(x, ly, z);
            }
        }
    }

    /// Reads all biome palette values into a flat array.
    ///
    /// Indexed as `[section_idx * 64 + qy * 16 + qz * 4 + qx]`.
    /// Holds each section's read lock once for all 64 biome reads.
    #[must_use]
    pub fn read_all_biomes(&self) -> Box<[u16]> {
        let total = self.sections.len() * 64;
        let mut biomes = vec![0u16; total];
        for (i, holder) in self.sections.iter().enumerate() {
            let guard = holder.read();
            let base = i * 64;
            for qy in 0..4 {
                for qz in 0..4 {
                    for qx in 0..4 {
                        biomes[base + qy * 16 + qz * 4 + qx] = guard.biomes.get(qx, qy, qz);
                    }
                }
            }
        }
        biomes.into_boxed_slice()
    }

    /// Writes multiple blocks in one column, holding each section's write guard
    /// across all writes to that section. Most efficient when blocks are grouped
    /// by section (e.g. descending `relative_y` from a top-to-bottom scan).
    pub fn write_column_blocks(&self, x: usize, z: usize, blocks: &[(usize, BlockStateId)]) {
        debug_assert!(x < BlockPalette::SIZE);
        debug_assert!(z < BlockPalette::SIZE);

        let mut i = 0;
        while i < blocks.len() {
            let section_idx = blocks[i].0 / BlockPalette::SIZE;
            let mut guard = self.sections[section_idx].write();
            while i < blocks.len() && blocks[i].0 / BlockPalette::SIZE == section_idx {
                let (rel_y, value) = blocks[i];
                guard.states.set(x, rel_y % BlockPalette::SIZE, z, value);
                i += 1;
            }
        }
    }

    /// Writes a batch of blocks at arbitrary positions, holding each section's
    /// write guard across consecutive entries in the same section. Blocks should
    /// be roughly grouped by section index for best performance.
    pub fn write_block_batch(&self, blocks: &[(usize, usize, usize, BlockStateId)]) {
        let mut i = 0;
        while i < blocks.len() {
            let section_idx = blocks[i].1 / BlockPalette::SIZE;
            let mut guard = self.sections[section_idx].write();
            while i < blocks.len() && blocks[i].1 / BlockPalette::SIZE == section_idx {
                let (x, rel_y, z, value) = blocks[i];
                guard.states.set(x, rel_y % BlockPalette::SIZE, z, value);
                i += 1;
            }
        }
    }

    /// Sets a block at a relative position in the chunk.
    pub fn set_relative_block(
        &self,
        relative_x: usize,
        relative_y: usize,
        relative_z: usize,
        value: BlockStateId,
    ) {
        debug_assert!(relative_x < BlockPalette::SIZE);
        debug_assert!(relative_z < BlockPalette::SIZE);

        let idx = relative_y / BlockPalette::SIZE;
        let relative_y = relative_y % BlockPalette::SIZE;
        self.sections[idx]
            .write()
            .states
            .set(relative_x, relative_y, relative_z, value);
    }
}

/// A chunk section.
///
/// Contains a 16x16x16 cube of block states and biomes, along with cached
/// counts for optimization (similar to vanilla's `LevelChunkSection`).
#[derive(Debug)]
pub struct ChunkSection {
    /// The block states in the section.
    pub states: BlockPalette,
    /// The biomes in the section.
    pub biomes: BiomePalette,
    /// Number of non-air blocks in this section (0-4096).
    /// Used to quickly check if a section is empty.
    non_empty_block_count: u16,
    /// Number of randomly-ticking blocks in this section (0-4096).
    pub ticking_block_count: u16,
}

impl ChunkSection {
    /// Creates a new chunk section with the given block states and biomes.
    ///
    /// Note: You must call `recalculate_counts()` after creation to initialize
    /// the cached counters if the states palette contains non-air blocks.
    #[must_use]
    pub const fn new_with_biomes(states: BlockPalette, biomes: BiomePalette) -> Self {
        Self {
            states,
            biomes,
            non_empty_block_count: 0,
            ticking_block_count: 0,
        }
    }

    /// Creates a new empty chunk section.
    #[must_use]
    pub fn new_empty() -> Self {
        let plains_id = *REGISTRY.biomes.get_id(&vanilla_biomes::PLAINS) as u16;
        Self {
            states: BlockPalette::Homogeneous(BlockStateId(0)),
            biomes: BiomePalette::Homogeneous(plains_id),
            non_empty_block_count: 0,
            ticking_block_count: 0,
        }
    }

    /// Returns true if this section contains no non-air blocks.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.non_empty_block_count == 0
    }

    /// Returns true if this section contains any randomly-ticking blocks.
    #[must_use]
    pub const fn is_randomly_ticking(&self) -> bool {
        self.ticking_block_count > 0
    }

    /// Returns the number of non-air blocks in this section.
    #[must_use]
    pub const fn non_empty_block_count(&self) -> u16 {
        self.non_empty_block_count
    }

    /// Returns the number of randomly-ticking blocks in this section.
    #[must_use]
    pub const fn ticking_block_count(&self) -> u16 {
        self.ticking_block_count
    }

    /// Recalculates both cached counters by iterating all blocks.
    ///
    /// This should be called after chunk loading or generation to initialize
    /// the counters. It requires the block behavior registry to be initialized.
    ///
    /// # Panics
    /// Panics if the block behavior registry has not been initialized.
    pub fn recalculate_counts(&mut self) {
        self.recalculate_counts_with(&BLOCK_BEHAVIORS);
    }

    /// Recalculates both cached counters using the provided behavior registry.
    pub fn recalculate_counts_with(&mut self, block_behaviors: &BlockBehaviorRegistry) {
        let mut non_empty: u16 = 0;
        let mut ticking: u16 = 0;

        for y in 0..16 {
            for z in 0..16 {
                for x in 0..16 {
                    let state = self.states.get(x, y, z);
                    if !state.is_air() {
                        non_empty += 1;
                        let block = state.get_block();
                        let behavior = block_behaviors.get_behavior(block);
                        if behavior.is_randomly_ticking(state) {
                            ticking += 1;
                        }
                    }
                }
            }
        }

        self.non_empty_block_count = non_empty;
        self.ticking_block_count = ticking;
    }

    /// Sets a block state and updates the cached counters.
    ///
    /// Returns the old block state.
    ///
    /// # Panics
    /// Panics if the block behavior registry has not been initialized.
    pub fn set_block_state(
        &mut self,
        x: usize,
        y: usize,
        z: usize,
        new_state: BlockStateId,
    ) -> BlockStateId {
        self.set_block_state_with(x, y, z, new_state, &BLOCK_BEHAVIORS)
    }

    /// Sets a block state and updates the cached counters using the provided behavior registry.
    ///
    /// Returns the old block state.
    pub fn set_block_state_with(
        &mut self,
        x: usize,
        y: usize,
        z: usize,
        new_state: BlockStateId,
        block_behaviors: &BlockBehaviorRegistry,
    ) -> BlockStateId {
        let old_state = self.states.set(x, y, z, new_state);

        if old_state != new_state {
            // Update non-empty count
            let old_is_air = old_state.is_air();
            let new_is_air = new_state.is_air();

            if !old_is_air && new_is_air {
                self.non_empty_block_count -= 1;
            } else if old_is_air && !new_is_air {
                self.non_empty_block_count += 1;
            }

            // Update ticking count
            let old_block = old_state.get_block();
            let new_block = new_state.get_block();
            let old_ticking = block_behaviors
                .get_behavior(old_block)
                .is_randomly_ticking(old_state);
            let new_ticking = block_behaviors
                .get_behavior(new_block)
                .is_randomly_ticking(new_state);

            if old_ticking && !new_ticking {
                self.ticking_block_count -= 1;
            } else if !old_ticking && new_ticking {
                self.ticking_block_count += 1;
            }
        }

        old_state
    }

    /// Writes the chunk section to a writer.
    ///
    /// # Panics
    /// - If the writer fails to write.
    pub fn write(&self, writer: &mut Cursor<Vec<u8>>) {
        self.non_empty_block_count
            .write(writer)
            .expect("Failed to write block count");

        self.states
            .write(writer)
            .expect("Failed to write block states");
        self.biomes.write(writer).expect("Failed to write biomes");
    }
}
