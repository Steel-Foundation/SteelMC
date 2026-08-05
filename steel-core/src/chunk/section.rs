//! This module contains the `Sections` and `ChunkSection` structs.
use std::{
    fmt::Debug,
    io::Cursor,
    ops::{Deref, DerefMut},
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
};

use parking_lot::{RwLockReadGuard, RwLockWriteGuard};
use steel_registry::blocks::block_state_ext::BlockStateExt;
use steel_registry::vanilla_biomes;
use steel_registry::{REGISTRY, RegistryEntry};
use steel_utils::{BlockPos, BlockStateId, ChunkPos, locks::SyncRwLock, serial::WriteTo};

use crate::chunk::paletted_container::{BiomePalette, BlockPalette};

/// Lock-free index of sections containing randomly-ticking blocks or fluids.
///
/// Section writers update their bit while still holding the section lock. Readers
/// use relaxed loads because this metadata only decides whether to attempt work;
/// section contents remain protected by the section lock and brief staleness is
/// acceptable in the same way as Vanilla's unsynchronized derived counters.
#[derive(Debug)]
pub(crate) struct RandomTickSectionBits {
    words: Box<[AtomicU64]>,
    section_count: usize,
}

impl RandomTickSectionBits {
    fn new(section_count: usize) -> Self {
        let word_count = section_count.div_ceil(u64::BITS as usize);
        let words = (0..word_count)
            .map(|_| AtomicU64::new(0))
            .collect::<Vec<_>>()
            .into_boxed_slice();
        Self {
            words,
            section_count,
        }
    }

    fn set(&self, section_index: usize, randomly_ticking: bool) {
        debug_assert!(section_index < self.section_count);
        let word_index = section_index / u64::BITS as usize;
        let mask = 1_u64 << (section_index % u64::BITS as usize);
        if randomly_ticking {
            self.words[word_index].fetch_or(mask, Ordering::Relaxed);
        } else {
            self.words[word_index].fetch_and(!mask, Ordering::Relaxed);
        }
    }

    fn contains(&self, section_index: usize) -> bool {
        debug_assert!(section_index < self.section_count);
        let word_index = section_index / u64::BITS as usize;
        let mask = 1_u64 << (section_index % u64::BITS as usize);
        self.words[word_index].load(Ordering::Relaxed) & mask != 0
    }

    /// Returns the next eligible section at or above `start`.
    ///
    /// Each call reloads the current word, so a random-tick callback changing a
    /// later section can affect that later section in the same chunk pass.
    #[must_use]
    pub(crate) fn next(&self, start: usize) -> Option<usize> {
        if start >= self.section_count {
            return None;
        }

        let mut word_index = start / u64::BITS as usize;
        let bit_index = start % u64::BITS as usize;
        let mut bits = self.words[word_index].load(Ordering::Relaxed) & (u64::MAX << bit_index);
        loop {
            if bits != 0 {
                let section_index =
                    word_index * u64::BITS as usize + bits.trailing_zeros() as usize;
                return (section_index < self.section_count).then_some(section_index);
            }
            word_index += 1;
            let word = self.words.get(word_index)?;
            bits = word.load(Ordering::Relaxed);
        }
    }

    #[inline]
    #[must_use]
    pub(crate) fn is_empty(&self) -> bool {
        self.next(0).is_none()
    }
}

/// A wrapper around a chunk section.
#[derive(Debug)]
pub struct SectionHolder {
    /// The chunk section data (requires lock to access).
    section: SyncRwLock<ChunkSection>,
    /// Shared lock-free random-tick section index.
    randomly_ticking_sections: Arc<RandomTickSectionBits>,
    section_index: usize,
}

impl SectionHolder {
    /// Creates a new section holder.
    #[must_use]
    pub fn new(section: ChunkSection) -> Self {
        let randomly_ticking_sections = Arc::new(RandomTickSectionBits::new(1));
        Self::with_random_tick_index(section, randomly_ticking_sections, 0)
    }

    fn with_random_tick_index(
        section: ChunkSection,
        randomly_ticking_sections: Arc<RandomTickSectionBits>,
        section_index: usize,
    ) -> Self {
        let randomly_ticking = section.is_randomly_ticking();
        let result = Self {
            section: SyncRwLock::new(section),
            randomly_ticking_sections,
            section_index,
        };
        if randomly_ticking {
            result.randomly_ticking_sections.set(section_index, true);
        }
        result
    }

