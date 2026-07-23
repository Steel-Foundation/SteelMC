//! Region access for chunk feature generation.
//!
//! Feature placement needs the center chunk plus its direct dependencies, while writes must
//! stay inside the stage's block-state write radius. `WorldGenRegion` centralizes that
//! contract so feature, structure, and vegetation code cannot bypass the chunk pyramid.

use std::{
    cell::RefCell,
    sync::{
        Arc, Weak,
        atomic::{AtomicI64, Ordering},
    },
    time::Instant,
};

use parking_lot::{RwLockReadGuard, RwLockWriteGuard};
use simdnbt::owned::NbtCompound;
use steel_registry::{
    REGISTRY, block_entity_type::BlockEntityTypeRef, blocks::BlockRef,
    blocks::block_state_ext::BlockStateExt as _, fluid::FluidRef, vanilla_blocks,
};
use steel_utils::random::RandomSource;
use steel_utils::{
    BlockPos, BlockStateId, ChunkPos, PackedSectionBlockPos, SectionPos, types::UpdateFlags,
};

use crate::behavior::FLUID_BEHAVIORS;
use crate::block_entity::{BLOCK_ENTITIES, SharedBlockEntity};
use crate::chunk::{
    chunk_access::{ChunkAccess, ChunkStatus},
    chunk_generation_task::StaticCache2D,
    chunk_holder::ChunkHolder,
    chunk_pyramid::ChunkStep,
    heightmap::{Heightmap, HeightmapType},
    section::{ChunkSection, SectionHolder},
};
use crate::entity::SharedEntity;
use crate::world::tick_scheduler::TickPriority;
use crate::world::{LevelAccessor, LevelReader, ScheduledTickAccess, World};
use crate::worldgen::context::WorldGenContext;
use crate::worldgen::feature::instrumentation::OreFeatureStats;

/// Chunk-cache backed worldgen view for the current generation step.
///
/// This deliberately differs from vanilla's `WorldGenRegion` in one area: it only exposes
/// chunks already collected by Steel's `StaticCache2D` and validated against `ChunkStep`.
/// That keeps generation deterministic and makes missing dependency declarations fail at
/// the region boundary instead of silently reading farther chunks.
pub struct WorldGenRegion<'a> {
    context: &'a WorldGenContext,
    step: &'a ChunkStep,
    cache: &'a StaticCache2D<Arc<ChunkHolder>>,
    center: ChunkPos,
    chunk_cache_radius: i32,
    chunks: RefCell<Box<[Option<CachedWorldGenChunk<'a>>]>>,
    worldgen_heightmaps: RefCell<Box<[CachedWorldgenHeightmaps]>>,
    random: RandomSource,
    sub_tick_count: AtomicI64,
}

/// Cached section-level access for feature code that mirrors vanilla `BulkSectionAccess`.
///
/// Vanilla exposes acquired `LevelChunkSection`s and lets some features mutate section-local
/// block states directly. Steel keeps chunk guards cached instead of section references because
/// sections live behind Rust locks; callers still get the same direct section semantics without
/// bypassing the worldgen dependency and write-radius checks.
pub(crate) struct WorldGenBulkSectionAccess<'region, 'world, 'profile> {
    region: &'region WorldGenRegion<'world>,
    chunk_cache_radius: i32,
    chunks: Box<[Option<CachedWorldGenChunk<'region>>]>,
    air: BlockStateId,
    ore_profile: Option<&'profile RefCell<OreFeatureStats>>,
}

struct CachedWorldGenChunk<'region> {
    guard: RwLockReadGuard<'region, ChunkAccess>,
    verified_status: ChunkStatus,
}

#[derive(Default)]
struct CachedWorldgenHeightmaps {
    world_surface_wg: Option<Box<[i32; 256]>>,
    ocean_floor_wg: Option<Box<[i32; 256]>>,
}

impl CachedWorldgenHeightmaps {
    const fn supports(heightmap_type: HeightmapType) -> bool {
        matches!(
            heightmap_type,
            HeightmapType::WorldSurfaceWg | HeightmapType::OceanFloorWg
        )
    }

    fn get(&self, heightmap_type: HeightmapType) -> Option<&[i32; 256]> {
        match heightmap_type {
            HeightmapType::WorldSurfaceWg => self.world_surface_wg.as_deref(),
            HeightmapType::OceanFloorWg => self.ocean_floor_wg.as_deref(),
            _ => None,
        }
    }

