//! This module contains the `ChunkAccess` enum, which is used to access chunks in different states.
use std::sync::Weak;
use std::sync::atomic::Ordering;
use steel_registry::{blocks::BlockRef, fluid::FluidRef};
use steel_utils::{BlockPos, BlockStateId, ChunkPos, types::UpdateFlags};
use wincode::{SchemaRead, SchemaWrite};

use parking_lot::{RwLockReadGuard, RwLockWriteGuard};

use crate::block_entity::SharedBlockEntity;
use crate::chunk::{
    heightmap::HeightmapType, level_chunk::LevelChunk, light::ChunkLightData,
    light::ChunkSkyLightSources, proto_chunk::ProtoChunk, section::Sections,
};
use crate::entity::SharedEntity;
use crate::world::World;
use crate::world::tick_scheduler::{BlockTick, FluidTick, TickPriority};
use steel_worldgen::structure::{StructureReferenceMap, StructureStartMap};

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
// Always stored behind `SyncRwLock` in `ChunkHolder`, so variant size doesn't matter.
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
                chunk.refresh_light_emptiness_maps();
                chunk.dirty.store(true, Ordering::Release);
            }
            Self::Proto(proto_chunk) => {
                proto_chunk
                    .sections
                    .set_relative_block(relative_x, relative_y, relative_z, value);
                if proto_chunk.status() >= ChunkStatus::InitializeLight {
                    proto_chunk.refresh_light_emptiness_maps();
                }
                proto_chunk.dirty.store(true, Ordering::Release);
            }
            Self::Unloaded => unreachable!(),
        }
    }

    /// Sets a relative block during generation and preserves heightmap side effects.
    ///
    /// This is for optimized generation paths that intentionally skip block
    /// behavior updates but still need vanilla's heightmap maintenance.
    pub(crate) fn set_relative_block_for_generation(
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
                chunk.refresh_light_emptiness_maps();
                chunk.dirty.store(true, Ordering::Release);
            }
            Self::Proto(proto_chunk) => {
                if proto_chunk.status() >= ChunkStatus::InitializeLight {
                    proto_chunk
                        .sections
                        .set_relative_block(relative_x, relative_y, relative_z, value);
                    proto_chunk.refresh_light_emptiness_maps();
                } else {
                    proto_chunk.sections.set_relative_block_for_generation(
                        relative_x, relative_y, relative_z, value,
                    );
                }
                proto_chunk.dirty.store(true, Ordering::Release);
            }
            Self::Unloaded => unreachable!(),
        }
        let y = self.min_y() + relative_y as i32;
        self.update_heightmaps_after_direct_write(relative_x, y, relative_z, value);
    }

    /// Writes multiple generation blocks in one batch.
    ///
    /// Uses the raw `Building` palette only while the actual chunk is still pre-light.
    /// Later chunks keep cached section counters/light emptiness coherent.
    pub(crate) fn write_block_batch_for_generation(
        &self,
        blocks: &[(usize, usize, usize, BlockStateId)],
    ) {
        if blocks.is_empty() {
            return;
        }

        if self.uses_pre_light_generation_writes() {
            self.sections().write_block_batch(blocks);
            return;
        }

        for &(x, relative_y, z, value) in blocks {
            self.sections().set_relative_block(x, relative_y, z, value);
        }
        self.refresh_light_emptiness_maps_after_generation_write();
    }

    /// Writes multiple generation blocks in one column.
    ///
    /// Heightmap maintenance remains the caller's responsibility, matching
    /// `write_column_blocks`.
    pub(crate) fn write_column_blocks_for_generation(
        &self,
        x: usize,
        z: usize,
        blocks: &[(usize, BlockStateId)],
    ) {
        if blocks.is_empty() {
            return;
        }

        if self.uses_pre_light_generation_writes() {
            self.sections().write_column_blocks(x, z, blocks);
            return;
        }

        for &(relative_y, value) in blocks {
            self.sections().set_relative_block(x, relative_y, z, value);
        }
        self.refresh_light_emptiness_maps_after_generation_write();
    }

    fn uses_pre_light_generation_writes(&self) -> bool {
        matches!(self, Self::Proto(proto) if proto.status() < ChunkStatus::InitializeLight)
    }

    fn refresh_light_emptiness_maps_after_generation_write(&self) {
        match self {
            Self::Full(chunk) => chunk.refresh_light_emptiness_maps(),
            Self::Proto(proto) if proto.status() >= ChunkStatus::InitializeLight => {
                proto.refresh_light_emptiness_maps();
            }
            Self::Proto(_) => {}
            Self::Unloaded => unreachable!(),
        }
    }

    /// Applies heightmap maintenance after a direct section write.
    pub(crate) fn update_heightmaps_after_direct_write(
        &self,
        local_x: usize,
        y: i32,
        local_z: usize,
        state: BlockStateId,
    ) {
        match self {
            Self::Full(chunk) => {
                let min_y = chunk.min_y();
                let sections = &chunk.sections;
                chunk
                    .heightmaps
                    .write()
                    .update(local_x, y, local_z, state, |lx, scan_y, lz| {
                        let scan_section_index = ((scan_y - min_y) / 16) as usize;
                        let scan_local_y = ((scan_y - min_y) % 16) as usize;
                        sections.sections[scan_section_index].read().states.get(
                            lx,
                            scan_local_y,
                            lz,
                        )
                    });
            }
            Self::Proto(proto) => {
                proto.update_status_heightmaps_after_block_change(local_x, y, local_z, state);
            }
            Self::Unloaded => unreachable!(),
        }
    }

    /// Applies heightmap maintenance after direct section writes in one column.
    pub(crate) fn update_heightmaps_after_direct_column_writes(
        &self,
        local_x: usize,
        local_z: usize,
        relative_writes: &[(usize, BlockStateId)],
    ) {
        if relative_writes.is_empty() {
            return;
        }

        match self {
            Self::Full(chunk) => {
                let min_y = chunk.min_y();
                let sections = &chunk.sections;
                let get_block = |lx: usize, scan_y: i32, lz: usize| {
                    let scan_section_index = ((scan_y - min_y) / 16) as usize;
                    let scan_local_y = ((scan_y - min_y) % 16) as usize;
                    sections.sections[scan_section_index]
                        .read()
                        .states
                        .get(lx, scan_local_y, lz)
                };
                let mut heightmaps = chunk.heightmaps.write();
                for &(relative_y, state) in relative_writes {
                    heightmaps.update(
                        local_x,
                        min_y + relative_y as i32,
                        local_z,
                        state,
                        get_block,
                    );
                }
            }
            Self::Proto(proto) => {
                proto.update_status_heightmaps_after_column_block_changes(
                    local_x,
                    local_z,
                    relative_writes,
                );
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

    /// Clears the dirty flag and returns whether it was previously set.
    pub fn take_dirty(&self) -> bool {
        match self {
            Self::Full(chunk) => chunk.dirty.swap(false, Ordering::AcqRel),
            Self::Proto(proto_chunk) => proto_chunk.dirty.swap(false, Ordering::AcqRel),
            Self::Unloaded => unreachable!(),
        }
    }

    /// Clears the dirty flag.
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

    /// Returns the minimum Y coordinate of the world this chunk belongs to.
    #[must_use]
    pub const fn min_y(&self) -> i32 {
        match self {
            Self::Full(chunk) => chunk.min_y(),
            Self::Proto(proto_chunk) => proto_chunk.min_y(),
            Self::Unloaded => unreachable!(),
        }
    }

    /// Returns a read guard on the proto heightmaps.
    ///
    /// # Panics
    /// Panics if the chunk is not a proto chunk.
    pub fn proto_heightmaps(
        &self,
    ) -> parking_lot::RwLockReadGuard<'_, super::heightmap::ProtoHeightmaps> {
        match self {
            Self::Proto(proto) => proto.heightmaps.read(),
            Self::Full(_) => panic!("proto_heightmaps not available on full chunks"),
            Self::Unloaded => unreachable!(),
        }
    }

    /// Ensure specific proto heightmaps are primed.
    ///
    /// Use this when vanilla explicitly asks a chunk to materialize a heightmap
    /// (for example through `getHeight`). Direct terrain writes should maintain
    /// their heightmap side effects as they write, matching vanilla's generator
    /// paths.
    ///
    /// # Lock ordering
    /// Acquires heightmap write lock, then section read locks. Callers must not
    /// hold a section write lock when calling this, or a deadlock will occur.
    ///
    /// # Panics
    /// Panics if the chunk is not a proto chunk.
    pub fn prime_heightmaps(&self, heightmap_types: &[HeightmapType]) {
        match self {
            Self::Proto(proto) => {
                let mut heightmaps = proto.heightmaps.write();
                heightmaps.prime_from_sections(
                    heightmap_types,
                    proto.min_y(),
                    proto.height(),
                    &proto.sections.sections,
                );
            }
            Self::Full(_) => panic!("prime_heightmaps not available on full chunks"),
            Self::Unloaded => unreachable!(),
        }
    }

    /// Ensure final heightmaps are primed for feature and post-feature generation.
    ///
    /// Vanilla primes `WORLD_SURFACE`, `MOTION_BLOCKING`, `MOTION_BLOCKING_NO_LEAVES`,
    /// and `OCEAN_FLOOR` before biome decoration, after carvers have finished.
    ///
    /// # Lock ordering
    /// Acquires heightmap write lock, then section read locks. Callers must not
    /// hold a section write lock when calling this, or a deadlock will occur.
    ///
    /// # Panics
    /// Panics if the chunk is not a proto chunk.
    pub fn prime_final_heightmaps(&self) {
        match self {
            Self::Proto(proto) => {
                let mut heightmaps = proto.heightmaps.write();
                heightmaps.prime_from_sections(
                    HeightmapType::final_types(),
                    proto.min_y(),
                    proto.height(),
                    &proto.sections.sections,
                );
            }
            Self::Full(_) => panic!("prime_final_heightmaps not available on full chunks"),
            Self::Unloaded => unreachable!(),
        }
    }

    /// Fills this chunk's vanilla skylight-source cache from current sections.
    pub fn initialize_light_sources(&self) {
        match self {
            Self::Full(chunk) => chunk.initialize_light_sources(),
            Self::Proto(proto) => proto.initialize_light_sources(),
            Self::Unloaded => unreachable!(),
        }
    }

    /// Returns a read guard for this chunk's skylight-source cache.
    pub fn sky_light_sources(&self) -> RwLockReadGuard<'_, ChunkSkyLightSources> {
        match self {
            Self::Full(chunk) => chunk.sky_light_sources.read(),
            Self::Proto(proto) => proto.sky_light_sources.read(),
            Self::Unloaded => unreachable!(),
        }
    }

    /// Returns block-light source positions in `ScalableLux` section/local-index order.
    #[must_use]
    pub fn block_light_sources(&self) -> Vec<BlockPos> {
        match self {
            Self::Full(chunk) => chunk.sections.block_light_sources(chunk.pos, chunk.min_y()),
            Self::Proto(proto) => proto.sections.block_light_sources(proto.pos, proto.min_y()),
            Self::Unloaded => unreachable!(),
        }
    }

    /// Returns a read guard for this chunk's committed light data.
    pub fn light(&self) -> RwLockReadGuard<'_, ChunkLightData> {
        match self {
            Self::Full(chunk) => chunk.light.read(),
            Self::Proto(proto) => proto.light.read(),
            Self::Unloaded => unreachable!(),
        }
    }

    /// Returns a write guard for this chunk's committed light data.
    pub fn light_mut(&self) -> RwLockWriteGuard<'_, ChunkLightData> {
        match self {
            Self::Full(chunk) => chunk.light.write(),
            Self::Proto(proto) => proto.light.write(),
            Self::Unloaded => unreachable!(),
        }
    }

    /// Gets the first available Y coordinate for a heightmap column.
    ///
    /// Missing proto heightmaps are primed lazily, matching vanilla
    /// `ChunkAccess.getHeight`. Full chunks map worldgen heightmap queries to
    /// their final equivalents, matching vanilla `ImposterProtoChunk`.
    #[must_use]
    pub fn height_at(&self, heightmap_type: HeightmapType, local_x: usize, local_z: usize) -> i32 {
        match self {
            Self::Full(chunk) => chunk.get_height(
                Self::full_chunk_heightmap_type(heightmap_type),
                local_x,
                local_z,
            ),
            Self::Proto(proto) => Self::proto_height_at(proto, heightmap_type, local_x, local_z),
            Self::Unloaded => unreachable!(),
        }
    }

    const fn full_chunk_heightmap_type(heightmap_type: HeightmapType) -> HeightmapType {
        match heightmap_type {
            HeightmapType::WorldSurfaceWg => HeightmapType::WorldSurface,
            HeightmapType::OceanFloorWg => HeightmapType::OceanFloor,
            other => other,
        }
    }

    fn proto_height_at(
        proto: &ProtoChunk,
        heightmap_type: HeightmapType,
        local_x: usize,
        local_z: usize,
    ) -> i32 {
        {
            let heightmaps = proto.heightmaps.read();
            if let Some(heightmap) = heightmaps.get(heightmap_type) {
                return heightmap.get_first_available(local_x, local_z);
            }
        }

        let mut heightmaps = proto.heightmaps.write();
        heightmaps.prime_from_sections(
            &[heightmap_type],
            proto.min_y(),
            proto.height(),
            &proto.sections.sections,
        );
        let Some(heightmap) = heightmaps.get(heightmap_type) else {
            panic!("heightmap {heightmap_type:?} missing after priming");
        };
        heightmap.get_first_available(local_x, local_z)
    }

    /// Marks a proto chunk block position for vanilla postprocessing after promotion.
    ///
    /// Full chunks mirror vanilla `ImposterProtoChunk.markPosForPostprocessing` and ignore
    /// late worldgen postprocessing marks.
    pub fn mark_pos_for_postprocessing(&self, pos: BlockPos) {
        match self {
            Self::Proto(proto) => proto.mark_pos_for_postprocessing(pos),
            Self::Full(_) => {}
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

    /// Gets a block entity at the given position.
    #[must_use]
    pub fn get_block_entity(&self, pos: BlockPos) -> Option<SharedBlockEntity> {
        match self {
            Self::Full(chunk) => chunk.get_block_entity(pos),
            Self::Proto(proto_chunk) => proto_chunk.get_block_entity(pos),
            Self::Unloaded => unreachable!(),
        }
    }

    /// Returns the weak world reference stored by this chunk.
    #[must_use]
    pub fn level_weak(&self) -> Weak<World> {
        match self {
            Self::Full(chunk) => chunk.level_weak(),
            Self::Proto(proto_chunk) => proto_chunk.level_weak(),
            Self::Unloaded => unreachable!(),
        }
    }

    /// Adds a block entity and registers it for ticking if needed.
    pub fn add_and_register_block_entity(&self, block_entity: SharedBlockEntity) {
        match self {
            Self::Full(chunk) => chunk.add_and_register_block_entity(block_entity),
            Self::Proto(proto_chunk) => proto_chunk.add_and_register_block_entity(block_entity),
            Self::Unloaded => unreachable!(),
        }
    }

    /// Removes a block entity at the given position.
    pub fn remove_block_entity(&self, pos: BlockPos) {
        match self {
            Self::Full(chunk) => chunk.remove_block_entity(pos),
            Self::Proto(proto_chunk) => proto_chunk.remove_block_entity(pos),
            Self::Unloaded => unreachable!(),
        }
    }

    /// Returns all block entities in this chunk.
    #[must_use]
    pub fn get_block_entities(&self) -> Vec<SharedBlockEntity> {
        match self {
            Self::Full(chunk) => chunk.get_block_entities(),
            Self::Proto(proto_chunk) => proto_chunk.get_block_entities(),
            Self::Unloaded => unreachable!(),
        }
    }

    /// Adds an entity to either a full or proto chunk.
    pub fn add_entity(&self, entity: SharedEntity) -> bool {
        match self {
            Self::Full(chunk) => {
                let Some(world) = chunk.get_level() else {
                    return false;
                };
                if let Err(error) = world.register_loaded_entity(entity) {
                    log::warn!("Failed to register entity in full chunk: {error}");
                    return false;
                }
                chunk.dirty.store(true, Ordering::Release);
                true
            }
            Self::Proto(proto_chunk) => {
                proto_chunk.add_entity(entity);
                true
            }
            Self::Unloaded => unreachable!(),
        }
    }

    /// Returns all saveable entities in this chunk.
    #[must_use]
    pub fn get_saveable_entities(&self) -> Vec<SharedEntity> {
        match self {
            Self::Full(_) => Vec::new(),
            Self::Proto(proto_chunk) => proto_chunk.get_saveable_entities(),
            Self::Unloaded => unreachable!(),
        }
    }

    /// Schedules a block tick on either a full or proto chunk.
    pub fn schedule_block_tick(
        &self,
        pos: BlockPos,
        block: BlockRef,
        delay: i32,
        priority: TickPriority,
        sub_tick_order: i64,
    ) {
        match self {
            Self::Full(chunk) => {
                let tick = BlockTick {
                    tick_type: block,
                    pos,
                    delay,
                    priority,
                    sub_tick_order,
                };
                if chunk.block_ticks.lock().schedule(tick) {
                    chunk.dirty.store(true, Ordering::Release);
                }
            }
            Self::Proto(proto_chunk) => proto_chunk.schedule_block_tick(pos, block, priority),
            Self::Unloaded => unreachable!(),
        }
    }

    /// Schedules a fluid tick on either a full or proto chunk.
    pub fn schedule_fluid_tick(
        &self,
        pos: BlockPos,
        fluid: FluidRef,
        delay: i32,
        priority: TickPriority,
        sub_tick_order: i64,
    ) {
        match self {
            Self::Full(chunk) => {
                let tick = FluidTick {
                    tick_type: fluid,
                    pos,
                    delay,
                    priority,
                    sub_tick_order,
                };
                if chunk.fluid_ticks.lock().schedule(tick) {
                    chunk.dirty.store(true, Ordering::Release);
                }
            }
            Self::Proto(proto_chunk) => proto_chunk.schedule_fluid_tick(pos, fluid, priority),
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

    /// Returns a read guard to the structure starts map.
    pub fn structure_starts(&self) -> RwLockReadGuard<'_, StructureStartMap> {
        match self {
            Self::Full(chunk) => chunk.structure_starts.read(),
            Self::Proto(proto) => proto.structure_starts.read(),
            Self::Unloaded => unreachable!(),
        }
    }

    /// Returns a write guard to the structure starts map.
    pub fn structure_starts_mut(&self) -> RwLockWriteGuard<'_, StructureStartMap> {
        match self {
            Self::Full(chunk) => chunk.structure_starts.write(),
            Self::Proto(proto) => proto.structure_starts.write(),
            Self::Unloaded => unreachable!(),
        }
    }

    /// Returns a read guard to the structure references map.
    pub fn structure_references(&self) -> RwLockReadGuard<'_, StructureReferenceMap> {
        match self {
            Self::Full(chunk) => chunk.structure_references.read(),
            Self::Proto(proto) => proto.structure_references.read(),
            Self::Unloaded => unreachable!(),
        }
    }

    /// Returns a write guard to the structure references map.
    pub fn structure_references_mut(&self) -> RwLockWriteGuard<'_, StructureReferenceMap> {
        match self {
            Self::Full(chunk) => chunk.structure_references.write(),
            Self::Proto(proto) => proto.structure_references.write(),
            Self::Unloaded => unreachable!(),
        }
    }

    /// Ticks the chunk if it's a full chunk.
    ///
    /// Drains ready scheduled ticks into the provided vecs, then processes random ticks.
    pub fn tick(
        &self,
        random_tick_speed: u32,
        tick_count: i32,
        ready_block_ticks: &mut Vec<BlockTick>,
        ready_fluid_ticks: &mut Vec<FluidTick>,
    ) {
        if let Self::Full(chunk) = self {
            chunk.tick(
                random_tick_speed,
                tick_count,
                ready_block_ticks,
                ready_fluid_ticks,
            );
        }
    }

    /// Drains ready scheduled ticks if this is a full chunk.
    pub fn drain_ready_scheduled_ticks(
        &self,
        ready_block_ticks: &mut Vec<BlockTick>,
        ready_fluid_ticks: &mut Vec<FluidTick>,
    ) {
        if let Self::Full(chunk) = self {
            chunk.drain_ready_scheduled_ticks(ready_block_ticks, ready_fluid_ticks);
        }
    }

    /// Ticks random blocks if this is a full chunk.
    pub fn tick_random_blocks(&self, random_tick_speed: u32) {
        if let Self::Full(chunk) = self {
            chunk.tick_random_blocks(random_tick_speed);
        }
    }

    /// Ticks block entities if this is a full chunk.
    pub fn tick_block_entities(&self) {
        if let Self::Full(chunk) = self {
            chunk.tick_block_entities();
        }
    }
}

