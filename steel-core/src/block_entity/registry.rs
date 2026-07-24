//! Block entity registry for creating block entity instances.

use std::io::Cursor;
use std::ops::Deref;
use std::sync::{Arc, OnceLock, Weak};

use simdnbt::borrow::NbtCompound as BorrowedRootNbtCompound;
use simdnbt::borrow::{
    BaseNbtCompound as BorrowedNbtCompound, read_compound as read_borrowed_compound,
};
use simdnbt::owned::NbtCompound;
use steel_registry::block_entity_type::BlockEntityTypeRef;
use steel_registry::vanilla_block_entity_types;
use steel_registry::{REGISTRY, RegistryEntry, RegistryExt};
use steel_utils::{BlockPos, BlockStateId};

use super::SharedBlockEntity;
use super::entities::{
    BarrelBlockEntity, BeehiveBlockEntity, ComparatorBlockEntity, DaylightDetectorBlockEntity,
    EndGatewayBlockEntity, EndPortalBlockEntity, PistonMovingBlockEntity, PotentSulfurBlockEntity,
    RawBlockEntity, SignBlockEntity,
};
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
        let id = block_entity_type.id();
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
        let id = block_entity_type.id();
        self.entries.get(id)?.factory.map(|f| f(level, pos, state))
    }

    /// Creates a block entity, falling back to an NBT-preserving raw entity.
    ///
    /// Use this for disk/worldgen paths where an unimplemented block entity type must still
    /// survive save/load. Gameplay paths that require concrete behavior should call
    /// [`Self::create`] and handle `None`.
    #[must_use]
    pub fn create_or_raw(
        &self,
        block_entity_type: BlockEntityTypeRef,
        level: Weak<World>,
        pos: BlockPos,
        state: BlockStateId,
    ) -> SharedBlockEntity {
        let id = block_entity_type.id();
        if let Some(factory) = self.entries.get(id).and_then(|entry| entry.factory) {
            factory(level, pos, state)
        } else {
            Arc::new(RawBlockEntity::new(block_entity_type, level, pos, state))
        }
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
        entity.load_additional(nbt);
        Some(entity)
    }

    /// Creates a block entity and loads borrowed NBT, falling back to raw preservation.
    #[must_use]
    pub fn create_and_load_or_raw(
        &self,
        block_entity_type: BlockEntityTypeRef,
        level: Weak<World>,
        pos: BlockPos,
        state: BlockStateId,
        nbt: &BorrowedNbtCompound<'_>,
    ) -> SharedBlockEntity {
        let id = block_entity_type.id();
        if let Some(factory) = self.entries.get(id).and_then(|entry| entry.factory) {
            let entity = factory(level, pos, state);
            entity.load_additional(nbt);
            entity
        } else {
            let nbt_view: BorrowedRootNbtCompound<'_, '_> = nbt.into();
            Arc::new(RawBlockEntity::with_data(
                block_entity_type,
                level,
                pos,
                state,
                nbt_view.to_owned(),
            ))
        }
    }

    /// Creates a block entity and loads owned NBT, falling back to raw preservation.
    #[must_use]
    pub fn create_and_load_owned_or_raw(
        &self,
        block_entity_type: BlockEntityTypeRef,
        level: Weak<World>,
        pos: BlockPos,
        state: BlockStateId,
        nbt: NbtCompound,
    ) -> SharedBlockEntity {
        let id = block_entity_type.id();
        if let Some(factory) = self.entries.get(id).and_then(|entry| entry.factory) {
            let entity = factory(level, pos, state);
            let mut nbt_bytes = Vec::new();
            nbt.write(&mut nbt_bytes);
            if let Ok(borrowed) = read_borrowed_compound(&mut Cursor::new(&nbt_bytes)) {
                entity.load_additional(&borrowed);
            } else {
                log::warn!(
                    "failed to reborrow owned NBT for block entity {}",
                    block_entity_type.key()
                );
            }
            entity
        } else {
            Arc::new(RawBlockEntity::with_data(
                block_entity_type,
                level,
                pos,
                state,
                nbt,
            ))
        }
    }

    /// Returns whether a factory is registered for the given type.
    #[must_use]
    pub fn has_factory(&self, block_entity_type: BlockEntityTypeRef) -> bool {
        let id = block_entity_type.id();
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

    fn get_or_init(&self, init: impl FnOnce() -> BlockEntityRegistry) -> &BlockEntityRegistry {
        self.0.get_or_init(init)
    }
}

/// Global block entity registry.
///
/// Access via deref: `BLOCK_ENTITIES.create(type, pos, state)`
pub static BLOCK_ENTITIES: BlockEntityRegistryLock = BlockEntityRegistryLock(OnceLock::new());

/// Initializes the global block entity registry.
///
/// This should be called after the main registry is frozen. Repeated calls are a no-op.
pub fn init_block_entities() {
    BLOCK_ENTITIES.get_or_init(|| {
        let mut registry = BlockEntityRegistry::new();

        // Register sign block entity factory
        registry.register(&vanilla_block_entity_types::SIGN, |level, pos, state| {
            Arc::new(SignBlockEntity::new(level, pos, state))
        });

        // Register hanging sign block entity factory
        registry.register(
            &vanilla_block_entity_types::HANGING_SIGN,
            |level, pos, state| Arc::new(SignBlockEntity::new_hanging(level, pos, state)),
        );

        // Register barrel block entity factory
        registry.register(&vanilla_block_entity_types::BARREL, |level, pos, state| {
            Arc::new(BarrelBlockEntity::new(level, pos, state))
        });

        // Register beehive block entity factory
        registry.register(&vanilla_block_entity_types::BEEHIVE, |level, pos, state| {
            Arc::new(BeehiveBlockEntity::new(level, pos, state))
        });

        // Register comparator block entity factory
        registry.register(
            &vanilla_block_entity_types::COMPARATOR,
            |level, pos, state| Arc::new(ComparatorBlockEntity::new(level, pos, state)),
        );

        registry.register(
            &vanilla_block_entity_types::DAYLIGHT_DETECTOR,
            |level, pos, state| Arc::new(DaylightDetectorBlockEntity::new(level, pos, state)),
        );

        registry.register(&vanilla_block_entity_types::PISTON, |level, pos, state| {
            Arc::new(PistonMovingBlockEntity::new(level, pos, state))
        });

        // Register End gateway block entity factory
        registry.register(
            &vanilla_block_entity_types::END_GATEWAY,
            |level, pos, state| Arc::new(EndGatewayBlockEntity::new(level, pos, state)),
        );

        // Register End portal block entity factory
        registry.register(
            &vanilla_block_entity_types::END_PORTAL,
            |level, pos, state| Arc::new(EndPortalBlockEntity::new(level, pos, state)),
        );

        // Register potent sulfur block entity factory
        registry.register(
            &vanilla_block_entity_types::POTENT_SULFUR,
            |level, pos, state| Arc::new(PotentSulfurBlockEntity::new(level, pos, state)),
        );

        registry
    });
}
