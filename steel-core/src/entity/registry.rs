//! Entity registry for creating entity instances.

use std::ops::Deref;
use std::sync::{Arc, OnceLock, Weak};

use glam::DVec3;
use simdnbt::borrow::{
    BaseNbtCompound as BorrowedNbtCompound, NbtCompound as BorrowedNbtCompoundView,
};
use steel_registry::RegistryExt;
use steel_registry::entity_type::EntityTypeRef;
use steel_registry::{REGISTRY, RegistryEntry};
use uuid::Uuid;

use super::entities::RawEntity;
use super::generated_entities::register_entity_factories;
use super::{
    EntityBaseLoad, EntityBaseSaveData, EntityFireFreezeState, SharedEntity, next_entity_id,
};
use crate::world::World;

/// Factory function type for creating entities.
///
/// Takes the entity type, entity ID, spawn position, and world reference.
/// Returns a new entity instance. The entity ID should be obtained from
/// `next_entity_id()`.
pub type EntityFactory = fn(EntityTypeRef, i32, DVec3, Weak<World>) -> SharedEntity;

/// Factory function type for loading entities from disk.
///
/// Takes the entity type and all base entity fields needed for reconstruction.
pub type EntityLoadFactory = fn(EntityTypeRef, EntityBaseLoad) -> SharedEntity;

/// Entity load request before the registry assigns a runtime ID.
pub struct EntityLoadRequest {
    /// Entity type to instantiate.
    pub entity_type: EntityTypeRef,
    /// Restored entity position.
    pub position: DVec3,
    /// Persisted entity UUID.
    pub uuid: Uuid,
    /// Restored velocity.
    pub velocity: DVec3,
    /// Restored yaw and pitch.
    pub rotation: (f32, f32),
    /// Restored accumulated fall distance.
    pub fall_distance: f64,
    /// Restored vanilla fire/freeze state.
    pub fire_freeze: EntityFireFreezeState,
    /// Restored ground-contact flag.
    pub on_ground: bool,
    /// Restored shared vanilla save data.
    pub save_data: EntityBaseSaveData,
    /// World reference for the loaded entity.
    pub world: Weak<World>,
}