#[cfg(test)]
mod tests {
    use steel_registry::{REGISTRY, test_support::init_test_registry, vanilla_blocks};
    use steel_utils::types::UpdateFlags;

    use super::*;
    use crate::behavior::init_behaviors;
    use crate::chunk::heightmap::ChunkHeightmaps;
    use crate::chunk::light::ChunkLightData;
    use crate::chunk::section::{ChunkSection, Sections};
    use crate::world::tick_scheduler::{BlockTickList, FluidTickList};
    use steel_worldgen::structure::{StructureReferenceMap, StructureStartMap};

    #[test]
    fn take_dirty_consumes_current_dirty_state_without_blocking_later_dirty() {
        init_test_registry();
        let proto = ProtoChunk::new(
            Sections::from_owned(vec![ChunkSection::new_empty()].into_boxed_slice()),
            ChunkPos::new(0, 0),
            0,
            16,
            Weak::new(),
        );
        let chunk = ChunkAccess::Proto(proto);

        assert!(chunk.is_dirty());
        assert!(chunk.take_dirty());
        assert!(!chunk.is_dirty());
        assert!(!chunk.take_dirty());

        chunk.mark_dirty();

        assert!(chunk.take_dirty());
        assert!(!chunk.is_dirty());
    }

    #[test]
    fn proto_height_at_primes_missing_heightmap() {
        init_test_registry();
        init_behaviors();
        let proto = ProtoChunk::new(
            Sections::from_owned(vec![ChunkSection::new_empty()].into_boxed_slice()),
            ChunkPos::new(0, 0),
            0,
            16,
            Weak::new(),
        );
        let stone = REGISTRY.blocks.get_default_state_id(&vanilla_blocks::STONE);
        let chunk = ChunkAccess::Proto(proto);
        chunk.set_relative_block(3, 5, 7, stone);

        assert_eq!(chunk.height_at(HeightmapType::OceanFloorWg, 3, 7), 6);

        let ChunkAccess::Proto(proto) = &chunk else {
            panic!("test chunk should remain proto");
        };
        assert!(
            proto
                .heightmaps
                .read()
                .get(HeightmapType::OceanFloorWg)
                .is_some()
        );
    }

