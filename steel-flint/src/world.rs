//! Test world implementation using the real steel-core World.
//!
//! This module provides a test world that wraps the real `Arc<World>` from steel-core,
//! configured with RAM-only storage for instant chunk creation without disk I/O.

use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};

use flint_core::Block;
use flint_core::{BlockPos as FlintBlockPos, FlintPlayer, FlintWorld};
use futures::executor;
use steel_core::chunk::chunk_access::{ChunkAccess, ChunkStatus};
use steel_core::chunk::chunk_generator::ChunkGenerator;
use steel_core::chunk::chunk_holder::ChunkHolder;
use steel_core::chunk::empty_chunk_generator::EmptyChunkGenerator;
use steel_core::chunk::proto_chunk::ProtoChunk;
use steel_core::chunk::section::{ChunkSection, Sections};
use steel_core::chunk::world_gen_context::ChunkGeneratorType;
use steel_core::world::{World, WorldConfig, WorldStorageConfig};
use steel_registry::vanilla_dimension_types::OVERWORLD;
use steel_utils::{BlockPos, ChunkPos, types::UpdateFlags};

use crate::convert::{flint_block_to_state_id, flint_pos_to_steel, state_id_to_block};
use crate::player::SteelTestPlayer;
use crate::runtime;

/// Test world implementation using the real steel-core World.
///
/// This wraps an `Arc<World>` configured with RAM-only storage:
/// - Chunks are created empty (all air) on-demand
/// - No disk I/O, no chunk generation delay
/// - Full block behavior system (neighbors, shapes, etc.)
/// - Real tick processing
pub struct SteelTestWorld {
    /// The underlying steel-core world.
    world: Arc<World>,
    /// Current tick count (for `FlintWorld` trait).
    tick: AtomicU64,
}

impl SteelTestWorld {
    /// Creates a new test world with RAM-only storage.
    ///
    /// The world uses the overworld dimension type and starts with seed 0.
    /// All chunks are created empty on-demand.
    ///
    /// # Panic
    /// shouldn't panic only something is completely broken and then it is ok
    #[allow(clippy::missing_panics_doc)]
    #[must_use]
    pub fn new() -> Self {
        let rt = runtime();

        // Create world with RAM-only storage
        let config = WorldConfig {
            storage: WorldStorageConfig::RamOnly,
            generator: Arc::new(ChunkGeneratorType::Empty(EmptyChunkGenerator::new())),
        };

        let dimension = OVERWORLD;

        // Block on async world creation
        let world = rt
            .block_on(async { World::new_with_config(rt.clone(), dimension, 0, config).await })
            .expect("Failed to create test world");

        Self {
            world,
            tick: AtomicU64::new(0),
        }
    }

    /// Gets a reference to the underlying steel-core world.
    #[must_use]
    pub const fn inner(&self) -> &Arc<World> {
        &self.world
    }

    /// Ensures the chunk containing the given block position is loaded.
    ///
    /// This is intended for testing only. It blocks until the chunk is loaded
    /// from storage. For RAM-only storage, this creates empty chunks on-demand.
    fn ensure_chunk_at(&self, pos: &BlockPos) {
        let chunk_x = pos.x() >> 4;
        let chunk_z = pos.z() >> 4;
        let chunk_pos = ChunkPos::new(chunk_x, chunk_z);

        let chunk_map = &self.world.chunk_map;

        // Check if already loaded
        if chunk_map.chunks.contains_sync(&chunk_pos) {
            return;
        }

        // Get dimension info from world
        let Some(world) = chunk_map.world_gen_context.weak_world().upgrade() else {
            tracing::error!("World has been dropped, cannot load chunk");
            return;
        };
        let dimension = &world.dimension;
        let min_y = dimension.min_y;
        let height = dimension.height;
        let level = chunk_map.world_gen_context.weak_world();

        // Block on async storage load
        let storage = &chunk_map.storage;
        let level_clone = level.clone();
        let result = executor::block_on(async {
            storage
                .load_chunk(chunk_pos, min_y, height, level_clone)
                .await
        });

        match result {
            Ok(Some((chunk, _status))) => {
                // Insert the chunk into the map
                // Use ticket level 0 (highest priority) for test chunks
                let holder = ChunkHolder::new(chunk_pos, 0, min_y, height);
                holder.insert_chunk(chunk, ChunkStatus::Full);

                // Use insert_sync since we're already in a blocking context
                // and the scc HashMap handles concurrent access
                let _ = chunk_map.chunks.insert_sync(chunk_pos, Arc::new(holder));
            }
            Ok(None) => {
                // Chunk doesn't exist in storage - generate it
                let holder = Arc::new(ChunkHolder::new(chunk_pos, 0, min_y, height));

                // Create empty sections (same as chunk_status_tasks::empty)
                let sections = (0..chunk_map.world_gen_context.section_count())
                    .map(|_| ChunkSection::new_empty())
                    .collect::<Vec<_>>()
                    .into_boxed_slice();

                let proto_chunk =
                    ProtoChunk::new(Sections::from_owned(sections), chunk_pos, min_y, height);

                // Insert with Empty status so try_chunk will work
                holder.insert_chunk(ChunkAccess::Proto(proto_chunk), ChunkStatus::Empty);

                // Run generator (fills blocks for flat world, no-op for empty world)
                if let Some(chunk) = holder.try_chunk(ChunkStatus::Empty) {
                    chunk_map
                        .world_gen_context
                        .generator
                        .fill_from_noise(&chunk);
                }

                // Upgrade to full LevelChunk and notify Full status
                holder.upgrade_to_full(level);
                holder.notify_status(ChunkStatus::Full);

                let _ = chunk_map.chunks.insert_sync(chunk_pos, holder);
            }
            Err(e) => {
                tracing::error!("Failed to load chunk {chunk_pos:?}: {e}");
            }
        }
    }
}