    /// Returns true if this section contains any randomly-ticking blocks or fluids.
    ///
    /// The mirror may briefly be stale relative to a concurrent section writer.
    /// Section contents and the authoritative counter remain protected by the
    /// section lock.
    #[inline]
    #[must_use]
    pub fn is_randomly_ticking(&self) -> bool {
        self.randomly_ticking_sections.contains(self.section_index)
    }

    /// Acquires a read lock on the section.
    #[inline]
    pub fn read(&self) -> RwLockReadGuard<'_, ChunkSection> {
        self.section.read()
    }

    /// Attempts to acquire a read lock on the section.
    #[inline]
    pub fn try_read(&self) -> Option<RwLockReadGuard<'_, ChunkSection>> {
        self.section.try_read()
    }

    /// Acquires a write lock on the section.
    #[inline]
    pub fn write(&self) -> SectionWriteGuard<'_> {
        SectionWriteGuard::new(
            self.section.write(),
            &self.randomly_ticking_sections,
            self.section_index,
        )
    }

    /// Attempts to acquire a write lock on the section.
    #[inline]
    pub fn try_write(&self) -> Option<SectionWriteGuard<'_>> {
        self.section.try_write().map(|guard| {
            SectionWriteGuard::new(guard, &self.randomly_ticking_sections, self.section_index)
        })
    }
}

/// A chunk-section write guard that republishes derived lock-free metadata.
pub struct SectionWriteGuard<'a> {
    guard: RwLockWriteGuard<'a, ChunkSection>,
    randomly_ticking_sections: &'a RandomTickSectionBits,
    section_index: usize,
    was_randomly_ticking: bool,
}

impl<'a> SectionWriteGuard<'a> {
    fn new(
        guard: RwLockWriteGuard<'a, ChunkSection>,
        randomly_ticking_sections: &'a RandomTickSectionBits,
        section_index: usize,
    ) -> Self {
        let was_randomly_ticking = guard.is_randomly_ticking();
        Self {
            guard,
            randomly_ticking_sections,
            section_index,
            was_randomly_ticking,
        }
    }
}

impl Deref for SectionWriteGuard<'_> {
    type Target = ChunkSection;

    fn deref(&self) -> &Self::Target {
        &self.guard
    }
}

impl DerefMut for SectionWriteGuard<'_> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.guard
    }
}

impl Drop for SectionWriteGuard<'_> {
    fn drop(&mut self) {
        let is_randomly_ticking = self.guard.is_randomly_ticking();
        if is_randomly_ticking != self.was_randomly_ticking {
            self.randomly_ticking_sections
                .set(self.section_index, is_randomly_ticking);
        }
    }
}

/// A collection of chunk sections.
#[derive(Debug)]
pub struct Sections {
    /// The sections in the collection.
    pub sections: Box<[SectionHolder]>,
    randomly_ticking_sections: Arc<RandomTickSectionBits>,
}

/// Cached section counter traits for one block state.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct BlockStateSectionCounts {
    is_air: bool,
    has_fluid: bool,
    randomly_ticking_block: bool,
    randomly_ticking_fluid: bool,
}

const BLOCKS_PER_SECTION: u16 = 16 * 16 * 16;

impl Sections {
    /// Creates a new `Sections` from a box of owned `ChunkSection`s.
    #[must_use]
    pub fn from_owned(sections: Box<[ChunkSection]>) -> Self {
        let randomly_ticking_sections = Arc::new(RandomTickSectionBits::new(sections.len()));
        let holders: Box<[SectionHolder]> = sections
            .into_vec()
            .into_iter()
            .enumerate()
            .map(|(section_index, section)| {
                SectionHolder::with_random_tick_index(
                    section,
                    Arc::clone(&randomly_ticking_sections),
                    section_index,
                )
            })
            .collect();
        Self {
            sections: holders,
            randomly_ticking_sections,
        }
    }

