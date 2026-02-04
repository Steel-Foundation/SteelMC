//! World-level entity cache using Weak references.
//!
//! Provides O(1) lookup by entity ID and UUID, plus spatial queries by section.
//! The cache uses `Weak` references - when a chunk unloads and drops its `Arc`,
//! the weak references become invalid and queries return `None`.

use std::sync::Arc;

use rustc_hash::FxHashSet;
use steel_registry::blocks::shapes::AABBd;
use steel_utils::SectionPos;
use uuid::Uuid;

use super::{SharedEntity, WeakEntity};

/// World-level entity cache for fast lookups.
///
/// Stores `Weak` references to entities owned by chunks.
/// When a chunk unloads, its entities' weak refs become invalid.
#[allow(clippy::struct_field_names)] // `by_` prefix is intentional for clarity
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
    pub fn register(&self, entity: &SharedEntity) {
        let id = entity.id();
        let uuid = entity.uuid();
        let weak = Arc::downgrade(entity);
        let pos = entity.position();
        let section = SectionPos::new(
            (pos.x as i32) >> 4,
            (pos.y as i32) >> 4,
            (pos.z as i32) >> 4,
        );

        // Add to ID lookup
        let _ = self.by_id.insert_sync(id, weak.clone());

        // Add to UUID lookup
        let _ = self.by_uuid.insert_sync(uuid, weak);

        // Add to section index
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
    pub fn get_entities_in_aabb(&self, aabb: &AABBd) -> Vec<SharedEntity> {
        let mut result = Vec::new();

        // Determine section range (with 2 block grace like vanilla)
        let min_x = ((aabb.min_x - 2.0) as i32) >> 4;
        let min_y = ((aabb.min_y - 2.0) as i32) >> 4;
        let min_z = ((aabb.min_z - 2.0) as i32) >> 4;
        let max_x = ((aabb.max_x + 2.0) as i32) >> 4;
        let max_y = ((aabb.max_y + 2.0) as i32) >> 4;
        let max_z = ((aabb.max_z + 2.0) as i32) >> 4;

        for sy in min_y..=max_y {
            for sz in min_z..=max_z {
                for sx in min_x..=max_x {
                    let section_pos = SectionPos::new(sx, sy, sz);

                    let entity_ids: Option<Vec<i32>> = self
                        .by_section
                        .read_sync(&section_pos, |_, set| set.iter().copied().collect());

                    if let Some(ids) = entity_ids {
                        for entity_id in ids {
                            if let Some(entity) = self.get_by_id(entity_id)
                                && entity.bounding_box().intersects(aabb)
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

        // Clean sections - remove empty sections
        // Note: scc::HashMap doesn't have a scan method that allows collecting,
        // so we use retain_sync which handles this case
        self.by_section.retain_sync(|_, set| !set.is_empty());
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
