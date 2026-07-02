//! This module contains the `Sections` and `ChunkSection` structs.
use std::{fmt::Debug, io::Cursor, sync::LazyLock};

use steel_registry::blocks::block_state_ext::BlockStateExt;
use steel_registry::vanilla_biomes;
use steel_registry::{REGISTRY, RegistryEntry};
use steel_utils::{BlockPos, BlockStateId, ChunkPos, locks::SyncRwLock, serial::WriteTo};

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
    /// Performs an unsynchronized read of the ticking block count to avoid
    /// lock overhead on every section during random ticks. A stale read is
    /// acceptable: worst case we acquire an unnecessary lock.
    #[inline]
    #[must_use]
    pub fn is_randomly_ticking(&self) -> bool {
        // SAFETY: `ticking_block_count` is a `u16` — reads are atomic on all
        // supported platforms. A torn/stale value only causes a harmless
        // false-positive (we take the lock when we didn't need to).
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

/// Cached section counter traits for one block state.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct BlockStateSectionCounts {
    is_air: bool,
    has_fluid: bool,
    randomly_ticking: bool,
}

const BLOCKS_PER_SECTION: u16 = 16 * 16 * 16;

static BLOCK_STATE_SECTION_COUNTS: LazyLock<Box<[BlockStateSectionCounts]>> = LazyLock::new(|| {
    let mut counts = Vec::with_capacity(REGISTRY.blocks.state_to_block_lookup.len());
    for state_index in 0..REGISTRY.blocks.state_to_block_lookup.len() {
        let Ok(raw_state_id) = u16::try_from(state_index) else {
            panic!("block state registry exceeded BlockStateId range");
        };
        counts.push(ChunkSection::block_state_section_counts_with(
            BlockStateId(raw_state_id),
            &BLOCK_BEHAVIORS,
        ));
    }
    counts.into_boxed_slice()
});

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
    /// unused old-value load. `recalculate_counts_with` finalizes.
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

    /// Recalculates cached counters from the global per-state counter table.
    ///
    /// This should be called after chunk loading or generation to initialize
    /// the counters. It requires the block behavior registry to be initialized.
    ///
    /// # Panics
    /// Panics if the block behavior registry has not been initialized.
    pub fn recalculate_counts(&mut self) {
        self.recalculate_counts_from_palette(Self::block_state_section_counts);
    }

    /// Recalculates all cached counters using the provided behavior registry.
    ///
    /// Iterates the palette (`O(palette_size)`) rather than every cube cell
    /// (`O(4096)`): each block-state appears at most once in the palette and
    /// carries its own occurrence count, so we just classify each unique state
    /// and multiply by its count. Mirrors Moonrise's `BlockCountingBitStorage`.
    /// For a `Homogeneous` section that's a single classify; for typical
    /// `Heterogeneous` sections palette is well under 16 entries.
    pub fn recalculate_counts_with(&mut self, block_behaviors: &BlockBehaviorRegistry) {
        self.recalculate_counts_from_palette(|state| {
            Self::block_state_section_counts_with(state, block_behaviors)
        });
    }

    fn recalculate_counts_from_palette(
        &mut self,
        mut counts_for_state: impl FnMut(BlockStateId) -> BlockStateSectionCounts,
    ) {
        self.states.finalize_building();

        let mut non_empty: u16 = 0;
        let mut fluid: u16 = 0;
        let mut ticking: u16 = 0;

        match &self.states {
            BlockPalette::Homogeneous(state) => {
                let counts = counts_for_state(*state);
                Self::accumulate_counter_traits(
                    &mut non_empty,
                    &mut fluid,
                    &mut ticking,
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
                        &mut ticking,
                        counts,
                        count,
                    );
                }
            }
            BlockPalette::Building(_) => unreachable!("finalize_building was just called"),
        }

        self.non_empty_block_count = non_empty;
        self.fluid_count = fluid;
        self.ticking_block_count = ticking;
    }

    const fn accumulate_counter_traits(
        non_empty: &mut u16,
        fluid: &mut u16,
        ticking: &mut u16,
        counts: BlockStateSectionCounts,
        block_count: u16,
    ) {
        if !counts.is_air {
            *non_empty += block_count;
        }
        if counts.has_fluid {
            *fluid += block_count;
        }
        if counts.randomly_ticking {
            *ticking += block_count;
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
        self.ensure_counter_ready_for_delta_with(block_behaviors);
        let old_state = self.states.set(x, y, z, new_state);

        if old_state != new_state {
            let old_counts = Self::block_state_section_counts_with(old_state, block_behaviors);
            let new_counts = Self::block_state_section_counts_with(new_state, block_behaviors);
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

    /// Returns the cached-counter traits for a block state using the global
    /// behavior registry.
    pub(crate) fn block_state_section_counts(state: BlockStateId) -> BlockStateSectionCounts {
        let Some(&counts) = BLOCK_STATE_SECTION_COUNTS.get(state.0 as usize) else {
            panic!("invalid block state id {}", state.0);
        };
        counts
    }

    pub(crate) fn finalize_generation_counts_if_needed(&mut self) {
        if matches!(&self.states, BlockPalette::Building(_)) {
            self.recalculate_counts();
        }
    }

    fn ensure_counter_ready_for_delta_with(&mut self, block_behaviors: &BlockBehaviorRegistry) {
        if matches!(&self.states, BlockPalette::Building(_)) {
            log::debug!(
                "finalizing worldgen Building palette before applying a counter-aware \
                 block-state delta"
            );
            self.recalculate_counts_with(block_behaviors);
        }
    }

    fn block_state_section_counts_with(
        state: BlockStateId,
        block_behaviors: &BlockBehaviorRegistry,
    ) -> BlockStateSectionCounts {
        let behavior = block_behaviors.get_behavior(state.get_block());
        BlockStateSectionCounts {
            is_air: state.is_air(),
            has_fluid: !behavior.get_fluid_state(state).is_empty(),
            randomly_ticking: behavior.is_randomly_ticking(state),
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

        if old_counts.randomly_ticking && !new_counts.randomly_ticking {
            self.ticking_block_count -= 1;
        } else if !old_counts.randomly_ticking && new_counts.randomly_ticking {
            self.ticking_block_count += 1;
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
