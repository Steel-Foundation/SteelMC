//! Per-chunk entity storage.
//!
//! Entities are stored in chunks similar to block entities.
//! The chunk owns the `Arc<dyn Entity>` and is responsible for ticking.

use std::sync::Arc;

use rustc_hash::FxHashMap;
use steel_protocol::packets::game::CSetEntityData;
use steel_utils::ChunkPos;
use steel_utils::locks::SyncRwLock;

use super::SharedEntity;
use crate::world::World;

/// Storage for entities in a chunk.
///
/// This mirrors `BlockEntityStorage` - entities are keyed by their ID
/// and ticked from the chunk's tick method.
pub struct EntityStorage {
    /// Entities in this chunk, keyed by entity ID.
    entities: SyncRwLock<FxHashMap<i32, SharedEntity>>,
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
        self.entities.write().insert(id, entity);
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
    /// - Players (saved separately in playerdata)
    /// - Entity types with `can_serialize = false`
    #[must_use]
    pub fn get_saveable_entities(&self) -> Vec<SharedEntity> {
        self.entities
            .read()
            .values()
            .filter(|e| {
                !e.is_removed()
                    && (*e).clone().as_player().is_none()
                    && e.entity_type().can_serialize
            })
            .cloned()
            .collect()
    }

    /// Ticks all entities in this chunk and broadcasts dirty entity data.
    ///
    /// Called from `LevelChunk::tick()`.
    /// Returns `true` if any entities were ticked (chunk should be marked dirty).
    pub fn tick(&self, world: &Arc<World>, chunk_pos: ChunkPos, tick_count: i32) -> bool {
        // Clone to avoid holding lock during tick
        let entities: Vec<SharedEntity> = self.entities.read().values().cloned().collect();

        let mut ticked_any = false;
        for entity in entities {
            if entity.is_removed() {
                continue;
            }

            ticked_any = true;

            // Entity-specific tick (entities access world via self.level())
            entity.tick();

            // Send position/velocity changes (mirrors vanilla's ServerEntity.sendChanges())
            entity.send_changes(tick_count);

            // Broadcast dirty entity data (base tick behavior)
            if let Some(dirty_data) = entity.pack_dirty_entity_data() {
                let packet = CSetEntityData::new(entity.id(), dirty_data);
                world.broadcast_to_nearby(chunk_pos, packet, None);
            }
        }

        // Cleanup removed entities
        self.entities.write().retain(|_, e| !e.is_removed());

        ticked_any
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
