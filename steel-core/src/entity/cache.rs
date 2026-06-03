//! World-level entity cache using Weak references.
//!
//! Provides O(1) lookup by entity ID and UUID, plus spatial queries by section.
//! The cache uses `Weak` references - when a chunk unloads and drops its `Arc`,
//! the weak references become invalid and queries return `None`.

use std::sync::Arc;

use rustc_hash::FxHashSet;
use steel_utils::{SectionPos, WorldAabb};
use uuid::Uuid;

use super::{SharedEntity, WeakEntity};

/// World-level entity cache for fast lookups.
///
/// Stores `Weak` references to entities owned by chunks.
/// When a chunk unloads, its entities' weak refs become invalid.
#[expect(
    clippy::struct_field_names,
    reason = "`by_` prefix is intentional for lookup clarity"
)]
pub struct EntityCache {
    /// Fast lookup by entity ID (network identifier).
    by_id: scc::HashMap<i32, WeakEntity>,
    /// Fast lookup by UUID (persistent identifier).
    by_uuid: scc::HashMap<Uuid, WeakEntity>,
    /// Spatial index by section position - stores entity IDs.
    by_section: scc::HashMap<SectionPos, FxHashSet<i32>>,
}

impl EntityCache {
    /// Creates a new empty entity cache.
    #[must_use]
    pub fn new() -> Self {
        Self {
            by_id: scc::HashMap::new(),
            by_uuid: scc::HashMap::new(),
            by_section: scc::HashMap::new(),
        }
    }

    /// Registers an entity in the cache.
    ///
    /// Called when an entity is added to a chunk.
    ///
    /// # Panics
    ///
    /// Panics if the entity ID or UUID is already registered. The world cache is
    /// a session-unique index; silently replacing entries leaves stale section
    /// membership and breaks packet/entity lookup invariants.
    pub fn register(&self, entity: &SharedEntity) {
        let id = entity.id();
        let uuid = entity.uuid();
        let weak = Arc::downgrade(entity);
        let pos = entity.position();
        let section = SectionPos::from_entity_pos(pos);

        if self.by_id.insert_sync(id, weak.clone()).is_err() {
            self.cleanup();
            if self.by_id.insert_sync(id, weak.clone()).is_err() {
                panic!("entity id {id} is already registered in the world cache");
            }
        }

        if self.by_uuid.insert_sync(uuid, weak.clone()).is_err() {
            self.cleanup();
            if self.by_uuid.insert_sync(uuid, weak).is_err() {
                let _ = self.by_id.remove_sync(&id);
                panic!("entity uuid {uuid} is already registered in the world cache");
            }
        }

        self.add_to_section(section, id);
    }

    /// Unregisters an entity from the cache.
    ///
    /// Called when an entity is removed from the world.
    pub fn unregister(&self, entity_id: i32, uuid: Uuid, section: SectionPos) {
        // Remove from ID lookup
        let _ = self.by_id.remove_sync(&entity_id);

        // Remove from UUID lookup
        let _ = self.by_uuid.remove_sync(&uuid);

        // Remove from section index
        self.remove_from_section(section, entity_id);
    }

    /// Updates the section index when an entity moves between sections.
    pub fn on_section_change(
        &self,
        entity_id: i32,
        old_section: SectionPos,
        new_section: SectionPos,
    ) {
        if old_section == new_section {
            return;
        }

        // Remove from old section
        self.remove_from_section(old_section, entity_id);

        // Add to new section
        self.add_to_section(new_section, entity_id);
    }

    /// Gets an entity by its network ID.
    ///
    /// Returns `None` if the entity doesn't exist or its chunk was unloaded.
    #[must_use]
    pub fn get_by_id(&self, id: i32) -> Option<SharedEntity> {
        self.by_id
            .read_sync(&id, |_, weak| weak.upgrade())
            .flatten()
    }

    /// Gets an entity by its UUID.
    ///
    /// Returns `None` if the entity doesn't exist or its chunk was unloaded.
    #[must_use]
    pub fn get_by_uuid(&self, uuid: &Uuid) -> Option<SharedEntity> {
        self.by_uuid
            .read_sync(uuid, |_, weak| weak.upgrade())
            .flatten()
    }

    /// Gets all entities intersecting the given bounding box.
    ///
    /// Only returns entities in loaded chunks (where weak refs are valid).
    #[must_use]
    pub fn get_entities_in_aabb(&self, aabb: &WorldAabb) -> Vec<SharedEntity> {
        let mut result = Vec::new();

        let min_section = SectionPos::from_entity_pos(glam::DVec3::new(
            aabb.min_x() - 2.0,
            aabb.min_y() - 2.0,
            aabb.min_z() - 2.0,
        ));
        let max_section = SectionPos::from_entity_pos(glam::DVec3::new(
            aabb.max_x() + 2.0,
            aabb.max_y() + 2.0,
            aabb.max_z() + 2.0,
        ));

        for sy in min_section.y()..=max_section.y() {
            for sz in min_section.z()..=max_section.z() {
                for sx in min_section.x()..=max_section.x() {
                    let section_pos = SectionPos::new(sx, sy, sz);

                    let entity_ids: Option<Vec<i32>> = self
                        .by_section
                        .read_sync(&section_pos, |_, set| set.iter().copied().collect());

                    if let Some(ids) = entity_ids {
                        for entity_id in ids {
                            if let Some(entity) = self.get_by_id(entity_id)
                                && entity.bounding_box().intersects(*aabb)
                            {
                                result.push(entity);
                            }
                        }
                    }
                }
            }
        }

        result
    }

