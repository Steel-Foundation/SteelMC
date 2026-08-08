use std::marker::PhantomData;

use steel_utils::{BlockPos, BlockStateId, ChunkPos, DowncastType, types::UpdateFlags};

use crate::chunk::Chunk;
use crate::chunk::chunk_holder::ChunkHolder;
use crate::chunk::heightmap::{Heightmap, HeightmapType};
use crate::chunk::status::ChunkStatus;
use crate::worldgen::carving_mask::CarvingMask;

/// Marker for the Noise generation operation.
pub enum NoisePhase {}

/// Marker for the Surface generation operation.
pub enum SurfacePhase {}

/// Marker for the Carvers generation operation.
pub enum CarversPhase {}

/// Stage-scoped access to the center chunk being generated.
///
/// The scheduler constructs this capability only after the chunk's input status is
/// published. It deliberately does not expose the underlying [`Chunk`] or its raw
/// section storage, so status-sensitive writes stay tied to the operation that owns
/// them.
///
/// ```compile_fail
/// use steel_core::worldgen::generator::{GenerationChunk, NoisePhase};
///
/// fn write_surface_column_during_noise(chunk: GenerationChunk<'_, NoisePhase>) {
///     chunk.write_column(0, 0, &[]);
/// }
/// ```
#[repr(transparent)]
pub struct GenerationChunk<'a, Phase> {
    chunk: &'a Chunk,
    phase: PhantomData<fn() -> Phase>,
}

impl<Phase> Copy for GenerationChunk<'_, Phase> {}

impl<Phase> Clone for GenerationChunk<'_, Phase> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<'a, Phase> GenerationChunk<'a, Phase> {
    const fn from_chunk(chunk: &'a Chunk) -> Self {
        Self {
            chunk,
            phase: PhantomData,
        }
    }

    #[cfg(test)]
    pub(crate) const fn for_test(chunk: &'a Chunk) -> Self {
        Self::from_chunk(chunk)
    }

    fn acquire_input(holder: &'a ChunkHolder, input_status: ChunkStatus) -> Self {
        assert_eq!(
            holder.published_status(),
            Some(input_status),
            "generation capability requires the exact published input status"
        );
        let Some(chunk) = holder.try_chunk(input_status) else {
            panic!("chunk data missing after {input_status:?} publication");
        };
        Self::from_chunk(chunk)
    }

    /// Returns the chunk position.
    #[must_use]
    pub const fn pos(self) -> ChunkPos {
        self.chunk.pos()
    }

    /// Returns the dimension's minimum block Y.
    #[must_use]
    pub const fn min_y(self) -> i32 {
        self.chunk.min_y()
    }

    /// Returns the dimension's build height.
    #[must_use]
    pub const fn height(self) -> i32 {
        self.chunk.height()
    }

    /// Returns the number of vertical chunk sections.
    #[must_use]
    #[inline]
    pub fn section_count(self) -> usize {
        self.chunk.sections().sections.len()
    }

    /// Reads one block using coordinates relative to the dimension's minimum Y.
    #[must_use]
    #[inline]
    pub fn get_relative_block(
        self,
        relative_x: usize,
        relative_y: usize,
        relative_z: usize,
    ) -> Option<BlockStateId> {
        self.chunk
            .get_relative_block(relative_x, relative_y, relative_z)
    }

    /// Reads one block at a world position.
    #[must_use]
    #[inline]
    pub fn get_block_state(self, pos: BlockPos) -> BlockStateId {
        self.chunk.get_block_state(pos)
    }
}