    #[test]
    fn generation_relative_write_updates_proto_heightmaps() {
        init_test_registry();
        let proto = ProtoChunk::new(
            Sections::from_owned(vec![ChunkSection::new_empty()].into_boxed_slice()),
            ChunkPos::new(0, 0),
            0,
            16,
            Weak::new(),
        );
        let stone = REGISTRY.blocks.get_default_state_id(&vanilla_blocks::STONE);
        let chunk = ChunkAccess::Proto(proto);

        chunk.set_relative_block_for_generation(3, 5, 7, stone);

        let ChunkAccess::Proto(proto) = &chunk else {
            panic!("test chunk should remain proto");
        };
        let heightmaps = proto.heightmaps.read();
        let ocean_floor = heightmaps
            .get(HeightmapType::OceanFloorWg)
            .expect("generation write should prime OceanFloorWg");
        assert_eq!(ocean_floor.get_first_available(3, 7), 6);
    }

    #[test]
    fn initialized_proto_generation_relative_write_keeps_counts_ready() {
        init_test_registry();
        init_behaviors();
        let proto = ProtoChunk::new(
            Sections::from_owned(vec![ChunkSection::new_empty()].into_boxed_slice()),
            ChunkPos::new(0, 0),
            0,
            16,
            Weak::new(),
        );
        proto.set_status(ChunkStatus::InitializeLight);
        let stone = REGISTRY.blocks.get_default_state_id(&vanilla_blocks::STONE);
        let chunk = ChunkAccess::Proto(proto);

        chunk.set_relative_block_for_generation(3, 5, 7, stone);

        let ChunkAccess::Proto(proto) = &chunk else {
            panic!("test chunk should remain proto");
        };
        assert_eq!(proto.sections.sections[0].read().non_empty_block_count(), 1);
    }