    /// Returns the shared lock-free random-tick section index.
    #[must_use]
    pub(crate) const fn random_tick_sections(&self) -> &Arc<RandomTickSectionBits> {
        &self.randomly_ticking_sections
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
        if buf.len() != total {
            buf.resize(total, BlockStateId::default());
        }
        for (i, holder) in self.sections.iter().enumerate() {
            let guard = holder.read();
            let base = i * 16;
            guard
                .states
                .copy_column_into(x, z, &mut buf[base..base + 16]);
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

    /// Visits every biome palette value in section order while holding each
    /// section's read lock once.
    pub fn for_each_biome_id(&self, mut visitor: impl FnMut(u16)) {
        for holder in &self.sections {
            let guard = holder.read();
            for qy in 0..4 {
                for qz in 0..4 {
                    for qx in 0..4 {
                        visitor(guard.biomes.get(qx, qy, qz));
                    }
                }
            }
        }
    }

    /// Returns whether each real chunk section contains no non-air blocks.
    #[must_use]
    pub fn section_emptiness_map(&self) -> Box<[bool]> {
        self.sections
            .iter()
            .map(|section| section.read().is_empty())
            .collect()
    }

    /// Returns block-light source positions in `ScalableLux` section/local-index order.
    #[must_use]
    pub fn block_light_sources(&self, chunk_pos: ChunkPos, min_y: i32) -> Vec<BlockPos> {
        let mut sources = Vec::new();
        let chunk_min_x = chunk_pos.0.x * BlockPalette::SIZE as i32;
        let chunk_min_z = chunk_pos.0.y * BlockPalette::SIZE as i32;

        for (section_index, section) in self.sections.iter().enumerate() {
            let section_min_y = min_y + (section_index * BlockPalette::SIZE) as i32;
            section.read().append_block_light_sources(
                chunk_min_x,
                section_min_y,
                chunk_min_z,
                &mut sources,
            );
        }

        sources
    }

    /// Writes multiple blocks in one column, holding each section's write guard
    /// across all writes to that section. Most efficient when blocks are grouped
    /// by section (e.g. descending `relative_y` from a top-to-bottom scan).
    pub fn write_column_blocks(&self, x: usize, z: usize, blocks: &[(usize, BlockStateId)]) {
        const DIM: usize = BlockPalette::SIZE;
        debug_assert!(x < DIM);
        debug_assert!(z < DIM);

        let mut i = 0;
        while i < blocks.len() {
            let section_idx = blocks[i].0 / DIM;
            let mut guard = self.sections[section_idx].write();
            guard.states.enter_building_mode();
            let Some(cube) = guard.states.as_building_slice_mut() else {
                unreachable!("just entered building mode")
            };
            let xz_base = z * DIM + x;
            while i < blocks.len() && blocks[i].0 / DIM == section_idx {
                let (rel_y, value) = blocks[i];
                let local_y = rel_y % DIM;
                cube[local_y * DIM * DIM + xz_base] = value;
                i += 1;
            }
        }
    }

    /// Writes a batch of blocks at arbitrary positions, holding each section's
    /// write guard across consecutive entries in the same section. Blocks should
    /// be roughly grouped by section index for best performance.
    ///
    /// Each touched section enters worldgen Building mode (raw cube, no palette
    /// tracking) so writes are O(1) stores. Per-write goes through a flat
    /// `&mut [V]` view of the cube — bypasses the 3-arm `set` match and the
    /// unused old-value load. `recalculate_counts` finalizes.
    pub fn write_block_batch(&self, blocks: &[(usize, usize, usize, BlockStateId)]) {
        const DIM: usize = BlockPalette::SIZE;
        let mut i = 0;
        while i < blocks.len() {
            let section_idx = blocks[i].1 / DIM;
            let mut guard = self.sections[section_idx].write();
            guard.states.enter_building_mode();
            let Some(cube) = guard.states.as_building_slice_mut() else {
                // enter_building_mode just transitioned to Building.
                unreachable!("just entered building mode")
            };
            while i < blocks.len() && blocks[i].1 / DIM == section_idx {
                let (x, rel_y, z, value) = blocks[i];
                let local_y = rel_y % DIM;
                cube[local_y * DIM * DIM + z * DIM + x] = value;
                i += 1;
            }
        }
    }

    /// Writes a batch of blocks while maintaining section counters and palette state.
    ///
    /// Blocks should be grouped by section index so each touched section only needs
    /// one write guard.
    pub(crate) fn write_tracked_block_batch(&self, blocks: &[(usize, usize, usize, BlockStateId)]) {
        const DIM: usize = BlockPalette::SIZE;
        let mut i = 0;
        while i < blocks.len() {
            let section_idx = blocks[i].1 / DIM;
            let mut guard = self.sections[section_idx].write();
            while i < blocks.len() && blocks[i].1 / DIM == section_idx {
                let (x, relative_y, z, value) = blocks[i];
                guard.set_block_state(x, relative_y % DIM, z, value);
                i += 1;
            }
        }
    }

    /// Sets a block at a relative position in the chunk and keeps section
    /// counters/palette serialization ready.
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
        let mut guard = self.sections[idx].write();
        guard.set_block_state(relative_x, relative_y, relative_z, value);
    }

    /// Sets a block during worldgen using the raw building palette path.
    ///
    /// Callers must finalize by recounting touched sections before save,
    /// promotion, or packet serialization.
    pub(crate) fn set_relative_block_for_generation(
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
        let mut guard = self.sections[idx].write();
        guard.set_block_state_for_generation(relative_x, relative_y, relative_z, value);
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
    /// Number of fluid-containing blocks in this section (0-4096).
    /// Includes water, lava, and waterlogged blocks.
    fluid_count: u16,
    /// Number of randomly-ticking blocks in this section (0-4096).
    pub ticking_block_count: u16,
    /// Number of randomly-ticking fluids in this section (0-4096).
    ticking_fluid_count: u16,
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
            fluid_count: 0,
            ticking_block_count: 0,
            ticking_fluid_count: 0,
        }
    }

