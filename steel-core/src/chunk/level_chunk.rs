//! This module contains the `LevelChunk` struct, which is a chunk that is ready to be sent to the client.
use std::{
    io::Cursor,
    mem,
    sync::{
        Arc, Weak,
        atomic::{AtomicBool, Ordering},
    },
};

use rand::RngExt;
use steel_protocol::packets::game::{
    BlockEntityInfo, ChunkPacketData, HeightmapType as ProtocolHeightmapType, Heightmaps,
    LightUpdatePacketData,
};
use steel_registry::{
    REGISTRY, RegistryEntry, blocks::block_state_ext::BlockStateExt, vanilla_blocks,
};
use steel_utils::{
    BlockPos, BlockStateId, ChunkPos, Direction, PackedChunkLocalXZ, SectionPos, codec::BitSet,
    locks::SyncRwLock, types::UpdateFlags,
};

use steel_utils::locks::SyncMutex;

use crate::block_entity::{BlockEntityStorage, BlockEntityTickAction, SharedBlockEntity};
use crate::chunk::{
    heightmap::{ChunkHeightmaps, HeightmapType},
    proto_chunk::ProtoChunk,
    section::Sections,
};
use crate::entity::SharedEntity;
use crate::world::World;
use crate::world::tick_scheduler::{BlockTick, BlockTickList, FluidTick, FluidTickList};
use crate::{
    behavior::{BLOCK_BEHAVIORS, BlockStateBehaviorExt, FLUID_BEHAVIORS},
    world::game_event_context::GameEventContext,
};
use steel_worldgen::structure::{StructureReferenceMap, StructureStartMap};

fn empty_postprocessing(height: i32) -> Box<[Vec<u16>]> {
    let section_count = (height / 16) as usize;
    (0..section_count).map(|_| Vec::new()).collect()
}

/// A chunk that is ready to be sent to the client.
///
/// Similar to Java's `LevelChunk`, this holds a weak reference to the world
/// (called `level` in Java) for callbacks during block state changes.
pub struct LevelChunk {
    /// The sections of the chunk.
    pub sections: Sections,
    /// The position of the chunk.
    pub pos: ChunkPos,
    /// Whether the chunk has been modified since last save.
    pub dirty: AtomicBool,
    /// The heightmaps for this chunk (wrapped in `RwLock` for interior mutability).
    pub heightmaps: SyncRwLock<ChunkHeightmaps>,
    /// The minimum Y coordinate of the world this chunk belongs to.
    min_y: i32,
    /// The total height of the world.
    height: i32,
    /// Weak reference to the world (called `level` in Java).
    /// This mirrors Java's `LevelChunk.level` field.
    level: Weak<World>,
    /// Block entities stored in this chunk.
    block_entities: BlockEntityStorage,
    /// Scheduled block ticks pending in this chunk.
    pub block_ticks: SyncMutex<BlockTickList>,
    /// Scheduled fluid ticks pending in this chunk.
    pub fluid_ticks: SyncMutex<FluidTickList>,
    /// Structure starts originating in this chunk (carried from proto).
    pub structure_starts: SyncRwLock<StructureStartMap>,
    /// References to structures from nearby origin chunks (carried from proto).
    pub structure_references: SyncRwLock<StructureReferenceMap>,
    /// Vanilla proto postprocessing offsets carried through promotion and drained once.
    postprocessing: SyncMutex<Box<[Vec<u16>]>>,
}

/// Result of promoting a proto chunk to a full chunk.
pub struct LevelChunkPromotion {
    /// The promoted full chunk.
    pub chunk: LevelChunk,
    /// Entities that should be registered after the full chunk is published.
    pub pending_entities: Vec<SharedEntity>,
}

