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
};

use parking_lot::{RwLockReadGuard, RwLockWriteGuard};
use rustc_hash::FxHashMap;
use simdnbt::owned::NbtCompound;
use steel_registry::{
    REGISTRY, block_entity_type::BlockEntityTypeRef, blocks::BlockRef,
    blocks::block_state_ext::BlockStateExt as _, fluid::FluidRef, vanilla_blocks,
};
use steel_utils::random::RandomSource;
use steel_utils::{BlockPos, BlockStateId, ChunkPos, SectionPos, types::UpdateFlags};

use crate::behavior::FLUID_BEHAVIORS;
use crate::block_entity::{BLOCK_ENTITIES, SharedBlockEntity};
use crate::chunk::{
    chunk_access::{ChunkAccess, ChunkStatus},
    chunk_generation_task::StaticCache2D,
    chunk_holder::ChunkHolder,
    chunk_pyramid::ChunkStep,
    heightmap::HeightmapType,
    section::{ChunkSection, SectionHolder},
};
use crate::entity::SharedEntity;
use crate::world::tick_scheduler::TickPriority;
use crate::world::{LevelReader, ScheduledTickAccess, World};
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
    chunks: FxHashMap<ChunkPos, CachedWorldGenChunk<'region>>,
    air: BlockStateId,
    ore_profile: Option<&'profile RefCell<OreFeatureStats>>,
}

