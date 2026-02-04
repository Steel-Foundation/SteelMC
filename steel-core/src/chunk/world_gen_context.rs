//! This module contains the `WorldGenContext` struct, which is used to provide context for chunk generation.

use std::sync::{Arc, Weak};

use enum_dispatch::enum_dispatch;

use crate::chunk::{
    chunk_access::ChunkAccess, chunk_generator::ChunkGenerator,
    flat_chunk_generator::FlatChunkGenerator,
};
use crate::world::World;

#[allow(missing_docs)]
#[enum_dispatch(ChunkGenerator)]
pub enum ChunkGeneratorType {
    Flat(FlatChunkGenerator),
    //Custom(Box<dyn ChunkGenerator>),
}

/// Context for world generation.
///
/// Similar to vanilla's `WorldGenContext`, this provides access to the level/dimension
/// and generation infrastructure.
pub struct WorldGenContext {
    /// The chunk generator to use.
    pub generator: Arc<ChunkGeneratorType>,
    /// Weak reference to the world (to avoid circular Arc reference).
    /// Use `world()` to get a strong reference when needed.
    world: Weak<World>,
    // Add other fields as needed:
    // pub structure_manager: StructureTemplateManager,
    // pub light_engine: ThreadedLevelLightEngine,
    // pub main_thread_executor: Executor,
    // pub unsaved_listener: UnsavedListener,
}

impl WorldGenContext {
    /// Creates a new `WorldGenContext`.
    #[must_use]
    pub const fn new(generator: Arc<ChunkGeneratorType>, world: Weak<World>) -> Self {
        Self { generator, world }
    }

    /// Gets a strong reference to the world.
    ///
    /// # Panics
    /// Panics if the world has been dropped.
    #[must_use]
    pub fn world(&self) -> Arc<World> {
        self.world.upgrade().expect("World has been dropped")
    }

    /// Gets a weak reference to the world.
    ///
    /// This is useful for passing to chunks without creating a strong reference cycle.
    #[must_use]
    pub fn weak_world(&self) -> Weak<World> {
        self.world.clone()
    }

    /// Returns the minimum Y coordinate of the world.
    #[must_use]
    pub fn min_y(&self) -> i32 {
        self.world().get_min_y()
    }

    /// Returns the total height of the world in blocks.
    #[must_use]
    pub fn height(&self) -> i32 {
        self.world().get_height()
    }

    #[must_use]
    /// How many sections this dimension has
    pub fn section_count(&self) -> usize {
        (self.height() / 16) as usize
    }
}