    #[test]
    fn batched_generation_column_writes_update_proto_heightmaps() {
        init_test_registry();
        let proto = ProtoChunk::new(
            Sections::from_owned(vec![ChunkSection::new_empty()].into_boxed_slice()),
            ChunkPos::new(0, 0),
            0,
            16,
            Weak::new(),
        );
        let stone = REGISTRY.blocks.get_default_state_id(&vanilla_blocks::STONE);
        let air = REGISTRY.blocks.get_default_state_id(&vanilla_blocks::AIR);
        let chunk = ChunkAccess::Proto(proto);

        chunk
            .sections()
            .write_column_blocks(3, 7, &[(5, stone), (8, stone)]);
        chunk.mark_dirty();
        chunk.update_heightmaps_after_direct_column_writes(3, 7, &[(5, stone), (8, stone)]);

        let ChunkAccess::Proto(proto) = &chunk else {
            panic!("test chunk should remain proto");
        };
        let heightmaps = proto.heightmaps.read();
        let ocean_floor = heightmaps
            .get(HeightmapType::OceanFloorWg)
            .expect("batched generation writes should prime OceanFloorWg");
        assert_eq!(ocean_floor.get_first_available(3, 7), 9);
        drop(heightmaps);

        chunk.sections().write_column_blocks(3, 7, &[(8, air)]);
        chunk.mark_dirty();
        chunk.update_heightmaps_after_direct_column_writes(3, 7, &[(8, air)]);

        let heightmaps = proto.heightmaps.read();
        let ocean_floor = heightmaps
            .get(HeightmapType::OceanFloorWg)
            .expect("OceanFloorWg should remain present");
        assert_eq!(ocean_floor.get_first_available(3, 7), 6);
    }