    fn set(&mut self, heightmap_type: HeightmapType, columns: Box<[i32; 256]>) {
        match heightmap_type {
            HeightmapType::WorldSurfaceWg => self.world_surface_wg = Some(columns),
            HeightmapType::OceanFloorWg => self.ocean_floor_wg = Some(columns),
            _ => {}
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
struct WritableSectionKey {
    chunk_x: i32,
    chunk_z: i32,
    status: ChunkStatus,
    section_index: usize,
}

impl<'a> WorldGenRegion<'a> {
    /// Creates a new region over the chunks collected for a generation step.
    #[must_use]
    pub fn new(
        context: &'a WorldGenContext,
        step: &'a ChunkStep,
        cache: &'a StaticCache2D<Arc<ChunkHolder>>,
        center: ChunkPos,
        random: RandomSource,
    ) -> Self {
        let chunk_cache_radius =
            i32::try_from(step.direct_dependencies.get_radius()).unwrap_or(i32::MAX);
        let chunk_cache_size = chunk_cache_radius.saturating_mul(2).saturating_add(1);
        let chunk_cache_len =
            usize::try_from(chunk_cache_size.saturating_mul(chunk_cache_size)).unwrap_or(0);
        let chunks = (0..chunk_cache_len).map(|_| None).collect();
        let worldgen_heightmaps = (0..chunk_cache_len)
            .map(|_| CachedWorldgenHeightmaps::default())
            .collect();

        Self {
            context,
            step,
            cache,
            center,
            chunk_cache_radius,
            chunks: RefCell::new(chunks),
            worldgen_heightmaps: RefCell::new(worldgen_heightmaps),
            random,
            sub_tick_count: AtomicI64::new(0),
        }
    }

    /// Returns the center chunk being generated.
    #[must_use]
    pub const fn center(&self) -> ChunkPos {
        self.center
    }

    /// Returns the random source exposed by vanilla `WorldGenRegion.getRandom()`.
    pub const fn random_mut(&mut self) -> &mut RandomSource {
        &mut self.random
    }

    /// Returns the minimum build height.
    #[must_use]
    pub const fn min_y(&self) -> i32 {
        self.context.min_y()
    }

    /// Returns the world height.
    #[must_use]
    pub const fn height(&self) -> i32 {
        self.context.height()
    }

    /// Returns the minimum Y coordinate used by vanilla `WorldGenerationContext`.
    #[must_use]
    pub fn generation_min_y(&self) -> i32 {
        self.context.generation_min_y()
    }

    /// Returns the vertical generation depth used by vanilla `WorldGenerationContext`.
    #[must_use]
    pub fn generation_height(&self) -> i32 {
        self.context.generation_height()
    }

    /// Returns this dimension's sea level.
    #[must_use]
    pub const fn sea_level(&self) -> i32 {
        self.context.sea_level()
    }

    /// Returns the world seed.
    #[must_use]
    pub fn seed(&self) -> i64 {
        self.context.world().seed()
    }

    /// Returns the weak world reference used by generated chunks and entities.
    #[must_use]
    pub fn weak_world(&self) -> Weak<World> {
        self.context.weak_world()
    }

    /// Returns block light as seen by feature-stage worldgen.
    ///
    /// Vanilla routes this through the level light engine from `WorldGenRegion`, but block light
    /// is not generated for the feature-stage proto chunks. Treating the region as dark keeps
    /// snow and freeze checks aligned with vanilla feature placement.
    #[must_use]
    #[expect(
        clippy::unused_self,
        reason = "keeps light lookup callable through the region instance"
    )]
    pub const fn block_light_at(&self, _pos: BlockPos) -> u8 {
        0
    }

    /// Returns the exclusive maximum build height.
    #[must_use]
    pub const fn max_y_exclusive(&self) -> i32 {
        self.min_y() + self.height()
    }

    /// Checks if a Y coordinate is outside the build height.
    #[must_use]
    pub const fn is_outside_build_height(&self, y: i32) -> bool {
        y < self.min_y() || y >= self.max_y_exclusive()
    }

    /// Returns the strongest status directly available for a chunk position in this step.
    #[must_use]
    pub const fn required_status_at(&self, chunk_x: i32, chunk_z: i32) -> Option<ChunkStatus> {
        self.step
            .direct_dependencies
            .get(Self::chessboard_distance(self.center, chunk_x, chunk_z))
    }

    /// Returns whether block writes are allowed in the given chunk.
    #[must_use]
    pub const fn can_write_to_chunk(&self, chunk_x: i32, chunk_z: i32) -> bool {
        let radius = self.step.block_state_write_radius;
        radius >= 0
            && (chunk_x - self.center.0.x).abs() <= radius
            && (chunk_z - self.center.0.y).abs() <= radius
    }

    /// Gets a chunk if the step declares enough direct dependency status for it.
    #[must_use]
    pub fn try_chunk(
        &self,
        chunk_x: i32,
        chunk_z: i32,
        status: ChunkStatus,
    ) -> Option<RwLockReadGuard<'a, ChunkAccess>> {
        let available_status = self.required_status_at(chunk_x, chunk_z)?;
        if status > available_status {
            return None;
        }

        self.cache.get(chunk_x, chunk_z).try_chunk(available_status)
    }

    /// Gets a chunk or panics if generation requested an undeclared dependency.
    ///
    /// # Panics
    /// Panics if the chunk is outside this step's direct dependencies, if the requested
    /// status is higher than the dependency contract, or if the holder has not reached
    /// the declared status. Those cases indicate a chunk-pyramid or scheduler bug.
    pub fn chunk(
        &self,
        chunk_x: i32,
        chunk_z: i32,
        status: ChunkStatus,
    ) -> RwLockReadGuard<'a, ChunkAccess> {
        let Some(chunk) = self.try_chunk(chunk_x, chunk_z, status) else {
            let available = self.required_status_at(chunk_x, chunk_z);
            panic!(
                "Worldgen requested chunk ({chunk_x}, {chunk_z}) at status {status:?}, \
                 but the {:?} step only provides {available:?} at that distance from ({}, {})",
                self.step.target_status, self.center.0.x, self.center.0.y
            );
        };