    /// Gets all entities in a specific section.
    #[must_use]
    pub fn get_entities_in_section(&self, section: SectionPos) -> Vec<SharedEntity> {
        let entity_ids: Option<Vec<i32>> = self
            .by_section
            .read_sync(&section, |_, set| set.iter().copied().collect());

        entity_ids
            .map(|ids| {
                ids.into_iter()
                    .filter_map(|id| self.get_by_id(id))
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Returns the number of registered entities (includes potentially stale weak refs).
    #[must_use]
    pub fn count(&self) -> usize {
        self.by_id.len()
    }

    /// Periodic cleanup of dead weak refs.
    ///
    /// Call occasionally to remove stale entries where chunks have unloaded.
    pub fn cleanup(&self) {
        // Clean by_id - remove entries where weak ref is dead
        self.by_id.retain_sync(|_, weak| weak.strong_count() > 0);

        // Clean by_uuid
        self.by_uuid.retain_sync(|_, weak| weak.strong_count() > 0);

        self.by_section.retain_sync(|_, set| {
            set.retain(|id| {
                self.by_id
                    .read_sync(id, |_, weak| weak.strong_count() > 0)
                    .unwrap_or(false)
            });
            !set.is_empty()
        });
    }

    fn add_to_section(&self, section: SectionPos, entity_id: i32) {
        // Try to update existing entry
        if self
            .by_section
            .update_sync(&section, |_, set| {
                set.insert(entity_id);
            })
            .is_none()
        {
            // Entry didn't exist, create new
            let mut set = FxHashSet::default();
            set.insert(entity_id);
            let _ = self.by_section.insert_sync(section, set);
        }
    }

    fn remove_from_section(&self, section: SectionPos, entity_id: i32) {
        let should_remove = self
            .by_section
            .update_sync(&section, |_, set| {
                set.remove(&entity_id);
                set.is_empty()
            })
            .unwrap_or(false);

        if should_remove {
            let _ = self
                .by_section
                .remove_if_sync(&section, |set| set.is_empty());
        }
    }
}

impl Default for EntityCache {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Weak};

    use glam::DVec3;
    use steel_registry::{
        entity_type::{EntityDimensions, EntityTypeRef},
        vanilla_entities,
    };
    use steel_utils::WorldAabb;
    use uuid::Uuid;

    use super::*;
    use crate::entity::{Entity, EntityBase};
    use crate::world::World;

    struct CacheTestEntity {
        base: EntityBase,
    }

    impl CacheTestEntity {
        fn new(id: i32, position: DVec3, dimensions: EntityDimensions) -> Arc<Self> {
            Self::with_uuid(id, Uuid::new_v4(), position, dimensions)
        }

        fn with_uuid(
            id: i32,
            uuid: Uuid,
            position: DVec3,
            dimensions: EntityDimensions,
        ) -> Arc<Self> {
            Arc::new(Self {
                base: EntityBase::with_uuid(id, uuid, position, dimensions, Weak::<World>::new()),
            })
        }
    }

    impl Entity for CacheTestEntity {
        fn base(&self) -> &EntityBase {
            &self.base
        }

        fn entity_type(&self) -> EntityTypeRef {
            &vanilla_entities::ITEM
        }
    }

    #[test]
    fn aabb_query_floors_section_coordinates_for_negative_bounds() {
        let cache = EntityCache::new();
        let entity = CacheTestEntity::new(
            1,
            DVec3::new(-0.25, 64.0, 0.0),
            EntityDimensions::new(4.0, 1.0, 0.5),
        );
        let entity: SharedEntity = entity;

        cache.register(&entity);

        let query = WorldAabb::new(1.5, 64.0, -0.1, 1.6, 64.5, 0.1);
        let entities = cache.get_entities_in_aabb(&query);

        assert_eq!(entities.len(), 1);
        assert_eq!(entities[0].id(), 1);
    }

    #[test]
    fn cleanup_removes_dead_section_ids() {
        let cache = EntityCache::new();
        let section = SectionPos::from_entity_pos(DVec3::ZERO);

        {
            let entity =
                CacheTestEntity::new(1, DVec3::ZERO, EntityDimensions::new(0.25, 0.25, 0.125));
            let entity: SharedEntity = entity;
            cache.register(&entity);
        }

        assert_eq!(cache.count(), 1);
        assert_eq!(cache.by_section.len(), 1);

        cache.cleanup();

        assert_eq!(cache.count(), 0);
        assert_eq!(cache.by_section.len(), 0);
        assert!(cache.get_entities_in_section(section).is_empty());
    }

    #[test]
    fn register_replaces_dead_duplicate_index_entries() {
        let cache = EntityCache::new();
        let dimensions = EntityDimensions::new(0.25, 0.25, 0.125);
        let uuid = Uuid::new_v4();

        {
            let entity: SharedEntity = CacheTestEntity::with_uuid(1, uuid, DVec3::ZERO, dimensions);
            cache.register(&entity);
        }

        let replacement: SharedEntity =
            CacheTestEntity::with_uuid(1, uuid, DVec3::new(1.0, 0.0, 0.0), dimensions);
        cache.register(&replacement);

        assert_eq!(cache.count(), 1);
        assert_eq!(cache.get_by_uuid(&uuid).map(|entity| entity.id()), Some(1));
        assert_eq!(cache.get_by_id(1).map(|entity| entity.uuid()), Some(uuid));
    }

    #[test]
    #[should_panic(expected = "already registered in the world cache")]
    fn register_rejects_duplicate_entity_ids() {
        let cache = EntityCache::new();
        let dimensions = EntityDimensions::new(0.25, 0.25, 0.125);
        let first: SharedEntity = CacheTestEntity::new(1, DVec3::ZERO, dimensions);
        let second: SharedEntity = CacheTestEntity::new(1, DVec3::new(1.0, 0.0, 0.0), dimensions);

        cache.register(&first);
        cache.register(&second);
    }
}
