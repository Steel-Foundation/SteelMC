//! Entity registry for creating entity instances.

use std::ops::Deref;
use std::sync::{Arc, OnceLock, Weak};

use glam::DVec3;
use simdnbt::borrow::BaseNbtCompound as BorrowedNbtCompound;
use steel_registry::entity_types::EntityTypeRef;
use steel_registry::{REGISTRY, RegistryEntry};
use steel_registry::{RegistryExt, vanilla_entities};
use steel_utils::{BlockPos, Direction};
use uuid::Uuid;

use super::entities::{
    BlockDisplayEntity, ChestMinecartEntity, EndCrystalEntity, ItemEntity, ItemFrameEntity,
    RawEntity,
};
use super::{SharedEntity, next_entity_id};
use crate::world::World;

/// Factory function type for creating entities.
///
/// Takes the entity ID, spawn position, and world reference.
/// Returns a new entity instance. The entity ID should be obtained from
/// `next_entity_id()`.
pub type EntityFactory = fn(i32, DVec3, Weak<World>) -> SharedEntity;

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
    i32,         // entity_id
    DVec3,       // position
    Uuid,        // uuid
    DVec3,       // velocity
    (f32, f32),  // rotation (yaw, pitch)
    bool,        // on_ground
    Weak<World>, // world
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
        let id = entity_type.id();
        self.entries[id].factory = Some(factory);
    }

    /// Registers a load factory function for an entity type.
    ///
    /// The load factory is used when loading entities from disk.
    pub fn register_load(&mut self, entity_type: EntityTypeRef, factory: EntityLoadFactory) {
        let id = entity_type.id();
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
            .map(|f| f(entity_id, pos, world))
    }

    /// Creates an entity from persisted data and loads its type-specific NBT.
    ///
    /// Returns `None` if no load factory is registered for the entity type.
    #[must_use]
    #[expect(
        clippy::too_many_arguments,
        reason = "all fields are required to reconstruct a persisted entity"
    )]
    pub fn create_and_load(
        &self,
        entity_type: EntityTypeRef,
        pos: DVec3,
        uuid: Uuid,
        velocity: DVec3,
        rotation: (f32, f32),
        on_ground: bool,
        world: Weak<World>,
        nbt: &BorrowedNbtCompound<'_>,
    ) -> Option<SharedEntity> {
        let id = entity_type.id();
        let load_factory = self.entries.get(id)?.load_factory?;

        let entity_id = next_entity_id();
        let entity = load_factory(entity_id, pos, uuid, velocity, rotation, on_ground, world);
        entity.load_additional(nbt);
        Some(entity)
    }

    /// Creates an entity from persisted data, falling back to raw NBT preservation.
    #[must_use]
    #[expect(
        clippy::too_many_arguments,
        reason = "all fields are required to reconstruct a persisted entity"
    )]
    pub fn create_and_load_or_raw(
        &self,
        entity_type: EntityTypeRef,
        pos: DVec3,
        uuid: Uuid,
        velocity: DVec3,
        rotation: (f32, f32),
        on_ground: bool,
        world: Weak<World>,
        nbt: &BorrowedNbtCompound<'_>,
    ) -> SharedEntity {
        let id = entity_type.id();
        if let Some(load_factory) = self.entries.get(id).and_then(|entry| entry.load_factory) {
            let entity_id = next_entity_id();
            let entity = load_factory(entity_id, pos, uuid, velocity, rotation, on_ground, world);
            entity.load_additional(nbt);
            return entity;
        }

        let entity: SharedEntity = Arc::new(RawEntity::from_saved(
            next_entity_id(),
            pos,
            uuid,
            velocity,
            rotation,
            on_ground,
            world,
            entity_type,
        ));
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
    registry.register(&vanilla_entities::BLOCK_DISPLAY, |id, pos, world| {
        Arc::new(BlockDisplayEntity::new(id, pos, world))
    });
    registry.register_load(
        &vanilla_entities::BLOCK_DISPLAY,
        |id, pos, uuid, _velocity, _rotation, _on_ground, world| {
            Arc::new(BlockDisplayEntity::from_saved(id, pos, uuid, world))
        },
    );

    // Register item entity factory
    registry.register(&vanilla_entities::ITEM, |id, pos, world| {
        Arc::new(ItemEntity::new(id, pos, world))
    });
    registry.register_load(
        &vanilla_entities::ITEM,
        |id, pos, uuid, velocity, rotation, on_ground, world| {
            Arc::new(ItemEntity::from_saved(
                id, pos, uuid, velocity, rotation, on_ground, world,
            ))
        },
    );

    // Register end crystal entity factory
    registry.register(&vanilla_entities::END_CRYSTAL, |id, pos, world| {
        Arc::new(EndCrystalEntity::new(id, pos, world))
    });
    registry.register_load(
        &vanilla_entities::END_CRYSTAL,
        |id, pos, uuid, _velocity, rotation, _on_ground, world| {
            Arc::new(EndCrystalEntity::from_saved(id, pos, uuid, rotation, world))
        },
    );

    // Register chest minecart entity factory
    registry.register(&vanilla_entities::CHEST_MINECART, |id, pos, world| {
        Arc::new(ChestMinecartEntity::new(id, pos, world))
    });
    registry.register_load(
        &vanilla_entities::CHEST_MINECART,
        |id, pos, uuid, velocity, rotation, on_ground, world| {
            Arc::new(ChestMinecartEntity::from_saved(
                id, pos, uuid, velocity, rotation, on_ground, world,
            ))
        },
    );

    registry.register(&vanilla_entities::ITEM_FRAME, |id, pos, world| {
        Arc::new(ItemFrameEntity::new(
            id,
            BlockPos::new(
                pos.x.floor() as i32,
                pos.y.floor() as i32,
                pos.z.floor() as i32,
            ),
            Direction::South,
            world,
        ))
    });
    registry.register_load(
        &vanilla_entities::ITEM_FRAME,
        |id, pos, uuid, _velocity, rotation, _on_ground, world| {
            Arc::new(ItemFrameEntity::from_saved(id, pos, uuid, rotation, world))
        },
    );

    assert!(
        ENTITIES.set(registry).is_ok(),
        "Entity registry already initialized"
    );
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use simdnbt::borrow::read_compound as read_borrowed_compound;
    use simdnbt::owned::NbtCompound;
    use steel_registry::test_support::init_test_registry;

    use super::*;

    #[test]
    fn create_and_load_or_raw_preserves_unregistered_entity_data() {
        init_test_registry();
        let registry = EntityRegistry::new();
        let mut nbt = NbtCompound::new();
        nbt.insert("CustomName", "raw");
        let mut bytes = Vec::new();
        nbt.write(&mut bytes);
        let borrowed =
            read_borrowed_compound(&mut Cursor::new(&bytes)).expect("test nbt should reborrow");

        let entity = registry.create_and_load_or_raw(
            &vanilla_entities::VILLAGER,
            DVec3::new(1.0, 2.0, 3.0),
            Uuid::from_u128(1),
            DVec3::new(0.1, 0.0, 0.2),
            (45.0, 10.0),
            true,
            Weak::new(),
            &borrowed,
        );

        assert_eq!(&entity.entity_type().key, &vanilla_entities::VILLAGER.key);
        assert_eq!(entity.position(), DVec3::new(1.0, 2.0, 3.0));
        assert_eq!(entity.velocity(), DVec3::new(0.1, 0.0, 0.2));
        assert_eq!(entity.rotation(), (45.0, 10.0));
        assert!(entity.on_ground());

        let mut saved = NbtCompound::new();
        entity.save_additional(&mut saved);
        assert_eq!(
            saved.string("CustomName").map(ToString::to_string),
            Some("raw".to_owned())
        );
    }
}