        chunk
    }

    fn with_cached_chunk<R>(
        &self,
        chunk_x: i32,
        chunk_z: i32,
        status: ChunkStatus,
        f: impl FnOnce(&ChunkAccess) -> R,
    ) -> R {
        let Some(cache_index) = self.chunk_cache_index(chunk_x, chunk_z) else {
            let chunk = self.chunk(chunk_x, chunk_z, status);
            return f(&chunk);
        };

        let cache_needs_update = self.chunks.borrow().get(cache_index).is_none_or(|cached| {
            cached
                .as_ref()
                .is_none_or(|cached| status > cached.verified_status)
        });

        if cache_needs_update {
            let guard = self.chunk(chunk_x, chunk_z, status);
            let mut chunks = self.chunks.borrow_mut();
            let Some(slot) = chunks.get_mut(cache_index) else {
                panic!("Worldgen region cache index {cache_index} escaped its storage");
            };
            if let Some(cached) = slot {
                cached.verified_status = status;
                drop(guard);
            } else {
                *slot = Some(CachedWorldGenChunk {
                    guard,
                    verified_status: status,
                });
            }
        }

        let chunks = self.chunks.borrow();
        let Some(Some(cached)) = chunks.get(cache_index) else {
            panic!("Worldgen region cache failed to store chunk ({chunk_x}, {chunk_z})");
        };
        f(&cached.guard)
    }

    fn chunk_cache_index(&self, chunk_x: i32, chunk_z: i32) -> Option<usize> {
        let radius = self.chunk_cache_radius;
        let size = radius.checked_mul(2)?.checked_add(1)?;
        let rel_x = chunk_x.checked_sub(self.center.0.x)?.checked_add(radius)?;
        let rel_z = chunk_z.checked_sub(self.center.0.y)?.checked_add(radius)?;
        if rel_x < 0 || rel_x >= size || rel_z < 0 || rel_z >= size {
            return None;
        }

        usize::try_from(rel_z.checked_mul(size)?.checked_add(rel_x)?).ok()
    }

    /// Gets a block state through the region dependency contract.
    ///
    /// # Panics
    /// Panics if the position's chunk is outside this step's direct dependencies.
    #[must_use]
    pub fn block_state(&self, pos: BlockPos) -> BlockStateId {
        let chunk_x = SectionPos::block_to_section_coord(pos.x());
        let chunk_z = SectionPos::block_to_section_coord(pos.z());
        self.with_cached_chunk(chunk_x, chunk_z, ChunkStatus::Empty, |chunk| {
            chunk.get_block_state(pos)
        })
    }

    /// Gets a block entity through the region dependency contract.
    ///
    /// # Panics
    /// Panics if the position's chunk is outside this step's direct dependencies.
    #[must_use]
    pub fn block_entity(&self, pos: BlockPos) -> Option<SharedBlockEntity> {
        let chunk_x = SectionPos::block_to_section_coord(pos.x());
        let chunk_z = SectionPos::block_to_section_coord(pos.z());
        self.with_cached_chunk(chunk_x, chunk_z, ChunkStatus::Empty, |chunk| {
            chunk.get_block_entity(pos)
        })
    }

    /// Gets the biome id at quart coordinates through the region dependency contract.
    ///
    /// The vertical quart coordinate is clamped to the chunk's biome section range,
    /// matching vanilla `ChunkAccess.getNoiseBiome`.
    ///
    /// # Panics
    /// Panics if the position's chunk is outside this step's direct dependencies.
    #[must_use]
    pub fn noise_biome_id(&self, quart_x: i32, quart_y: i32, quart_z: i32) -> u16 {
        let chunk_x = quart_x >> 2;
        let chunk_z = quart_z >> 2;
        let local_quart_x = (quart_x & 3) as usize;
        let local_quart_z = (quart_z & 3) as usize;

        self.with_cached_chunk(chunk_x, chunk_z, ChunkStatus::Biomes, |chunk| {
            let sections = chunk.sections();
            let (section_index, local_quart_y) =
                Self::biome_quart_y_indices(self.min_y(), sections.sections.len(), quart_y);
            let section = &sections.sections[section_index];

            section
                .read()
                .biomes
                .get(local_quart_x, local_quart_y, local_quart_z)
        })
    }

    /// Sets a block state if the position is inside the step's write radius.
    ///
    /// Returns whether the write was accepted by the region. Positions outside the write
    /// radius are rejected without touching chunk data, matching vanilla's
    /// `WorldGenRegion.ensureCanWrite` gate.
    ///
    /// # Panics
    /// Panics if a position inside the write radius is not covered by this step's
    /// direct dependencies, or if the holder has not reached the declared status.
    #[must_use]
    pub fn set_block_state(&self, pos: BlockPos, state: BlockStateId, flags: UpdateFlags) -> bool {
        let Some((chunk_x, chunk_z, status)) = self.writable_chunk_for_pos(pos, "write block")
        else {
            return false;
        };

        self.with_cached_chunk(chunk_x, chunk_z, status, |chunk| {
            chunk.set_block_state(pos, state, flags);
        });
        if !flags.contains(UpdateFlags::UPDATE_KNOWN_SHAPE)
            && let Some(postprocess_pos) = Self::postprocess_pos_for_state(state, pos)
        {
            self.mark_pos_for_postprocessing(postprocess_pos);
        }
        true
    }

    /// Mirrors the vanilla `Blocks` post-process hooks that can affect worldgen output.
    ///
    /// Vanilla hardcodes these callbacks in `Blocks.java`: mushrooms postprocess themselves,
    /// while soul sand and magma blocks postprocess the block above.
    fn postprocess_pos_for_state(state: BlockStateId, pos: BlockPos) -> Option<BlockPos> {
        let block = state.get_block();
        if block == &vanilla_blocks::BROWN_MUSHROOM || block == &vanilla_blocks::RED_MUSHROOM {
            Some(pos)
        } else if block == &vanilla_blocks::SOUL_SAND || block == &vanilla_blocks::MAGMA_BLOCK {
            Some(pos.above())
        } else {
            None
        }
    }

    /// Attaches block entity data at a writable worldgen position.
    ///
    /// This mirrors vanilla's feature paths that place a block first, then configure its block
    /// entity. If Steel does not have concrete behavior for the type yet, the raw fallback keeps
    /// the NBT intact for later save/load.
    #[must_use]
    pub fn set_block_entity_data(
        &self,
        pos: BlockPos,
        block_entity_type: BlockEntityTypeRef,
        state: BlockStateId,
        nbt: NbtCompound,
    ) -> bool {
        let Some((chunk_x, chunk_z, status)) =
            self.writable_chunk_for_pos(pos, "write block entity")
        else {
            return false;
        };

        self.with_cached_chunk(chunk_x, chunk_z, status, |chunk| {
            let entity = BLOCK_ENTITIES.create_and_load_owned_or_raw(
                block_entity_type,
                chunk.level_weak(),
                pos,
                state,
                nbt,
            );
            chunk.add_and_register_block_entity(entity);
        });
        true
    }

    /// Removes block entity data at a writable worldgen position.
    #[must_use]
    pub fn remove_block_entity(&self, pos: BlockPos) -> bool {
        let Some((chunk_x, chunk_z, status)) =
            self.writable_chunk_for_pos(pos, "remove block entity")
        else {
            return false;
        };

        self.with_cached_chunk(chunk_x, chunk_z, status, |chunk| {
            chunk.remove_block_entity(pos);
        });
        true
    }

    /// Adds an entity to the chunk that owns its position.
    ///
    /// Vanilla `WorldGenRegion.addFreshEntity` does not call `ensureCanWrite`, so entity
    /// insertion is allowed anywhere covered by the generation step's chunk dependencies.
    #[must_use]
    pub fn add_fresh_entity(&self, entity: SharedEntity) -> bool {
        let pos = BlockPos::from(entity.position());
        let (chunk_x, chunk_z, status) = self.dependency_chunk_for_pos(pos, "add entity");

        self.with_cached_chunk(chunk_x, chunk_z, status, |chunk| chunk.add_entity(entity))
    }

    /// Schedules a block tick in the chunk that owns the target position.
    ///
    /// Vanilla `WorldGenTickAccess` resolves the owning chunk directly and does not apply
    /// `WorldGenRegion.ensureCanWrite`, so ticks can be recorded outside the block write radius
    /// as long as the generation step declared the chunk dependency.
    #[must_use]
    pub fn schedule_block_tick(
        &self,
        pos: BlockPos,
        block: BlockRef,
        delay: i32,
        priority: TickPriority,
    ) -> bool {
        let (chunk_x, chunk_z, status) = self.dependency_chunk_for_pos(pos, "schedule block tick");
        let sub_tick_order = self.sub_tick_count.fetch_add(1, Ordering::Relaxed);
        self.with_cached_chunk(chunk_x, chunk_z, status, |chunk| {
            chunk.schedule_block_tick(pos, block, delay, priority, sub_tick_order);
        });
        true
    }

    /// Schedules a block tick with vanilla's normal priority.
    #[must_use]
    pub fn schedule_block_tick_default(&self, pos: BlockPos, block: BlockRef, delay: i32) -> bool {
        self.schedule_block_tick(pos, block, delay, TickPriority::Normal)
    }

    /// Schedules a fluid tick in the chunk that owns the target position.
    ///
    /// This mirrors vanilla tick scheduling and intentionally does not apply the block write radius.
    #[must_use]
    pub fn schedule_fluid_tick(
        &self,
        pos: BlockPos,
        fluid: FluidRef,
        delay: i32,
        priority: TickPriority,
    ) -> bool {
        let (chunk_x, chunk_z, status) = self.dependency_chunk_for_pos(pos, "schedule fluid tick");
        let sub_tick_order = self.sub_tick_count.fetch_add(1, Ordering::Relaxed);
        self.with_cached_chunk(chunk_x, chunk_z, status, |chunk| {
            chunk.schedule_fluid_tick(pos, fluid, delay, priority, sub_tick_order);
        });
        true
    }

    /// Schedules a fluid tick with vanilla's normal priority.
    #[must_use]
    pub fn schedule_fluid_tick_default(&self, pos: BlockPos, fluid: FluidRef, delay: i32) -> bool {
        self.schedule_fluid_tick(pos, fluid, delay, TickPriority::Normal)
    }

    /// Marks a position for vanilla proto-chunk postprocessing after full promotion.
    ///
    /// # Panics
    /// Panics if the target chunk is outside this step's direct dependencies.
    pub fn mark_pos_for_postprocessing(&self, pos: BlockPos) {
        let chunk_x = SectionPos::block_to_section_coord(pos.x());
        let chunk_z = SectionPos::block_to_section_coord(pos.z());
        self.with_cached_chunk(chunk_x, chunk_z, ChunkStatus::Empty, |chunk| {
            chunk.mark_pos_for_postprocessing(pos);
        });
    }

    /// Gets the first available Y coordinate for a heightmap column.
    ///
    /// Mirrors vanilla `WorldGenRegion.getHeight`, which requests the target
    /// chunk at `EMPTY` and then reads whichever generated status the step
    /// dependency cache already holds for that chunk.
    #[must_use]
    pub fn height_at(&self, heightmap_type: HeightmapType, x: i32, z: i32) -> i32 {
        let chunk_x = SectionPos::block_to_section_coord(x);
        let chunk_z = SectionPos::block_to_section_coord(z);
        let local_x = (x & 15) as usize;
        let local_z = (z & 15) as usize;
        let column_index = local_x + local_z * 16;

        if let Some(height) =
            self.cached_worldgen_height_at(heightmap_type, chunk_x, chunk_z, column_index)
        {
            return height;
        }

        self.with_cached_chunk(chunk_x, chunk_z, ChunkStatus::Empty, |chunk| {
            chunk.height_at(heightmap_type, local_x, local_z)
        })
    }

    fn cached_worldgen_height_at(
        &self,
        heightmap_type: HeightmapType,
        chunk_x: i32,
        chunk_z: i32,
        column_index: usize,
    ) -> Option<i32> {
        if !CachedWorldgenHeightmaps::supports(heightmap_type) {
            return None;
        }
        let cache_index = self.chunk_cache_index(chunk_x, chunk_z)?;

        {
            let heightmaps = self.worldgen_heightmaps.borrow();
            let Some(cached) = heightmaps.get(cache_index) else {
                panic!("Worldgen heightmap cache index {cache_index} escaped its storage");
            };
            if let Some(columns) = cached.get(heightmap_type) {
                return Some(columns[column_index]);
            }
        }

        let columns = self.load_worldgen_heightmap_columns(chunk_x, chunk_z, heightmap_type);
        let height = columns[column_index];
        let mut heightmaps = self.worldgen_heightmaps.borrow_mut();
        let Some(cached) = heightmaps.get_mut(cache_index) else {
            panic!("Worldgen heightmap cache index {cache_index} escaped its storage");
        };
        cached.set(heightmap_type, columns);
        Some(height)
    }

    fn load_worldgen_heightmap_columns(
        &self,
        chunk_x: i32,
        chunk_z: i32,
        heightmap_type: HeightmapType,
    ) -> Box<[i32; 256]> {
        self.with_cached_chunk(chunk_x, chunk_z, ChunkStatus::Empty, |chunk| {
            Self::worldgen_heightmap_columns_from_chunk(chunk, heightmap_type)
        })
    }

    fn worldgen_heightmap_columns_from_chunk(
        chunk: &ChunkAccess,
        heightmap_type: HeightmapType,
    ) -> Box<[i32; 256]> {
        let ChunkAccess::Proto(proto) = chunk else {
            let mut columns = Box::new([0; 256]);
            for local_z in 0..16 {
                for local_x in 0..16 {
                    columns[local_x + local_z * 16] =
                        chunk.height_at(heightmap_type, local_x, local_z);
                }
            }
            return columns;
        };

        {
            let heightmaps = proto.heightmaps.read();
            if let Some(heightmap) = heightmaps.get(heightmap_type) {
                return Self::copy_heightmap_columns(heightmap, proto.min_y());
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

        Self::copy_heightmap_columns(heightmap, proto.min_y())
    }

    fn copy_heightmap_columns(heightmap: &Heightmap, min_y: i32) -> Box<[i32; 256]> {
        let mut columns = Box::new([0; 256]);
        for (index, &height) in heightmap.raw_data().iter().enumerate() {
            columns[index] = i32::from(height) + min_y;
        }
        columns
    }

    pub(crate) fn bulk_section_access_for_ore<'profile>(
        &self,
        profile: Option<&'profile RefCell<OreFeatureStats>>,
    ) -> WorldGenBulkSectionAccess<'_, 'a, 'profile> {
        WorldGenBulkSectionAccess::new(self, profile)
    }

    fn writable_chunk_for_pos(
        &self,
        pos: BlockPos,
        action: &str,
    ) -> Option<(i32, i32, ChunkStatus)> {
        let chunk_x = SectionPos::block_to_section_coord(pos.x());
        let chunk_z = SectionPos::block_to_section_coord(pos.z());

        if !self.can_write_to_chunk(chunk_x, chunk_z) {
            log::error!(
                "Worldgen attempted to {action} at ({}, {}, {}) outside {:?} write radius {} centered on ({}, {})",
                pos.x(),
                pos.y(),
                pos.z(),
                self.step.target_status,
                self.step.block_state_write_radius,
                self.center.0.x,
                self.center.0.y,
            );
            return None;
        }

        let Some(status) = self.required_status_at(chunk_x, chunk_z) else {
            panic!(
                "Worldgen attempted to {action} at ({}, {}, {}) in chunk ({chunk_x}, {chunk_z}), \
                 but {:?} declares no direct dependency for that chunk",
                pos.x(),
                pos.y(),
                pos.z(),
                self.step.target_status,
            );
        };

        Some((chunk_x, chunk_z, status))
    }

    fn dependency_chunk_for_pos(&self, pos: BlockPos, action: &str) -> (i32, i32, ChunkStatus) {
        let chunk_x = SectionPos::block_to_section_coord(pos.x());
        let chunk_z = SectionPos::block_to_section_coord(pos.z());
        let Some(status) = self.required_status_at(chunk_x, chunk_z) else {
            panic!(
                "Worldgen attempted to {action} at ({}, {}, {}) in chunk ({chunk_x}, {chunk_z}), \
                 but {:?} declares no direct dependency for that chunk",
                pos.x(),
                pos.y(),
                pos.z(),
                self.step.target_status,
            );
        };

        (chunk_x, chunk_z, status)
    }

    const fn chessboard_distance(center: ChunkPos, chunk_x: i32, chunk_z: i32) -> usize {
        let dx = abs_diff(center.0.x, chunk_x);
        let dz = abs_diff(center.0.y, chunk_z);
        if dx > dz { dx as usize } else { dz as usize }
    }

    fn biome_quart_y_indices(min_y: i32, section_count: usize, quart_y: i32) -> (usize, usize) {
        let Some(total_quart_y) = section_count.checked_mul(4) else {
            panic!("Worldgen chunk section count {section_count} overflows biome quart range");
        };
        assert!(
            total_quart_y > 0,
            "Worldgen chunk must have at least one biome section"
        );

        let relative_quart_y = i64::from(quart_y) - i64::from(min_y >> 2);
        let max_relative_quart_y = total_quart_y - 1;
        let clamped_relative_quart_y = if relative_quart_y <= 0 {
            0
        } else {
            usize::try_from(relative_quart_y).map_or(max_relative_quart_y, |relative| {
                relative.min(max_relative_quart_y)
            })
        };

        (clamped_relative_quart_y / 4, clamped_relative_quart_y & 3)
    }
}