    /// Creates a new empty chunk section.
    #[must_use]
    pub fn new_empty() -> Self {
        let plains_id = vanilla_biomes::PLAINS.id() as u16;
        Self {
            states: BlockPalette::Homogeneous(BlockStateId(0)),
            biomes: BiomePalette::Homogeneous(plains_id),
            non_empty_block_count: 0,
            fluid_count: 0,
            ticking_block_count: 0,
            ticking_fluid_count: 0,
        }
    }

    /// Returns true if this section contains no non-air blocks.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.non_empty_block_count == 0
    }

    /// Returns true if this section contains any randomly-ticking blocks or fluids.
    #[must_use]
    pub const fn is_randomly_ticking(&self) -> bool {
        self.is_randomly_ticking_blocks() || self.is_randomly_ticking_fluids()
    }

    /// Returns true if this section contains any randomly-ticking blocks.
    #[must_use]
    pub const fn is_randomly_ticking_blocks(&self) -> bool {
        self.ticking_block_count > 0
    }

    /// Returns true if this section contains any randomly-ticking fluids.
    #[must_use]
    pub const fn is_randomly_ticking_fluids(&self) -> bool {
        self.ticking_fluid_count > 0
    }

    /// Returns true if this section's palette may contain block-light sources.
    #[must_use]
    pub fn maybe_has_block_light_sources(&self) -> bool {
        !self.is_empty()
            && self
                .states
                .maybe_has(|state| state.get_light_emission() > 0)
    }

    /// Appends block-light source positions in `ScalableLux` local-index order.
    pub fn append_block_light_sources(
        &self,
        chunk_min_x: i32,
        section_min_y: i32,
        chunk_min_z: i32,
        sources: &mut Vec<BlockPos>,
    ) {
        if !self.maybe_has_block_light_sources() {
            return;
        }

        for local_index in 0..BlockPalette::VOLUME {
            let state = self.states.get_at_index(local_index);
            if state.get_light_emission() == 0 {
                continue;
            }

            sources.push(BlockPos::new(
                chunk_min_x + (local_index & 15) as i32,
                section_min_y + (local_index >> 8) as i32,
                chunk_min_z + ((local_index >> 4) & 15) as i32,
            ));
        }
    }

    /// Returns the number of non-air blocks in this section.
    #[must_use]
    pub const fn non_empty_block_count(&self) -> u16 {
        self.non_empty_block_count
    }

    /// Returns the number of fluid-containing blocks in this section.
    #[must_use]
    pub const fn fluid_count(&self) -> u16 {
        self.fluid_count
    }

    /// Returns if the chunk has fluid.
    #[must_use]
    pub const fn has_fluid(&self) -> bool {
        self.fluid_count > 0
    }

    /// Returns the number of randomly-ticking blocks in this section.
    #[must_use]
    pub const fn ticking_block_count(&self) -> u16 {
        self.ticking_block_count
    }

    /// Returns the number of randomly-ticking fluids in this section.
    #[must_use]
    pub const fn ticking_fluid_count(&self) -> u16 {
        self.ticking_fluid_count
    }

    /// Recalculates cached counters from extracted per-state metadata.
    ///
    /// Iterates the palette (`O(palette_size)`) rather than every cube cell
    /// (`O(4096)`): each block-state appears at most once in the palette and
    /// carries its own occurrence count, so we just classify each unique state
    /// and multiply by its count. Mirrors Moonrise's `BlockCountingBitStorage`.
    /// For a `Homogeneous` section that's a single classify; for typical
    /// `Heterogeneous` sections palette is well under 16 entries.
    pub fn recalculate_counts(&mut self) {
        self.recalculate_counts_from_palette(Self::block_state_section_counts);
    }

    fn recalculate_counts_from_palette(
        &mut self,
        mut counts_for_state: impl FnMut(BlockStateId) -> BlockStateSectionCounts,
    ) {
        self.states.finalize_building();

        let mut non_empty: u16 = 0;
        let mut fluid: u16 = 0;
        let mut ticking_blocks: u16 = 0;
        let mut ticking_fluids: u16 = 0;

        match &self.states {
            BlockPalette::Homogeneous(state) => {
                let counts = counts_for_state(*state);
                Self::accumulate_counter_traits(
                    &mut non_empty,
                    &mut fluid,
                    &mut ticking_blocks,
                    &mut ticking_fluids,
                    counts,
                    BLOCKS_PER_SECTION,
                );
            }
            BlockPalette::Heterogeneous(data) => {
                for &(state, count) in &data.palette {
                    let counts = counts_for_state(state);
                    Self::accumulate_counter_traits(
                        &mut non_empty,
                        &mut fluid,
                        &mut ticking_blocks,
                        &mut ticking_fluids,
                        counts,
                        count,
                    );
                }
            }
            BlockPalette::Building(_) => unreachable!("finalize_building was just called"),
        }

        self.non_empty_block_count = non_empty;
        self.fluid_count = fluid;
        self.ticking_block_count = ticking_blocks;
        self.ticking_fluid_count = ticking_fluids;
    }

    const fn accumulate_counter_traits(
        non_empty: &mut u16,
        fluid: &mut u16,
        ticking_blocks: &mut u16,
        ticking_fluids: &mut u16,
        counts: BlockStateSectionCounts,
        block_count: u16,
    ) {
        if !counts.is_air {
            *non_empty += block_count;
        }
        if counts.has_fluid {
            *fluid += block_count;
        }
        if counts.randomly_ticking_block {
            *ticking_blocks += block_count;
        }
        if counts.randomly_ticking_fluid {
            *ticking_fluids += block_count;
        }
    }

    /// Whether this section's palette contains any POI-type block state.
    ///
    /// Lets the Full-stage POI populate skip the full 4096-block
    /// `scan_and_populate` for the overwhelming majority of sections
    /// (stone/dirt/air) that hold no POI blocks — a palette scan of `O(≤16)`
    /// instead of `O(4096)`. Mirrors vanilla's `LevelChunkSection.maybeHas`.
    #[must_use]
    pub fn contains_poi(&self) -> bool {
        let poi = &REGISTRY.poi_types;
        match &self.states {
            BlockPalette::Homogeneous(state) => poi.is_poi_state(*state),
            BlockPalette::Heterogeneous(data) => data
                .palette
                .iter()
                .any(|(state, _)| poi.is_poi_state(*state)),
            // Not yet finalized (only happens mid-worldgen, not at promotion);
            // fall back to scanning rather than risk missing a POI.
            BlockPalette::Building(_) => true,
        }
    }

    /// Sets a block state and updates the cached counters.
    ///
    /// Returns the old block state.
    ///
    pub fn set_block_state(
        &mut self,
        x: usize,
        y: usize,
        z: usize,
        new_state: BlockStateId,
    ) -> BlockStateId {
        self.ensure_counter_ready_for_delta();
        let old_state = self.states.set(x, y, z, new_state);

        if old_state != new_state {
            let old_counts = Self::block_state_section_counts(old_state);
            let new_counts = Self::block_state_section_counts(new_state);
            self.apply_count_change(old_counts, new_counts);
        }

        old_state
    }

    /// Sets a block state through the raw worldgen building path.
    ///
    /// Returns the old block state. Cached counters are intentionally not
    /// updated; callers must recount before light, promotion, save, or packet
    /// serialization.
    pub(crate) fn set_block_state_for_generation(
        &mut self,
        x: usize,
        y: usize,
        z: usize,
        new_state: BlockStateId,
    ) -> BlockStateId {
        self.states.enter_building_mode();
        self.states.set(x, y, z, new_state)
    }

    /// Returns the cached-counter traits for a block state.
    pub(crate) fn block_state_section_counts(state: BlockStateId) -> BlockStateSectionCounts {
        let metadata = state.get_ticking_metadata();
        BlockStateSectionCounts {
            is_air: metadata.is_air(),
            has_fluid: metadata.has_fluid(),
            randomly_ticking_block: metadata.randomly_ticking_block(),
            randomly_ticking_fluid: metadata.randomly_ticking_fluid(),
        }
    }

    pub(crate) fn finalize_generation_counts_if_needed(&mut self) {
        if matches!(&self.states, BlockPalette::Building(_)) {
            self.recalculate_counts();
        }
    }

    fn ensure_counter_ready_for_delta(&mut self) {
        if matches!(&self.states, BlockPalette::Building(_)) {
            log::debug!(
                "finalizing worldgen Building palette before applying a counter-aware \
                 block-state delta"
            );
            self.recalculate_counts();
        }
    }

    const fn apply_count_change(
        &mut self,
        old_counts: BlockStateSectionCounts,
        new_counts: BlockStateSectionCounts,
    ) {
        if !old_counts.is_air && new_counts.is_air {
            self.non_empty_block_count -= 1;
        } else if old_counts.is_air && !new_counts.is_air {
            self.non_empty_block_count += 1;
        }

        if old_counts.has_fluid && !new_counts.has_fluid {
            self.fluid_count -= 1;
        } else if !old_counts.has_fluid && new_counts.has_fluid {
            self.fluid_count += 1;
        }

        if old_counts.randomly_ticking_block && !new_counts.randomly_ticking_block {
            self.ticking_block_count -= 1;
        } else if !old_counts.randomly_ticking_block && new_counts.randomly_ticking_block {
            self.ticking_block_count += 1;
        }

        if old_counts.randomly_ticking_fluid && !new_counts.randomly_ticking_fluid {
            self.ticking_fluid_count -= 1;
        } else if !old_counts.randomly_ticking_fluid && new_counts.randomly_ticking_fluid {
            self.ticking_fluid_count += 1;
        }
    }

    /// Writes the chunk section to a writer.
    ///
    /// # Panics
    /// - If the writer fails to write.
    pub fn write(&self, writer: &mut Cursor<Vec<u8>>) {
        self.non_empty_block_count
            .write(writer)
            .expect("Failed to write block count");
        self.fluid_count
            .write(writer)
            .expect("Failed to write fluid count");

        self.states
            .write(writer)
            .expect("Failed to write block states");
        self.biomes.write(writer).expect("Failed to write biomes");
    }
}