    #[test]
    fn initialized_proto_generation_batch_writes_keep_counts_ready() {
        init_test_registry();
        init_behaviors();
        let proto = ProtoChunk::new(
            Sections::from_owned(vec![ChunkSection::new_empty()].into_boxed_slice()),
            ChunkPos::new(0, 0),
            0,
            16,
            Weak::new(),
        );
        proto.set_status(ChunkStatus::InitializeLight);
        let stone = REGISTRY.blocks.get_default_state_id(&vanilla_blocks::STONE);
        let chunk = ChunkAccess::Proto(proto);

        chunk.write_block_batch_for_generation(&[(1, 2, 3, stone), (4, 5, 6, stone)]);
        chunk.write_column_blocks_for_generation(7, 8, &[(9, stone)]);

        let ChunkAccess::Proto(proto) = &chunk else {
            panic!("test chunk should remain proto");
        };
        assert_eq!(proto.sections.sections[0].read().non_empty_block_count(), 3);
    }

    #[test]
    fn initialize_light_sources_reads_direct_generation_writes() {
        init_test_registry();
        init_behaviors();
        let proto = ProtoChunk::new(
            Sections::from_owned(vec![ChunkSection::new_empty()].into_boxed_slice()),
            ChunkPos::new(0, 0),
            0,
            16,
            Weak::new(),
        );
        let stone = REGISTRY.blocks.get_default_state_id(&vanilla_blocks::STONE);
        let chunk = ChunkAccess::Proto(proto);

        chunk.set_relative_block_for_generation(0, 4, 0, stone);
        chunk.initialize_light_sources();

        assert_eq!(chunk.sky_light_sources().get_lowest_source_y(0, 0), 5);
    }