impl<'region, 'world, 'profile> WorldGenBulkSectionAccess<'region, 'world, 'profile> {
    fn new(
        region: &'region WorldGenRegion<'world>,
        ore_profile: Option<&'profile RefCell<OreFeatureStats>>,
    ) -> Self {
        let chunk_cache_radius = region.chunk_cache_radius;
        let chunk_cache_size = chunk_cache_radius.saturating_mul(2).saturating_add(1);
        let chunk_cache_len =
            usize::try_from(chunk_cache_size.saturating_mul(chunk_cache_size)).unwrap_or(0);
        let chunks = (0..chunk_cache_len).map(|_| None).collect();

        Self {
            region,
            chunk_cache_radius,
            chunks,
            air: REGISTRY.blocks.get_default_state_id(&vanilla_blocks::AIR),
            ore_profile,
        }
    }

    pub(crate) fn record_ore_candidate_position(&mut self) {
        self.with_ore_profile(OreFeatureStats::record_candidate_position);
    }

    pub(crate) fn record_ore_unique_position(&mut self) {
        self.with_ore_profile(OreFeatureStats::record_unique_position);
    }

    pub(crate) fn record_ore_write_allowed_position(&mut self) {
        self.with_ore_profile(OreFeatureStats::record_write_allowed_position);
    }