#[cfg(test)]
mod tests {
    use steel_registry::test_support::init_test_registry;
    use steel_registry::vanilla_blocks;

    use crate::behavior::init_behaviors;

    use super::*;

    fn plains_biomes() -> BiomePalette {
        BiomePalette::Homogeneous(vanilla_biomes::PLAINS.id() as u16)
    }

    fn init_test_behaviors() {
        init_test_registry();
        init_behaviors();
    }

    #[test]
    fn recount_uses_homogeneous_palette_frequency() {
        init_test_behaviors();

        let mut section = ChunkSection::new_with_biomes(
            BlockPalette::Homogeneous(vanilla_blocks::LAVA.default_state()),
            plains_biomes(),
        );

        section.recalculate_counts();

        assert_eq!(section.non_empty_block_count(), BLOCKS_PER_SECTION);
        assert_eq!(section.fluid_count(), BLOCKS_PER_SECTION);
        assert_eq!(section.ticking_block_count(), BLOCKS_PER_SECTION);
        assert_eq!(section.ticking_fluid_count(), BLOCKS_PER_SECTION);
    }

    #[test]
    fn recount_uses_heterogeneous_palette_frequencies() {
        init_test_behaviors();

        let air = vanilla_blocks::AIR.default_state();
        let stone = vanilla_blocks::STONE.default_state();
        let water = vanilla_blocks::WATER.default_state();
        let lava = vanilla_blocks::LAVA.default_state();
        let mut cube = Box::new([[[air; 16]; 16]; 16]);

        cube[0][0][0] = stone;
        cube[1][0][0] = stone;
        cube[2][0][0] = water;
        cube[3][0][0] = water;
        cube[4][0][0] = water;
        cube[5][0][0] = lava;

        let mut section =
            ChunkSection::new_with_biomes(BlockPalette::from_cube(cube), plains_biomes());

        section.recalculate_counts();

        assert_eq!(section.non_empty_block_count(), 6);
        assert_eq!(section.fluid_count(), 4);
        assert_eq!(section.ticking_block_count(), 1);
        assert_eq!(section.ticking_fluid_count(), 1);
    }

