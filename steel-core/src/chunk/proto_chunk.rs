//! A proto chunk is a chunk that is still being generated.
use std::sync::{
    Weak,
    atomic::{AtomicBool, Ordering},
};

use crossbeam::atomic::AtomicCell;
use parking_lot::{MappedRwLockWriteGuard, RwLockWriteGuard};
use rustc_hash::FxHashMap;
use steel_registry::{
    REGISTRY,
    blocks::{BlockRef, block_state_ext::BlockStateExt},
    fluid::FluidRef,
    vanilla_blocks,
};
use steel_utils::{
    BlockPos, BlockStateId, ChunkPos,
    locks::{SyncMutex, SyncRwLock},
    types::UpdateFlags,
};

use crate::behavior::BLOCK_BEHAVIORS;
use crate::block_entity::{BlockEntityStorage, SharedBlockEntity};
use crate::chunk::{
    chunk_access::ChunkStatus,
    heightmap::{HeightmapType, ProtoHeightmaps},
    section::Sections,
};
use crate::entity::{EntityStorage, SharedEntity};
use crate::world::World;
use crate::world::tick_scheduler::{
    BlockTick, BlockTickList, FluidTick, FluidTickList, TickPriority,
};
use crate::worldgen::carving_mask::CarvingMask;
use steel_worldgen::structure::{StructureReferenceMap, StructureStartMap};

fn empty_postprocessing(height: i32) -> Box<[Vec<u16>]> {
    let section_count = (height / 16) as usize;
    (0..section_count).map(|_| Vec::new()).collect()
}

fn postprocessing_from_disk(height: i32, mut postprocessing: Vec<Vec<u16>>) -> Box<[Vec<u16>]> {
    let section_count = (height / 16) as usize;
    postprocessing.resize_with(section_count, Vec::new);
    postprocessing.truncate(section_count);
    postprocessing.into_boxed_slice()
}

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
    /// Heightmaps (lazily initialized based on generation status).
    pub heightmaps: SyncRwLock<ProtoHeightmaps>,
    /// The minimum Y coordinate of the world this chunk belongs to.
    min_y: i32,
    /// The total height of the world.
    height: i32,
    /// Weak reference to the world for block entity dirty callbacks while the chunk is proto.
    level: Weak<World>,
    /// Block entities created during generation before promotion to a full chunk.
    pub(crate) block_entities: BlockEntityStorage,
    /// Entities created during generation before promotion to a full chunk.
    pub(crate) entities: EntityStorage,
    /// Structure starts originating in this chunk.
    pub structure_starts: SyncRwLock<StructureStartMap>,
    /// References to structures from nearby origin chunks.
    pub structure_references: SyncRwLock<StructureReferenceMap>,
    /// Bitset of positions visited by carvers (lazily initialized).
    pub carving_mask: SyncRwLock<Option<CarvingMask>>,
    /// Section-indexed packed offsets that need vanilla postprocessing after promotion.
    pub postprocessing: SyncRwLock<Box<[Vec<u16>]>>,
    /// Scheduled block ticks queued while this chunk is still a proto chunk.
    pub block_ticks: SyncMutex<BlockTickList>,
    /// Scheduled fluid ticks queued while this chunk is still a proto chunk.
    pub fluid_ticks: SyncMutex<FluidTickList>,
    // TODO: research persisting NoiseChunk/Aquifer across stages like vanilla
    // does. Vanilla caches `NoiseChunk` on `ChunkAccess` so noise, surface,
    // and carvers share one instance; we currently rebuild per stage. Blocked
    // by the type-erasure question — `NoiseChunk<N: DimensionNoises>` is
    // generic, `ProtoChunk` is not, and `Any` is forbidden by CLAUDE.md.
    // Options to evaluate: (1) object-safe trait returning carver-needed
    // values, (2) generic `ProtoChunk<N>` (big ripple), (3) stay as-is if
    // rebuild cost stays negligible.
}