impl LevelChunk {
    /// Ticks this chunk, processing random block ticks and block entity ticks.
    ///
    /// For each section that contains randomly-ticking blocks, selects
    /// `random_tick_speed` random blocks and calls their `random_tick` behavior.
    /// Also ticks all ticking block entities in this chunk.
    ///
    /// # Arguments
    /// * `random_tick_speed` - Number of random blocks to tick per section per tick.
    ///   This is controlled by the `randomTickSpeed` game rule.
    /// * `tick_count` - Current server tick count (for entity sync timing).
    ///
    /// # Panics
    /// Panics if the block behavior registry has not been initialized.
    pub fn tick(
        &self,
        random_tick_speed: u32,
        _tick_count: i32,
        ready_block_ticks: &mut Vec<BlockTick>,
        ready_fluid_ticks: &mut Vec<FluidTick>,
    ) {
        // Drain ready scheduled ticks (decrement delays, collect those at 0)
        ready_block_ticks.extend(self.block_ticks.lock().drain_ready());
        ready_fluid_ticks.extend(self.fluid_ticks.lock().drain_ready());

        // Tick block entities regardless of random tick speed
        self.tick_block_entities();

        if random_tick_speed == 0 {
            return;
        }

        for (section_index, section) in self.sections.sections.iter().enumerate() {
            // Skip sections with no randomly-ticking blocks (lock-free check)
            if !section.is_randomly_ticking() {
                continue;
            }

            let Some(world) = self.get_level() else {
                return;
            };

            let block_behaviors = &*BLOCK_BEHAVIORS;
            let mut rng = rand::rng();
            let chunk_base_x = self.pos.0.x * 16;
            let chunk_base_z = self.pos.0.y * 16;

            let section_base_y = self.min_y + (section_index as i32 * 16);

            // Collect blocks to tick while holding the read lock, then release it
            // before calling random_tick to avoid deadlock (random_tick may call set_block)
            let blocks_to_tick: Vec<(BlockStateId, BlockPos)> = {
                let section_guard = section.read();

                let mut blocks = Vec::with_capacity(random_tick_speed as usize);

                for _ in 0..random_tick_speed {
                    let local_x = rng.random_range(0..16);
                    let local_y = rng.random_range(0..16);
                    let local_z = rng.random_range(0..16);

                    let state = section_guard.states.get(local_x, local_y, local_z);

                    if block_behaviors
                        .get_behavior(state.get_block())
                        .is_randomly_ticking(state)
                    {
                        let pos = BlockPos::new(
                            chunk_base_x + local_x as i32,
                            section_base_y + local_y as i32,
                            chunk_base_z + local_z as i32,
                        );
                        blocks.push((state, pos));
                    }
                }

                blocks
            }; // section_guard dropped here

            // Now process the collected blocks without holding any lock
            for (state, pos) in blocks_to_tick {
                let behavior = block_behaviors.get_behavior(state.get_block());
                behavior.random_tick(state, &world, pos);
            }
        }
    }