    #[test]
    fn holder_keeps_random_tick_eligibility_in_sync() {
        init_test_behaviors();

        let mut loaded_section = ChunkSection::new_with_biomes(
            BlockPalette::Homogeneous(vanilla_blocks::LAVA.default_state()),
            plains_biomes(),
        );
        loaded_section.recalculate_counts();
        let loaded_holder = SectionHolder::new(loaded_section);
        assert!(loaded_holder.is_randomly_ticking());

        let holder = SectionHolder::new(ChunkSection::new_empty());
        {
            let mut section = holder.write();
            section.set_block_state(0, 0, 0, vanilla_blocks::LAVA.default_state());
            assert_eq!(section.ticking_block_count(), 1);
            assert_eq!(section.ticking_fluid_count(), 1);
        }
        assert!(holder.is_randomly_ticking());

        {
            let Some(mut section) = holder.try_write() else {
                panic!("uncontended section write lock was unavailable");
            };
            section.set_block_state(0, 0, 0, vanilla_blocks::AIR.default_state());
            assert_eq!(section.ticking_block_count(), 0);
            assert_eq!(section.ticking_fluid_count(), 0);
        }
        assert!(!holder.is_randomly_ticking());
    }

    #[test]
    fn shared_random_tick_section_bits_follow_cross_word_updates() {
        init_test_behaviors();
        let sections = Sections::from_owned(
            (0..65)
                .map(|_| ChunkSection::new_empty())
                .collect::<Vec<_>>()
                .into_boxed_slice(),
        );
        let bits = Arc::clone(sections.random_tick_sections());
        assert!(bits.is_empty());

        {
            let mut section = sections.sections[64].write();
            section.set_block_state(0, 0, 0, vanilla_blocks::LAVA.default_state());
        }
        assert_eq!(bits.next(0), Some(64));

        {
            let mut section = sections.sections[1].write();
            section.set_block_state(0, 0, 0, vanilla_blocks::LAVA.default_state());
        }
        assert_eq!(bits.next(0), Some(1));
        assert_eq!(bits.next(2), Some(64));

        {
            let mut section = sections.sections[1].write();
            section.set_block_state(0, 0, 0, vanilla_blocks::AIR.default_state());
        }
        assert_eq!(bits.next(0), Some(64));

        {
            let mut section = sections.sections[64].write();
            section.set_block_state(0, 0, 0, vanilla_blocks::AIR.default_state());
        }
        assert!(bits.is_empty());
    }

