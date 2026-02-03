//! Entity registry for creating entity instances.

use std::ops::Deref;
use std::sync::{Arc, OnceLock, Weak};

use simdnbt::borrow::BaseNbtCompound as BorrowedNbtCompound;
use steel_registry::REGISTRY;
use steel_registry::entity_types::EntityTypeRef;
use steel_registry::vanilla_entities;
use steel_utils::math::Vector3;
use uuid::Uuid;

use super::entities::{BlockDisplayEntity, ItemEntity};
use super::{SharedEntity, next_entity_id};
use crate::world::World;

/// Factory function type for creating entities.
///
/// Takes the entity ID, spawn position, and world reference.
/// Returns a new entity instance. The entity ID should be obtained from
/// `next_entity_id()`.
pub type EntityFactory = fn(i32, Vector3<f64>, Weak<World>) -> SharedEntity;

/// Factory function type for loading entities from disk.
///
/// Takes all base entity fields needed for reconstruction:
/// - `entity_id`: Fresh ID from `next_entity_id()` (not persisted)
/// - position: Restored position
/// - uuid: Persisted UUID
/// - velocity: Restored velocity
/// - rotation: Restored (yaw, pitch)
/// - `on_ground`: Restored ground state
/// - world: Reference to the world
pub type EntityLoadFactory = fn(
    i32,          // entity_id
    Vector3<f64>, // position
    Uuid,         // uuid
    Vector3<f64>, // velocity
    (f32, f32),   // rotation (yaw, pitch)
    bool,         // on_ground
    Weak<World>,  // world
) -> SharedEntity;

/// Registry entry for an entity type.
struct EntityEntry {
    /// Factory function to create new instances.
    factory: Option<EntityFactory>,
    /// Factory function to load instances from disk.
    load_factory: Option<EntityLoadFactory>,
}

/// Registry for entity factories.
///
/// Maps `EntityType` to factory functions that can create entity instances.
/// This is used when loading entities from disk or when entities are spawned.
pub struct EntityRegistry {
    entries: Vec<EntityEntry>,
}

impl EntityRegistry {
    /// Creates a new empty registry with entries for all entity types.
    #[must_use]
    pub fn new() -> Self {
        let count = REGISTRY.entity_types.len();
        let entries = (0..count)
            .map(|_| EntityEntry {
                factory: None,
                load_factory: None,
            })
            .collect();

        Self { entries }
    }

    /// Registers a factory function for an entity type.
    pub fn register(&mut self, entity_type: EntityTypeRef, factory: EntityFactory) {
        let id = *REGISTRY.entity_types.get_id(entity_type);
        self.entries[id].factory = Some(factory);
    }

    /// Registers a load factory function for an entity type.
    ///
    /// The load factory is used when loading entities from disk.
    pub fn register_load(&mut self, entity_type: EntityTypeRef, factory: EntityLoadFactory) {
        let id = *REGISTRY.entity_types.get_id(entity_type);
        self.entries[id].load_factory = Some(factory);
    }

    /// Creates a new entity instance.
    ///
    /// Returns `None` if no factory is registered for the given type.
    #[must_use]
    pub fn create(
        &self,
        entity_type: EntityTypeRef,
        entity_id: i32,
        pos: Vector3<f64>,
        world: Weak<World>,
    ) -> Option<SharedEntity> {
        let id = *REGISTRY.entity_types.get_id(entity_type);
        self.entries
            .get(id)?
            .factory
            .map(|f| f(entity_id, pos, world))
    }

    /// Creates an entity from persisted data and loads its type-specific NBT.
    ///
    /// Returns `None` if no load factory is registered for the entity type.
    #[must_use]
    #[allow(clippy::too_many_arguments)]
    pub fn create_and_load(
        &self,
        entity_type: EntityTypeRef,
        pos: Vector3<f64>,
        uuid: Uuid,
        velocity: Vector3<f64>,
        rotation: (f32, f32),
        on_ground: bool,
        world: Weak<World>,
        nbt: &BorrowedNbtCompound<'_>,
    ) -> Option<SharedEntity> {
        let id = *REGISTRY.entity_types.get_id(entity_type);
        let load_factory = self.entries.get(id)?.load_factory?;

        let entity_id = next_entity_id();
        let entity = load_factory(entity_id, pos, uuid, velocity, rotation, on_ground, world);
        entity.load_additional(nbt);
        Some(entity)
    }

    /// Returns whether a factory is registered for the given type.
    #[must_use]
    pub fn has_factory(&self, entity_type: EntityTypeRef) -> bool {
        let id = *REGISTRY.entity_types.get_id(entity_type);
        self.entries.get(id).is_some_and(|e| e.factory.is_some())
    }

    /// Returns whether a load factory is registered for the given type.
    #[must_use]
    pub fn has_load_factory(&self, entity_type: EntityTypeRef) -> bool {
        let id = *REGISTRY.entity_types.get_id(entity_type);
        self.entries
            .get(id)
            .is_some_and(|e| e.load_factory.is_some())
    }
}

impl Default for EntityRegistry {
    fn default() -> Self {
        Self::new()
    }
}

/// Wrapper for the global entity registry that implements `Deref`.
pub struct EntityRegistryLock(OnceLock<EntityRegistry>);

impl Deref for EntityRegistryLock {
    type Target = EntityRegistry;

    fn deref(&self) -> &Self::Target {
        self.0.get().expect("Entity registry not initialized")
    }
}

impl EntityRegistryLock {
    /// Sets the registry. Returns `Err` if already initialized.
    pub fn set(&self, registry: EntityRegistry) -> Result<(), EntityRegistry> {
        self.0.set(registry)
    }
}

/// Global entity registry.
///
/// Access via deref: `ENTITIES.create(type, entity_id, pos)`
pub static ENTITIES: EntityRegistryLock = EntityRegistryLock(OnceLock::new());

/// Initializes the global entity registry.
///
/// This should be called once after the main registry is frozen.
///
/// # Panics
///
/// Panics if called more than once.
pub fn init_entities() {
    let mut registry = EntityRegistry::new();

    // Register block display entity factory
    registry.register(vanilla_entities::BLOCK_DISPLAY, |id, pos, world| {
        Arc::new(BlockDisplayEntity::new(id, pos, world))
    });
    registry.register_load(
        vanilla_entities::BLOCK_DISPLAY,
        |id, pos, uuid, _velocity, _rotation, _on_ground, world| {
            Arc::new(BlockDisplayEntity::from_saved(id, pos, uuid, world))
        },
    );

    // Register item entity factory
    registry.register(vanilla_entities::ITEM, |id, pos, world| {
        Arc::new(ItemEntity::new(id, pos, world))
    });
    registry.register_load(
        vanilla_entities::ITEM,
        |id, pos, uuid, velocity, rotation, on_ground, world| {
            Arc::new(ItemEntity::from_saved(
                id, pos, uuid, velocity, rotation, on_ground, world,
            ))
        },
    );

    assert!(
        ENTITIES.set(registry).is_ok(),
        "Entity registry already initialized"
    );
}