    pub(crate) fn ore_target_block_state(&mut self, pos: BlockPos) -> BlockStateId {
        self.with_ore_profile(OreFeatureStats::record_target_read);
        self.block_state(pos)
    }

    pub(crate) fn ore_neighbor_block_state(&mut self, pos: BlockPos) -> BlockStateId {
        self.with_ore_profile(OreFeatureStats::record_neighbor_read);
        self.block_state(pos)
    }

    /// Replaces an ore target block after reading it under the section write lock.
    ///
    /// This is only suitable for ore paths that do not need neighbor reads while deciding
    /// whether the replacement is allowed.
    #[must_use]
    pub(crate) fn replace_ore_target_block_state(
        &mut self,
        pos: BlockPos,
        replacement: impl FnOnce(BlockStateId) -> Option<BlockStateId>,
    ) -> bool {
        self.with_ore_profile(OreFeatureStats::record_target_read);
        let ore_profile = self.ore_profile;
        let started_at = ore_profile.map(|_| Instant::now());
        let Some(key) = self.writable_section_key(pos) else {
            Self::record_ore_write_time(ore_profile, started_at);
            return false;
        };

        let chunk = self.chunk(key.chunk_x, key.chunk_z, key.status);
        let Some(section) = chunk.guard.sections().sections.get(key.section_index) else {
            panic!(
                "Worldgen bulk section write at ({}, {}, {}) resolved missing section index {}",
                pos.x(),
                pos.y(),
                pos.z(),
                key.section_index
            );
        };

        let mut section_guard = Self::ore_section_write_guard(ore_profile, section, key);

        let local_x = Self::local_coord(pos.x());
        let local_y = Self::local_coord(pos.y());
        let local_z = Self::local_coord(pos.z());
        let old_state = section_guard.states.get(local_x, local_y, local_z);
        let Some(state) = replacement(old_state) else {
            Self::record_ore_write_time(ore_profile, started_at);
            return false;
        };

        let old_state = Self::set_bulk_block_state(
            &chunk.guard,
            &mut section_guard,
            local_x,
            local_y,
            local_z,
            state,
        );
        Self::with_ore_profile_ref(ore_profile, OreFeatureStats::record_write);
        if old_state != state {
            chunk.guard.mark_dirty();
        }

        Self::record_ore_write_time(ore_profile, started_at);
        true
    }