impl EntityLoadRequest {
    fn into_base_load(self) -> (EntityTypeRef, EntityBaseLoad) {
        (
            self.entity_type,
            EntityBaseLoad {
                id: next_entity_id(),
                position: self.position,
                uuid: self.uuid,
                velocity: self.velocity,
                rotation: self.rotation,
                fall_distance: self.fall_distance,
                fire_freeze: self.fire_freeze,
                on_ground: self.on_ground,
                save_data: self.save_data,
                world: self.world,
            },
        )
    }
}

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
    ///
    /// # Panics
    ///
    /// Panics if a factory is already registered for the entity type.
    pub fn register(&mut self, entity_type: EntityTypeRef, factory: EntityFactory) {
        let id = entity_type.id();
        assert!(
            self.entries[id].factory.is_none(),
            "entity factory for {} is already registered",
            entity_type.key
        );
        self.entries[id].factory = Some(factory);
    }

    /// Registers a load factory function for an entity type.
    ///
    /// The load factory is used when loading entities from disk.
    ///
    /// # Panics
    ///
    /// Panics if a load factory is already registered for the entity type.
    pub fn register_load(&mut self, entity_type: EntityTypeRef, factory: EntityLoadFactory) {
        let id = entity_type.id();
        assert!(
            self.entries[id].load_factory.is_none(),
            "entity load factory for {} is already registered",
            entity_type.key
        );
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
        pos: DVec3,
        world: Weak<World>,
    ) -> Option<SharedEntity> {
        let id = entity_type.id();
        self.entries
            .get(id)?
            .factory
            .map(|f| f(entity_type, entity_id, pos, world))
    }

    /// Creates an entity from persisted data and loads its type-specific NBT.
    ///
    /// Returns `None` if no load factory is registered for the entity type.
    #[must_use]
    pub fn create_and_load(
        &self,
        request: EntityLoadRequest,
        nbt: &BorrowedNbtCompound<'_>,
    ) -> Option<SharedEntity> {
        let (entity_type, load) = request.into_base_load();
        let id = entity_type.id();
        let load_factory = self.entries.get(id)?.load_factory?;

        let entity = load_factory(entity_type, load);
        let nbt: BorrowedNbtCompoundView<'_, '_> = nbt.into();
        entity.load_additional(nbt);
        entity.sync_base_entity_data();
        Some(entity)
    }

    /// Creates an entity from persisted data, falling back to raw NBT preservation.
    #[must_use]
    pub fn create_and_load_or_raw(
        &self,
        request: EntityLoadRequest,
        nbt: &BorrowedNbtCompound<'_>,
    ) -> SharedEntity {
        let (entity_type, load) = request.into_base_load();
        let id = entity_type.id();
        if let Some(load_factory) = self.entries.get(id).and_then(|entry| entry.load_factory) {
            let entity = load_factory(entity_type, load);
            let nbt: BorrowedNbtCompoundView<'_, '_> = nbt.into();
            entity.load_additional(nbt);
            entity.sync_base_entity_data();
            return entity;
        }

        let entity: SharedEntity = Arc::new(RawEntity::from_saved(load, entity_type));
        let nbt: BorrowedNbtCompoundView<'_, '_> = nbt.into();
        entity.load_additional(nbt);
        entity
    }

    /// Returns whether a factory is registered for the given type.
    #[must_use]
    pub fn has_factory(&self, entity_type: EntityTypeRef) -> bool {
        let id = entity_type.id();
        self.entries.get(id).is_some_and(|e| e.factory.is_some())
    }

    /// Returns whether a load factory is registered for the given type.
    #[must_use]
    pub fn has_load_factory(&self, entity_type: EntityTypeRef) -> bool {
        let id = entity_type.id();
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

    /// Returns the initialized registry, if entity factories have been installed.
    #[must_use]
    pub fn get(&self) -> Option<&EntityRegistry> {
        self.0.get()
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
    register_entity_factories(&mut registry);

    assert!(
        ENTITIES.set(registry).is_ok(),
        "Entity registry already initialized"
    );
}

#[cfg(test)]
pub(crate) fn init_test_entities() {
    use steel_registry::test_support::init_test_registry;

    init_test_registry();

    if ENTITIES.get().is_some() {
        return;
    }

    let mut registry = EntityRegistry::new();
    register_entity_factories(&mut registry);
    let _ = ENTITIES.set(registry);
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use simdnbt::borrow::read_compound as read_borrowed_compound;
    use simdnbt::owned::NbtCompound;
    use steel_registry::test_support::init_test_registry;
    use steel_registry::vanilla_entities;

    use super::*;

    #[test]
    fn create_and_load_or_raw_preserves_unregistered_entity_data() {
        init_test_registry();
        let registry = EntityRegistry::new();
        let mut nbt = NbtCompound::new();
        nbt.insert("SteelRawMarker", "raw");
        let mut bytes = Vec::new();
        nbt.write(&mut bytes);
        let borrowed =
            read_borrowed_compound(&mut Cursor::new(&bytes)).expect("test nbt should reborrow");

        let entity = registry.create_and_load_or_raw(
            EntityLoadRequest {
                entity_type: &vanilla_entities::VILLAGER,
                position: DVec3::new(1.0, 2.0, 3.0),
                uuid: Uuid::from_u128(1),
                velocity: DVec3::new(0.1, 0.0, 0.2),
                rotation: (45.0, 10.0),
                fall_distance: 2.25,
                fire_freeze: EntityFireFreezeState::new(),
                on_ground: true,
                save_data: EntityBaseSaveData {
                    no_gravity: true,
                    invulnerable: true,
                    ..EntityBaseSaveData::new()
                },
                world: Weak::new(),
            },
            &borrowed,
        );

        assert_eq!(&entity.entity_type().key, &vanilla_entities::VILLAGER.key);
        assert_eq!(entity.position(), DVec3::new(1.0, 2.0, 3.0));
        assert_eq!(entity.velocity(), DVec3::new(0.1, 0.0, 0.2));
        assert_eq!(entity.rotation(), (45.0, 10.0));
        assert!((entity.fall_distance() - 2.25).abs() <= f64::EPSILON);
        assert!(entity.on_ground());
        assert!(entity.is_no_gravity());
        assert!(entity.is_invulnerable());

        let mut saved = NbtCompound::new();
        entity.save_additional(&mut saved);
        assert_eq!(
            saved.string("SteelRawMarker").map(ToString::to_string),
            Some("raw".to_owned())
        );
    }

    #[test]
    fn create_forwards_entity_type_to_factory() {
        init_test_registry();
        let mut registry = EntityRegistry::new();
        registry.register(
            &vanilla_entities::OAK_BOAT,
            |entity_type, id, pos, world| Arc::new(RawEntity::new(id, pos, world, entity_type)),
        );

        let Some(entity) =
            registry.create(&vanilla_entities::OAK_BOAT, 5, DVec3::ZERO, Weak::new())
        else {
            panic!("registered entity factory should create an entity");
        };

        assert_eq!(entity.entity_type(), &vanilla_entities::OAK_BOAT);
    }
}