impl ProtoChunk {
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
            status: AtomicCell::new(ChunkStatus::Empty),
            heightmaps: SyncRwLock::new(ProtoHeightmaps::new()),
            min_y,
            height,
            level,
            block_entities: BlockEntityStorage::new(),
            entities: EntityStorage::new(),
            structure_starts: SyncRwLock::new(FxHashMap::default()),
            structure_references: SyncRwLock::new(FxHashMap::default()),
            carving_mask: SyncRwLock::new(None),
            postprocessing: SyncRwLock::new(empty_postprocessing(height)),
            block_ticks: SyncMutex::new(BlockTickList::new()),
            fluid_ticks: SyncMutex::new(FluidTickList::new()),
        }
    }

    /// Creates a proto chunk that was loaded from disk.
    #[expect(
        clippy::too_many_arguments,
        reason = "disk rehydration mirrors the persisted proto chunk fields"
    )]
    #[must_use]
    pub fn from_disk(
        sections: Sections,
        pos: ChunkPos,
        status: ChunkStatus,
        min_y: i32,
        height: i32,
        structure_starts: StructureStartMap,
        structure_references: StructureReferenceMap,
        carving_mask: Option<CarvingMask>,
        postprocessing: Vec<Vec<u16>>,
        block_ticks: BlockTickList,
        fluid_ticks: FluidTickList,
        level: Weak<World>,
    ) -> Self {
        Self {
            sections,
            pos,
            dirty: AtomicBool::new(false),
            status: AtomicCell::new(status),
            // Proto heightmaps will be re-primed during generation on the first set_block_state call
            heightmaps: SyncRwLock::new(ProtoHeightmaps::new()),
            min_y,
            height,
            level,
            block_entities: BlockEntityStorage::new(),
            entities: EntityStorage::new(),
            structure_starts: SyncRwLock::new(structure_starts),
            structure_references: SyncRwLock::new(structure_references),
            carving_mask: SyncRwLock::new(carving_mask),
            postprocessing: SyncRwLock::new(postprocessing_from_disk(height, postprocessing)),
            block_ticks: SyncMutex::new(block_ticks),
            fluid_ticks: SyncMutex::new(fluid_ticks),
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

    /// Returns a write guard to this chunk's carving mask, initializing it on
    /// first access. Mirrors vanilla's `ProtoChunk.getOrCreateCarvingMask`.
    ///
    /// # Panics
    /// Never — the mask is populated immediately before projecting the guard.
    pub fn get_or_create_carving_mask(&self) -> MappedRwLockWriteGuard<'_, CarvingMask> {
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
    pub fn mark_pos_for_postprocessing(&self, pos: BlockPos) {
        let y = pos.0.y;
        if y < self.min_y || y >= self.min_y + self.height {
            return;
        }

        let section_index = self.get_section_index(y);
        let packed = Self::pack_postprocessing_offset(pos);
        self.postprocessing.write()[section_index].push(packed);
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
    pub fn level_weak(&self) -> Weak<World> {
        self.level.clone()
    }

    /// Gets a block entity at the given position.
    #[must_use]
    pub fn get_block_entity(&self, pos: BlockPos) -> Option<SharedBlockEntity> {
        self.block_entities.get(pos)
    }

    /// Adds a block entity and registers it for ticking if needed.
    pub fn add_and_register_block_entity(&self, block_entity: SharedBlockEntity) {
        self.block_entities.add_and_register(block_entity);
        self.mark_unsaved();
    }

    /// Removes a block entity at the given position.
    pub fn remove_block_entity(&self, pos: BlockPos) {
        self.block_entities.remove(pos);
        self.mark_unsaved();
    }

    /// Updates the ticking status of a block entity.
    pub fn update_block_entity_ticker(&self, block_entity: &SharedBlockEntity) {
        self.block_entities.update_ticker(block_entity);
    }

    /// Returns all block entities in this proto chunk.
    #[must_use]
    pub fn get_block_entities(&self) -> Vec<SharedBlockEntity> {
        self.block_entities.get_all()
    }

    /// Returns a reference to the block entity storage.
    #[must_use]
    pub const fn block_entity_storage(&self) -> &BlockEntityStorage {
        &self.block_entities
    }

    /// Adds an entity to proto storage.
    pub fn add_entity(&self, entity: SharedEntity) {
        self.entities.add(entity);
        self.mark_unsaved();
    }

    /// Returns all entities in this proto chunk.
    #[must_use]
    pub fn get_entities(&self) -> Vec<SharedEntity> {
        self.entities.get_all()
    }

    /// Returns entities that should be persisted from this proto chunk.
    #[must_use]
    pub fn get_saveable_entities(&self) -> Vec<SharedEntity> {
        self.entities.get_saveable_entities()
    }

    /// Schedules a block tick in proto storage.
    ///
    /// Vanilla `ProtoChunkTicks.schedule(ScheduledTick)` stores a saved tick with delay `0`,
    /// so worldgen-scheduled proto ticks run after promotion instead of preserving the
    /// requested delay from generation time.
    pub fn schedule_block_tick(&self, pos: BlockPos, block: BlockRef, priority: TickPriority) {
        let tick = BlockTick {
            tick_type: block,
            pos,
            delay: 0,
            priority,
            sub_tick_order: 0,
        };

        if self.block_ticks.lock().schedule(tick) {
            self.mark_unsaved();
        }
    }

    /// Schedules a fluid tick in proto storage.
    ///
    /// See [`Self::schedule_block_tick`] for why proto ticks use delay `0`.
    pub fn schedule_fluid_tick(&self, pos: BlockPos, fluid: FluidRef, priority: TickPriority) {
        let tick = FluidTick {
            tick_type: fluid,
            pos,
            delay: 0,
            priority,
            sub_tick_order: 0,
        };

        if self.fluid_ticks.lock().schedule(tick) {
            self.mark_unsaved();
        }
    }

    /// Sets a block state at the given position.
    ///
    /// Returns the old block state at the position, or `VOID_AIR` if out of bounds.
    pub fn set_block_state(
        &self,
        pos: BlockPos,
        state: BlockStateId,
        flags: UpdateFlags,
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
        let mut section_guard = section.write();
        let old_state = section_guard.states.set(local_x, local_y, local_z, state);
        drop(section_guard);

        if old_state == state {
            return None;
        }

        self.update_status_heightmaps_after_block_change(local_x, y, local_z, state);

        self.update_block_entity_lifecycle(pos, old_state, state, flags);
        self.mark_unsaved();
        Some(old_state)
    }

    /// Applies the heightmap side effect for an optimized direct section write.
    ///
    /// Use this only for generation paths that intentionally bypass
    /// [`Self::set_block_state`] but still need vanilla heightmap maintenance.
    pub(crate) fn update_status_heightmaps_after_block_change(
        &self,
        local_x: usize,
        y: i32,
        local_z: usize,
        state: BlockStateId,
    ) {
        self.update_heightmaps_after_block_change(
            self.status().heightmaps_after(),
            local_x,
            y,
            local_z,
            state,
        );
    }

    pub(crate) fn update_status_heightmaps_after_column_block_changes(
        &self,
        local_x: usize,
        local_z: usize,
        relative_writes: &[(usize, BlockStateId)],
    ) {
        self.update_heightmaps_after_column_block_changes(
            self.status().heightmaps_after(),
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
        heightmaps.prime(heightmap_types, min_y, height, get_block);

        for &hm_type in heightmap_types {
            if let Some(heightmap) = heightmaps.get_mut(hm_type) {
                heightmap.update(local_x, y, local_z, state, get_block);
            }
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
        heightmaps.prime(heightmap_types, min_y, height, get_block);

        for &(relative_y, state) in relative_writes {
            let y = min_y + relative_y as i32;
            for &hm_type in heightmap_types {
                if let Some(heightmap) = heightmaps.get_mut(hm_type) {
                    heightmap.update(local_x, y, local_z, state, get_block);
                }
            }
        }
    }

    fn update_block_entity_lifecycle(
        &self,
        pos: BlockPos,
        old_state: BlockStateId,
        state: BlockStateId,
        flags: UpdateFlags,
    ) {
        let old_block = old_state.get_block();
        let new_block = state.get_block();
        let block_changed = old_block != new_block;
        let side_effects = !flags.contains(UpdateFlags::UPDATE_SKIP_BLOCK_ENTITY_SIDEEFFECTS);

        let block_behaviors = &*BLOCK_BEHAVIORS;
        let old_behavior = block_behaviors.get_behavior(old_block);
        let new_behavior = block_behaviors.get_behavior(new_block);

        if block_changed && old_behavior.has_block_entity() {
            let should_keep = new_behavior.should_keep_block_entity(old_state, state);
            if !should_keep {
                if side_effects && let Some(block_entity) = self.get_block_entity(pos) {
                    block_entity.lock().pre_remove_side_effects(pos, old_state);
                }
                self.remove_block_entity(pos);
            }
        }

        if new_behavior.has_block_entity() {
            if let Some(existing) = self.get_block_entity(pos) {
                existing.lock().set_block_state(state);
                self.update_block_entity_ticker(&existing);
            } else if let Some(entity) =
                new_behavior.new_block_entity(self.level.clone(), pos, state)
            {
                self.add_and_register_block_entity(entity);
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
}

#[cfg(test)]
mod tests {
    use std::sync::Weak;

    use super::ProtoChunk;
    use crate::behavior::init_behaviors;
    use crate::chunk::section::{ChunkSection, Sections};
    use crate::world::tick_scheduler::TickPriority;
    use steel_registry::{test_support::init_test_registry, vanilla_blocks};
    use steel_utils::{BlockPos, ChunkPos, types::UpdateFlags};

    #[test]
    fn postprocessing_offset_pack_unpack_matches_vanilla_layout() {
        let chunk_pos = ChunkPos::new(-2, 1);
        let section_y = -4;
        let pos = BlockPos::new(-17, -63, 31);

        let packed = ProtoChunk::pack_postprocessing_offset(pos);

        assert_eq!(packed, 15 | (1 << 4) | (15 << 8));
        assert_eq!(
            ProtoChunk::unpack_postprocessing_offset(packed, section_y, chunk_pos),
            pos
        );
    }

    #[test]
    fn proto_scheduled_block_ticks_use_vanilla_zero_delay() {
        init_test_registry();
        let proto = ProtoChunk::new(
            Sections::from_owned(vec![ChunkSection::new_empty()].into_boxed_slice()),
            ChunkPos::new(0, 0),
            0,
            16,
            Weak::new(),
        );
        let pos = BlockPos::new(3, 4, 5);

        proto.schedule_block_tick(pos, &vanilla_blocks::DIRT, TickPriority::Normal);

        let ticks = proto.block_ticks.lock();
        let tick = ticks
            .iter()
            .next()
            .expect("proto chunk should store scheduled block tick");

        assert_eq!(tick.pos, pos);
        assert_eq!(tick.tick_type, &vanilla_blocks::DIRT);
        assert_eq!(tick.delay, 0);
        assert_eq!(tick.priority, TickPriority::Normal);
    }

    #[test]
    fn proto_chunk_preserves_distinct_air_states_in_empty_sections() {
        init_test_registry();
        init_behaviors();
        let proto = ProtoChunk::new(
            Sections::from_owned(vec![ChunkSection::new_empty()].into_boxed_slice()),
            ChunkPos::new(0, 0),
            0,
            16,
            Weak::new(),
        );
        let pos = BlockPos::new(3, 4, 5);
        let cave_air = vanilla_blocks::CAVE_AIR.default_state();

        proto.set_block_state(pos, cave_air, UpdateFlags::UPDATE_CLIENTS);

        assert_eq!(proto.get_block_state(pos), cave_air);
    }
}