    /// Creates a new `LevelChunk` from a `ProtoChunk`.
    ///
    /// Transfers final heightmaps from the proto chunk if available.
    /// Recalculates section block counts for random tick optimization.
    ///
    /// # Arguments
    /// * `proto_chunk` - The proto chunk to convert
    /// * `min_y` - The minimum Y coordinate of the world
    /// * `height` - The total height of the world
    /// * `level` - Weak reference to the world (mirrors Java's `LevelChunk.level`)
    ///
    /// # Panics
    /// Panics if the block behavior registry has not been initialized.
    #[must_use]
    pub fn from_proto(
        proto_chunk: ProtoChunk,
        min_y: i32,
        height: i32,
        level: Weak<World>,
    ) -> LevelChunkPromotion {
        // Ensure full chunks always have populated final heightmaps. Some stages
        // may not touch blocks (carvers are currently empty), so lazy final
        // heightmaps are not guaranteed to exist before promotion.
        let mut proto_heightmaps = proto_chunk.heightmaps.into_inner();
        proto_heightmaps.prime_from_sections(
            HeightmapType::final_types(),
            min_y,
            height,
            &proto_chunk.sections.sections,
        );
        let chunk_heightmaps = ChunkHeightmaps::from_proto(&mut proto_heightmaps, min_y, height);

        // Recalculate section counts for random tick optimization
        for section in &proto_chunk.sections.sections {
            section.write().recalculate_counts();
        }

        let structure_starts = proto_chunk.structure_starts.into_inner();
        let structure_references = proto_chunk.structure_references.into_inner();
        let postprocessing = proto_chunk.postprocessing.into_inner();
        let block_ticks = proto_chunk.block_ticks.into_inner();
        let fluid_ticks = proto_chunk.fluid_ticks.into_inner();
        let block_entities = proto_chunk.block_entities;
        let pending_entities = proto_chunk.entities.get_all();

        Self::populate_poi(&level, &proto_chunk.sections, proto_chunk.pos, min_y);

        let chunk = Self {
            sections: proto_chunk.sections,
            pos: proto_chunk.pos,
            dirty: AtomicBool::new(proto_chunk.dirty.load(Ordering::Acquire)),
            heightmaps: SyncRwLock::new(chunk_heightmaps),
            min_y,
            height,
            level,
            block_entities,
            block_ticks: SyncMutex::new(block_ticks),
            fluid_ticks: SyncMutex::new(fluid_ticks),
            structure_starts: SyncRwLock::new(structure_starts),
            structure_references: SyncRwLock::new(structure_references),
            postprocessing: SyncMutex::new(postprocessing),
        };
        LevelChunkPromotion {
            chunk,
            pending_entities,
        }
    }