    #[test]
    fn full_chunk_heightmap_type_maps_worldgen_types_to_final_types() {
        assert_eq!(
            ChunkAccess::full_chunk_heightmap_type(HeightmapType::WorldSurfaceWg),
            HeightmapType::WorldSurface
        );
        assert_eq!(
            ChunkAccess::full_chunk_heightmap_type(HeightmapType::OceanFloorWg),
            HeightmapType::OceanFloor
        );
        assert_eq!(
            ChunkAccess::full_chunk_heightmap_type(HeightmapType::MotionBlocking),
            HeightmapType::MotionBlocking
        );
    }

    #[test]
    fn public_relative_write_keeps_full_chunk_serializable() {
        init_test_registry();
        init_behaviors();
        let chunk = ChunkAccess::Full(LevelChunk::from_disk(
            Sections::from_owned(vec![ChunkSection::new_empty()].into_boxed_slice()),
            ChunkPos::new(0, 0),
            0,
            16,
            Weak::new(),
            BlockTickList::new(),
            FluidTickList::new(),
            ChunkHeightmaps::new(0, 16),
            StructureStartMap::default(),
            StructureReferenceMap::default(),
            ChunkLightData::for_valid_world_height(0, 16),
        ));
        let stone = REGISTRY.blocks.get_default_state_id(&vanilla_blocks::STONE);

        chunk.set_relative_block(3, 5, 7, stone);

        let ChunkAccess::Full(level_chunk) = &chunk else {
            panic!("test chunk should remain full");
        };
        let _ = level_chunk.extract_chunk_data();
    }