impl GenerationChunk<'_, NoisePhase> {
    pub(crate) fn acquire(holder: &ChunkHolder) -> GenerationChunk<'_, NoisePhase> {
        GenerationChunk::acquire_input(holder, ChunkStatus::Biomes)
    }

    /// Writes a Noise-stage block batch using the published Biomes semantics.
    #[inline]
    pub fn write_block_batch(self, blocks: &[(usize, usize, usize, BlockStateId)]) {
        self.chunk
            .write_block_batch_for_generation(ChunkStatus::Biomes, blocks);
    }

    /// Replaces the two heightmaps initialized directly by terrain noise filling.
    ///
    /// # Panics
    ///
    /// Panics if either heightmap has a type other than its corresponding noise-stage type.
    pub fn replace_noise_heightmaps(self, ocean_floor: Heightmap, world_surface: Heightmap) {
        assert_eq!(ocean_floor.heightmap_type(), HeightmapType::OceanFloorWg);
        assert_eq!(
            world_surface.heightmap_type(),
            HeightmapType::WorldSurfaceWg
        );
        let mut heightmaps = self.chunk.heightmaps.write();
        heightmaps.replace(ocean_floor);
        heightmaps.replace(world_surface);
    }

    /// Installs generator-owned state for reuse by Surface and Carvers.
    pub fn install_post_noise_state<T>(self, state: T)
    where
        T: DowncastType + Send + Sync,
    {
        self.chunk.install_transient_generation_state(state);
    }

    /// Marks a position for Vanilla generation postprocessing.
    #[inline]
    pub fn mark_pos_for_postprocessing(self, pos: BlockPos) {
        self.chunk.mark_pos_for_postprocessing(pos);
    }

    /// Writes one block using the published Biomes semantics.
    #[inline]
    pub fn set_relative_block(
        self,
        relative_x: usize,
        relative_y: usize,
        relative_z: usize,
        state: BlockStateId,
    ) {
        self.chunk.set_relative_block_for_generation(
            ChunkStatus::Biomes,
            relative_x,
            relative_y,
            relative_z,
            state,
        );
    }
}

impl GenerationChunk<'_, SurfacePhase> {
    pub(crate) fn acquire(holder: &ChunkHolder) -> GenerationChunk<'_, SurfacePhase> {
        GenerationChunk::acquire_input(holder, ChunkStatus::Noise)
    }

    /// Ensures the world-surface worldgen heightmap exists.
    pub fn prime_world_surface_heightmap(self) {
        self.chunk
            .prime_heightmaps(&[HeightmapType::WorldSurfaceWg]);
    }

    /// Borrows generator-owned state retained after Noise.
    pub fn with_post_noise_state_mut<T, R>(self, f: impl FnOnce(&mut T) -> R) -> Option<R>
    where
        T: DowncastType + Send + Sync,
    {
        self.chunk.with_transient_generation_state_mut(f)
    }

    /// Copies every biome palette entry in section order.
    #[must_use]
    pub fn read_all_biomes(self) -> Box<[u16]> {
        self.chunk.sections().read_all_biomes()
    }

    /// Reads one complete block column into `output`.
    #[inline]
    pub fn read_column_into(self, local_x: usize, local_z: usize, output: &mut Vec<BlockStateId>) {
        self.chunk
            .sections()
            .read_column_into(local_x, local_z, output);
    }

    /// Reads the world-surface worldgen height, lazily priming it if needed.
    #[must_use]
    #[inline]
    pub fn world_surface_height_at(self, local_x: usize, local_z: usize) -> i32 {
        self.chunk
            .generation_height_at(HeightmapType::WorldSurfaceWg, local_x, local_z)
    }

    /// Writes one Surface-stage column and applies its Noise-input heightmap effects.
    #[inline]
    pub fn write_column(self, local_x: usize, local_z: usize, blocks: &[(usize, BlockStateId)]) {
        self.chunk
            .write_column_blocks_for_generation(ChunkStatus::Noise, local_x, local_z, blocks);
        self.chunk.update_heightmaps_after_direct_column_writes(
            ChunkStatus::Noise,
            local_x,
            local_z,
            blocks,
        );
        self.chunk.mark_dirty();
    }

    /// Writes one block using the published Noise semantics.
    #[inline]
    pub fn set_relative_block(
        self,
        relative_x: usize,
        relative_y: usize,
        relative_z: usize,
        state: BlockStateId,
    ) {
        self.chunk.set_relative_block_for_generation(
            ChunkStatus::Noise,
            relative_x,
            relative_y,
            relative_z,
            state,
        );
    }
}