impl Default for SteelTestWorld {
    fn default() -> Self {
        Self::new()
    }
}

impl FlintWorld for SteelTestWorld {
    fn do_tick(&mut self) {
        let tick_count = self.tick.fetch_add(1, Ordering::SeqCst);

        // Run a real world tick
        // Note: For testing we run with `runs_normally = true`
        self.world.tick_b(tick_count, true);
    }

    fn current_tick(&self) -> u64 {
        self.tick.load(Ordering::SeqCst)
    }

    fn get_block(&self, pos: FlintBlockPos) -> Block {
        let steel_pos = flint_pos_to_steel(pos);

        // Ensure the chunk is loaded (for RAM storage this creates empty chunks)
        self.ensure_chunk_at(&steel_pos);

        let state = self.world.get_block_state(&steel_pos);
        state_id_to_block(state)
    }

    fn set_block(&mut self, pos: FlintBlockPos, block: &Block) {
        let Some(state_id) = flint_block_to_state_id(block) else {
            tracing::warn!("Unknown block: {} - skipping placement", block.id);
            return;
        };

        let steel_pos = flint_pos_to_steel(pos);

        // Ensure the chunk is loaded before setting blocks
        self.ensure_chunk_at(&steel_pos);

        // Use the real World::set_block which handles:
        // - Neighbor updates
        // - Shape updates
        // - Block behavior callbacks (on_place, etc.)
        self.world
            .set_block(steel_pos, state_id, UpdateFlags::UPDATE_ALL);
    }

    fn create_player(&mut self) -> Box<dyn FlintPlayer> {
        Box::new(SteelTestPlayer::new(self.world.clone()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::init_test_registries;

    #[test]
    fn test_world_creation() {
        init_test_registries();
        let world = SteelTestWorld::new();
        assert_eq!(world.current_tick(), 0);
    }

    #[test]
    fn test_world_tick() {
        init_test_registries();
        let mut world = SteelTestWorld::new();
        assert_eq!(world.current_tick(), 0);

        world.do_tick();
        assert_eq!(world.current_tick(), 1);

        world.do_tick();
        world.do_tick();
        assert_eq!(world.current_tick(), 3);
    }

    #[test]
    fn test_get_air_by_default() {
        init_test_registries();
        let world = SteelTestWorld::new();
        let block = world.get_block([0, 64, 0]);
        // Empty chunks are filled with air (or void_air depending on implementation)
        assert!(
            block.id == "minecraft:air" || block.id == "minecraft:void_air",
            "Expected air or void_air, got: {}",
            block.id
        );
    }

    #[test]
    fn test_set_and_get_block() {
        init_test_registries();
        let mut world = SteelTestWorld::new();

        let stone = Block::new("minecraft:stone");
        world.set_block([0, 64, 0], &stone);

        let retrieved = world.get_block([0, 64, 0]);
        assert_eq!(retrieved.id, "minecraft:stone");
    }

    #[test]
    fn test_set_air_clears_block() {
        init_test_registries();
        let mut world = SteelTestWorld::new();

        // Place a block
        let stone = Block::new("minecraft:stone");
        world.set_block([0, 64, 0], &stone);

        let retrieved = world.get_block([0, 64, 0]);
        assert_eq!(retrieved.id, "minecraft:stone");

        // Remove with air
        let air = Block::new("minecraft:air");
        world.set_block([0, 64, 0], &air);

        let retrieved = world.get_block([0, 64, 0]);
        // Accept both air and void_air as valid "cleared" states
        assert!(
            retrieved.id == "minecraft:air" || retrieved.id == "minecraft:void_air",
            "Expected air or void_air, got: {}",
            retrieved.id
        );
    }
}