    #[test]
    fn full_chunk_light_emptiness_map_tracks_public_relative_writes() {
        init_test_registry();
        init_behaviors();
        let chunk = ChunkAccess::Full(LevelChunk::from_disk(
            Sections::from_owned(vec![ChunkSection::new_empty()].into_boxed_slice()),
            ChunkPos::new(0, 0),
            0,
            16,
            Weak::new(),
            BlockTickList::new(),
            FluidTickList::new(),
            ChunkHeightmaps::new(0, 16),
            StructureStartMap::default(),
            StructureReferenceMap::default(),
            ChunkLightData::for_valid_world_height(0, 16),
        ));
        let stone = REGISTRY.blocks.get_default_state_id(&vanilla_blocks::STONE);
        let air = REGISTRY.blocks.get_default_state_id(&vanilla_blocks::AIR);

        let ChunkAccess::Full(level_chunk) = &chunk else {
            panic!("test chunk should remain full");
        };
        {
            let light = level_chunk.light.read();
            assert_eq!(light.block.emptiness_map(), Some(&[true][..]));
            assert_eq!(light.sky.emptiness_map(), Some(&[true][..]));
        }

        chunk.set_relative_block(3, 5, 7, stone);
        {
            let light = level_chunk.light.read();
            assert_eq!(light.block.emptiness_map(), Some(&[false][..]));
            assert_eq!(light.sky.emptiness_map(), Some(&[false][..]));
        }

        chunk.set_relative_block(3, 5, 7, air);
        let light = level_chunk.light.read();
        assert_eq!(light.block.emptiness_map(), Some(&[true][..]));
        assert_eq!(light.sky.emptiness_map(), Some(&[true][..]));
    }