    /// Replaces already-filtered ore target positions that all belong to one section.
    pub(crate) fn replace_ore_target_block_states_in_section(
        &mut self,
        chunk_x: i32,
        chunk_z: i32,
        section_index: usize,
        positions: &[PackedSectionBlockPos],
        mut replacement: impl FnMut(BlockStateId) -> Option<BlockStateId>,
    ) -> u64 {
        let ore_profile = self.ore_profile;
        let started_at = ore_profile.map(|_| Instant::now());
        if !self.region.can_write_to_chunk(chunk_x, chunk_z) {
            Self::record_ore_write_time(ore_profile, started_at);
            return 0;
        }
        Self::with_ore_profile_ref(ore_profile, |profile| {
            profile.record_write_allowed_positions(positions.len() as u64);
        });
        let Some(status) = self.region.required_status_at(chunk_x, chunk_z) else {
            panic!(
                "Worldgen attempted to bulk write ore in chunk ({chunk_x}, {chunk_z}), \
                 but {:?} declares no direct dependency for that chunk",
                self.region.step.target_status,
            );
        };
        let key = WritableSectionKey {
            chunk_x,
            chunk_z,
            status,
            section_index,
        };
        let chunk = self.chunk(chunk_x, chunk_z, status);
        let Some(section) = chunk.guard.sections().sections.get(section_index) else {
            panic!(
                "Worldgen bulk section write in chunk ({chunk_x}, {chunk_z}) resolved missing section index {section_index}",
            );
        };

        let mut section_guard = Self::ore_section_write_guard(ore_profile, section, key);
        let mut placed = 0_u64;
        let mut dirty = false;

        if let Some(profile) = ore_profile {
            for &pos in positions {
                Self::with_ore_profile_ref(Some(profile), OreFeatureStats::record_target_read);
                let local_x = usize::from(pos.x());
                let local_y = usize::from(pos.y());
                let local_z = usize::from(pos.z());
                let old_state = section_guard.states.get(local_x, local_y, local_z);
                if let Some(state) = replacement(old_state) {
                    let old_state = Self::set_bulk_block_state(
                        &chunk.guard,
                        &mut section_guard,
                        local_x,
                        local_y,
                        local_z,
                        state,
                    );
                    Self::with_ore_profile_ref(Some(profile), OreFeatureStats::record_write);
                    dirty |= old_state != state;
                    placed += 1;
                }
            }
        } else {
            for &pos in positions {
                let local_x = usize::from(pos.x());
                let local_y = usize::from(pos.y());
                let local_z = usize::from(pos.z());
                let old_state = section_guard.states.get(local_x, local_y, local_z);
                if let Some(state) = replacement(old_state) {
                    let old_state = Self::set_bulk_block_state(
                        &chunk.guard,
                        &mut section_guard,
                        local_x,
                        local_y,
                        local_z,
                        state,
                    );
                    dirty |= old_state != state;
                    placed += 1;
                }
            }
        }

        drop(section_guard);
        if dirty {
            chunk.guard.mark_dirty();
        }

        Self::record_ore_write_time(ore_profile, started_at);
        placed
    }

    /// Reads a block state through cached section access.
    ///
    /// Out-of-height reads return air, matching vanilla `BulkSectionAccess.getBlockState`.
    #[must_use]
    pub(crate) fn block_state(&mut self, pos: BlockPos) -> BlockStateId {
        let ore_profile = self.ore_profile;
        let started_at = ore_profile.map(|_| Instant::now());
        let air = self.air;
        let Some(section_index) =
            Self::section_index(self.region.min_y(), self.region.height(), pos.y())
        else {
            Self::record_ore_read_time(ore_profile, started_at);
            return air;
        };

        let chunk_x = SectionPos::block_to_section_coord(pos.x());
        let chunk_z = SectionPos::block_to_section_coord(pos.z());
        let chunk = self.chunk(chunk_x, chunk_z, ChunkStatus::Empty);
        let Some(section) = chunk.guard.sections().sections.get(section_index) else {
            Self::record_ore_read_time(ore_profile, started_at);
            return air;
        };

        Self::with_ore_profile_ref(ore_profile, |profile| {
            profile.record_section_read_attempt(chunk_x, chunk_z, section_index);
        });
        let section_guard = if let Some(profile) = ore_profile {
            if let Some(guard) = section.section.try_read() {
                guard
            } else {
                if let Ok(mut profile) = profile.try_borrow_mut() {
                    profile.record_section_read_contention();
                }
                let wait_started_at = Instant::now();
                let guard = section.read();
                if let Ok(mut profile) = profile.try_borrow_mut() {
                    profile.record_read_contention_wait_time(wait_started_at.elapsed());
                }
                guard
            }
        } else {
            section.read()
        };
        if section_guard.states.has_only_air() {
            Self::record_ore_read_time(ore_profile, started_at);
            return air;
        }

        let state = section_guard.states.get(
            Self::local_coord(pos.x()),
            Self::local_coord(pos.y()),
            Self::local_coord(pos.z()),
        );
        Self::record_ore_read_time(ore_profile, started_at);
        state
    }

    /// Writes a block state directly to the containing section.
    ///
    /// This mirrors vanilla `BulkSectionAccess` by skipping heightmaps, neighbor updates,
    /// block entity callbacks, and other `WorldGenRegion.setBlock` side effects. Steel also
    /// defers section block counts until light initialization, matching the rest of its
    /// pre-light worldgen write paths.
    #[must_use]
    pub(crate) fn set_block_state(&mut self, pos: BlockPos, state: BlockStateId) -> bool {
        let ore_profile = self.ore_profile;
        let started_at = ore_profile.map(|_| Instant::now());
        let Some(key) = self.writable_section_key(pos) else {
            Self::record_ore_write_time(ore_profile, started_at);
            return false;
        };

        let chunk = self.chunk(key.chunk_x, key.chunk_z, key.status);
        let Some(section) = chunk.guard.sections().sections.get(key.section_index) else {
            panic!(
                "Worldgen bulk section write at ({}, {}, {}) resolved missing section index {}",
                pos.x(),
                pos.y(),
                pos.z(),
                key.section_index
            );
        };

        let mut section_guard = Self::ore_section_write_guard(ore_profile, section, key);
        let old_state = Self::set_bulk_block_state(
            &chunk.guard,
            &mut section_guard,
            Self::local_coord(pos.x()),
            Self::local_coord(pos.y()),
            Self::local_coord(pos.z()),
            state,
        );
        Self::with_ore_profile_ref(ore_profile, OreFeatureStats::record_write);
        if old_state != state {
            chunk.guard.mark_dirty();
        }

        Self::record_ore_write_time(ore_profile, started_at);
        true
    }

    fn writable_section_key(&self, pos: BlockPos) -> Option<WritableSectionKey> {
        let (chunk_x, chunk_z, status) = self
            .region
            .writable_chunk_for_pos(pos, "bulk write block")?;
        let section_index =
            Self::section_index(self.region.min_y(), self.region.height(), pos.y())?;
        Some(WritableSectionKey {
            chunk_x,
            chunk_z,
            status,
            section_index,
        })
    }

