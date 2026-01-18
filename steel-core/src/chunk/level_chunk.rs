//! This module contains the `LevelChunk` struct, which is a chunk that is ready to be sent to the client.
use std::{
    io::Cursor,
    ptr,
    sync::{
        Arc, Weak,
        atomic::{AtomicBool, Ordering},
    },
};

use rand::Rng;

use steel_protocol::packets::game::{
    ChunkPacketData, HeightmapType as ProtocolHeightmapType, Heightmaps, LightUpdatePacketData,
};
use steel_registry::{REGISTRY, blocks::block_state_ext::BlockStateExt, vanilla_blocks};
use steel_utils::{
    BlockPos, BlockStateId, ChunkPos, codec::BitSet, locks::SyncRwLock, types::UpdateFlags,
};

use crate::behavior::BLOCK_BEHAVIORS;
use crate::chunk::{
    heightmap::{ChunkHeightmaps, HeightmapType},
    proto_chunk::ProtoChunk,
    section::Sections,
};
use crate::world::World;

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
}

impl LevelChunk {
    /// Ticks this chunk, processing random block ticks.
    ///
    /// For each section that contains randomly-ticking blocks, selects
    /// `random_tick_speed` random blocks and calls their `random_tick` behavior.
    ///
    /// # Arguments
    /// * `random_tick_speed` - Number of random blocks to tick per section per tick.
    ///   This is controlled by the `randomTickSpeed` game rule.
    ///
    /// # Panics
    /// Panics if the block behavior registry has not been initialized.
    pub fn tick(&self, random_tick_speed: u32) {
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
    ) -> Self {
        // Transfer final heightmaps from proto chunk if available
        let proto_heightmaps = proto_chunk.heightmaps.read();
        let mut chunk_heightmaps = ChunkHeightmaps::new(min_y, height);

        // Copy final heightmap data if available in proto chunk
        for &hm_type in HeightmapType::final_types() {
            if let Some(proto_hm) = proto_heightmaps.get(&hm_type) {
                chunk_heightmaps
                    .get_mut(hm_type)
                    .set_raw_data(&proto_hm.get_raw_data());
            }
        }
        drop(proto_heightmaps);

        // Recalculate section counts for random tick optimization
        for section in &proto_chunk.sections.sections {
            section.write().recalculate_counts();
        }

        Self {
            sections: proto_chunk.sections,
            pos: proto_chunk.pos,
            dirty: AtomicBool::new(proto_chunk.dirty.load(Ordering::Acquire)),
            heightmaps: SyncRwLock::new(chunk_heightmaps),
            min_y,
            height,
            level,
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
    ///
    /// # Panics
    /// Panics if the block behavior registry has not been initialized.
    #[must_use]
    pub fn from_disk(
        sections: Sections,
        pos: ChunkPos,
        min_y: i32,
        height: i32,
        level: Weak<World>,
    ) -> Self {
        // Recalculate section counts for random tick optimization
        for section in &sections.sections {
            section.write().recalculate_counts();
        }

        Self {
            sections,
            pos,
            dirty: AtomicBool::new(false),
            heightmaps: SyncRwLock::new(ChunkHeightmaps::new(min_y, height)),
            min_y,
            height,
            level,
        }
    }

    /// Returns a reference to the world if it's still alive.
    ///
    /// This mirrors Java's `LevelChunk.getLevel()`.
    #[must_use]
    pub fn get_level(&self) -> Option<Arc<World>> {
        self.level.upgrade()
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

    /// Gets the section index for a given Y coordinate.
    #[must_use]
    fn get_section_index(&self, y: i32) -> usize {
        ((y - self.min_y) / 16) as usize
    }

    /// Marks the chunk as unsaved.
    fn mark_unsaved(&self) {
        self.dirty.store(true, Ordering::Release);
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

        let was_empty = section.read().is_empty();
        if was_empty && state.is_air() {
            return None;
        }

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
        if !ptr::eq(current_block, new_block) {
            return None;
        }

        if let Some(level) = self.get_level() {
            let block_changed = !ptr::eq(old_block, new_block);
            let moved_by_piston = flags.contains(UpdateFlags::UPDATE_MOVE_BY_PISTON);
            let side_effects = !flags.contains(UpdateFlags::UPDATE_SKIP_BLOCK_ENTITY_SIDEEFFECTS);

            // TODO: Block entity handling
            // In vanilla, block entities are removed when the block type changes:
            // if block_changed && old_state.has_block_entity() && !state.should_keep_block_entity(old_state) {
            //     if !level.is_client_side() && side_effects {
            //         if let Some(block_entity) = level.get_block_entity(pos) {
            //             block_entity.pre_remove_side_effects(pos, old_state);
            //         }
            //     }
            //     self.remove_block_entity(pos);
            // }
            let _ = side_effects; // suppress unused warning until block entities are implemented

            // Notify neighbors that we were removed (for rails, etc.)
            if block_changed && (flags.contains(UpdateFlags::UPDATE_NEIGHBORS) || moved_by_piston) {
                let block_behaviors = &*BLOCK_BEHAVIORS;
                let behavior = block_behaviors.get_behavior(old_block);
                behavior.affect_neighbors_after_removal(old_state, &level, pos, moved_by_piston);
            }

            // Call on_place for the new block
            if !flags.contains(UpdateFlags::UPDATE_SKIP_ON_PLACE) {
                let block_behaviors = &*BLOCK_BEHAVIORS;
                let behavior = block_behaviors.get_behavior(new_block);
                behavior.on_place(state, &level, pos, old_state, moved_by_piston);
            }

            // TODO: Block entity creation
            // In vanilla, new block entities are created after on_place:
            // if state.has_block_entity() {
            //     let block_entity = self.get_block_entity(pos, CHECK);
            //     if block_entity.is_none() {
            //         let new_entity = new_block.new_block_entity(pos, state);
            //         if let Some(entity) = new_entity {
            //             self.add_and_register_block_entity(entity);
            //         }
            //     } else {
            //         block_entity.set_block_state(state);
            //         self.update_block_entity_ticker(block_entity);
            //     }
            // }
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
            return REGISTRY.blocks.get_base_state_id(vanilla_blocks::VOID_AIR);
        }

        let section = &self.sections.sections[section_index];
        let section_guard = section.read();

        if section_guard.is_empty() {
            return REGISTRY.blocks.get_base_state_id(vanilla_blocks::VOID_AIR);
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
            block_entities: Vec::new(),
        }
    }

    /// Extracts the light data for sending to the client.
    #[must_use]
    pub fn extract_light_data(&self) -> LightUpdatePacketData {
        let section_count = self.sections.sections.len();
        let mut sky_y_mask = BitSet(vec![0; section_count.div_ceil(64)].into_boxed_slice());
        let mut block_y_mask = BitSet(vec![0; section_count.div_ceil(64)].into_boxed_slice());
        let empty_sky_y_mask = BitSet(vec![0; section_count.div_ceil(64)].into_boxed_slice());
        let empty_block_y_mask = BitSet(vec![0; section_count.div_ceil(64)].into_boxed_slice());

        let mut sky_updates = Vec::new();
        let mut block_updates = Vec::new();

        for i in 0..section_count {
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