    #[test]
    fn loaded_proto_light_emptiness_map_tracks_set_block_state_after_initialize_light() {
        init_test_registry();
        init_behaviors();
        let proto = ProtoChunk::from_disk(
            Sections::from_owned(vec![ChunkSection::new_empty()].into_boxed_slice()),
            ChunkPos::new(0, 0),
            ChunkStatus::InitializeLight,
            0,
            16,
            StructureStartMap::default(),
            StructureReferenceMap::default(),
            None,
            Vec::new(),
            BlockTickList::new(),
            FluidTickList::new(),
            Weak::new(),
            ChunkLightData::for_valid_world_height(0, 16),
        );
        let stone = REGISTRY.blocks.get_default_state_id(&vanilla_blocks::STONE);

        {
            let light = proto.light.read();
            assert_eq!(light.block.emptiness_map(), Some(&[true][..]));
            assert_eq!(light.sky.emptiness_map(), Some(&[true][..]));
        }

        proto.set_block_state(BlockPos::new(3, 5, 7), stone, UpdateFlags::UPDATE_NONE);

        assert_eq!(proto.sky_light_sources.read().get_lowest_source_y(3, 7), 6);
        let light = proto.light.read();
        assert_eq!(light.block.emptiness_map(), Some(&[false][..]));
        assert_eq!(light.sky.emptiness_map(), Some(&[false][..]));
    }

    #[test]
    fn full_chunk_postprocessing_mark_is_vanilla_noop() {
        init_test_registry();
        init_behaviors();
        let chunk = ChunkAccess::Full(LevelChunk::from_disk(
            Sections::from_owned(vec![ChunkSection::new_empty()].into_boxed_slice()),
            ChunkPos::new(0, 0),
            0,
            16,
            Weak::new(),
            BlockTickList::new(),
            FluidTickList::new(),
            ChunkHeightmaps::new(0, 16),
            StructureStartMap::default(),
            StructureReferenceMap::default(),
            ChunkLightData::for_valid_world_height(0, 16),
        ));

        chunk.mark_pos_for_postprocessing(BlockPos::new(1, 2, 3));
    }

    #[test]
    fn full_block_change_updates_sky_light_sources() {
        init_test_registry();
        init_behaviors();
        let chunk = ChunkAccess::Full(LevelChunk::from_disk(
            Sections::from_owned(vec![ChunkSection::new_empty()].into_boxed_slice()),
            ChunkPos::new(0, 0),
            0,
            16,
            Weak::new(),
            BlockTickList::new(),
            FluidTickList::new(),
            ChunkHeightmaps::new(0, 16),
            StructureStartMap::default(),
            StructureReferenceMap::default(),
            ChunkLightData::for_valid_world_height(0, 16),
        ));
        let stone = REGISTRY.blocks.get_default_state_id(&vanilla_blocks::STONE);

        chunk.set_block_state(BlockPos::new(0, 4, 0), stone, UpdateFlags::UPDATE_CLIENTS);

        assert_eq!(chunk.sky_light_sources().get_lowest_source_y(0, 0), 5);
    }
}