    fn set_bulk_block_state(
        chunk: &ChunkAccess,
        section: &mut ChunkSection,
        local_x: usize,
        local_y: usize,
        local_z: usize,
        state: BlockStateId,
    ) -> BlockStateId {
        if matches!(chunk, ChunkAccess::Proto(proto) if proto.status() < ChunkStatus::InitializeLight)
        {
            return section.set_block_state_for_generation(local_x, local_y, local_z, state);
        }

        section.finalize_generation_counts_if_needed();
        section.set_block_state(local_x, local_y, local_z, state)
    }

    fn ore_section_write_guard<'section>(
        ore_profile: Option<&RefCell<OreFeatureStats>>,
        section: &'section SectionHolder,
        key: WritableSectionKey,
    ) -> RwLockWriteGuard<'section, ChunkSection> {
        Self::with_ore_profile_ref(ore_profile, |profile| {
            profile.record_section_write_attempt(key.chunk_x, key.chunk_z, key.section_index);
        });
        if let Some(profile) = ore_profile {
            if let Some(guard) = section.section.try_write() {
                guard
            } else {
                if let Ok(mut profile) = profile.try_borrow_mut() {
                    profile.record_section_write_contention();
                }
                let wait_started_at = Instant::now();
                let guard = section.write();
                if let Ok(mut profile) = profile.try_borrow_mut() {
                    profile.record_write_contention_wait_time(wait_started_at.elapsed());
                }
                guard
            }
        } else {
            section.write()
        }
    }

    /// Returns whether a section-local write would be allowed for this position.
    #[must_use]
    pub(crate) const fn can_write_to_pos(&self, pos: BlockPos) -> bool {
        self.region.can_write_to_chunk(
            SectionPos::block_to_section_coord(pos.x()),
            SectionPos::block_to_section_coord(pos.z()),
        )
    }

    fn chunk(
        &mut self,
        chunk_x: i32,
        chunk_z: i32,
        status: ChunkStatus,
    ) -> &CachedWorldGenChunk<'region> {
        let Some(cache_index) = self.chunk_cache_index(chunk_x, chunk_z) else {
            panic!(
                "Worldgen bulk section requested chunk ({chunk_x}, {chunk_z}) outside the region cache centered on ({}, {})",
                self.region.center.0.x, self.region.center.0.y
            );
        };

        let cache_needs_insert = self.chunks.get(cache_index).is_none_or(Option::is_none);
        if cache_needs_insert {
            self.with_ore_profile(OreFeatureStats::record_chunk_cache_miss);
            let guard = self.region.chunk(chunk_x, chunk_z, status);
            let Some(slot) = self.chunks.get_mut(cache_index) else {
                panic!("Worldgen bulk section cache index {cache_index} escaped its storage");
            };
            *slot = Some(CachedWorldGenChunk {
                guard,
                verified_status: status,
            });
        } else if self.chunks.get(cache_index).is_some_and(|cached| {
            cached
                .as_ref()
                .is_some_and(|cached| status > cached.verified_status)
        }) {
            self.with_ore_profile(OreFeatureStats::record_chunk_status_upgrade);
            drop(self.region.chunk(chunk_x, chunk_z, status));
            let Some(Some(cached)) = self.chunks.get_mut(cache_index) else {
                panic!("Worldgen bulk section cache lost verified chunk ({chunk_x}, {chunk_z})");
            };
            cached.verified_status = status;
        }

        let Some(Some(cached)) = self.chunks.get(cache_index) else {
            panic!("Worldgen bulk section cache failed to store chunk ({chunk_x}, {chunk_z})");
        };
        cached
    }

    fn chunk_cache_index(&self, chunk_x: i32, chunk_z: i32) -> Option<usize> {
        let radius = self.chunk_cache_radius;
        let size = radius.checked_mul(2)?.checked_add(1)?;
        let rel_x = chunk_x
            .checked_sub(self.region.center.0.x)?
            .checked_add(radius)?;
        let rel_z = chunk_z
            .checked_sub(self.region.center.0.y)?
            .checked_add(radius)?;
        if rel_x < 0 || rel_x >= size || rel_z < 0 || rel_z >= size {
            return None;
        }

        usize::try_from(rel_z.checked_mul(size)?.checked_add(rel_x)?).ok()
    }

    fn section_index(min_y: i32, height: i32, y: i32) -> Option<usize> {
        if y < min_y || y >= min_y + height {
            return None;
        }

        usize::try_from((y - min_y) / 16).ok()
    }

    const fn local_coord(coord: i32) -> usize {
        (coord & 15) as usize
    }

    fn record_ore_read_time(
        profile: Option<&RefCell<OreFeatureStats>>,
        started_at: Option<Instant>,
    ) {
        if let Some(started_at) = started_at {
            Self::with_ore_profile_ref(profile, |profile| {
                profile.record_read_time(started_at.elapsed());
            });
        }
    }

    fn record_ore_write_time(
        profile: Option<&RefCell<OreFeatureStats>>,
        started_at: Option<Instant>,
    ) {
        if let Some(started_at) = started_at {
            Self::with_ore_profile_ref(profile, |profile| {
                profile.record_write_time(started_at.elapsed());
            });
        }
    }

    fn with_ore_profile(&self, f: impl FnOnce(&mut OreFeatureStats)) {
        let Some(profile) = self.ore_profile else {
            return;
        };
        if let Ok(mut profile) = profile.try_borrow_mut() {
            f(&mut profile);
        }
    }

    fn with_ore_profile_ref(
        profile: Option<&RefCell<OreFeatureStats>>,
        f: impl FnOnce(&mut OreFeatureStats),
    ) {
        let Some(profile) = profile else {
            return;
        };
        if let Ok(mut profile) = profile.try_borrow_mut() {
            f(&mut profile);
        }
    }
}

const fn abs_diff(left: i32, right: i32) -> i32 {
    if left >= right {
        left - right
    } else {
        right - left
    }
}

