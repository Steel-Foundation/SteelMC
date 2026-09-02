//! Chunk data retained from generation through Full runtime use.
use std::fmt::{self, Formatter};
use std::sync::{
    Arc, OnceLock, Weak,
    atomic::{AtomicBool, Ordering},
};

use parking_lot::{MappedRwLockWriteGuard, RwLockReadGuard, RwLockWriteGuard};
use rustc_hash::FxHashMap;
use steel_registry::{
    REGISTRY,
    blocks::{BlockRef, block_state_ext::BlockStateExt},
    fluid::FluidRef,
    vanilla_blocks,
};
use steel_utils::{
    BlockPos, BlockStateId, ChunkPos, Downcast as _, DowncastType, ErasedType, SectionPos,
    locks::{SyncMutex, SyncRwLock},
    types::UpdateFlags,
};

use crate::behavior::{BLOCK_BEHAVIORS, BlockEntityCreation};
use crate::block_entity::{BlockEntityLookup, BlockEntityStorage, SharedBlockEntity};
use crate::chunk::{
    full_chunk::FullChunkRuntime,
    heightmap::{ChunkHeightmaps, HeightmapType},
    light::{
        ChunkLightData, ChunkSkyLightSources, LightSectionEmptinessChange,
        has_different_light_properties,
    },
    section::Sections,
    status::ChunkStatus,
};
use crate::entity::{EntityStorage, EntityStorageAddResult, SharedEntity};
use crate::world::World;
use crate::world::tick_scheduler::{
    BlockTickList, ChunkTickContainer, ChunkTickLists, FluidTickList, TickPriority,
};
use crate::worldgen::carving_mask::CarvingMask;
use steel_worldgen::structure::{StructureReferenceMap, StructureStartMap};

pub(crate) fn empty_postprocessing(height: i32) -> Box<[Vec<u16>]> {
    let section_count = (height / 16) as usize;
    (0..section_count).map(|_| Vec::new()).collect()
}

pub(crate) fn postprocessing_from_disk(
    height: i32,
    mut postprocessing: Vec<Vec<u16>>,
) -> Box<[Vec<u16>]> {
    let section_count = (height / 16) as usize;
    postprocessing.resize_with(section_count, Vec::new);
    postprocessing.truncate(section_count);
    postprocessing.into_boxed_slice()
}

#[derive(Default)]
struct TransientGenerationState {
    value: Option<Box<dyn ErasedType + Send + Sync>>,
}

impl fmt::Debug for TransientGenerationState {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("TransientGenerationState")
            .field(
                "type_key",
                &self.value.as_deref().map(ErasedType::downcast_type_key),
            )
            .finish()
    }
}

/// Canonical chunk storage retained across generation and Full runtime phases.
#[derive(Debug)]
pub struct Chunk {
    /// The sections of the chunk.
    pub sections: Sections,
    /// The position of the chunk.
    pub pos: ChunkPos,
    /// Whether the chunk has been modified since last save.
    /// Newly generated chunks start dirty.
    pub dirty: AtomicBool,
    /// Heightmaps retained across every generation phase.
    pub(crate) heightmaps: SyncRwLock<ChunkHeightmaps>,
    /// The minimum Y coordinate of the world this chunk belongs to.
    min_y: i32,
    /// The total height of the world.
    height: i32,
    /// Weak reference to the owning world.
    level: Weak<World>,
    /// Stable block-entity storage retained through Full promotion.
    pub(crate) block_entities: BlockEntityStorage,
    /// Entity staging storage closed and drained at Full promotion.
    pub(crate) entities: EntityStorage,
    /// Structure starts originating in this chunk.
    pub structure_starts: SyncRwLock<StructureStartMap>,
    /// References to structures from nearby origin chunks.
    pub structure_references: SyncRwLock<StructureReferenceMap>,
    /// Bitset of positions visited by carvers (lazily initialized).
    pub carving_mask: SyncRwLock<Option<CarvingMask>>,
    /// Section-indexed packed offsets that need vanilla postprocessing after promotion.
    pub postprocessing: SyncMutex<Box<[Vec<u16>]>>,
    /// Stable block and fluid scheduled-tick storage retained through Full promotion.
    pub(crate) scheduled_ticks: Arc<ChunkTickContainer>,
    /// Vanilla skylight source edge cache for this chunk.
    pub sky_light_sources: SyncRwLock<ChunkSkyLightSources>,
    /// Chunk-owned light sections and section emptiness maps.
    pub light: SyncRwLock<ChunkLightData>,
    /// Full-only runtime state, installed once before Full publication.
    full_runtime: OnceLock<Box<FullChunkRuntime>>,
    /// Generator-owned state retained only between generation stages.
    transient_generation_state: SyncMutex<TransientGenerationState>,
}

enum PendingPromotionCommit {
    Retry,
    Complete(Option<SharedBlockEntity>),
}

impl Chunk {
    /// Creates a new proto chunk at the given position with empty sections.
    #[must_use]
    pub fn new(
        sections: Sections,
        pos: ChunkPos,
        min_y: i32,
        height: i32,
        level: Weak<World>,
    ) -> Self {
        Self {
            sections,
            pos,
            dirty: AtomicBool::new(true), // New chunks are always dirty
            heightmaps: SyncRwLock::new(ChunkHeightmaps::empty()),
            min_y,
            height,
            level,
            block_entities: BlockEntityStorage::new(),
            entities: EntityStorage::new(),
            structure_starts: SyncRwLock::new(FxHashMap::default()),
            structure_references: SyncRwLock::new(FxHashMap::default()),
            carving_mask: SyncRwLock::new(None),
            postprocessing: SyncMutex::new(empty_postprocessing(height)),
            scheduled_ticks: Arc::new(ChunkTickContainer::new_proto(ChunkTickLists::new(
                BlockTickList::new_pending(),
                FluidTickList::new_pending(),
            ))),
            sky_light_sources: SyncRwLock::new(ChunkSkyLightSources::for_valid_world_height(
                min_y, height,
            )),
            light: SyncRwLock::new(ChunkLightData::for_valid_world_height(min_y, height)),
            full_runtime: OnceLock::new(),
            transient_generation_state: SyncMutex::new(TransientGenerationState::default()),
        }
    }

