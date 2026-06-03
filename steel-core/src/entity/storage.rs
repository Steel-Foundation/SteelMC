//! Per-chunk entity storage.
//!
//! Entities are stored in chunks similar to block entities.
//! The chunk owns the `Arc<dyn Entity>` and is responsible for ticking.

use std::fmt;
use std::sync::Arc;

use rustc_hash::{FxHashMap, FxHashSet};
use steel_protocol::packets::game::CSetEntityData;
use steel_utils::ChunkPos;
use steel_utils::locks::SyncRwLock;

use super::{RemovalReason, SharedEntity, tick_vehicle_passengers};
use crate::world::World;

/// Storage for entities in a chunk.
///
/// This mirrors `BlockEntityStorage` - entities are keyed by their ID
/// and ticked from the chunk's tick method.
pub struct EntityStorage {
    /// Entities in this chunk, keyed by entity ID.
    entities: SyncRwLock<FxHashMap<i32, SharedEntity>>,
}

fn should_keep_for_save(entity: &SharedEntity) -> bool {
    !entity.is_removed()
        || entity
            .removal_reason()
            .is_some_and(RemovalReason::should_save)
}

impl fmt::Debug for EntityStorage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("EntityStorage")
            .field("len", &self.len())
            .finish()
    }
}

impl EntityStorage {
    /// Creates a new empty entity storage.
    #[must_use]
    pub fn new() -> Self {
        Self {
            entities: SyncRwLock::new(FxHashMap::default()),
        }
    }

    /// Adds an entity to this chunk's storage.
    pub fn add(&self, entity: SharedEntity) {
        let id = entity.id();
        if self.entities.write().insert(id, entity).is_some() {
            panic!("entity id {id} is already present in chunk entity storage");
        }
    }

    /// Removes an entity from this chunk's storage by ID.
    ///
    /// Returns the entity if it was present.
    pub fn remove(&self, entity_id: i32) -> Option<SharedEntity> {
        self.entities.write().remove(&entity_id)
    }

    /// Gets an entity by ID.
    #[must_use]
    pub fn get(&self, entity_id: i32) -> Option<SharedEntity> {
        self.entities.read().get(&entity_id).cloned()
    }

    /// Returns all entities in this chunk.
    #[must_use]
    pub fn get_all(&self) -> Vec<SharedEntity> {
        self.entities.read().values().cloned().collect()
    }

    /// Returns the number of entities in this chunk.
    #[must_use]
    pub fn len(&self) -> usize {
        self.entities.read().len()
    }

    /// Returns whether there are no entities in this chunk.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entities.read().is_empty()
    }

    /// Returns entities that should be saved when the chunk is persisted.
    ///
    /// Excludes:
    /// - Removed entities
    /// - Entity types with `can_serialize = false` (including players)
    #[must_use]
    pub fn get_saveable_entities(&self) -> Vec<SharedEntity> {
        self.entities
            .read()
            .values()
            .filter(|e| should_keep_for_save(e) && e.entity_type().can_serialize)
            .cloned()
            .collect()
    }

    /// Ticks all entities in this chunk and broadcasts dirty entity data.
    ///
    /// Called from `LevelChunk::tick()`.
    /// Returns `true` if any entities were ticked (chunk should be marked dirty).
    ///
    /// Uses `tick_count` to prevent double-ticking: if an entity moves to a
    /// different chunk during its tick and that chunk is ticked later in the
    /// same server tick, the entity will be skipped.
    pub fn tick(&self, world: &Arc<World>, _chunk_pos: ChunkPos, tick_count: i32) -> bool {
        let mut post_tick = |entity: &SharedEntity| {
            Self::sync_after_tick(world, entity, tick_count);
        };
        self.tick_entities(tick_count, &mut post_tick)
    }

    fn tick_entities(&self, tick_count: i32, post_tick: &mut impl FnMut(&SharedEntity)) -> bool {
        // Clone to avoid holding lock during tick
        let entities: Vec<SharedEntity> = self.entities.read().values().cloned().collect();

        let mut ticked_any = false;
        for entity in entities {
            if entity.is_removed() {
                continue;
            }

            // Skip entities that were already ticked this server tick.
            // This happens when an entity moves from a chunk that was already
            // ticked to this chunk within the same server tick.
            if entity.was_ticked_this_tick(tick_count) {
                continue;
            }

            if Self::is_valid_passenger_or_stop_riding(&entity) {
                continue;
            }

            ticked_any = true;
            Self::tick_non_passenger(&entity, tick_count, post_tick);
        }

        // Cleanup removed entities
        self.entities.write().retain(|_, e| should_keep_for_save(e));

        ticked_any
    }

    fn is_valid_passenger_or_stop_riding(entity: &SharedEntity) -> bool {
        let Some(vehicle) = entity.vehicle() else {
            return false;
        };

        if !vehicle.is_removed() && vehicle.has_passenger(entity.as_ref()) {
            Self::assert_acyclic_vehicle_chain(entity);
            return true;
        }

        entity.stop_riding();
        false
    }

    fn assert_acyclic_vehicle_chain(entity: &SharedEntity) {
        let mut visited = FxHashSet::default();
        visited.insert(entity.id());

        let mut vehicle = entity.vehicle();
        while let Some(current) = vehicle {
            if !visited.insert(current.id()) {
                panic!(
                    "cyclic passenger relationship involving entity {}",
                    entity.id()
                );
            }
            vehicle = current.vehicle();
        }
    }

    fn tick_non_passenger(
        entity: &SharedEntity,
        tick_count: i32,
        post_tick: &mut impl FnMut(&SharedEntity),
    ) {
        // Mark as ticked before running tick() to prevent double-tick
        // even if the entity moves during its own tick.
        entity.mark_ticked(tick_count);
        entity.advance_tick_count();

        // Entity-specific tick (entities access world via self.level()).
        entity.tick();
        post_tick(entity);

        tick_vehicle_passengers(entity.as_ref(), tick_count, post_tick);
    }

    fn sync_after_tick(world: &Arc<World>, entity: &SharedEntity, tick_count: i32) {
        // Send position/velocity changes (mirrors vanilla's ServerEntity.sendChanges()).
        entity.send_changes(tick_count);

        // Broadcast dirty entity data (base tick behavior).
        if let Some(dirty_data) = entity.pack_dirty_entity_data() {
            let packet = CSetEntityData::new(entity.id(), dirty_data);
            world.broadcast_to_entity_trackers(entity.id(), packet, None);
        }
    }

    /// Clears all entities from storage.
    pub fn clear(&self) {
        self.entities.write().clear();
    }
}

