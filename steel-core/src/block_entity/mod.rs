//! Block entity system for blocks that need additional data storage.
//!
//! Block entities provide additional data storage and functionality for blocks
//! that need more than what block state properties can offer (e.g., chests,
//! furnaces, signs, etc.).
//!
//! # Architecture
//!
//! Similar to the block/item behavior system, block entities use a registry
//! pattern:
//! - `BlockEntityRegistry` - maps `BlockEntityType` to factory functions
//! - `BlockEntityStorage` - stores block entities in a chunk
//!
//! # Usage
//!
//! ```ignore
//! use steel_core::block_entity::{init_block_entities, BLOCK_ENTITIES};
//!
//! // After registry is frozen, call once at startup:
//! init_block_entities();
//!
//! // Create a block entity:
//! let entity = BLOCK_ENTITIES.create(block_entity_type, pos, state);
//! ```

pub mod entities;
mod registry;
mod storage;

use std::any::Any;
use std::sync::Arc;

use simdnbt::borrow::BaseNbtCompound as BorrowedNbtCompound;
use simdnbt::owned::NbtCompound;
use steel_registry::block_entity_type::BlockEntityTypeRef;
use steel_utils::{BlockPos, BlockStateId, locks::SyncMutex};

pub use registry::{BLOCK_ENTITIES, BlockEntityFactory, BlockEntityRegistry, init_block_entities};
pub use storage::BlockEntityStorage;

use crate::inventory::container::Container;
use crate::world::World;

/// Trait for all block entities.
///
/// Block entities are attached to specific blocks in the world and provide
/// additional data storage beyond what block states can hold.
pub trait BlockEntity: Send + Sync {
    // === Downcasting ===

    /// Returns a reference to the block entity as `Any` for downcasting.
    fn as_any(&self) -> &dyn Any;

    /// Returns a mutable reference to the block entity as `Any` for downcasting.
    fn as_any_mut(&mut self) -> &mut dyn Any;

    // === Identity ===

    /// Returns the type of this block entity.
    fn get_type(&self) -> BlockEntityTypeRef;

    /// Returns the position of this block entity in the world.
    fn get_block_pos(&self) -> BlockPos;

    /// Returns the current block state associated with this entity.
    fn get_block_state(&self) -> BlockStateId;

    /// Updates the cached block state.
    ///
    /// Called when the block state changes but the block entity is kept.
    fn set_block_state(&mut self, state: BlockStateId);

    // === Lifecycle ===

    /// Returns whether this block entity has been marked for removal.
    fn is_removed(&self) -> bool;

    /// Marks this block entity as removed.
    ///
    /// Removed block entities will be cleaned up and should not be ticked.
    fn set_removed(&mut self);

    /// Clears the removed flag.
    ///
    /// Used when re-adding a block entity that was previously removed.
    fn clear_removed(&mut self);

    /// Called when the block entity's data changes.
    ///
    /// Marks the containing chunk as dirty so changes are persisted to disk.
    fn set_changed(&self) {
        if let Some(world) = self.get_level() {
            world.block_entity_changed(self.get_block_pos());
        }
    }

    /// Gets the world reference if still valid.
    ///
    /// Block entities receive a `Weak<World>` at construction time.
    fn get_level(&self) -> Option<Arc<World>>;

    /// Called before the block entity is removed to handle side effects.
    ///
    /// For example, containers should drop their contents here.
    ///
    /// # Arguments
    /// * `pos` - The position of the block entity
    /// * `state` - The block state being removed
    #[allow(unused_variables)]
    fn pre_remove_side_effects(&mut self, pos: BlockPos, state: BlockStateId) {
        // Default: no side effects
    }

    // === Persistence (disk) ===

    /// Loads additional data from NBT.
    ///
    /// Called when loading the block entity from disk or receiving initial
    /// chunk data from the server.
    fn load_additional(&mut self, nbt: &BorrowedNbtCompound<'_>);

    /// Saves additional data to NBT.
    ///
    /// Called when saving the block entity to disk.
    fn save_additional(&self, nbt: &mut NbtCompound);

    // === Client Sync ===

    /// Returns the NBT data to send to clients for initial sync.
    ///
    /// This is included in the chunk data packet when the chunk is first sent.
    /// Return `None` if no client sync is needed.
    fn get_update_tag(&self) -> Option<NbtCompound> {
        None
    }

    // === Ticking ===

    /// Returns whether this block entity should be ticked every game tick.
    ///
    /// Block entities that return `true` will have their `tick()` method called
    /// each game tick.
    fn is_ticking(&self) -> bool {
        false
    }

    /// Called every game tick for ticking block entities.
    ///
    /// Only called if `is_ticking()` returns `true`.
    #[allow(unused_variables)]
    fn tick(&mut self, world: &World) {
        // Default: no-op
    }

    // === Container Access ===

    /// Returns this block entity as a container, if it implements Container.
    ///
    /// Override this in block entities that are also containers (e.g., chests,
    /// furnaces) to enable integration with the inventory locking system.
    fn as_container(&self) -> Option<&(dyn Container + 'static)> {
        None
    }

    /// Returns this block entity as a mutable container, if it implements Container.
    ///
    /// Override this in block entities that are also containers (e.g., chests,
    /// furnaces) to enable integration with the inventory locking system.
    fn as_container_mut(&mut self) -> Option<&mut (dyn Container + 'static)> {
        None
    }
}

/// Type alias for a shared, thread-safe block entity.
pub type SharedBlockEntity = Arc<SyncMutex<dyn BlockEntity>>;