    /// Creates a chunk that was loaded from disk.
    ///
    /// # Panics
    ///
    /// Panics when persisted light data does not match the loaded section range.
    #[expect(
        clippy::too_many_arguments,
        reason = "disk rehydration mirrors the persisted proto chunk fields"
    )]
    #[must_use]
    pub(crate) fn from_disk(
        sections: Sections,
        pos: ChunkPos,
        status: ChunkStatus,
        min_y: i32,
        height: i32,
        heightmaps: ChunkHeightmaps,
        structure_starts: StructureStartMap,
        structure_references: StructureReferenceMap,
        carving_mask: Option<CarvingMask>,
        postprocessing: Vec<Vec<u16>>,
        block_ticks: BlockTickList,
        fluid_ticks: FluidTickList,
        level: Weak<World>,
        mut light: ChunkLightData,
    ) -> Self {
        if let Err(error) = light.refresh_emptiness_maps_from_sections(&sections) {
            panic!("invalid loaded proto chunk light emptiness map length: {error:?}");
        }

        let chunk = Self {
            sections,
            pos,
            dirty: AtomicBool::new(false),
            heightmaps: SyncRwLock::new(heightmaps),
            min_y,
            height,
            level,
            block_entities: BlockEntityStorage::new(),
            entities: if status == ChunkStatus::Full {
                EntityStorage::new_closed()
            } else {
                EntityStorage::new()
            },
            structure_starts: SyncRwLock::new(structure_starts),
            structure_references: SyncRwLock::new(structure_references),
            carving_mask: SyncRwLock::new(carving_mask),
            postprocessing: SyncMutex::new(postprocessing_from_disk(height, postprocessing)),
            scheduled_ticks: Arc::new(if status == ChunkStatus::Full {
                ChunkTickContainer::new(ChunkTickLists::new(block_ticks, fluid_ticks))
            } else {
                ChunkTickContainer::new_proto(ChunkTickLists::new(block_ticks, fluid_ticks))
            }),
            sky_light_sources: SyncRwLock::new(ChunkSkyLightSources::for_valid_world_height(
                min_y, height,
            )),
            light: SyncRwLock::new(light),
            full_runtime: OnceLock::new(),
            transient_generation_state: SyncMutex::new(TransientGenerationState::default()),
        };

        if status >= ChunkStatus::InitializeLight {
            chunk.initialize_light_sources();
        }

        chunk
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

    /// Returns the chunk position.
    #[must_use]
    pub const fn pos(&self) -> ChunkPos {
        self.pos
    }

    /// Returns the stable section storage.
    #[must_use]
    pub const fn sections(&self) -> &Sections {
        &self.sections
    }

    /// Returns whether this chunk has unsaved changes.
    #[must_use]
    pub fn is_dirty(&self) -> bool {
        self.dirty.load(Ordering::Acquire)
    }

    /// Marks this chunk as needing persistence.
    pub fn mark_dirty(&self) {
        self.dirty.store(true, Ordering::Release);
    }

    /// Clears the dirty flag and returns its previous value.
    pub fn take_dirty(&self) -> bool {
        self.dirty.swap(false, Ordering::AcqRel)
    }

    /// Clears the dirty flag.
    pub fn clear_dirty(&self) {
        self.dirty.store(false, Ordering::Release);
    }

    /// Reads a block using coordinates relative to the chunk's minimum Y.
    #[must_use]
    pub fn get_relative_block(
        &self,
        relative_x: usize,
        relative_y: usize,
        relative_z: usize,
    ) -> Option<BlockStateId> {
        self.sections
            .get_relative_block(relative_x, relative_y, relative_z)
    }

    /// Writes one generation block while preserving the current stage's side effects.
    pub(crate) fn set_relative_block_for_generation(
        &self,
        status: ChunkStatus,
        relative_x: usize,
        relative_y: usize,
        relative_z: usize,
        value: BlockStateId,
    ) {
        if status >= ChunkStatus::InitializeLight {
            self.sections
                .set_relative_block(relative_x, relative_y, relative_z, value);
            self.refresh_light_emptiness_maps();
        } else {
            self.sections
                .set_relative_block_for_generation(relative_x, relative_y, relative_z, value);
        }
        self.mark_dirty();
        self.update_status_heightmaps_after_block_change(
            status,
            relative_x,
            self.min_y + relative_y as i32,
            relative_z,
            value,
        );
    }

    /// Writes a batch of generation blocks.
    pub(crate) fn write_block_batch_for_generation(
        &self,
        status: ChunkStatus,
        blocks: &[(usize, usize, usize, BlockStateId)],
    ) {
        if blocks.is_empty() {
            return;
        }
        if status < ChunkStatus::InitializeLight {
            self.sections.write_block_batch(blocks);
        } else {
            self.sections.write_tracked_block_batch(blocks);
            self.refresh_light_emptiness_maps();
        }
        self.mark_dirty();
    }

    /// Writes one column of generation blocks.
    pub(crate) fn write_column_blocks_for_generation(
        &self,
        status: ChunkStatus,
        x: usize,
        z: usize,
        blocks: &[(usize, BlockStateId)],
    ) {
        if blocks.is_empty() {
            return;
        }
        if status < ChunkStatus::InitializeLight {
            self.sections.write_column_blocks(x, z, blocks);
        } else {
            for &(relative_y, value) in blocks {
                self.sections.set_relative_block(x, relative_y, z, value);
            }
            self.refresh_light_emptiness_maps();
        }
        self.mark_dirty();
    }

    /// Ensures the requested generation heightmaps exist.
    pub(crate) fn prime_heightmaps(&self, heightmap_types: &[HeightmapType]) {
        self.heightmaps.write().prime_from_sections(
            heightmap_types,
            self.min_y,
            self.height,
            &self.sections.sections,
        );
    }

    /// Ensures every final heightmap exists before feature generation or promotion.
    pub fn prime_final_heightmaps(&self) {
        self.prime_heightmaps(HeightmapType::final_types());
    }

    /// Reads a generation heightmap, priming it lazily when needed.
    #[must_use]
    pub(crate) fn generation_height_at(
        &self,
        heightmap_type: HeightmapType,
        local_x: usize,
        local_z: usize,
    ) -> i32 {
        {
            let heightmaps = self.heightmaps.read();
            if let Some(heightmap) = heightmaps.get(heightmap_type) {
                return heightmap.get_first_available(local_x, local_z);
            }
        }
        self.prime_heightmaps(&[heightmap_type]);
        let heightmaps = self.heightmaps.read();
        let Some(heightmap) = heightmaps.get(heightmap_type) else {
            panic!("heightmap {heightmap_type:?} missing after priming");
        };
        heightmap.get_first_available(local_x, local_z)
    }

    /// Returns the generation heightmaps retained by this chunk.
    pub(crate) fn generation_heightmaps(&self) -> RwLockReadGuard<'_, ChunkHeightmaps> {
        self.heightmaps.read()
    }

    /// Applies generation heightmap maintenance after direct writes in one column.
    pub(crate) fn update_heightmaps_after_direct_column_writes(
        &self,
        status: ChunkStatus,
        local_x: usize,
        local_z: usize,
        relative_writes: &[(usize, BlockStateId)],
    ) {
        if relative_writes.is_empty() {
            return;
        }
        self.update_status_heightmaps_after_column_block_changes(
            status,
            local_x,
            local_z,
            relative_writes,
        );
    }

    /// Returns a read guard for the skylight-source cache.
    pub fn sky_light_sources(&self) -> RwLockReadGuard<'_, ChunkSkyLightSources> {
        self.sky_light_sources.read()
    }

    /// Returns every block position that emits light in this chunk.
    #[must_use]
    pub fn block_light_sources(&self) -> Vec<BlockPos> {
        self.sections.block_light_sources(self.pos, self.min_y)
    }

    /// Returns committed chunk light data.
    pub fn light(&self) -> RwLockReadGuard<'_, ChunkLightData> {
        self.light.read()
    }

    /// Returns mutable committed chunk light data.
    pub(crate) fn light_mut(&self) -> RwLockWriteGuard<'_, ChunkLightData> {
        self.light.write()
    }

    /// Returns structure starts originating in this chunk.
    pub fn structure_starts(&self) -> RwLockReadGuard<'_, StructureStartMap> {
        self.structure_starts.read()
    }

    /// Returns mutable structure starts originating in this chunk.
    pub fn structure_starts_mut(&self) -> RwLockWriteGuard<'_, StructureStartMap> {
        self.structure_starts.write()
    }

    /// Returns references to nearby structures.
    pub fn structure_references(&self) -> RwLockReadGuard<'_, StructureReferenceMap> {
        self.structure_references.read()
    }

    /// Returns mutable references to nearby structures.
    pub fn structure_references_mut(&self) -> RwLockWriteGuard<'_, StructureReferenceMap> {
        self.structure_references.write()
    }

    /// Installs Full-only runtime state before this chunk is published as Full.
    pub(crate) fn initialize_full_runtime(
        &self,
        runtime: FullChunkRuntime,
    ) -> Result<(), FullChunkRuntime> {
        self.full_runtime
            .set(Box::new(runtime))
            .map_err(|runtime| *runtime)
    }

    /// Returns the Full-only runtime state after promotion or Full disk construction.
    #[must_use]
    pub(crate) fn full_runtime(&self) -> Option<&FullChunkRuntime> {
        self.full_runtime.get().map(Box::as_ref)
    }

    /// Installs generator-owned state for reuse by later generation stages.
    ///
    /// # Panics
    ///
    /// Panics if the current generator has already installed transient state.
    pub(crate) fn install_transient_generation_state<T>(&self, state: T)
    where
        T: DowncastType + Send + Sync,
    {
        let mut slot = self.transient_generation_state.lock();
        if let Some(current) = slot.value.as_deref() {
            panic!(
                "chunk transient generation state {} was not consumed before installing {}",
                current.downcast_type_key(),
                T::TYPE_KEY
            );
        }
        slot.value = Some(Box::new(state));
    }

    /// Borrows generator-owned state without extending its lifetime beyond the callback.
    ///
    /// Returns `None` when no state was installed, such as after reloading a partially
    /// generated chunk from disk.
    ///
    /// # Panics
    ///
    /// Panics if another generator's state occupies this chunk.
    pub(crate) fn with_transient_generation_state_mut<T, R>(
        &self,
        f: impl FnOnce(&mut T) -> R,
    ) -> Option<R>
    where
        T: DowncastType + Send + Sync,
    {
        let mut slot = self.transient_generation_state.lock();
        let state = slot.value.as_deref_mut()?;
        let actual_key = state.downcast_type_key();
        let Some(state) = state.downcast_mut::<T>() else {
            panic!(
                "chunk transient generation state type mismatch: expected {}, found {}",
                T::TYPE_KEY,
                actual_key
            );
        };
        Some(f(state))
    }

    /// Removes generator-owned state and passes it to `f` before dropping it.
    ///
    /// The erased allocation is moved out before the callback, so it is also dropped if
    /// the callback unwinds. `None` represents a cold continuation loaded from disk.
    ///
    /// # Panics
    ///
    /// Panics if another generator's state occupies this chunk.
    pub(crate) fn consume_transient_generation_state<T, R>(
        &self,
        f: impl FnOnce(Option<&mut T>) -> R,
    ) -> R
    where
        T: DowncastType + Send + Sync,
    {
        let state = self.transient_generation_state.lock().value.take();
        let Some(mut state) = state else {
            return f(None);
        };
        let actual_key = state.downcast_type_key();
        let Some(state) = state.downcast_mut::<T>() else {
            panic!(
                "chunk transient generation state type mismatch: expected {}, found {}",
                T::TYPE_KEY,
                actual_key
            );
        };
        f(Some(state))
    }

    /// Drops any generator-owned state that is no longer needed by the pipeline.
    pub(crate) fn clear_transient_generation_state(&self) {
        self.transient_generation_state.lock().value = None;
    }

    /// Returns a write guard to this chunk's carving mask, initializing it on
    /// first access. Mirrors vanilla's `ProtoChunk.getOrCreateCarvingMask`.
    ///
    /// # Panics
    /// Never — the mask is populated immediately before projecting the guard.
    pub(crate) fn get_or_create_carving_mask(&self) -> MappedRwLockWriteGuard<'_, CarvingMask> {
        let mut guard = self.carving_mask.write();
        if guard.is_none() {
            *guard = Some(CarvingMask::new(self.height, self.min_y));
        }
        RwLockWriteGuard::map(guard, |opt| match opt {
            Some(mask) => mask,
            None => unreachable!("carving mask initialized immediately above"),
        })
    }

    /// Vanilla `ProtoChunk.packOffsetCoordinates` for postprocessing offsets.
    #[must_use]
    pub const fn pack_postprocessing_offset(pos: BlockPos) -> u16 {
        let x = (pos.0.x & 15) as u16;
        let y = (pos.0.y & 15) as u16;
        let z = (pos.0.z & 15) as u16;
        x | (y << 4) | (z << 8)
    }

    /// Vanilla `ProtoChunk.unpackOffsetCoordinates` for postprocessing offsets.
    #[must_use]
    pub fn unpack_postprocessing_offset(
        packed: u16,
        section_y: i32,
        chunk_pos: ChunkPos,
    ) -> BlockPos {
        let x = chunk_pos.0.x * 16 + i32::from(packed & 15);
        let y = section_y * 16 + i32::from((packed >> 4) & 15);
        let z = chunk_pos.0.y * 16 + i32::from((packed >> 8) & 15);
        BlockPos::new(x, y, z)
    }

    /// Marks a block position for postprocessing after proto-to-full promotion.
    pub(crate) fn mark_pos_for_postprocessing(&self, pos: BlockPos) {
        let y = pos.0.y;
        if y < self.min_y || y >= self.min_y + self.height {
            return;
        }

        let section_index = self.get_section_index(y);
        let packed = Self::pack_postprocessing_offset(pos);
        self.postprocessing.lock()[section_index].push(packed);
        self.mark_unsaved();
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

    /// Returns the weak reference to the world.
    #[must_use]
    pub(crate) fn level_weak(&self) -> Weak<World> {
        self.level.clone()
    }

    /// Returns a reference to the world if it is still alive.
    #[must_use]
    pub(crate) fn get_level(&self) -> Option<Arc<World>> {
        self.level.upgrade()
    }

    /// Returns this chunk's block-entity storage.
    #[must_use]
    pub(crate) const fn block_entity_storage(&self) -> &BlockEntityStorage {
        &self.block_entities
    }

    /// Returns this chunk's stable scheduled-tick container.
    #[must_use]
    pub(crate) const fn scheduled_tick_container(&self) -> &Arc<ChunkTickContainer> {
        &self.scheduled_ticks
    }

    /// Fills the vanilla skylight-source cache from current section contents.
    pub fn initialize_light_sources(&self) {
        for section in &self.sections.sections {
            section.write().recalculate_counts();
        }
        self.refresh_light_emptiness_maps();
        self.sky_light_sources
            .write()
            .fill_from_sections(&self.sections);
    }

    /// Gets a block entity at the given position.
    #[must_use]
    pub(crate) fn get_block_entity(&self, pos: BlockPos) -> Option<SharedBlockEntity> {
        self.block_entities.get(pos)
    }

    /// Stores a concrete proto block entity if its type accepts the live block state.
    #[must_use]
    pub(crate) fn set_block_entity(&self, block_entity: SharedBlockEntity) -> bool {
        let pos = block_entity.get_block_pos();
        if ChunkPos::from_block_pos(pos) != self.pos {
            log::warn!(
                "Trying to set block entity {} at {pos:?} in proto chunk {:?}",
                block_entity.get_type().key,
                self.pos,
            );
            return false;
        }

        loop {
            let state = self.get_block_state(pos);
            let valid = state.has_block_entity() && block_entity.is_valid_block_state(state);
            if !valid {
                let state_unchanged =
                    self.with_locked_block_state(pos, |live_state| live_state == state);
                if !state_unchanged {
                    continue;
                }
                log::warn!(
                    "Trying to set block entity {} at {pos:?}, but block {} does not accept that type",
                    block_entity.get_type().key,
                    state.get_block().key,
                );
                return false;
            }

            let committed = self.with_locked_block_state(pos, |live_state| {
                if live_state != state {
                    return false;
                }
                let _ = self.block_entities.set_without_lifecycle(&block_entity);
                true
            });
            if !committed {
                continue;
            }
            self.mark_unsaved();
            return true;
        }
    }

    /// Stores Vanilla's pending `DUMMY` marker for a worldgen-placed entity block.
    pub(crate) fn set_pending_block_entity(&self, pos: BlockPos) {
        if ChunkPos::from_block_pos(pos) != self.pos {
            log::warn!(
                "Trying to set a pending block entity at {pos:?} in proto chunk {:?}",
                self.pos,
            );
            return;
        }
        if self.block_entities.set_pending(pos) {
            self.mark_unsaved();
        }
    }

    /// Stores a pending marker only while `expected_state` is still live.
    pub(crate) fn set_pending_block_entity_if_state(
        &self,
        pos: BlockPos,
        expected_state: BlockStateId,
    ) -> bool {
        if ChunkPos::from_block_pos(pos) != self.pos {
            return false;
        }
        let inserted = self.with_locked_block_state(pos, |live_state| {
            live_state == expected_state && self.block_entities.set_pending(pos)
        });
        if inserted {
            self.mark_unsaved();
        }
        inserted
    }

    /// Returns pending `DUMMY` positions for promotion or serialization.
    #[must_use]
    pub fn pending_block_entity_positions(&self) -> Vec<BlockPos> {
        self.block_entities.pending_positions()
    }

    /// Promotes a pending worldgen `DUMMY` on explicit region access without ticking it.
    pub(crate) fn promote_pending_block_entity(&self, pos: BlockPos) -> Option<SharedBlockEntity> {
        if ChunkPos::from_block_pos(pos) != self.pos {
            return None;
        }
        loop {
            match self.block_entities.lookup(pos) {
                BlockEntityLookup::Concrete(block_entity) => return Some(block_entity),
                BlockEntityLookup::Pending => {}
                BlockEntityLookup::Absent => return None,
            }

            let state = self.get_block_state(pos);
            if !state.has_block_entity() {
                let state_unchanged =
                    self.with_locked_block_state(pos, |live_state| live_state == state);
                if state_unchanged {
                    return None;
                }
                continue;
            }

            let behavior = BLOCK_BEHAVIORS.get_behavior(state.get_block());
            let creation = behavior.new_block_entity(self.level.clone(), pos, state);
            match self.commit_pending_creation(pos, state, creation) {
                PendingPromotionCommit::Retry => {}
                PendingPromotionCommit::Complete(block_entity) => return block_entity,
            }
        }
    }

    fn commit_pending_creation(
        &self,
        pos: BlockPos,
        expected_state: BlockStateId,
        creation: BlockEntityCreation,
    ) -> PendingPromotionCommit {
        let BlockEntityCreation::Created(block_entity) = creation else {
            // Proto chunks retain markers for both intentional null factories and Steel's
            // deferred implementations. Full promotion resolves their final semantics.
            return self.with_locked_block_state(pos, |live_state| {
                if live_state == expected_state {
                    PendingPromotionCommit::Complete(None)
                } else {
                    PendingPromotionCommit::Retry
                }
            });
        };
        let valid = block_entity.get_block_pos() == pos
            && ChunkPos::from_block_pos(pos) == self.pos
            && block_entity.is_valid_block_state(expected_state);
        self.with_locked_block_state(pos, |live_state| {
            if live_state != expected_state {
                return PendingPromotionCommit::Retry;
            }
            if !valid {
                return PendingPromotionCommit::Complete(None);
            }
            PendingPromotionCommit::Complete(
                self.block_entities
                    .promote_without_lifecycle(pos, block_entity),
            )
        })
    }

    /// Removes a block entity at the given position.
    pub(crate) fn remove_block_entity(&self, pos: BlockPos) {
        self.block_entities.remove_without_lifecycle(pos);
        self.mark_unsaved();
    }

    /// Removes an entity or marker only while `expected_state` is still live.
    pub(crate) fn remove_block_entity_if_state(
        &self,
        pos: BlockPos,
        expected_state: BlockStateId,
    ) -> bool {
        if ChunkPos::from_block_pos(pos) != self.pos {
            return false;
        }
        let removed = self.with_locked_block_state(pos, |live_state| {
            live_state == expected_state && self.block_entities.remove_without_lifecycle(pos)
        });
        if removed {
            self.mark_unsaved();
        }
        removed
    }

    /// Drops every block entity without Full-chunk lifecycle callbacks or dirtying.
    pub(crate) fn clear_all_block_entities(&self) {
        self.block_entities.clear_without_lifecycle();
    }

    /// Returns all block entities in this proto chunk.
    #[must_use]
    pub fn get_block_entities(&self) -> Vec<SharedBlockEntity> {
        self.block_entities.get_all_without_lifecycle_filter()
    }

    /// Adds an entity to proto storage.
    pub(crate) fn add_entity(&self, entity: SharedEntity) -> bool {
        match self.entities.add(entity) {
            EntityStorageAddResult::Staged => {
                self.mark_unsaved();
                true
            }
            EntityStorageAddResult::Closed(entity) => {
                // A retained worldgen reference becomes Vanilla's false-write
                // ImposterProtoChunk after promotion. Its addEntity call drops
                // the entity, while WorldGenRegion.addFreshEntity still reports
                // success.
                drop(entity);
                true
            }
        }
    }

    /// Returns all entities in this proto chunk.
    #[must_use]
    pub fn get_entities(&self) -> Vec<SharedEntity> {
        self.entities.get_all()
    }

    /// Returns entities that should be persisted from this proto chunk.
    #[must_use]
    pub(crate) fn get_saveable_entities(&self) -> Vec<SharedEntity> {
        self.entities.get_saveable_entities()
    }

    /// Schedules a block tick in proto storage.
    ///
    /// Vanilla `ProtoChunkTicks.schedule(ScheduledTick)` stores a saved tick with delay `0`,
    /// so worldgen-scheduled proto ticks run after promotion instead of preserving the
    /// requested delay from generation time.
    pub(crate) fn schedule_block_tick(
        &self,
        pos: BlockPos,
        block: BlockRef,
        priority: TickPriority,
    ) {
        if self
            .scheduled_ticks
            .schedule_pending_block(block, pos, priority)
            == Some(true)
        {
            self.mark_unsaved();
        }
    }

    /// Schedules a fluid tick in proto storage.
    ///
    /// See [`Self::schedule_block_tick`] for why proto ticks use delay `0`.
    pub(crate) fn schedule_fluid_tick(
        &self,
        pos: BlockPos,
        fluid: FluidRef,
        priority: TickPriority,
    ) {
        if self
            .scheduled_ticks
            .schedule_pending_fluid(fluid, pos, priority)
            == Some(true)
        {
            self.mark_unsaved();
        }
    }

    /// Sets a block state at the given position.
    ///
    /// Returns the old block state at the position, or `VOID_AIR` if out of bounds.
    pub(crate) fn set_block_state_for_generation(
        &self,
        status: ChunkStatus,
        pos: BlockPos,
        state: BlockStateId,
        _flags: UpdateFlags,
    ) -> Option<BlockStateId> {
        let y = pos.0.y;

        if y < self.min_y || y >= self.min_y + self.height {
            return Some(
                REGISTRY
                    .blocks
                    .get_default_state_id(&vanilla_blocks::VOID_AIR),
            );
        }

        let local_x = (pos.0.x & 15) as usize;
        let local_y = (y & 15) as usize;
        let local_z = (pos.0.z & 15) as usize;

        let section_index = self.get_section_index(y);
        let section = &self.sections.sections[section_index];
        let (old_state, empty_section_changed_to) = {
            let mut section_guard = section.write();
            if status >= ChunkStatus::InitializeLight {
                let was_empty = section_guard.is_empty();
                let old_state = section_guard.set_block_state(local_x, local_y, local_z, state);
                let is_empty = section_guard.is_empty();
                let empty_section_changed_to = (was_empty != is_empty).then_some(is_empty);
                (old_state, empty_section_changed_to)
            } else {
                (
                    section_guard.set_block_state_for_generation(local_x, local_y, local_z, state),
                    None,
                )
            }
        };

        if old_state == state {
            return None;
        }

        if status >= ChunkStatus::InitializeLight {
            let empty_section_change = empty_section_changed_to.map(|is_empty| {
                self.update_light_section_emptiness(y, is_empty);
                LightSectionEmptinessChange {
                    section_pos: SectionPos::new(
                        self.pos.0.x,
                        SectionPos::block_to_section_coord(y),
                        self.pos.0.y,
                    ),
                    empty: is_empty,
                }
            });

            let light_properties_changed = has_different_light_properties(old_state, state);
            if light_properties_changed {
                self.update_sky_light_sources(local_x, y, local_z);
            }
            if status >= ChunkStatus::Light
                && (light_properties_changed || empty_section_change.is_some())
                && let Some(level) = self.level.upgrade()
            {
                level.queue_light_change_after_block_set(
                    pos,
                    old_state,
                    state,
                    empty_section_change,
                );
            }
        }

        self.update_status_heightmaps_after_block_change(status, local_x, y, local_z, state);

        self.mark_unsaved();
        Some(old_state)
    }

    fn update_light_section_emptiness(&self, y: i32, is_empty: bool) {
        let section_y = SectionPos::block_to_section_coord(y);
        self.light.write().set_section_empty(section_y, is_empty);
    }

    fn update_sky_light_sources(&self, local_x: usize, y: i32, local_z: usize) {
        let chunk_min_x = self.pos.0.x * 16;
        let chunk_min_z = self.pos.0.y * 16;
        self.sky_light_sources
            .write()
            .update(local_x, y, local_z, |scan_x, scan_y, scan_z| {
                self.get_block_state(BlockPos::new(
                    chunk_min_x + scan_x as i32,
                    scan_y,
                    chunk_min_z + scan_z as i32,
                ))
            });
    }

    pub(crate) fn refresh_light_emptiness_maps(&self) {
        if let Err(error) = self
            .light
            .write()
            .refresh_emptiness_maps_from_sections(&self.sections)
        {
            panic!("invalid proto chunk light emptiness map length: {error:?}");
        }
    }

    /// Applies the heightmap side effect for an optimized direct section write.
    ///
    /// Use this only for generation paths that intentionally bypass
    /// [`Self::set_block_state`] but still need vanilla heightmap maintenance.
    pub(crate) fn update_status_heightmaps_after_block_change(
        &self,
        status: ChunkStatus,
        local_x: usize,
        y: i32,
        local_z: usize,
        state: BlockStateId,
    ) {
        self.update_heightmaps_after_block_change(
            status.heightmaps_after(),
            local_x,
            y,
            local_z,
            state,
        );
    }

    pub(crate) fn update_status_heightmaps_after_column_block_changes(
        &self,
        status: ChunkStatus,
        local_x: usize,
        local_z: usize,
        relative_writes: &[(usize, BlockStateId)],
    ) {
        self.update_heightmaps_after_column_block_changes(
            status.heightmaps_after(),
            local_x,
            local_z,
            relative_writes,
        );
    }

    fn update_heightmaps_after_block_change(
        &self,
        heightmap_types: &[HeightmapType],
        local_x: usize,
        y: i32,
        local_z: usize,
        state: BlockStateId,
    ) {
        let min_y = self.min_y;
        let height = self.height;
        let sections = &self.sections;

        let get_block = |lx: usize, scan_y: i32, lz: usize| {
            let scan_section_index = ((scan_y - min_y) / 16) as usize;
            let scan_local_y = ((scan_y - min_y) % 16) as usize;
            sections.sections[scan_section_index]
                .read()
                .states
                .get(lx, scan_local_y, lz)
        };

        let mut heightmaps = self.heightmaps.write();
        heightmaps.prime_from_sections(heightmap_types, min_y, height, &sections.sections);

        for &hm_type in heightmap_types {
            let Some(heightmap) = heightmaps.get_mut(hm_type) else {
                panic!("heightmap {hm_type:?} missing after priming");
            };
            heightmap.update(local_x, y, local_z, state, get_block);
        }
    }

    fn update_heightmaps_after_column_block_changes(
        &self,
        heightmap_types: &[HeightmapType],
        local_x: usize,
        local_z: usize,
        relative_writes: &[(usize, BlockStateId)],
    ) {
        if relative_writes.is_empty() {
            return;
        }

        let min_y = self.min_y;
        let height = self.height;
        let sections = &self.sections;

        let get_block = |lx: usize, scan_y: i32, lz: usize| {
            let scan_section_index = ((scan_y - min_y) / 16) as usize;
            let scan_local_y = ((scan_y - min_y) % 16) as usize;
            sections.sections[scan_section_index]
                .read()
                .states
                .get(lx, scan_local_y, lz)
        };

        let mut heightmaps = self.heightmaps.write();
        heightmaps.prime_from_sections(heightmap_types, min_y, height, &sections.sections);

        for &(relative_y, state) in relative_writes {
            let y = min_y + relative_y as i32;
            for &hm_type in heightmap_types {
                let Some(heightmap) = heightmaps.get_mut(hm_type) else {
                    panic!("heightmap {hm_type:?} missing after priming");
                };
                heightmap.update(local_x, y, local_z, state, get_block);
            }
        }
    }

    /// Gets a block state at the given position.
    #[must_use]
    pub fn get_block_state(&self, pos: BlockPos) -> BlockStateId {
        let y = pos.0.y;

        // Check bounds
        if y < self.min_y || y >= self.min_y + self.height {
            // Out of bounds - return default air
            return REGISTRY.blocks.get_default_state_id(&vanilla_blocks::AIR);
        }

        let section_index = self.get_section_index(y);
        let section = &self.sections.sections[section_index];
        let section_guard = section.read();

        let local_x = (pos.0.x & 15) as usize;
        let local_y = (y & 15) as usize;
        let local_z = (pos.0.z & 15) as usize;

        section_guard.states.get(local_x, local_y, local_z)
    }

    fn with_locked_block_state<R>(&self, pos: BlockPos, f: impl FnOnce(BlockStateId) -> R) -> R {
        let y = pos.y();
        if y < self.min_y || y >= self.min_y + self.height {
            return f(REGISTRY.blocks.get_default_state_id(&vanilla_blocks::AIR));
        }

        let section = self.sections.sections[self.get_section_index(y)].read();
        let state = section.states.get(
            (pos.x() & 15) as usize,
            (y & 15) as usize,
            (pos.z() & 15) as usize,
        );
        f(state)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{
        Arc, Weak,
        atomic::{AtomicUsize, Ordering},
    };

    use super::{Chunk, PendingPromotionCommit};
    use crate::behavior::{BlockEntityCreation, init_behaviors};
    use crate::block_entity::{
        BlockEntityLifecycleExt as _, SharedBlockEntity,
        entities::{RawBlockEntity, SignBlockEntity},
        init_block_entities,
    };
    use crate::chunk::{
        section::{ChunkSection, Sections},
        status::ChunkStatus,
    };
    use crate::world::tick_scheduler::TickPriority;
    use steel_registry::{init_vanilla_registry, vanilla_block_entity_types, vanilla_blocks};
    use steel_utils::{BlockPos, ChunkPos, DowncastType, DowncastTypeKey, types::UpdateFlags};

    struct DropSentinel(Arc<AtomicUsize>);

    // SAFETY: This key uniquely identifies the test-only sentinel type.
    unsafe impl DowncastType for DropSentinel {
        const TYPE_KEY: DowncastTypeKey =
            DowncastTypeKey::new("steel:test/chunk/transient_generation_state");
    }

    impl Drop for DropSentinel {
        fn drop(&mut self) {
            self.0.fetch_add(1, Ordering::Relaxed);
        }
    }

    #[test]
    fn transient_generation_state_survives_borrow_and_is_consumed_once() {
        init_vanilla_registry();
        let chunk = Chunk::new(
            Sections::from_owned(vec![ChunkSection::new_empty()].into_boxed_slice()),
            ChunkPos::new(0, 0),
            0,
            16,
            Weak::new(),
        );
        let drops = Arc::new(AtomicUsize::new(0));
        chunk.install_transient_generation_state(DropSentinel(Arc::clone(&drops)));

        assert!(
            chunk
                .with_transient_generation_state_mut::<DropSentinel, _>(|_| ())
                .is_some()
        );
        assert_eq!(drops.load(Ordering::Relaxed), 0);

        let present =
            chunk.consume_transient_generation_state::<DropSentinel, _>(|state| state.is_some());
        assert!(present);
        assert_eq!(drops.load(Ordering::Relaxed), 1);
        assert!(
            !chunk
                .consume_transient_generation_state::<DropSentinel, _>(|state| { state.is_some() })
        );
        assert_eq!(drops.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn full_promotion_drops_leftover_transient_generation_state() {
        init_vanilla_registry();
        init_behaviors();
        let chunk = Chunk::new(
            Sections::from_owned(vec![ChunkSection::new_empty()].into_boxed_slice()),
            ChunkPos::new(0, 0),
            0,
            16,
            Weak::new(),
        );
        let drops = Arc::new(AtomicUsize::new(0));
        chunk.install_transient_generation_state(DropSentinel(Arc::clone(&drops)));

        let _ = chunk.promote_to_full();

        assert_eq!(drops.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn postprocessing_offset_pack_unpack_matches_vanilla_layout() {
        let chunk_pos = ChunkPos::new(-2, 1);
        let section_y = -4;
        let pos = BlockPos::new(-17, -63, 31);

        let packed = Chunk::pack_postprocessing_offset(pos);

        assert_eq!(packed, 15 | (1 << 4) | (15 << 8));
        assert_eq!(
            Chunk::unpack_postprocessing_offset(packed, section_y, chunk_pos),
            pos
        );
    }

    #[test]
    fn proto_scheduled_block_ticks_use_vanilla_zero_delay() {
        init_vanilla_registry();
        let proto = Chunk::new(
            Sections::from_owned(vec![ChunkSection::new_empty()].into_boxed_slice()),
            ChunkPos::new(0, 0),
            0,
            16,
            Weak::new(),
        );
        let pos = BlockPos::new(3, 4, 5);

        proto.schedule_block_tick(pos, &vanilla_blocks::DIRT, TickPriority::Normal);

        let Some(snapshot) = proto.scheduled_ticks.snapshot(0) else {
            panic!("proto chunk scheduled ticks should remain available");
        };
        let Some(tick) = snapshot.block.first() else {
            panic!("proto chunk should store scheduled block tick");
        };

        assert_eq!(tick.pos, pos);
        assert_eq!(tick.tick_type, &vanilla_blocks::DIRT);
        assert_eq!(tick.delay, 0);
        assert_eq!(tick.priority, TickPriority::Normal);
    }

    #[test]
    fn full_promotion_retains_and_closes_proto_scheduled_tick_container() {
        init_vanilla_registry();
        let proto = Chunk::new(
            Sections::from_owned(vec![ChunkSection::new_empty()].into_boxed_slice()),
            ChunkPos::new(0, 0),
            0,
            16,
            Weak::new(),
        );
        let scheduled_ticks = Arc::clone(&proto.scheduled_ticks);
        let pos = BlockPos::new(3, 4, 5);
        proto.schedule_block_tick(pos, &vanilla_blocks::DIRT, TickPriority::Normal);

        let full = proto.promote_to_full().chunk;

        assert_eq!(
            scheduled_ticks.schedule_pending_block(
                &vanilla_blocks::DIRT,
                BlockPos::new(6, 7, 8),
                TickPriority::Normal,
            ),
            None
        );
        assert!(Arc::ptr_eq(
            &scheduled_ticks,
            full.scheduled_tick_container()
        ));
        let snapshot = full.scheduled_tick_snapshot();
        assert_eq!(snapshot.block.len(), 1);
        assert_eq!(snapshot.block[0].pos, pos);
    }

    #[test]
    fn proto_chunk_preserves_distinct_air_states_in_empty_sections() {
        init_vanilla_registry();
        init_behaviors();
        let proto = Chunk::new(
            Sections::from_owned(vec![ChunkSection::new_empty()].into_boxed_slice()),
            ChunkPos::new(0, 0),
            0,
            16,
            Weak::new(),
        );
        let pos = BlockPos::new(3, 4, 5);
        let cave_air = vanilla_blocks::CAVE_AIR.default_state();

        proto.set_block_state_for_generation(
            ChunkStatus::Empty,
            pos,
            cave_air,
            UpdateFlags::UPDATE_CLIENTS,
        );

        assert_eq!(proto.get_block_state(pos), cave_air);
    }

    #[test]
    fn pre_light_block_writes_defer_counts_until_light_initialization() {
        init_vanilla_registry();
        init_behaviors();
        let proto = Chunk::new(
            Sections::from_owned(vec![ChunkSection::new_empty()].into_boxed_slice()),
            ChunkPos::new(0, 0),
            0,
            16,
            Weak::new(),
        );
        let pos = BlockPos::new(3, 4, 5);
        let stone = vanilla_blocks::STONE.default_state();
        let air = vanilla_blocks::AIR.default_state();

        proto
            .sections
            .set_relative_block_for_generation(3, 4, 5, stone);
        assert_eq!(proto.sections.sections[0].read().non_empty_block_count(), 0);

        assert_eq!(
            proto.set_block_state_for_generation(
                ChunkStatus::Empty,
                pos,
                air,
                UpdateFlags::UPDATE_CLIENTS,
            ),
            Some(stone)
        );
        assert_eq!(proto.get_block_state(pos), air);

        assert_eq!(
            proto.set_block_state_for_generation(
                ChunkStatus::Empty,
                pos,
                stone,
                UpdateFlags::UPDATE_CLIENTS,
            ),
            Some(air)
        );
        assert_eq!(proto.sections.sections[0].read().non_empty_block_count(), 0);

        proto.initialize_light_sources();
        assert_eq!(proto.sections.sections[0].read().non_empty_block_count(), 1);
    }

    #[test]
    fn proto_mutation_defers_lifecycle_and_promotion_revalidates_concrete_entities() {
        init_vanilla_registry();
        init_behaviors();
        let proto = Chunk::new(
            Sections::from_owned(vec![ChunkSection::new_empty()].into_boxed_slice()),
            ChunkPos::new(0, 0),
            0,
            16,
            Weak::new(),
        );
        let pos = BlockPos::new(3, 4, 5);
        let sign = vanilla_blocks::OAK_SIGN.default_state();
        assert!(
            proto
                .set_block_state_for_generation(
                    ChunkStatus::Empty,
                    pos,
                    sign,
                    UpdateFlags::UPDATE_NONE,
                )
                .is_some()
        );
        let entity: SharedBlockEntity = Arc::new(SignBlockEntity::new(Weak::new(), pos, sign));
        assert!(proto.set_block_entity(Arc::clone(&entity)));

        assert_eq!(
            proto.set_block_state_for_generation(
                ChunkStatus::Empty,
                pos,
                vanilla_blocks::STONE.default_state(),
                UpdateFlags::UPDATE_NONE,
            ),
            Some(sign)
        );
        assert!(proto.get_block_entity(pos).is_some());

        let full = proto.promote_to_full().chunk;
        assert!(!entity.is_removed());
        assert!(full.get_block_entity(pos).is_none());
    }

    #[test]
    fn proto_storage_preserves_removed_entries_until_full_promotion() {
        init_vanilla_registry();
        init_behaviors();
        let proto = Chunk::new(
            Sections::from_owned(vec![ChunkSection::new_empty()].into_boxed_slice()),
            ChunkPos::new(0, 0),
            0,
            16,
            Weak::new(),
        );
        let pos = BlockPos::new(3, 4, 5);
        let sign = vanilla_blocks::OAK_SIGN.default_state();
        assert!(
            proto
                .set_block_state_for_generation(
                    ChunkStatus::Empty,
                    pos,
                    sign,
                    UpdateFlags::UPDATE_NONE,
                )
                .is_some()
        );
        let entity: SharedBlockEntity = Arc::new(SignBlockEntity::new(Weak::new(), pos, sign));
        entity.set_removed();
        assert!(proto.set_block_entity(Arc::clone(&entity)));

        assert_eq!(proto.get_block_entities().len(), 1);
        assert_eq!(
            proto
                .block_entities
                .save_snapshot_without_lifecycle_filter()
                .0
                .len(),
            1
        );

        let promotion = proto.promote_to_full();
        let full = promotion.chunk;
        assert!(!entity.is_removed());
        assert!(full.get_block_entity(pos).is_some());
    }

    #[test]
    fn pending_proto_entity_promotes_without_running_live_lifecycle() {
        init_vanilla_registry();
        init_behaviors();
        init_block_entities();
        let proto = Chunk::new(
            Sections::from_owned(vec![ChunkSection::new_empty()].into_boxed_slice()),
            ChunkPos::new(0, 0),
            0,
            16,
            Weak::new(),
        );
        let pos = BlockPos::new(3, 4, 5);
        let sign = vanilla_blocks::OAK_SIGN.default_state();
        assert!(
            proto
                .set_block_state_for_generation(
                    ChunkStatus::Empty,
                    pos,
                    sign,
                    UpdateFlags::UPDATE_NONE,
                )
                .is_some()
        );
        proto.set_pending_block_entity(pos);

        assert!(proto.promote_pending_block_entity(pos).is_some());
        assert_eq!(proto.pending_block_entity_positions().len(), 0);
    }

    #[test]
    fn conditional_proto_marker_mutation_rejects_stale_worldgen_state() {
        init_vanilla_registry();
        init_behaviors();
        let proto = Chunk::new(
            Sections::from_owned(vec![ChunkSection::new_empty()].into_boxed_slice()),
            ChunkPos::new(0, 0),
            0,
            16,
            Weak::new(),
        );
        let pos = BlockPos::new(3, 4, 5);
        let copper = vanilla_blocks::COPPER_CHEST.default_state();
        let exposed = vanilla_blocks::EXPOSED_COPPER_CHEST.default_state();
        assert!(
            proto
                .set_block_state_for_generation(
                    ChunkStatus::Empty,
                    pos,
                    exposed,
                    UpdateFlags::UPDATE_NONE,
                )
                .is_some()
        );

        assert!(!proto.set_pending_block_entity_if_state(pos, copper));
        assert_eq!(proto.pending_block_entity_positions().len(), 0);
        assert!(proto.set_pending_block_entity_if_state(pos, exposed));

        let stone = vanilla_blocks::STONE.default_state();
        assert_eq!(
            proto.set_block_state_for_generation(
                ChunkStatus::Empty,
                pos,
                stone,
                UpdateFlags::UPDATE_NONE,
            ),
            Some(exposed)
        );
        assert!(!proto.remove_block_entity_if_state(pos, exposed));
        assert_eq!(proto.pending_block_entity_positions(), [pos]);
        assert!(proto.remove_block_entity_if_state(pos, stone));
        assert_eq!(proto.pending_block_entity_positions().len(), 0);
    }

    #[test]
    fn dummy_factory_outcomes_keep_proto_and_full_stage_semantics() {
        init_vanilla_registry();
        init_behaviors();
        let proto = Chunk::new(
            Sections::from_owned(vec![ChunkSection::new_empty()].into_boxed_slice()),
            ChunkPos::new(0, 0),
            0,
            16,
            Weak::new(),
        );
        let moving_pos = BlockPos::new(2, 4, 5);
        let chest_pos = BlockPos::new(3, 4, 5);
        assert!(
            proto
                .set_block_state_for_generation(
                    ChunkStatus::Empty,
                    moving_pos,
                    vanilla_blocks::MOVING_PISTON.default_state(),
                    UpdateFlags::UPDATE_NONE,
                )
                .is_some()
        );
        assert!(
            proto
                .set_block_state_for_generation(
                    ChunkStatus::Empty,
                    chest_pos,
                    vanilla_blocks::CHEST.default_state(),
                    UpdateFlags::UPDATE_NONE,
                )
                .is_some()
        );
        proto.set_pending_block_entity(moving_pos);
        proto.set_pending_block_entity(chest_pos);

        assert!(proto.promote_pending_block_entity(moving_pos).is_none());
        assert!(proto.promote_pending_block_entity(chest_pos).is_none());
        let pending = proto.pending_block_entity_positions();
        assert!(pending.contains(&moving_pos));
        assert!(pending.contains(&chest_pos));

        let full = proto.promote_to_full().chunk;
        assert!(full.get_block_entity(moving_pos).is_none());
        assert!(!full.pending_block_entity_positions().contains(&moving_pos));
        assert!(full.get_block_entity(chest_pos).is_none());
        assert!(full.pending_block_entity_positions().contains(&chest_pos));
    }

    #[test]
    fn stale_proto_factory_cannot_consume_a_replacement_marker() {
        init_vanilla_registry();
        init_behaviors();
        let proto = Chunk::new(
            Sections::from_owned(vec![ChunkSection::new_empty()].into_boxed_slice()),
            ChunkPos::new(0, 0),
            0,
            16,
            Weak::new(),
        );
        let pos = BlockPos::new(2, 4, 5);
        let copper = vanilla_blocks::COPPER_CHEST.default_state();
        let exposed = vanilla_blocks::EXPOSED_COPPER_CHEST.default_state();
        assert!(
            proto
                .set_block_state_for_generation(
                    ChunkStatus::Empty,
                    pos,
                    copper,
                    UpdateFlags::UPDATE_NONE,
                )
                .is_some()
        );
        proto.set_pending_block_entity(pos);
        let stale_entity: SharedBlockEntity = Arc::new(RawBlockEntity::new(
            &vanilla_block_entity_types::CHEST,
            Weak::new(),
            pos,
            copper,
        ));

        assert_eq!(
            proto.set_block_state_for_generation(
                ChunkStatus::Empty,
                pos,
                exposed,
                UpdateFlags::UPDATE_NONE,
            ),
            Some(copper)
        );
        assert!(matches!(
            proto.commit_pending_creation(pos, copper, BlockEntityCreation::Created(stale_entity),),
            PendingPromotionCommit::Retry
        ));
        assert_eq!(proto.pending_block_entity_positions(), [pos]);
        assert!(proto.get_block_entity(pos).is_none());
    }
}