impl Default for EntityStorage {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Weak};

    use glam::DVec3;
    use steel_registry::vanilla_entities;
    use steel_utils::locks::SyncMutex;

    use super::*;
    use crate::entity::entities::RawEntity;
    use crate::entity::{Entity, EntityBase, WeakEntity};

    fn raw_item(id: i32) -> SharedEntity {
        Arc::new(RawEntity::new(
            id,
            DVec3::ZERO,
            Weak::new(),
            &vanilla_entities::ITEM,
        ))
    }

    struct TickTestEntity {
        base: EntityBase,
        vehicle: SyncMutex<Option<WeakEntity>>,
        passengers: SyncMutex<Vec<WeakEntity>>,
        tick_calls: SyncMutex<i32>,
        ride_tick_calls: SyncMutex<i32>,
        stop_riding_calls: SyncMutex<i32>,
    }

    impl TickTestEntity {
        fn new(id: i32) -> Arc<Self> {
            Arc::new(Self {
                base: EntityBase::new(
                    id,
                    DVec3::ZERO,
                    vanilla_entities::ITEM.dimensions,
                    Weak::new(),
                ),
                vehicle: SyncMutex::new(None),
                passengers: SyncMutex::new(Vec::new()),
                tick_calls: SyncMutex::new(0),
                ride_tick_calls: SyncMutex::new(0),
                stop_riding_calls: SyncMutex::new(0),
            })
        }

        fn set_vehicle(&self, vehicle: &SharedEntity) {
            *self.vehicle.lock() = Some(Arc::downgrade(vehicle));
        }

        fn add_passenger(&self, passenger: &SharedEntity) {
            self.passengers.lock().push(Arc::downgrade(passenger));
        }

        fn tick_call_count(&self) -> i32 {
            *self.tick_calls.lock()
        }

        fn ride_tick_call_count(&self) -> i32 {
            *self.ride_tick_calls.lock()
        }

        fn stop_riding_call_count(&self) -> i32 {
            *self.stop_riding_calls.lock()
        }
    }

    impl Entity for TickTestEntity {
        fn base(&self) -> &EntityBase {
            &self.base
        }

        fn entity_type(&self) -> steel_registry::entity_type::EntityTypeRef {
            &vanilla_entities::ITEM
        }

        fn vehicle(&self) -> Option<SharedEntity> {
            let vehicle = self.vehicle.lock().as_ref().and_then(Weak::upgrade);
            if vehicle.is_none() {
                *self.vehicle.lock() = None;
            }
            vehicle
        }

        fn passengers(&self) -> Vec<SharedEntity> {
            let mut live_passengers = Vec::new();
            self.passengers.lock().retain(|passenger| {
                let Some(entity) = passenger.upgrade() else {
                    return false;
                };
                live_passengers.push(entity);
                true
            });
            live_passengers
        }

        fn has_passenger(&self, passenger: &dyn Entity) -> bool {
            self.passengers()
                .iter()
                .any(|entity| entity.id() == passenger.id())
        }

        fn stop_riding(&self) {
            *self.vehicle.lock() = None;
            *self.stop_riding_calls.lock() += 1;
        }

        fn tick(&self) {
            *self.tick_calls.lock() += 1;
        }

        fn ride_tick(&self) {
            *self.ride_tick_calls.lock() += 1;
        }
    }

    fn shared(entity: &Arc<TickTestEntity>) -> SharedEntity {
        entity.clone()
    }

    fn link_vehicle_and_passenger(
        vehicle: &Arc<TickTestEntity>,
        vehicle_entity: &SharedEntity,
        passenger: &Arc<TickTestEntity>,
        passenger_entity: &SharedEntity,
    ) {
        passenger.set_vehicle(vehicle_entity);
        vehicle.add_passenger(passenger_entity);
    }

    #[test]
    fn saveable_entities_keep_unloaded_to_chunk_removals() {
        let storage = EntityStorage::new();
        let unloaded = raw_item(1);
        let discarded = raw_item(2);

        unloaded.set_removed(RemovalReason::UnloadedToChunk);
        discarded.set_removed(RemovalReason::Discarded);
        storage.add(unloaded);
        storage.add(discarded);

        let saveable = storage.get_saveable_entities();

        assert_eq!(saveable.len(), 1);
        assert_eq!(saveable[0].id(), 1);
    }

    #[test]
    #[should_panic(expected = "already present in chunk entity storage")]
    fn add_rejects_duplicate_entity_ids() {
        let storage = EntityStorage::new();

        storage.add(raw_item(1));
        storage.add(raw_item(1));
    }

    #[test]
    fn tick_routes_valid_passengers_through_vehicle_ride_tick() {
        let storage = EntityStorage::new();
        let vehicle = TickTestEntity::new(1);
        let passenger = TickTestEntity::new(2);
        let vehicle_entity = shared(&vehicle);
        let passenger_entity = shared(&passenger);
        link_vehicle_and_passenger(&vehicle, &vehicle_entity, &passenger, &passenger_entity);

        storage.add(passenger_entity);
        storage.add(vehicle_entity);

        let mut synced = Vec::new();
        assert!(storage.tick_entities(7, &mut |entity| synced.push(entity.id())));
        synced.sort_unstable();

        assert_eq!(vehicle.tick_call_count(), 1);
        assert_eq!(vehicle.ride_tick_call_count(), 0);
        assert_eq!(vehicle.tick_count(), 1);
        assert_eq!(passenger.tick_call_count(), 0);
        assert_eq!(passenger.ride_tick_call_count(), 1);
        assert_eq!(passenger.tick_count(), 1);
        assert_eq!(synced, vec![1, 2]);
    }

    #[test]
    fn invalid_vehicle_link_stops_riding_and_ticks_as_root() {
        let storage = EntityStorage::new();
        let vehicle = TickTestEntity::new(1);
        let passenger = TickTestEntity::new(2);
        let vehicle_entity = shared(&vehicle);
        let passenger_entity = shared(&passenger);
        passenger.set_vehicle(&vehicle_entity);

        storage.add(passenger_entity);

        let mut synced = Vec::new();
        assert!(storage.tick_entities(7, &mut |entity| synced.push(entity.id())));

        assert_eq!(passenger.stop_riding_call_count(), 1);
        assert_eq!(passenger.tick_call_count(), 1);
        assert_eq!(passenger.ride_tick_call_count(), 0);
        assert_eq!(passenger.tick_count(), 1);
        assert_eq!(synced, vec![2]);
        assert!(passenger.vehicle().is_none());
    }

    #[test]
    #[should_panic(expected = "cyclic passenger relationship")]
    fn cyclic_passenger_relationship_panics() {
        let storage = EntityStorage::new();
        let first = TickTestEntity::new(1);
        let second = TickTestEntity::new(2);
        let first_entity = shared(&first);
        let second_entity = shared(&second);
        link_vehicle_and_passenger(&first, &first_entity, &second, &second_entity);
        link_vehicle_and_passenger(&second, &second_entity, &first, &first_entity);

        storage.add(first_entity);
        storage.add(second_entity);

        let mut synced = Vec::new();
        storage.tick_entities(7, &mut |entity| synced.push(entity.id()));
    }
}