struct CachedWorldGenChunk<'region> {
    guard: RwLockReadGuard<'region, ChunkAccess>,
    verified_status: ChunkStatus,
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
    pub const fn new(
        context: &'a WorldGenContext,
        step: &'a ChunkStep,
        cache: &'a StaticCache2D<Arc<ChunkHolder>>,
        center: ChunkPos,
        random: RandomSource,
    ) -> Self {
        Self {
            context,
            step,
            cache,
            center,
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
    pub fn random_mut(&mut self) -> &mut RandomSource {
        &mut self.random
    }

    /// Returns the minimum build height.
    #[must_use]
    pub fn min_y(&self) -> i32 {
        self.context.min_y()
    }

    /// Returns the world height.
    #[must_use]
    pub fn height(&self) -> i32 {
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
    pub fn sea_level(&self) -> i32 {
        self.context.world().sea_level
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
    pub const fn block_light_at(&self, _pos: BlockPos) -> u8 {
        0
    }

    /// Returns the exclusive maximum build height.
    #[must_use]
    pub fn max_y_exclusive(&self) -> i32 {
        self.min_y() + self.height()
    }

    /// Checks if a Y coordinate is outside the build height.
    #[must_use]
    pub fn is_outside_build_height(&self, y: i32) -> bool {
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
    ) -> Option<RwLockReadGuard<'_, ChunkAccess>> {
        let available_status = self.required_status_at(chunk_x, chunk_z)?;
        if status > available_status {
            return None;
        }

        self.cache.get(chunk_x, chunk_z).try_chunk(status)
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
    ) -> RwLockReadGuard<'_, ChunkAccess> {
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

    /// Gets a block state through the region dependency contract.
    ///
    /// # Panics
    /// Panics if the position's chunk is outside this step's direct dependencies.
    #[must_use]
    pub fn block_state(&self, pos: BlockPos) -> BlockStateId {
        let chunk_x = SectionPos::block_to_section_coord(pos.x());
        let chunk_z = SectionPos::block_to_section_coord(pos.z());
        self.chunk(chunk_x, chunk_z, ChunkStatus::Empty)
            .get_block_state(pos)
    }

    /// Gets a block entity through the region dependency contract.
    ///
    /// # Panics
    /// Panics if the position's chunk is outside this step's direct dependencies.
    #[must_use]
    pub fn block_entity(&self, pos: BlockPos) -> Option<SharedBlockEntity> {
        let chunk_x = SectionPos::block_to_section_coord(pos.x());
        let chunk_z = SectionPos::block_to_section_coord(pos.z());
        self.chunk(chunk_x, chunk_z, ChunkStatus::Empty)
            .get_block_entity(pos)
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

        let chunk = self.chunk(chunk_x, chunk_z, ChunkStatus::Biomes);
        let sections = chunk.sections();
        let (section_index, local_quart_y) =
            Self::biome_quart_y_indices(self.min_y(), sections.sections.len(), quart_y);
        let section = &sections.sections[section_index];

        section
            .read()
            .biomes
            .get(local_quart_x, local_quart_y, local_quart_z)
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

        self.chunk(chunk_x, chunk_z, status)
            .set_block_state(pos, state, flags);
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

        let chunk = self.chunk(chunk_x, chunk_z, status);
        let entity = BLOCK_ENTITIES.create_and_load_owned_or_raw(
            block_entity_type,
            chunk.level_weak(),
            pos,
            state,
            nbt,
        );
        chunk.add_and_register_block_entity(entity);
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

        self.chunk(chunk_x, chunk_z, status)
            .remove_block_entity(pos);
        true
    }

    /// Adds an entity at a writable worldgen position.
    #[must_use]
    pub fn add_fresh_entity(&self, entity: SharedEntity) -> bool {
        let pos = BlockPos::from(entity.position());
        let Some((chunk_x, chunk_z, status)) = self.writable_chunk_for_pos(pos, "add entity")
        else {
            return false;
        };

        self.chunk(chunk_x, chunk_z, status).add_entity(entity)
    }

    /// Schedules a block tick through the same region write contract as block placement.
    #[must_use]
    pub fn schedule_block_tick(
        &self,
        pos: BlockPos,
        block: BlockRef,
        delay: i32,
        priority: TickPriority,
    ) -> bool {
        let Some((chunk_x, chunk_z, status)) =
            self.writable_chunk_for_pos(pos, "schedule block tick")
        else {
            return false;
        };
        let sub_tick_order = self.sub_tick_count.fetch_add(1, Ordering::Relaxed);
        self.chunk(chunk_x, chunk_z, status).schedule_block_tick(
            pos,
            block,
            delay,
            priority,
            sub_tick_order,
        );
        true
    }

    /// Schedules a block tick with vanilla's normal priority.
    #[must_use]
    pub fn schedule_block_tick_default(&self, pos: BlockPos, block: BlockRef, delay: i32) -> bool {
        self.schedule_block_tick(pos, block, delay, TickPriority::Normal)
    }

    /// Schedules a fluid tick through the same region write contract as block placement.
    #[must_use]
    pub fn schedule_fluid_tick(
        &self,
        pos: BlockPos,
        fluid: FluidRef,
        delay: i32,
        priority: TickPriority,
    ) -> bool {
        let Some((chunk_x, chunk_z, status)) =
            self.writable_chunk_for_pos(pos, "schedule fluid tick")
        else {
            return false;
        };
        let sub_tick_order = self.sub_tick_count.fetch_add(1, Ordering::Relaxed);
        self.chunk(chunk_x, chunk_z, status).schedule_fluid_tick(
            pos,
            fluid,
            delay,
            priority,
            sub_tick_order,
        );
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
        self.chunk(chunk_x, chunk_z, ChunkStatus::Empty)
            .mark_pos_for_postprocessing(pos);
    }

    /// Gets the first available Y coordinate for a heightmap column.
    ///
    /// # Panics
    /// Panics if the target chunk is not available at `Carvers` status.
    #[must_use]
    pub fn height_at(&self, heightmap_type: HeightmapType, x: i32, z: i32) -> i32 {
        let chunk_x = SectionPos::block_to_section_coord(x);
        let chunk_z = SectionPos::block_to_section_coord(z);
        let local_x = (x & 15) as usize;
        let local_z = (z & 15) as usize;

        self.chunk(chunk_x, chunk_z, ChunkStatus::Carvers)
            .height_at(heightmap_type, local_x, local_z)
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
        Self {
            region,
            chunks: FxHashMap::default(),
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
        let started_at = ore_profile.map(|_| std::time::Instant::now());
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

        let old_state = section_guard.set_block_state(local_x, local_y, local_z, state);
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
        positions: &[BlockPos],
        mut replacement: impl FnMut(BlockStateId) -> Option<BlockStateId>,
    ) -> u64 {
        let ore_profile = self.ore_profile;
        let started_at = ore_profile.map(|_| std::time::Instant::now());
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
        let min_y = self.region.min_y();
        let height = self.region.height();
        let chunk = self.chunk(chunk_x, chunk_z, status);
        let Some(section) = chunk.guard.sections().sections.get(section_index) else {
            panic!(
                "Worldgen bulk section write in chunk ({chunk_x}, {chunk_z}) resolved missing section index {section_index}",
            );
        };

        let mut section_guard = Self::ore_section_write_guard(ore_profile, section, key);
        let mut placed = 0_u64;
        let mut dirty = false;
        for &pos in positions {
            debug_assert_eq!(
                Self::section_key_parts(min_y, height, pos),
                Some((chunk_x, chunk_z, section_index))
            );
            Self::with_ore_profile_ref(ore_profile, OreFeatureStats::record_target_read);
            let local_x = Self::local_coord(pos.x());
            let local_y = Self::local_coord(pos.y());
            let local_z = Self::local_coord(pos.z());
            let old_state = section_guard.states.get(local_x, local_y, local_z);
            if let Some(state) = replacement(old_state) {
                let old_state = section_guard.set_block_state(local_x, local_y, local_z, state);
                Self::with_ore_profile_ref(ore_profile, OreFeatureStats::record_write);
                dirty |= old_state != state;
                placed += 1;
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
        let started_at = ore_profile.map(|_| std::time::Instant::now());
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
                let wait_started_at = std::time::Instant::now();
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
    /// This intentionally mirrors vanilla `LevelChunkSection.setBlockState` as used through
    /// `BulkSectionAccess`: section block counts are updated, but heightmaps, neighbor updates,
    /// block entity callbacks, and other `WorldGenRegion.setBlock` side effects are skipped.
    #[must_use]
    pub(crate) fn set_block_state(&mut self, pos: BlockPos, state: BlockStateId) -> bool {
        let ore_profile = self.ore_profile;
        let started_at = ore_profile.map(|_| std::time::Instant::now());
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
        let old_state = section_guard.set_block_state(
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

    fn section_key_parts(min_y: i32, height: i32, pos: BlockPos) -> Option<(i32, i32, usize)> {
        Some((
            SectionPos::block_to_section_coord(pos.x()),
            SectionPos::block_to_section_coord(pos.z()),
            Self::section_index(min_y, height, pos.y())?,
        ))
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
                let wait_started_at = std::time::Instant::now();
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
    pub(crate) fn can_write_to_pos(&self, pos: BlockPos) -> bool {
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
        let key = ChunkPos::new(chunk_x, chunk_z);

        if !self.chunks.contains_key(&key) {
            self.with_ore_profile(OreFeatureStats::record_chunk_cache_miss);
            let guard = self.region.chunk(chunk_x, chunk_z, status);
            self.chunks.insert(
                key,
                CachedWorldGenChunk {
                    guard,
                    verified_status: status,
                },
            );
        } else if self
            .chunks
            .get(&key)
            .is_some_and(|cached| status > cached.verified_status)
        {
            self.with_ore_profile(OreFeatureStats::record_chunk_status_upgrade);
            drop(self.region.chunk(chunk_x, chunk_z, status));
            let Some(cached) = self.chunks.get_mut(&key) else {
                panic!("Worldgen bulk section cache lost verified chunk ({chunk_x}, {chunk_z})");
            };
            cached.verified_status = status;
        }

        let Some(cached) = self.chunks.get(&key) else {
            panic!("Worldgen bulk section cache failed to store chunk ({chunk_x}, {chunk_z})");
        };
        cached
    }

    fn section_index(min_y: i32, height: i32, y: i32) -> Option<usize> {
        if y < min_y || y >= min_y + height {
            return None;
        }

        usize::try_from((y - min_y) / 16).ok()
    }

    fn local_coord(coord: i32) -> usize {
        (coord & 15) as usize
    }

    fn record_ore_read_time(
        profile: Option<&RefCell<OreFeatureStats>>,
        started_at: Option<std::time::Instant>,
    ) {
        if let Some(started_at) = started_at {
            Self::with_ore_profile_ref(profile, |profile| {
                profile.record_read_time(started_at.elapsed());
            });
        }
    }

    fn record_ore_write_time(
        profile: Option<&RefCell<OreFeatureStats>>,
        started_at: Option<std::time::Instant>,
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
            f(&mut *profile);
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
            f(&mut *profile);
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

#[cfg(test)]
mod tests {
    use steel_utils::ChunkPos;

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
}