    #[test]
    fn generation_recount_publishes_random_tick_section_bit() {
        init_test_behaviors();
        let sections = Sections::from_owned(vec![ChunkSection::new_empty()].into_boxed_slice());
        let bits = Arc::clone(sections.random_tick_sections());

        {
            let mut section = sections.sections[0].write();
            section.set_block_state_for_generation(0, 0, 0, vanilla_blocks::LAVA.default_state());
        }
        assert!(bits.is_empty());

        {
            let mut section = sections.sections[0].write();
            section.finalize_generation_counts_if_needed();
        }
        assert_eq!(bits.next(0), Some(0));
    }

    #[test]
    fn counter_aware_write_recounts_building_palette_before_delta() {
        init_test_behaviors();

        let air = vanilla_blocks::AIR.default_state();
        let stone = vanilla_blocks::STONE.default_state();
        let mut section = ChunkSection::new_empty();

        section.set_block_state_for_generation(0, 0, 0, stone);
        assert_eq!(section.non_empty_block_count(), 0);

        let old_state = section.set_block_state(0, 0, 0, air);

        assert_eq!(old_state, stone);
        assert_eq!(section.non_empty_block_count(), 0);

        let old_state = section.set_block_state(0, 0, 0, stone);

        assert_eq!(old_state, air);
        assert_eq!(section.non_empty_block_count(), 1);
    }
}