impl LevelReader for WorldGenRegion<'_> {
    fn get_block_state(&self, pos: BlockPos) -> BlockStateId {
        self.block_state(pos)
    }

    fn raw_brightness(&self, pos: BlockPos, sky_darkening: u8) -> u8 {
        let sky_light = if self.context.world().dimension_type.has_skylight {
            15_u8.saturating_sub(sky_darkening)
        } else {
            0
        };

        sky_light.max(self.block_light_at(pos))
    }

    fn can_see_sky(&self, pos: BlockPos) -> bool {
        if !self.context.world().dimension_type.has_skylight {
            return false;
        }

        self.height_at(HeightmapType::MotionBlocking, pos.x(), pos.z()) <= pos.y()
    }

    fn ambient_light(&self) -> f32 {
        self.context.world().dimension_type.ambient_light
    }

    fn min_y(&self) -> i32 {
        WorldGenRegion::min_y(self)
    }

    fn height(&self) -> i32 {
        WorldGenRegion::height(self)
    }
}

impl ScheduledTickAccess for WorldGenRegion<'_> {
    fn fluid_tick_delay(&self, fluid: FluidRef) -> i32 {
        FLUID_BEHAVIORS
            .get_behavior(fluid)
            .tick_delay(&self.context.world())
    }

    fn schedule_block_tick_default(&self, pos: BlockPos, block: BlockRef, delay: i32) -> bool {
        WorldGenRegion::schedule_block_tick_default(self, pos, block, delay)
    }

    fn schedule_fluid_tick_default(&self, pos: BlockPos, fluid: FluidRef, delay: i32) -> bool {
        WorldGenRegion::schedule_fluid_tick_default(self, pos, fluid, delay)
    }
}

impl LevelAccessor for WorldGenRegion<'_> {
    fn set_block_state(&self, pos: BlockPos, state: BlockStateId, flags: UpdateFlags) -> bool {
        WorldGenRegion::set_block_state(self, pos, state, flags)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Weak;

    use steel_registry::{test_support::init_test_registry, vanilla_blocks};
    use steel_utils::ChunkPos;

    use crate::behavior::init_behaviors;
    use crate::chunk::{
        chunk_access::{ChunkAccess, ChunkStatus},
        proto_chunk::ProtoChunk,
        section::{ChunkSection, Sections},
    };

    use super::{WorldGenBulkSectionAccess, WorldGenRegion};

    #[test]
    fn chessboard_distance_matches_chunk_dependency_radius() {
        let center = ChunkPos::new(4, -2);

        assert_eq!(WorldGenRegion::chessboard_distance(center, 4, -2), 0);
        assert_eq!(WorldGenRegion::chessboard_distance(center, 5, -3), 1);
        assert_eq!(WorldGenRegion::chessboard_distance(center, -4, 6), 8);
    }

    #[test]
    fn biome_quart_y_indices_clamp_to_vertical_biome_range() {
        assert_eq!(WorldGenRegion::biome_quart_y_indices(-64, 24, -17), (0, 0));
        assert_eq!(WorldGenRegion::biome_quart_y_indices(-64, 24, -16), (0, 0));
        assert_eq!(WorldGenRegion::biome_quart_y_indices(-64, 24, -13), (0, 3));
        assert_eq!(WorldGenRegion::biome_quart_y_indices(-64, 24, -12), (1, 0));
        assert_eq!(WorldGenRegion::biome_quart_y_indices(-64, 24, 79), (23, 3));
        assert_eq!(WorldGenRegion::biome_quart_y_indices(-64, 24, 80), (23, 3));
        assert_eq!(WorldGenRegion::biome_quart_y_indices(-64, 24, 81), (23, 3));
    }

    #[test]
    fn bulk_section_index_matches_world_height_bounds() {
        assert_eq!(
            WorldGenBulkSectionAccess::section_index(-64, 384, -65),
            None
        );
        assert_eq!(
            WorldGenBulkSectionAccess::section_index(-64, 384, -64),
            Some(0)
        );
        assert_eq!(
            WorldGenBulkSectionAccess::section_index(-64, 384, -49),
            Some(0)
        );
        assert_eq!(
            WorldGenBulkSectionAccess::section_index(-64, 384, -48),
            Some(1)
        );
        assert_eq!(
            WorldGenBulkSectionAccess::section_index(-64, 384, 319),
            Some(23)
        );
        assert_eq!(
            WorldGenBulkSectionAccess::section_index(-64, 384, 320),
            None
        );
    }

    #[test]
    fn bulk_section_local_coord_uses_vanilla_section_mask() {
        assert_eq!(WorldGenBulkSectionAccess::local_coord(-17), 15);
        assert_eq!(WorldGenBulkSectionAccess::local_coord(-16), 0);
        assert_eq!(WorldGenBulkSectionAccess::local_coord(-1), 15);
        assert_eq!(WorldGenBulkSectionAccess::local_coord(0), 0);
        assert_eq!(WorldGenBulkSectionAccess::local_coord(31), 15);
    }

    #[test]
    fn bulk_write_defers_counts_only_for_pre_light_proto_chunks() {
        init_test_registry();
        init_behaviors();
        let stone = vanilla_blocks::STONE.default_state();
        let air = vanilla_blocks::AIR.default_state();

        let pre_light_proto = ProtoChunk::new(
            Sections::from_owned(vec![ChunkSection::new_empty()].into_boxed_slice()),
            ChunkPos::new(0, 0),
            0,
            16,
            Weak::new(),
        );
        let pre_light_chunk = ChunkAccess::Proto(pre_light_proto);
        let mut pre_light_section = ChunkSection::new_empty();

        WorldGenBulkSectionAccess::set_bulk_block_state(
            &pre_light_chunk,
            &mut pre_light_section,
            1,
            2,
            3,
            stone,
        );

        assert_eq!(pre_light_section.non_empty_block_count(), 0);

        let initialized_proto = ProtoChunk::new(
            Sections::from_owned(vec![ChunkSection::new_empty()].into_boxed_slice()),
            ChunkPos::new(0, 0),
            0,
            16,
            Weak::new(),
        );
        initialized_proto.set_status(ChunkStatus::InitializeLight);
        let initialized_chunk = ChunkAccess::Proto(initialized_proto);
        let mut initialized_section = ChunkSection::new_empty();

        WorldGenBulkSectionAccess::set_bulk_block_state(
            &initialized_chunk,
            &mut initialized_section,
            1,
            2,
            3,
            stone,
        );

        assert_eq!(initialized_section.non_empty_block_count(), 1);

        let mut initialized_building_section = ChunkSection::new_empty();
        initialized_building_section.set_block_state_for_generation(1, 2, 3, stone);

        let old_state = WorldGenBulkSectionAccess::set_bulk_block_state(
            &initialized_chunk,
            &mut initialized_building_section,
            4,
            5,
            6,
            stone,
        );

        assert_eq!(old_state, air);
        assert_eq!(initialized_building_section.non_empty_block_count(), 2);
    }
}
