//! Block entity registry for creating block entity instances.

use std::ops::Deref;
use std::sync::{Arc, OnceLock, Weak};

use simdnbt::borrow::BaseNbtCompound as BorrowedNbtCompound;
use steel_registry::REGISTRY;
use steel_registry::block_entity_type::BlockEntityTypeRef;
use steel_registry::vanilla_block_entity_types;
use steel_utils::locks::SyncMutex;
use steel_utils::{BlockPos, BlockStateId};

use super::SharedBlockEntity;
use super::entities::{BarrelBlockEntity, SignBlockEntity};
use crate::world::World;

/// Factory function type for creating block entities.
///
/// Takes the world, position and block state, returns a new block entity instance.
pub type BlockEntityFactory = fn(Weak<World>, BlockPos, BlockStateId) -> SharedBlockEntity;

/// Registry entry for a block entity type.
struct BlockEntityEntry {
    /// Factory function to create instances.
    factory: Option<BlockEntityFactory>,
}

/// Registry for block entity factories.
///
/// Maps `BlockEntityType` to factory functions that can create block entity instances.
/// This is used when loading block entities from disk or when blocks with entities
/// are placed.
pub struct BlockEntityRegistry {
    entries: Vec<BlockEntityEntry>,
}

impl BlockEntityRegistry {
    /// Creates a new empty registry with entries for all block entity types.
    #[must_use]
    pub fn new() -> Self {
        let count = REGISTRY.block_entity_types.len();
        let entries = (0..count)
            .map(|_| BlockEntityEntry { factory: None })
            .collect();

        Self { entries }
    }

    /// Registers a factory function for a block entity type.
    pub fn register(&mut self, block_entity_type: BlockEntityTypeRef, factory: BlockEntityFactory) {
        let id = *REGISTRY.block_entity_types.get_id(block_entity_type);
        self.entries[id].factory = Some(factory);
    }

    /// Creates a new block entity instance.
    ///
    /// Returns `None` if no factory is registered for the given type.
    #[must_use]
    pub fn create(
        &self,
        block_entity_type: BlockEntityTypeRef,
        level: Weak<World>,
        pos: BlockPos,
        state: BlockStateId,
    ) -> Option<SharedBlockEntity> {
        let id = *REGISTRY.block_entity_types.get_id(block_entity_type);
        self.entries.get(id)?.factory.map(|f| f(level, pos, state))
    }

    /// Creates a new block entity and loads NBT data into it.
    ///
    /// Returns `None` if no factory is registered for the given type.
    #[must_use]
    pub fn create_and_load(
        &self,
        block_entity_type: BlockEntityTypeRef,
        level: Weak<World>,
        pos: BlockPos,
        state: BlockStateId,
        nbt: &BorrowedNbtCompound<'_>,
    ) -> Option<SharedBlockEntity> {
        let entity = self.create(block_entity_type, level, pos, state)?;
        entity.lock().load_additional(nbt);
        Some(entity)
    }

    /// Returns whether a factory is registered for the given type.
    #[must_use]
    pub fn has_factory(&self, block_entity_type: BlockEntityTypeRef) -> bool {
        let id = *REGISTRY.block_entity_types.get_id(block_entity_type);
        self.entries.get(id).is_some_and(|e| e.factory.is_some())
    }
}

impl Default for BlockEntityRegistry {
    fn default() -> Self {
        Self::new()
    }
}

/// Wrapper for the global block entity registry that implements `Deref`.
pub struct BlockEntityRegistryLock(OnceLock<BlockEntityRegistry>);

impl Deref for BlockEntityRegistryLock {
    type Target = BlockEntityRegistry;

    fn deref(&self) -> &Self::Target {
        self.0.get().expect("Block entity registry not initialized")
    }
}

impl BlockEntityRegistryLock {
    /// Sets the registry. Returns `Err` if already initialized.
    pub fn set(&self, registry: BlockEntityRegistry) -> Result<(), BlockEntityRegistry> {
        self.0.set(registry)
    }
}

/// Global block entity registry.
///
/// Access via deref: `BLOCK_ENTITIES.create(type, pos, state)`
pub static BLOCK_ENTITIES: BlockEntityRegistryLock = BlockEntityRegistryLock(OnceLock::new());

/// Initializes the global block entity registry.
///
/// This should be called once after the main registry is frozen.
///
/// # Panics
///
/// Panics if called more than once.
pub fn init_block_entities() {
    let mut registry = BlockEntityRegistry::new();

    // Register sign block entity factory
    registry.register(vanilla_block_entity_types::SIGN, |level, pos, state| {
        Arc::new(SyncMutex::new(SignBlockEntity::new(level, pos, state)))
    });

    // Register hanging sign block entity factory
    registry.register(
        vanilla_block_entity_types::HANGING_SIGN,
        |level, pos, state| {
            Arc::new(SyncMutex::new(SignBlockEntity::new_hanging(
                level, pos, state,
            )))
        },
    );

    // Register barrel block entity factory
    registry.register(vanilla_block_entity_types::BARREL, |level, pos, state| {
        Arc::new(SyncMutex::new(BarrelBlockEntity::new(level, pos, state)))
    });

    assert!(
        BLOCK_ENTITIES.set(registry).is_ok(),
        "Block entity registry already initialized"
    );
}