impl GenerationChunk<'_, CarversPhase> {
    pub(crate) fn acquire(holder: &ChunkHolder) -> GenerationChunk<'_, CarversPhase> {
        GenerationChunk::acquire_input(holder, ChunkStatus::Surface)
    }

    /// Ensures the world-surface worldgen heightmap exists.
    pub fn prime_world_surface_heightmap(self) {
        self.chunk
            .prime_heightmaps(&[HeightmapType::WorldSurfaceWg]);
    }

    /// Drops retained generator state without running Carvers.
    pub fn clear_post_noise_state(self) {
        self.chunk.clear_transient_generation_state();
    }

    /// Consumes generator-owned state retained after Noise.
    pub fn consume_post_noise_state<T, R>(self, f: impl FnOnce(Option<&mut T>) -> R) -> R
    where
        T: DowncastType + Send + Sync,
    {
        self.chunk.consume_transient_generation_state(f)
    }

    /// Runs `f` with the chunk's lazily initialized carving mask.
    pub fn with_carving_mask<R>(self, f: impl FnOnce(&mut CarvingMask) -> R) -> R {
        let mut mask = self.chunk.get_or_create_carving_mask();
        f(&mut mask)
    }

    /// Sets one carved block using the published Surface semantics.
    #[inline]
    pub fn set_block_state(self, pos: BlockPos, state: BlockStateId) {
        let _ = self.chunk.set_block_state_for_generation(
            ChunkStatus::Surface,
            pos,
            state,
            UpdateFlags::empty(),
        );
    }

    /// Runs `f` with the world-surface worldgen heightmap if priming provides it.
    pub fn with_world_surface_heightmap<R>(self, f: impl FnOnce(&Heightmap) -> R) -> Option<R> {
        let heightmaps = self.chunk.generation_heightmaps();
        if let Some(heightmap) = heightmaps.get(HeightmapType::WorldSurfaceWg) {
            return Some(f(heightmap));
        }
        drop(heightmaps);

        self.prime_world_surface_heightmap();
        let heightmaps = self.chunk.generation_heightmaps();
        heightmaps.get(HeightmapType::WorldSurfaceWg).map(f)
    }

    /// Marks a position for Vanilla generation postprocessing.
    #[inline]
    pub fn mark_pos_for_postprocessing(self, pos: BlockPos) {
        self.chunk.mark_pos_for_postprocessing(pos);
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Weak};

    use steel_registry::init_vanilla_registry;
    use steel_utils::ChunkPos;

    use super::{GenerationChunk, NoisePhase, SurfacePhase};
    use crate::chunk::Chunk;
    use crate::chunk::chunk_holder::ChunkHolder;
    use crate::chunk::chunk_ticket_manager::ChunkTicketLevel;
    use crate::chunk::section::{ChunkSection, Sections};
    use crate::chunk::status::ChunkStatus;

    fn holder_at(status: ChunkStatus) -> Arc<ChunkHolder> {
        init_vanilla_registry();
        let pos = ChunkPos::new(0, 0);
        let holder = Arc::new(ChunkHolder::new(
            pos,
            ChunkTicketLevel::STRONGEST,
            None,
            0,
            16,
        ));
        let chunk = Chunk::new(
            Sections::from_owned(vec![ChunkSection::new_empty()].into_boxed_slice()),
            pos,
            0,
            16,
            Weak::new(),
        );
        holder.insert_chunk(chunk, status);
        holder
    }

    #[test]
    #[should_panic(expected = "generation capability requires the exact published input status")]
    fn capability_rejects_the_wrong_published_input() {
        let holder = holder_at(ChunkStatus::Biomes);
        let _ = GenerationChunk::<SurfacePhase>::acquire(&holder);
    }

    #[test]
    #[should_panic(expected = "generation capability requires the exact published input status")]
    fn capability_rejects_an_advanced_published_input() {
        let holder = holder_at(ChunkStatus::Surface);
        let _ = GenerationChunk::<NoisePhase>::acquire(&holder);
    }
}

#[cfg(feature = "benchmark-support")]
/// Direct generator calls used by Criterion benchmarks.
pub mod benchmark_support {
    use glam::IVec3;
    use steel_worldgen::noise::Beardifier;

    use super::{CarversPhase, GenerationChunk, NoisePhase, SurfacePhase};
    use crate::chunk::Chunk;
    use crate::worldgen::generator::ChunkGenerator;

    /// Calls Noise directly for a Criterion benchmark.
    pub fn fill_from_noise<G>(generator: &G, chunk: &Chunk, beardifier: Option<&Beardifier>)
    where
        G: ChunkGenerator + ?Sized,
    {
        generator.fill_from_noise(GenerationChunk::<NoisePhase>::from_chunk(chunk), beardifier);
    }

    /// Calls Surface directly for a Criterion benchmark.
    pub fn build_surface<G>(generator: &G, chunk: &Chunk, neighbor_biomes: &dyn Fn(IVec3) -> u16)
    where
        G: ChunkGenerator + ?Sized,
    {
        generator.build_surface(
            GenerationChunk::<SurfacePhase>::from_chunk(chunk),
            neighbor_biomes,
        );
    }

    /// Calls Carvers directly for a Criterion benchmark.
    pub fn apply_carvers<G>(generator: &G, chunk: &Chunk)
    where
        G: ChunkGenerator + ?Sized,
    {
        generator.apply_carvers(GenerationChunk::<CarversPhase>::from_chunk(chunk));
    }
}