    /// Creates a new `LevelChunk` that was loaded from disk (not dirty).
    ///
    /// Recalculates section block counts for random tick optimization.
    ///
    /// # Arguments
    /// * `sections` - The chunk sections
    /// * `pos` - The chunk position
    /// * `min_y` - The minimum Y coordinate of the world
    /// * `height` - The total height of the world
    /// * `level` - Weak reference to the world (mirrors Java's `LevelChunk.level`)
    /// * `block_ticks` - Scheduled block ticks loaded from disk
    /// * `fluid_ticks` - Scheduled fluid ticks loaded from disk
    /// * `heightmaps` - Heightmaps loaded from disk
    ///
    /// # Panics
    /// Panics if the block behavior registry has not been initialized.
    #[must_use]
    #[expect(
        clippy::too_many_arguments,
        reason = "all parameters are required to fully restore a chunk from disk"
    )]
    pub fn from_disk(
        sections: Sections,
        pos: ChunkPos,
        min_y: i32,
        height: i32,
        level: Weak<World>,
        block_ticks: BlockTickList,
        fluid_ticks: FluidTickList,
        heightmaps: ChunkHeightmaps,
        structure_starts: StructureStartMap,
        structure_references: StructureReferenceMap,
    ) -> Self {
        // Recalculate section counts for random tick optimization
        for section in &sections.sections {
            section.write().recalculate_counts();
        }

        Self::populate_poi(&level, &sections, pos, min_y);

        Self {
            sections,
            pos,
            dirty: AtomicBool::new(false),
            heightmaps: SyncRwLock::new(heightmaps),
            min_y,
            height,
            level,
            block_entities: BlockEntityStorage::new(),
            block_ticks: SyncMutex::new(block_ticks),
            fluid_ticks: SyncMutex::new(fluid_ticks),
            structure_starts: SyncRwLock::new(structure_starts),
            structure_references: SyncRwLock::new(structure_references),
            postprocessing: SyncMutex::new(empty_postprocessing(height)),
        }
    }

    /// Returns a reference to the world if it's still alive.
    ///
    /// This mirrors Java's `LevelChunk.getLevel()`.
    #[must_use]
    pub fn get_level(&self) -> Option<Arc<World>> {
        self.level.upgrade()
    }

    /// Returns the weak reference to the world.
    ///
    /// Use this when you need to pass the world reference to block entities
    /// at construction time.
    #[must_use]
    pub fn level_weak(&self) -> Weak<World> {
        self.level.clone()
    }

    /// Drains the vanilla proto postprocessing offsets carried through promotion.
    pub(crate) fn take_postprocessing(&self) -> Option<Box<[Vec<u16>]>> {
        let mut postprocessing = self.postprocessing.lock();
        if postprocessing.iter().all(Vec::is_empty) {
            return None;
        }

        Some(mem::replace(
            &mut *postprocessing,
            empty_postprocessing(self.height),
        ))
    }

    /// Runs vanilla proto postprocessing after this chunk has been promoted to full.
    pub(crate) fn post_process_generation(
        world: &Arc<World>,
        chunk_pos: ChunkPos,
        min_y: i32,
        postprocessing: Box<[Vec<u16>]>,
    ) {
        for (section_index, packed_offsets) in postprocessing.into_vec().into_iter().enumerate() {
            if packed_offsets.is_empty() {
                continue;
            }

            let section_y = Self::section_y_from_section_index(min_y, section_index);
            for packed in packed_offsets {
                let pos = ProtoChunk::unpack_postprocessing_offset(packed, section_y, chunk_pos);
                let state = world.get_postprocessing_block_state(pos);
                let fluid_state = state.get_fluid_state();

                if !fluid_state.is_empty() {
                    FLUID_BEHAVIORS
                        .get_behavior(fluid_state.fluid_id)
                        .tick(world, pos);
                }

                if state.get_block().config.liquid {
                    BLOCK_BEHAVIORS
                        .get_behavior(state.get_block())
                        .tick(state, world, pos);
                } else {
                    let new_state = Self::update_from_neighbor_shapes(world, state, pos);
                    if new_state != state {
                        let flags = UpdateFlags::UPDATE_INVISIBLE
                            | UpdateFlags::UPDATE_KNOWN_SHAPE
                            | UpdateFlags::UPDATE_SKIP_BLOCK_ENTITY_SIDEEFFECTS;
                        world.set_block(pos, new_state, flags);
                    }
                }
            }
        }
    }

    fn update_from_neighbor_shapes(
        world: &Arc<World>,
        state: BlockStateId,
        pos: BlockPos,
    ) -> BlockStateId {
        let mut updated = state;
        for direction in Direction::UPDATE_SHAPE_ORDER {
            let neighbor_pos = pos.relative(direction);
            let neighbor_state = world.get_postprocessing_block_state(neighbor_pos);
            let behavior = BLOCK_BEHAVIORS.get_behavior(updated.get_block());
            updated =
                behavior.update_shape(updated, world, pos, direction, neighbor_pos, neighbor_state);
        }
        updated
    }

    /// Scans chunk sections for POI block states and populates world POI storage.
    fn populate_poi(level: &Weak<World>, sections: &Sections, pos: ChunkPos, min_y: i32) {
        let Some(world) = level.upgrade() else {
            return;
        };

        // Palette pre-check WITHOUT the global POI lock: collect only the
        // sections that actually contain POI blocks. The vast majority of
        // worldgen chunks have none, so they never touch the (heavily
        // contended) `poi_storage` mutex and never do a per-block scan.
        // `Vec::new()` doesn't allocate until the first push, so the common
        // empty case is allocation-free.
        let mut poi_sections: Vec<(usize, SectionPos)> = Vec::new();
        for (i, section) in sections.sections.iter().enumerate() {
            if section.read().contains_poi() {
                let section_y = min_y / 16 + i as i32;
                poi_sections.push((i, SectionPos::new(pos.0.x, section_y, pos.0.y)));
            }
        }
        if poi_sections.is_empty() {
            return;
        }

        let mut poi_storage = world.poi_storage.lock();
        for (i, section_pos) in poi_sections {
            let guard = sections.sections[i].read();
            poi_storage.scan_and_populate(&guard, section_pos);
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

    /// Gets the first available Y coordinate for a heightmap column.
    #[must_use]
    pub fn get_height(&self, heightmap_type: HeightmapType, local_x: usize, local_z: usize) -> i32 {
        self.heightmaps
            .read()
            .get(heightmap_type)
            .get_first_available(local_x, local_z)
    }

    /// Gets the section index for a given Y coordinate.
    #[must_use]
    const fn get_section_index(&self, y: i32) -> usize {
        ((y - self.min_y) / 16) as usize
    }

    #[must_use]
    const fn section_y_from_section_index(min_y: i32, index: usize) -> i32 {
        min_y.div_euclid(16) + index as i32
    }

    /// Marks the chunk as unsaved.
    fn mark_unsaved(&self) {
        self.dirty.store(true, Ordering::Release);
    }

    /// Gets a block entity at the given position.
    ///
    /// Returns `None` if no block entity exists at the position.
    #[must_use]
    pub fn get_block_entity(&self, pos: BlockPos) -> Option<SharedBlockEntity> {
        self.block_entities.get(pos)
    }

    /// Removes a block entity at the given position.
    ///
    /// Marks the entity as removed and removes it from the ticking list.
    pub fn remove_block_entity(&self, pos: BlockPos) {
        self.block_entities.remove(pos);
        self.mark_unsaved();
    }

    /// Adds a block entity and registers it for ticking if needed.
    ///
    /// This is the main entry point for adding block entities. It:
    /// 1. Stores the block entity in the chunk
    /// 2. Registers it for ticking if `is_ticking()` returns true
    ///
    /// Note: The world reference should be passed at block entity construction time.
    pub fn add_and_register_block_entity(&self, block_entity: SharedBlockEntity) {
        self.block_entities.add_and_register(block_entity);
        self.mark_unsaved();
    }

    /// Updates the ticking status of a block entity.
    ///
    /// Call this when a block entity's ticking status may have changed
    /// (e.g., after its block state is updated).
    pub fn update_block_entity_ticker(&self, block_entity: &SharedBlockEntity) {
        self.block_entities.update_ticker(block_entity);
    }

    /// Returns all block entities in this chunk.
    #[must_use]
    pub fn get_block_entities(&self) -> Vec<SharedBlockEntity> {
        self.block_entities.get_all()
    }

    /// Returns a reference to the block entity storage.
    #[must_use]
    pub const fn block_entity_storage(&self) -> &BlockEntityStorage {
        &self.block_entities
    }

    /// Clears all block entities from this chunk.
    ///
    /// Marks all entities as removed.
    pub fn clear_all_block_entities(&self) {
        self.block_entities.clear();
    }

    /// Ticks all ticking block entities in this chunk.
    ///
    /// Called each game tick for chunks that are in ticking range.
    pub fn tick_block_entities(&self) {
        let Some(world) = self.get_level() else {
            return;
        };

        // Get entities to tick (already filters out removed)
        let entities = self.block_entities.get_tickers();

        // Tick each entity
        for entity in entities {
            let action = {
                let mut guard = entity.lock();
                if guard.is_removed() {
                    continue;
                }
                guard.tick(&world)
            };

            if let Some(action) = action {
                match action {
                    BlockEntityTickAction::SetBlock {
                        pos,
                        state,
                        flags,
                        game_event,
                    } => {
                        world.set_block(pos, state, flags);
                        if let Some((event, event_state)) = game_event {
                            world.game_event(
                                event,
                                pos,
                                &GameEventContext::new(None, Some(event_state)),
                            );
                        }
                    }
                }
            }
        }

        // Clean up removed entities from the ticking list
        self.block_entities.cleanup_tickers();
    }

    /// Sets a block state at the given position.
    ///
    /// Returns the old block state, or `None` if nothing changed.
    ///
    /// # Arguments
    /// * `pos` - The absolute block position
    /// * `state` - The new block state to set
    /// * `flags` - Update flags controlling behavior
    ///
    /// # Panics
    ///
    /// Panics if the behavior registry has not been initialized.
    #[must_use]
    pub fn set_block_state(
        &self,
        pos: BlockPos,
        state: BlockStateId,
        flags: UpdateFlags,
    ) -> Option<BlockStateId> {
        let y = pos.0.y;

        if y < self.min_y || y >= self.min_y + self.height {
            return None;
        }

        let section_index = self.get_section_index(y);

        if section_index >= self.sections.sections.len() {
            return None;
        }

        let section = &self.sections.sections[section_index];

        let local_x = (pos.0.x & 15) as usize;
        let local_y = (y & 15) as usize;
        let local_z = (pos.0.z & 15) as usize;

        let old_state = section
            .write()
            .set_block_state(local_x, local_y, local_z, state);

        if old_state == state {
            return None;
        }

        let min_y = self.min_y;
        let sections = &self.sections;
        self.heightmaps
            .write()
            .update(local_x, y, local_z, state, |lx, scan_y, lz| {
                let scan_section_index = ((scan_y - min_y) / 16) as usize;
                let scan_local_y = ((scan_y - min_y) % 16) as usize;
                sections.sections[scan_section_index]
                    .read()
                    .states
                    .get(lx, scan_local_y, lz)
            });

        let old_block = old_state.get_block();
        let new_block = state.get_block();

        // TODO: Light updates
        // In vanilla, light engine is notified when section emptiness changes:
        // let is_empty = section.read().states.has_only_air();
        // if was_empty != is_empty {
        //     level.chunk_source.light_engine.update_section_status(pos, is_empty);
        //     level.chunk_source.on_section_emptiness_changed(chunk_pos.x, section_y, chunk_pos.z, is_empty);
        // }
        //
        // And when light properties change:
        // if LightEngine::has_different_light_properties(old_state, state) {
        //     self.sky_light_sources.update(self, local_x, y, local_z);
        //     level.chunk_source.light_engine.check_block(pos);
        // }

        // Re-read the block to verify it wasn't changed concurrently
        let current_block = section
            .read()
            .states
            .get(local_x, local_y, local_z)
            .get_block();
        if current_block != new_block {
            return None;
        }

        if let Some(level) = self.get_level() {
            // Update POI storage when block states change
            level
                .poi_storage
                .lock()
                .on_block_state_change(pos, old_state, state);
            let block_changed = old_block != new_block;
            let moved_by_piston = flags.contains(UpdateFlags::UPDATE_MOVE_BY_PISTON);
            let side_effects = !flags.contains(UpdateFlags::UPDATE_SKIP_BLOCK_ENTITY_SIDEEFFECTS);

            let block_behaviors = &*BLOCK_BEHAVIORS;
            let old_behavior = block_behaviors.get_behavior(old_block);
            let new_behavior = block_behaviors.get_behavior(new_block);

            // Block entity removal when block type changes
            if block_changed && old_behavior.has_block_entity() {
                let should_keep = new_behavior.should_keep_block_entity(old_state, state);
                if !should_keep {
                    if side_effects && let Some(block_entity) = self.get_block_entity(pos) {
                        block_entity.lock().pre_remove_side_effects(pos, old_state);
                    }
                    self.remove_block_entity(pos);
                }
            }

            // Notify neighbors that we were removed (for rails, etc.)
            if block_changed && (flags.contains(UpdateFlags::UPDATE_NEIGHBORS) || moved_by_piston) {
                old_behavior.affect_neighbors_after_removal(
                    old_state,
                    &level,
                    pos,
                    moved_by_piston,
                );
            }

            // Call on_place for the new block
            if !flags.contains(UpdateFlags::UPDATE_SKIP_ON_PLACE) {
                new_behavior.on_place(state, &level, pos, old_state, moved_by_piston);
            }

            // Block entity creation after on_place
            if new_behavior.has_block_entity() {
                if let Some(existing) = self.get_block_entity(pos) {
                    // Update existing block entity's state
                    existing.lock().set_block_state(state);
                    self.update_block_entity_ticker(&existing);
                } else {
                    // Create new block entity
                    if let Some(entity) =
                        new_behavior.new_block_entity(self.level.clone(), pos, state)
                    {
                        self.add_and_register_block_entity(entity);
                    }
                }
            }
        }

        self.mark_unsaved();
        Some(old_state)
    }

    /// Gets a block state at the given position.
    #[must_use]
    pub fn get_block_state(&self, pos: BlockPos) -> BlockStateId {
        let y = pos.0.y;
        let section_index = self.get_section_index(y);

        // Bounds check - return air if out of range
        if section_index >= self.sections.sections.len() {
            return REGISTRY.blocks.get_base_state_id(&vanilla_blocks::VOID_AIR);
        }

        let section = &self.sections.sections[section_index];
        let section_guard = section.read();

        if section_guard.is_empty() {
            return REGISTRY.blocks.get_base_state_id(&vanilla_blocks::VOID_AIR);
        }

        let local_x = (pos.0.x & 15) as usize;
        let local_y = (y & 15) as usize;
        let local_z = (pos.0.z & 15) as usize;

        section_guard.states.get(local_x, local_y, local_z)
    }

    /// Extracts the chunk data for sending to the client.
    #[must_use]
    pub fn extract_chunk_data(&self) -> ChunkPacketData {
        let data = Vec::new();

        let mut cursor = Cursor::new(data);
        self.sections.sections.iter().for_each(|section| {
            section.read().write(&mut cursor);
        });

        let heightmaps_guard = self.heightmaps.read();

        // Collect block entity data for client sync
        let block_entities: Vec<BlockEntityInfo> = self
            .block_entities
            .get_all()
            .iter()
            .map(|entity| {
                let guard = entity.lock();
                let pos = guard.get_block_pos();
                let type_id = guard.get_type().id() as i32;
                let update_tag = guard.get_update_tag();

                BlockEntityInfo {
                    packed_xz: PackedChunkLocalXZ::from_block_pos(pos),
                    y: pos.0.y as i16,
                    type_id,
                    data: update_tag.into(),
                }
            })
            .collect();

        ChunkPacketData {
            heightmaps: Heightmaps {
                heightmaps: vec![
                    (
                        ProtocolHeightmapType::WorldSurface,
                        heightmaps_guard
                            .get(HeightmapType::WorldSurface)
                            .get_raw_data(),
                    ),
                    (
                        ProtocolHeightmapType::MotionBlocking,
                        heightmaps_guard
                            .get(HeightmapType::MotionBlocking)
                            .get_raw_data(),
                    ),
                    (
                        ProtocolHeightmapType::MotionBlockingNoLeaves,
                        heightmaps_guard
                            .get(HeightmapType::MotionBlockingNoLeaves)
                            .get_raw_data(),
                    ),
                ],
            },
            data: cursor.into_inner(),
            block_entities,
        }
    }

    /// Extracts the light data for sending to the client.
    #[must_use]
    pub fn extract_light_data(&self) -> LightUpdatePacketData {
        // Vanilla's light section count is sectionsCount + 2 (one below and one above the world)
        let light_section_count = self.sections.sections.len() + 2;
        let mut sky_y_mask = BitSet(vec![0; light_section_count.div_ceil(64)].into_boxed_slice());
        let mut block_y_mask = BitSet(vec![0; light_section_count.div_ceil(64)].into_boxed_slice());
        let empty_sky_y_mask = BitSet(vec![0; light_section_count.div_ceil(64)].into_boxed_slice());
        let empty_block_y_mask =
            BitSet(vec![0; light_section_count.div_ceil(64)].into_boxed_slice());

        let mut sky_updates = Vec::new();
        let mut block_updates = Vec::new();

        for i in 0..light_section_count {
            sky_y_mask.set(i, true);
            block_y_mask.set(i, true);
            sky_updates.push(vec![0xFF; 2048]);
            block_updates.push(vec![0xFF; 2048]);
        }

        LightUpdatePacketData {
            sky_y_mask,
            block_y_mask,
            empty_sky_y_mask,
            empty_block_y_mask,
            sky_updates,
            block_updates,
        }
    }
}
