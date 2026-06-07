//! World-level entity ownership and lookup.
//!
//! Steel deliberately uses a simpler loaded/simulated split than vanilla's
//! entity section manager. The manager owns runtime entity lookup regardless
//! of chunk load state; chunks are still the persistence boundary, and only
//! full simulated chunks tick entities.

use std::{error::Error, fmt, slice, sync::Arc};

use glam::DVec3;
use rustc_hash::{FxHashMap, FxHashSet};
use steel_utils::locks::SyncRwLock;
use steel_utils::{ChunkPos, SectionPos, WorldAabb};
use uuid::Uuid;

use super::{
    Entity, NullEntityCallback, RemovalReason, SharedEntity, tick_vehicle_passengers_with_ticked_if,
};

/// Error returned when adding an entity to the runtime world fails.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AddEntityError {
    /// The entity is in a chunk that is not active in the world entity manager.
    ChunkNotLoaded {
        /// Entity network ID.
        entity_id: i32,
        /// Chunk containing the entity.
        chunk: ChunkPos,
    },
    /// Another live entity with the same persistent UUID is already registered.
    DuplicateUuid {
        /// Entity network ID.
        entity_id: i32,
        /// Duplicate persistent UUID.
        uuid: Uuid,
    },
    /// The entity is already removed and cannot be added to the live world.
    RemovedEntity {
        /// Entity network ID.
        entity_id: i32,
    },
}

impl fmt::Display for AddEntityError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ChunkNotLoaded { entity_id, chunk } => {
                write!(f, "entity {entity_id} is in non-loaded chunk {chunk:?}")
            }
            Self::DuplicateUuid { entity_id, uuid } => {
                write!(f, "entity {entity_id} has duplicate UUID {uuid}")
            }
            Self::RemovedEntity { entity_id } => {
                write!(f, "entity {entity_id} is already removed")
            }
        }
    }
}

impl Error for AddEntityError {}

/// Error returned when a live entity move cannot be committed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EntityMoveError {
    /// The entity is no longer managed as live world state.
    NotLive {
        /// Entity network ID.
        entity_id: i32,
    },
    /// The entity is deliberately frozen outside live world membership.
    Inactive {
        /// Entity network ID.
        entity_id: i32,
    },
    /// The entity tried to move into a chunk outside active world ownership.
    UnloadedDestination {
        /// Entity network ID.
        entity_id: i32,
        /// Destination chunk.
        chunk: ChunkPos,
    },
}

impl fmt::Display for EntityMoveError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotLive { entity_id } => {
                write!(f, "entity {entity_id} is not live in the world")
            }
            Self::Inactive { entity_id } => {
                write!(f, "entity {entity_id} is inactive outside live world state")
            }
            Self::UnloadedDestination { entity_id, chunk } => {
                write!(
                    f,
                    "entity {entity_id} cannot move into non-loaded chunk {chunk:?}"
                )
            }
        }
    }
}

impl Error for EntityMoveError {}

/// Whether the manager owns persistence for an entity.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntityOwnership {
    /// Normal non-player entity owned by the world entity manager.
    ManagerOwned,
    /// Entity whose lifetime is owned elsewhere, such as a player.
    External,
}

/// Section/chunk membership update caused by a committed entity move.
#[derive(Debug, Clone)]
pub struct EntityMoveUpdate {
    /// Entity network ID.
    pub entity_id: i32,
    /// Previous section membership.
    pub old_section: SectionPos,
    /// New section membership.
    pub new_section: SectionPos,
    /// Previous chunk membership.
    pub old_chunk: ChunkPos,
    /// New chunk membership.
    pub new_chunk: ChunkPos,
    /// Whether the entity was visible to normal world/tracker queries before the move.
    pub old_accessible: bool,
    /// Whether the entity is visible to normal world/tracker queries after the move.
    pub new_accessible: bool,
}

impl EntityMoveUpdate {
    /// Returns whether the entity changed sections.
    #[must_use]
    pub fn section_changed(&self) -> bool {
        self.old_section != self.new_section
    }

    /// Returns whether the entity changed chunks.
    #[must_use]
    pub fn chunk_changed(&self) -> bool {
        self.old_chunk != self.new_chunk
    }

    /// Returns whether the entity crossed an accessibility boundary.
    #[must_use]
    pub const fn accessibility_changed(&self) -> bool {
        self.old_accessible != self.new_accessible
    }

    /// Returns whether this move made a previously hidden entity accessible.
    #[must_use]
    pub const fn became_accessible(&self) -> bool {
        !self.old_accessible && self.new_accessible
    }

    /// Returns whether this move made a previously accessible entity hidden.
    #[must_use]
    pub const fn became_inaccessible(&self) -> bool {
        self.old_accessible && !self.new_accessible
    }
}

/// Saveable entity that could not be persisted by a chunk save pass.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnsavedEntityReport {
    /// Entity network ID.
    pub entity_id: i32,
    /// Entity persistent UUID.
    pub uuid: Uuid,
    /// Chunk containing the entity.
    pub chunk: ChunkPos,
}

/// Entity changes produced when a chunk becomes loaded.
#[derive(Default)]
pub struct ChunkEntityLoadResult {
    /// Retained entities restored to live world membership.
    pub restored: Vec<SharedEntity>,
    /// Live entities in this chunk whose tracking became visible again.
    pub tracking_started: Vec<SharedEntity>,
    /// Whether recovery created save-pending entity state for this chunk.
    pub needs_save: bool,
}

/// Entity changes produced when a chunk starts unloading.
#[derive(Default)]
pub struct ChunkEntityUnloadStart {
    /// Entities removed from live ownership and retained for chunk recovery.
    pub retained: Vec<SharedEntity>,
    /// Entities whose tracker visibility should stop for this chunk transition.
    pub tracking_stopped: Vec<SharedEntity>,
}

#[derive(Clone)]
struct EntityEntry {
    entity: SharedEntity,
    uuid: Uuid,
    section: SectionPos,
    chunk: ChunkPos,
    ownership: EntityOwnership,
}

impl EntityEntry {
    fn new(entity: SharedEntity, ownership: EntityOwnership) -> Self {
        let section = SectionPos::from_entity_pos(entity.position());
        let chunk = ChunkPos::new(section.x(), section.z());
        Self {
            uuid: entity.uuid(),
            entity,
            section,
            chunk,
            ownership,
        }
    }

    #[must_use]
    fn should_save(&self) -> bool {
        self.ownership == EntityOwnership::ManagerOwned
            && (!self.entity.is_removed()
                || self
                    .entity
                    .removal_reason()
                    .is_some_and(RemovalReason::should_save))
            && !self.entity.is_passenger()
            && !self.entity.has_exactly_one_player_passenger()
            && self.entity.entity_type().can_serialize
    }
}

#[derive(Default)]
struct ManagerState {
    loaded_chunks: FxHashSet<ChunkPos>,
    live_by_id: FxHashMap<i32, EntityEntry>,
    live_by_uuid: FxHashMap<Uuid, i32>,
    by_section: FxHashMap<SectionPos, FxHashSet<i32>>,
    by_chunk: FxHashMap<ChunkPos, FxHashSet<i32>>,
    unloading_by_chunk: FxHashMap<ChunkPos, Vec<EntityEntry>>,
    save_pending_by_chunk: FxHashMap<ChunkPos, Vec<EntityEntry>>,
}

/// Central world entity manager.
pub struct WorldEntityManager {
    state: SyncRwLock<ManagerState>,
}

impl fmt::Debug for WorldEntityManager {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let state = self.state.read();
        f.debug_struct("WorldEntityManager")
            .field("loaded_chunks", &state.loaded_chunks.len())
            .field("live_entities", &state.live_by_id.len())
            .field("unloading_chunks", &state.unloading_by_chunk.len())
            .finish()
    }
}

impl WorldEntityManager {
    /// Creates an empty manager.
    #[must_use]
    pub fn new() -> Self {
        Self {
            state: SyncRwLock::new(ManagerState::default()),
        }
    }

    /// Marks a chunk as loaded and reactivates retained unloading entities.
    pub fn on_chunk_loaded(&self, pos: ChunkPos) -> ChunkEntityLoadResult {
        let mut state = self.state.write();
        let was_hidden = state.loaded_chunks.insert(pos);

        let mut result = ChunkEntityLoadResult::default();
        if was_hidden {
            result.tracking_started = Self::live_manager_owned_entities_in_chunk(&state, pos);
        }

        if let Some(entries) = state.unloading_by_chunk.remove(&pos) {
            result.restored.reserve(entries.len());
            for entry in entries {
                if entry.entity.is_removed() {
                    if entry.should_save() {
                        result.needs_save = true;
                        state
                            .save_pending_by_chunk
                            .entry(pos)
                            .or_default()
                            .push(entry);
                    }
                    continue;
                }

                let entity = entry.entity.clone();
                Self::insert_live_entry(&mut state, entry);
                result.restored.push(entity);
            }
        }

        result
    }

    fn live_manager_owned_entities_in_chunk(
        state: &ManagerState,
        chunk: ChunkPos,
    ) -> Vec<SharedEntity> {
        state
            .by_chunk
            .get(&chunk)
            .map(|entity_ids| {
                entity_ids
                    .iter()
                    .filter_map(|id| state.live_by_id.get(id))
                    .filter(|entry| entry.ownership == EntityOwnership::ManagerOwned)
                    .map(|entry| entry.entity.clone())
                    .collect()
            })
            .unwrap_or_default()
    }

    fn push_unique_entity(
        entity: &SharedEntity,
        seen: &mut FxHashSet<i32>,
        entities: &mut Vec<SharedEntity>,
    ) {
        if seen.insert(entity.id()) {
            entities.push(entity.clone());
        }
    }

    /// Moves manager-owned root entities in `pos` out of live world membership while
    /// retaining them for possible chunk recovery.
    pub fn begin_chunk_unload(&self, pos: ChunkPos) -> ChunkEntityUnloadStart {
        let mut state = self.state.write();
        state.loaded_chunks.remove(&pos);

        let ids = state
            .by_chunk
            .get(&pos)
            .map(|set| set.iter().copied().collect::<Vec<_>>())
            .unwrap_or_default();

        let mut result = ChunkEntityUnloadStart::default();
        let mut tracking_stopped_ids = FxHashSet::default();
        let mut root_ids = Vec::new();
        for entity_id in ids {
            let Some(entry) = state.live_by_id.get(&entity_id) else {
                continue;
            };
            if entry.ownership != EntityOwnership::ManagerOwned {
                continue;
            }

            Self::push_unique_entity(
                &entry.entity,
                &mut tracking_stopped_ids,
                &mut result.tracking_stopped,
            );
            if !entry.entity.is_passenger() {
                root_ids.push(entity_id);
            }
        }

        let mut retained = Vec::new();
        let mut visited = FxHashSet::default();
        for entity_id in root_ids {
            Self::retain_unloading_entity_tree(
                &mut state,
                entity_id,
                &mut visited,
                &mut retained,
                &mut result.retained,
                &mut tracking_stopped_ids,
                &mut result.tracking_stopped,
            );
        }

        if !retained.is_empty() {
            state
                .unloading_by_chunk
                .entry(pos)
                .or_default()
                .extend(retained);
        }

        result
    }

    fn retain_unloading_entity_tree(
        state: &mut ManagerState,
        entity_id: i32,
        visited: &mut FxHashSet<i32>,
        retained: &mut Vec<EntityEntry>,
        retained_entities: &mut Vec<SharedEntity>,
        tracking_stopped_ids: &mut FxHashSet<i32>,
        tracking_stopped: &mut Vec<SharedEntity>,
    ) {
        if !visited.insert(entity_id) {
            return;
        }

        let Some(entry) = Self::remove_live_entry(state, entity_id) else {
            return;
        };

        if entry.ownership != EntityOwnership::ManagerOwned {
            Self::insert_live_entry(state, entry);
            return;
        }

        let passengers = entry.entity.passengers();
        Self::push_unique_entity(&entry.entity, tracking_stopped_ids, tracking_stopped);
        retained_entities.push(Arc::clone(&entry.entity));
        retained.push(entry);
        for passenger in passengers {
            Self::retain_unloading_entity_tree(
                state,
                passenger.id(),
                visited,
                retained,
                retained_entities,
                tracking_stopped_ids,
                tracking_stopped,
            );
        }
    }

    /// Finalizes an unloading chunk. Retained entities are detached and dropped.
    pub fn finalize_chunk_unload(&self, pos: ChunkPos) {
        let entries = self
            .state
            .write()
            .unloading_by_chunk
            .remove(&pos)
            .unwrap_or_default();

        for entry in entries {
            entry
                .entity
                .set_level_callback(Arc::new(NullEntityCallback));
            entry.entity.set_removed(RemovalReason::UnloadedToChunk);
        }
    }

    /// Registers a live runtime entity.
    ///
    /// # Panics
    ///
    /// Panics if an entity with the same session network ID is already present. Duplicate runtime
    /// IDs indicate corrupted manager ownership and cannot be recovered without losing identity.
    pub fn add_live_entity(
        &self,
        entity: SharedEntity,
        ownership: EntityOwnership,
    ) -> Result<(), AddEntityError> {
        let entry = Self::checked_live_entry(entity, ownership)?;
        let mut state = self.state.write();
        Self::validate_live_entries(&state, slice::from_ref(&entry), ownership)?;
        Self::insert_live_entry(&mut state, entry);
        Ok(())
    }

    /// Adds a related group of live entities atomically.
    ///
    /// Use this for persisted vehicle/passenger trees so registration either
    /// publishes the whole tree or leaves world indexes unchanged.
    ///
    /// # Panics
    ///
    /// Panics if the entity tree contains the same session network ID more
    /// than once. Duplicate runtime IDs indicate corrupted ownership.
    pub fn add_live_entity_tree(
        &self,
        entities: &[SharedEntity],
        ownership: EntityOwnership,
    ) -> Result<(), AddEntityError> {
        let mut entries = Vec::with_capacity(entities.len());
        for entity in entities {
            entries.push(Self::checked_live_entry(Arc::clone(entity), ownership)?);
        }

        let mut seen_ids = FxHashSet::default();
        let mut seen_uuids = FxHashSet::default();
        for entry in &entries {
            let entity_id = entry.entity.id();
            assert!(
                seen_ids.insert(entity_id),
                "entity id {entity_id} appears more than once in a live entity tree"
            );
            if !seen_uuids.insert(entry.uuid) {
                return Err(AddEntityError::DuplicateUuid {
                    entity_id,
                    uuid: entry.uuid,
                });
            }
        }

        let mut state = self.state.write();
        Self::validate_live_entries(&state, &entries, ownership)?;
        for entry in entries {
            Self::insert_live_entry(&mut state, entry);
        }
        Ok(())
    }

    fn checked_live_entry(
        entity: SharedEntity,
        ownership: EntityOwnership,
    ) -> Result<EntityEntry, AddEntityError> {
        if entity.is_removed() {
            return Err(AddEntityError::RemovedEntity {
                entity_id: entity.id(),
            });
        }

        Ok(EntityEntry::new(entity, ownership))
    }

    fn validate_live_entries(
        state: &ManagerState,
        entries: &[EntityEntry],
        ownership: EntityOwnership,
    ) -> Result<(), AddEntityError> {
        for entry in entries {
            let entity_id = entry.entity.id();
            assert!(
                !Self::contains_id(state, entity_id),
                "entity id {entity_id} is already registered in the world entity manager"
            );
            if Self::contains_uuid(state, entry.uuid) {
                return Err(AddEntityError::DuplicateUuid {
                    entity_id,
                    uuid: entry.uuid,
                });
            }
            if ownership == EntityOwnership::ManagerOwned
                && !state.loaded_chunks.contains(&entry.chunk)
            {
                return Err(AddEntityError::ChunkNotLoaded {
                    entity_id,
                    chunk: entry.chunk,
                });
            }
        }
        Ok(())
    }

    /// Removes a live entity for an explicit entity removal reason.
    pub fn remove_live_entity(
        &self,
        entity_id: i32,
        reason: RemovalReason,
    ) -> Option<SharedEntity> {
        let mut state = self.state.write();
        let entry = Self::remove_live_entry(&mut state, entity_id)?;
        let entity = entry.entity.clone();

        if reason.should_save() && entry.should_save() {
            state
                .save_pending_by_chunk
                .entry(entry.chunk)
                .or_default()
                .push(entry);
        }

        Some(entity)
    }

    /// Acknowledges that selected save-pending entities for `chunk` were persisted.
    pub fn on_chunk_saved(&self, chunk: ChunkPos, saved_entity_ids: &[i32]) {
        if saved_entity_ids.is_empty() {
            return;
        }

        let saved_entity_ids = saved_entity_ids.iter().copied().collect::<FxHashSet<_>>();
        let mut state = self.state.write();
        let Some(entries) = state.save_pending_by_chunk.get_mut(&chunk) else {
            return;
        };

        entries.retain(|entry| !saved_entity_ids.contains(&entry.entity.id()));
        if entries.is_empty() {
            state.save_pending_by_chunk.remove(&chunk);
        }
    }

    /// Returns whether `chunk` has removed runtime entities waiting for a save acknowledgement.
    #[must_use]
    pub fn has_save_pending_for_chunk(&self, chunk: ChunkPos) -> bool {
        self.state
            .read()
            .save_pending_by_chunk
            .get(&chunk)
            .is_some_and(|entries| !entries.is_empty())
    }

    /// Validates that a live entity can move to `new_pos`.
    pub fn validate_move(&self, entity_id: i32, new_pos: DVec3) -> Result<(), EntityMoveError> {
        let state = self.state.read();
        let Some(entry) = state.live_by_id.get(&entity_id) else {
            return Err(EntityMoveError::NotLive { entity_id });
        };

        if entry.ownership == EntityOwnership::ManagerOwned {
            let new_section = SectionPos::from_entity_pos(new_pos);
            let new_chunk = ChunkPos::new(new_section.x(), new_section.z());
            if !Self::can_move_manager_owned_to_chunk(&state, entry, new_chunk) {
                return Err(EntityMoveError::UnloadedDestination {
                    entity_id,
                    chunk: new_chunk,
                });
            }
        }

        Ok(())
    }

    /// Commits manager indexes after a live entity position change.
    pub fn commit_move(
        &self,
        entity_id: i32,
        new_pos: DVec3,
    ) -> Result<EntityMoveUpdate, EntityMoveError> {
        let mut state = self.state.write();
        let Some(current) = state.live_by_id.get(&entity_id) else {
            return Err(EntityMoveError::NotLive { entity_id });
        };

        let new_section = SectionPos::from_entity_pos(new_pos);
        let new_chunk = ChunkPos::new(new_section.x(), new_section.z());
        if current.ownership == EntityOwnership::ManagerOwned
            && !Self::can_move_manager_owned_to_chunk(&state, current, new_chunk)
        {
            return Err(EntityMoveError::UnloadedDestination {
                entity_id,
                chunk: new_chunk,
            });
        }

        let old_section = current.section;
        let old_chunk = current.chunk;
        let old_accessible = Self::is_accessible(&state, current);
        let new_accessible = Self::is_accessible_at(&state, current.ownership, new_chunk);
        if old_section == new_section && old_chunk == new_chunk {
            return Ok(EntityMoveUpdate {
                entity_id,
                old_section,
                new_section,
                old_chunk,
                new_chunk,
                old_accessible,
                new_accessible,
            });
        }

        Self::remove_from_section(&mut state, old_section, entity_id);
        Self::remove_from_chunk(&mut state, old_chunk, entity_id);

        if let Some(entry) = state.live_by_id.get_mut(&entity_id) {
            entry.section = new_section;
            entry.chunk = new_chunk;
        }

        state
            .by_section
            .entry(new_section)
            .or_default()
            .insert(entity_id);
        state
            .by_chunk
            .entry(new_chunk)
            .or_default()
            .insert(entity_id);

        Ok(EntityMoveUpdate {
            entity_id,
            old_section,
            new_section,
            old_chunk,
            new_chunk,
            old_accessible,
            new_accessible,
        })
    }

    fn can_move_manager_owned_to_chunk(
        state: &ManagerState,
        entry: &EntityEntry,
        new_chunk: ChunkPos,
    ) -> bool {
        state.loaded_chunks.contains(&new_chunk)
            || (entry.entity.is_passenger()
                && Self::has_live_loaded_root_vehicle(state, &entry.entity))
    }

    fn has_live_loaded_root_vehicle(state: &ManagerState, entity: &SharedEntity) -> bool {
        let mut visited = FxHashSet::default();
        visited.insert(entity.id());

        let mut passenger = Arc::clone(entity);
        let Some(mut vehicle) = passenger.vehicle() else {
            return false;
        };

        loop {
            assert!(
                visited.insert(vehicle.id()),
                "cyclic passenger relationship involving entity {}",
                entity.id()
            );
            if vehicle.is_removed() || !vehicle.has_passenger(passenger.as_ref()) {
                return false;
            }

            let Some(vehicle_entry) = state.live_by_id.get(&vehicle.id()) else {
                return false;
            };

            let Some(next_vehicle) = vehicle.vehicle() else {
                return match vehicle_entry.ownership {
                    EntityOwnership::External => true,
                    EntityOwnership::ManagerOwned => {
                        state.loaded_chunks.contains(&vehicle_entry.chunk)
                    }
                };
            };

            passenger = vehicle;
            vehicle = next_vehicle;
        }
    }

    #[must_use]
    /// Gets a live entity by session network ID.
    pub fn get_by_id(&self, entity_id: i32) -> Option<SharedEntity> {
        self.state
            .read()
            .live_by_id
            .get(&entity_id)
            .map(|entry| entry.entity.clone())
    }

    #[must_use]
    /// Gets a live entity by persistent UUID.
    pub fn get_by_uuid(&self, uuid: &Uuid) -> Option<SharedEntity> {
        let state = self.state.read();
        let entity_id = state.live_by_uuid.get(uuid)?;
        state
            .live_by_id
            .get(entity_id)
            .map(|entry| entry.entity.clone())
    }

    #[must_use]
    /// Gets live entities whose bounding boxes intersect `aabb`.
    pub fn get_entities_in_aabb(&self, aabb: &WorldAabb) -> Vec<SharedEntity> {
        let min_section = SectionPos::from_entity_pos(DVec3::new(
            aabb.min_x() - 2.0,
            aabb.min_y() - 2.0,
            aabb.min_z() - 2.0,
        ));
        let max_section = SectionPos::from_entity_pos(DVec3::new(
            aabb.max_x() + 2.0,
            aabb.max_y() + 2.0,
            aabb.max_z() + 2.0,
        ));

        let state = self.state.read();
        let mut result = Vec::new();
        for sy in min_section.y()..=max_section.y() {
            for sz in min_section.z()..=max_section.z() {
                for sx in min_section.x()..=max_section.x() {
                    let section_pos = SectionPos::new(sx, sy, sz);
                    let Some(entity_ids) = state.by_section.get(&section_pos) else {
                        continue;
                    };

                    for entity_id in entity_ids {
                        let Some(entry) = state.live_by_id.get(entity_id) else {
                            continue;
                        };
                        if Self::is_accessible(&state, entry)
                            && entry.entity.bounding_box().intersects(*aabb)
                        {
                            result.push(entry.entity.clone());
                        }
                    }
                }
            }
        }

        result
    }

    /// Reports saveable entities whose chunks were not part of a chunk save pass.
    #[must_use]
    pub fn saveable_entities_outside_chunks(
        &self,
        saved_chunks: &[ChunkPos],
    ) -> Vec<UnsavedEntityReport> {
        let saved_chunks = saved_chunks.iter().copied().collect::<FxHashSet<_>>();
        let state = self.state.read();
        let mut seen = FxHashSet::default();
        let mut reports = Vec::new();

        for entry in state.live_by_id.values() {
            Self::push_unsaved_entity_report(&saved_chunks, &mut seen, &mut reports, entry);
        }

        for entries in state.unloading_by_chunk.values() {
            for entry in entries {
                Self::push_unsaved_entity_report(&saved_chunks, &mut seen, &mut reports, entry);
            }
        }

        for entries in state.save_pending_by_chunk.values() {
            for entry in entries {
                Self::push_unsaved_entity_report(&saved_chunks, &mut seen, &mut reports, entry);
            }
        }

        reports.sort_by_key(|report| (report.chunk.0.x, report.chunk.0.y, report.entity_id));
        reports
    }

    #[must_use]
    /// Gets entities that should be serialized for `chunk`.
    pub fn get_saveable_entities_for_chunk(&self, chunk: ChunkPos) -> Vec<SharedEntity> {
        let state = self.state.read();
        let mut result = Vec::new();
        let mut seen_ids = FxHashSet::default();
        let mut seen_uuids = FxHashSet::default();

        if let Some(entity_ids) = state.by_chunk.get(&chunk) {
            for entity_id in entity_ids {
                let Some(entry) = state.live_by_id.get(entity_id) else {
                    continue;
                };
                Self::push_saveable_entity(&mut result, &mut seen_ids, &mut seen_uuids, entry);
            }
        }

        if let Some(entries) = state.unloading_by_chunk.get(&chunk) {
            for entry in entries {
                Self::push_saveable_entity(&mut result, &mut seen_ids, &mut seen_uuids, entry);
            }
        }

        if let Some(entries) = state.save_pending_by_chunk.get(&chunk) {
            for entry in entries {
                Self::push_saveable_entity(&mut result, &mut seen_ids, &mut seen_uuids, entry);
            }
        }

        result
    }

    #[must_use]
    /// Gets live entities currently indexed in `chunk`.
    pub fn live_entities_in_chunk(&self, chunk: ChunkPos) -> Vec<SharedEntity> {
        let state = self.state.read();
        state
            .by_chunk
            .get(&chunk)
            .map(|entity_ids| {
                entity_ids
                    .iter()
                    .filter_map(|id| state.live_by_id.get(id))
                    .map(|entry| entry.entity.clone())
                    .collect()
            })
            .unwrap_or_default()
    }

    #[must_use]
    /// Returns the number of live indexed entities.
    pub fn count(&self) -> usize {
        self.state.read().live_by_id.len()
    }

    /// Ticks live entities in the supplied full simulated chunks.
    pub fn tick_entities(
        &self,
        _tick_count: i32,
        tickable_chunks: &[ChunkPos],
    ) -> FxHashSet<ChunkPos> {
        let mut dirty_chunks = FxHashSet::default();
        let mut ticked_entities = FxHashSet::default();
        let tickable_chunk_set = tickable_chunks.iter().copied().collect::<FxHashSet<_>>();
        for chunk in tickable_chunks {
            let entities = self.manager_owned_entities_in_chunk(*chunk);
            for entity in entities {
                if !self.is_live_manager_owned_in_chunk(entity.id(), *chunk) {
                    continue;
                }

                if entity.is_removed() {
                    continue;
                }

                if Self::is_valid_passenger_or_stop_riding(&entity) {
                    continue;
                }

                if !ticked_entities.insert(entity.id()) {
                    continue;
                }

                self.tick_non_passenger(
                    &entity,
                    &mut ticked_entities,
                    &tickable_chunk_set,
                    &mut dirty_chunks,
                );
            }
        }
        dirty_chunks
    }

    /// Ticks eligible passengers for an externally ticked root, such as a player.
    pub(crate) fn tick_vehicle_passengers_for_root(
        &self,
        vehicle: &dyn Entity,
        tickable_chunks: &FxHashSet<ChunkPos>,
    ) -> FxHashSet<ChunkPos> {
        let mut dirty_chunks = FxHashSet::default();
        let mut ticked_entities = FxHashSet::default();
        ticked_entities.insert(vehicle.id());
        self.tick_vehicle_passengers_with_ticked(
            vehicle,
            &mut ticked_entities,
            tickable_chunks,
            &mut dirty_chunks,
        );
        dirty_chunks
    }

    fn manager_owned_entities_in_chunk(&self, chunk: ChunkPos) -> Vec<SharedEntity> {
        let state = self.state.read();
        state
            .by_chunk
            .get(&chunk)
            .map(|entity_ids| {
                entity_ids
                    .iter()
                    .filter_map(|id| state.live_by_id.get(id))
                    .filter(|entry| entry.ownership == EntityOwnership::ManagerOwned)
                    .map(|entry| entry.entity.clone())
                    .collect()
            })
            .unwrap_or_default()
    }

    fn is_live_manager_owned_in_chunk(&self, entity_id: i32, chunk: ChunkPos) -> bool {
        let state = self.state.read();
        state.live_by_id.get(&entity_id).is_some_and(|entry| {
            entry.ownership == EntityOwnership::ManagerOwned && entry.chunk == chunk
        })
    }

    fn is_accessible(state: &ManagerState, entry: &EntityEntry) -> bool {
        Self::is_accessible_at(state, entry.ownership, entry.chunk)
    }

    fn is_accessible_at(state: &ManagerState, ownership: EntityOwnership, chunk: ChunkPos) -> bool {
        ownership == EntityOwnership::External || state.loaded_chunks.contains(&chunk)
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
            assert!(
                visited.insert(current.id()),
                "cyclic passenger relationship involving entity {}",
                entity.id()
            );
            vehicle = current.vehicle();
        }
    }

    fn tick_non_passenger(
        &self,
        entity: &SharedEntity,
        ticked_entities: &mut FxHashSet<i32>,
        tickable_chunks: &FxHashSet<ChunkPos>,
        dirty_chunks: &mut FxHashSet<ChunkPos>,
    ) {
        entity.advance_tick_count();
        entity.tick();
        dirty_chunks.insert(ChunkPos::from_entity_pos(entity.position()));
        self.tick_vehicle_passengers_with_ticked(
            entity.as_ref(),
            ticked_entities,
            tickable_chunks,
            dirty_chunks,
        );
    }

    fn tick_vehicle_passengers_with_ticked(
        &self,
        vehicle: &dyn Entity,
        ticked_entities: &mut FxHashSet<i32>,
        tickable_chunks: &FxHashSet<ChunkPos>,
        dirty_chunks: &mut FxHashSet<ChunkPos>,
    ) {
        let mut post_tick = |entity: &SharedEntity| {
            dirty_chunks.insert(ChunkPos::from_entity_pos(entity.position()));
        };
        tick_vehicle_passengers_with_ticked_if(
            vehicle,
            ticked_entities,
            &mut post_tick,
            &mut |entity| self.can_tick_entity_now(entity.id(), tickable_chunks),
        );
    }

    fn can_tick_entity_now(&self, entity_id: i32, tickable_chunks: &FxHashSet<ChunkPos>) -> bool {
        let state = self.state.read();
        let Some(entry) = state.live_by_id.get(&entity_id) else {
            return false;
        };

        match entry.ownership {
            EntityOwnership::External => true,
            EntityOwnership::ManagerOwned => {
                Self::is_accessible(&state, entry) && tickable_chunks.contains(&entry.chunk)
            }
        }
    }

    fn insert_live_entry(state: &mut ManagerState, entry: EntityEntry) {
        let entity_id = entry.entity.id();
        assert!(
            !state.live_by_id.contains_key(&entity_id),
            "entity id {entity_id} is already registered in the world entity manager"
        );
        assert!(
            state.live_by_uuid.insert(entry.uuid, entity_id).is_none(),
            "entity uuid {} is already registered in the world entity manager",
            entry.uuid
        );
        state
            .by_section
            .entry(entry.section)
            .or_default()
            .insert(entity_id);
        state
            .by_chunk
            .entry(entry.chunk)
            .or_default()
            .insert(entity_id);
        state.live_by_id.insert(entity_id, entry);
    }

    fn contains_uuid(state: &ManagerState, uuid: Uuid) -> bool {
        state.live_by_uuid.contains_key(&uuid)
            || state
                .unloading_by_chunk
                .values()
                .flatten()
                .any(|entry| entry.uuid == uuid)
            || state
                .save_pending_by_chunk
                .values()
                .flatten()
                .any(|entry| entry.uuid == uuid)
    }

    fn contains_id(state: &ManagerState, entity_id: i32) -> bool {
        state.live_by_id.contains_key(&entity_id)
            || state
                .unloading_by_chunk
                .values()
                .flatten()
                .any(|entry| entry.entity.id() == entity_id)
            || state
                .save_pending_by_chunk
                .values()
                .flatten()
                .any(|entry| entry.entity.id() == entity_id)
    }

    fn push_saveable_entity(
        result: &mut Vec<SharedEntity>,
        seen_ids: &mut FxHashSet<i32>,
        seen_uuids: &mut FxHashSet<Uuid>,
        entry: &EntityEntry,
    ) {
        if !entry.should_save() || !seen_ids.insert(entry.entity.id()) {
            return;
        }
        assert!(
            seen_uuids.insert(entry.uuid),
            "duplicate saveable entity uuid {} in world entity manager",
            entry.uuid
        );
        result.push(entry.entity.clone());
    }

    fn push_unsaved_entity_report(
        saved_chunks: &FxHashSet<ChunkPos>,
        seen: &mut FxHashSet<i32>,
        reports: &mut Vec<UnsavedEntityReport>,
        entry: &EntityEntry,
    ) {
        if saved_chunks.contains(&entry.chunk)
            || !entry.should_save()
            || !seen.insert(entry.entity.id())
        {
            return;
        }

        reports.push(UnsavedEntityReport {
            entity_id: entry.entity.id(),
            uuid: entry.uuid,
            chunk: entry.chunk,
        });
    }

    fn remove_live_entry(state: &mut ManagerState, entity_id: i32) -> Option<EntityEntry> {
        let entry = state.live_by_id.remove(&entity_id)?;
        state.live_by_uuid.remove(&entry.uuid);
        Self::remove_from_section(state, entry.section, entity_id);
        Self::remove_from_chunk(state, entry.chunk, entity_id);
        Some(entry)
    }

    fn remove_from_section(state: &mut ManagerState, section: SectionPos, entity_id: i32) {
        let remove_section = if let Some(entity_ids) = state.by_section.get_mut(&section) {
            entity_ids.remove(&entity_id);
            entity_ids.is_empty()
        } else {
            false
        };
        if remove_section {
            state.by_section.remove(&section);
        }
    }

    fn remove_from_chunk(state: &mut ManagerState, chunk: ChunkPos, entity_id: i32) {
        let remove_chunk = if let Some(entity_ids) = state.by_chunk.get_mut(&chunk) {
            entity_ids.remove(&entity_id);
            entity_ids.is_empty()
        } else {
            false
        };
        if remove_chunk {
            state.by_chunk.remove(&chunk);
        }
    }
}

impl Default for WorldEntityManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Weak};

    use steel_registry::entity_type::EntityTypeRef;
    use steel_registry::vanilla_entities;
    use uuid::Uuid;

    use crate::entity::{Entity, EntityBase};

    use super::*;

    struct ManagerTestEntity {
        base: EntityBase,
    }

    impl ManagerTestEntity {
        fn shared(id: i32, uuid: Uuid, position: DVec3) -> SharedEntity {
            Arc::new(Self {
                base: EntityBase::with_uuid(
                    id,
                    uuid,
                    position,
                    vanilla_entities::ITEM.dimensions,
                    Weak::new(),
                ),
            })
        }
    }

    impl Entity for ManagerTestEntity {
        fn base(&self) -> &EntityBase {
            &self.base
        }

        fn entity_type(&self) -> EntityTypeRef {
            &vanilla_entities::ITEM
        }
    }

    fn entity(id: i32, uuid_seed: u128, position: DVec3) -> SharedEntity {
        ManagerTestEntity::shared(id, Uuid::from_u128(uuid_seed), position)
    }

    fn load_chunk(manager: &WorldEntityManager, chunk: ChunkPos) {
        let result = manager.on_chunk_loaded(chunk);
        assert!(result.restored.is_empty());
        assert!(!result.needs_save);
    }

    #[test]
    fn add_live_entity_rejects_manager_owned_unloaded_chunk() {
        let manager = WorldEntityManager::new();
        let entity = entity(1, 1, DVec3::new(1.0, 64.0, 1.0));

        assert!(matches!(
            manager.add_live_entity(entity.clone(), EntityOwnership::ManagerOwned),
            Err(AddEntityError::ChunkNotLoaded {
                entity_id: 1,
                chunk,
            }) if chunk == ChunkPos::new(0, 0)
        ));
        assert_eq!(manager.count(), 0);
        assert!(manager.get_by_id(entity.id()).is_none());
    }

    #[test]
    fn add_live_entity_accepts_external_unloaded_chunk() {
        let manager = WorldEntityManager::new();
        let entity = entity(1, 1, DVec3::new(1.0, 64.0, 1.0));

        assert!(
            manager
                .add_live_entity(entity.clone(), EntityOwnership::External)
                .is_ok()
        );
        assert_eq!(manager.count(), 1);

        let Some(live_entity) = manager.get_by_id(entity.id()) else {
            panic!("entity in unloaded chunk should be live");
        };
        assert!(Arc::ptr_eq(&entity, &live_entity));
    }

    #[test]
    fn add_live_entity_rejects_duplicate_uuid_without_registering_second_entity() {
        let manager = WorldEntityManager::new();
        load_chunk(&manager, ChunkPos::new(0, 0));

        let uuid = Uuid::from_u128(5);
        let first = ManagerTestEntity::shared(1, uuid, DVec3::new(1.0, 64.0, 1.0));
        let second = ManagerTestEntity::shared(2, uuid, DVec3::new(2.0, 64.0, 1.0));

        assert!(
            manager
                .add_live_entity(first.clone(), EntityOwnership::ManagerOwned)
                .is_ok()
        );
        assert!(matches!(
            manager.add_live_entity(second, EntityOwnership::ManagerOwned),
            Err(AddEntityError::DuplicateUuid {
                entity_id: 2,
                uuid: duplicate,
            }) if duplicate == uuid
        ));

        let Some(live_first) = manager.get_by_id(1) else {
            panic!("first entity should stay registered");
        };
        assert!(Arc::ptr_eq(&first, &live_first));
        assert!(manager.get_by_id(2).is_none());
        assert_eq!(manager.count(), 1);
    }

    #[test]
    fn add_live_entity_tree_rejects_duplicate_uuid_without_partial_registration() {
        let manager = WorldEntityManager::new();
        load_chunk(&manager, ChunkPos::new(0, 0));

        let existing_uuid = Uuid::from_u128(5);
        let existing = ManagerTestEntity::shared(1, existing_uuid, DVec3::new(1.0, 64.0, 1.0));
        let result = manager.add_live_entity(Arc::clone(&existing), EntityOwnership::ManagerOwned);
        assert!(
            result.is_ok(),
            "existing entity should register before duplicate UUID test: {result:?}"
        );

        let vehicle = entity(2, 6, DVec3::new(2.0, 64.0, 2.0));
        let passenger = ManagerTestEntity::shared(3, existing_uuid, DVec3::new(2.0, 64.0, 2.0));
        EntityBase::restore_passenger_relationship(&vehicle, &passenger);

        assert!(matches!(
            manager.add_live_entity_tree(
                &[Arc::clone(&vehicle), Arc::clone(&passenger)],
                EntityOwnership::ManagerOwned,
            ),
            Err(AddEntityError::DuplicateUuid {
                entity_id: 3,
                uuid,
            }) if uuid == existing_uuid
        ));
        assert!(manager.get_by_id(2).is_none());
        assert!(manager.get_by_id(3).is_none());
        assert_eq!(manager.count(), 1);
    }

    #[test]
    #[should_panic(expected = "entity id 1 is already registered in the world entity manager")]
    fn duplicate_entity_id_is_a_loud_invariant_failure() {
        let manager = WorldEntityManager::new();
        load_chunk(&manager, ChunkPos::new(0, 0));

        assert!(
            manager
                .add_live_entity(
                    entity(1, 1, DVec3::new(1.0, 64.0, 1.0)),
                    EntityOwnership::ManagerOwned,
                )
                .is_ok()
        );
        let _ = manager.add_live_entity(
            entity(1, 2, DVec3::new(2.0, 64.0, 1.0)),
            EntityOwnership::ManagerOwned,
        );
    }

    #[test]
    fn committed_move_updates_chunk_index_for_loaded_destination() {
        let manager = WorldEntityManager::new();
        load_chunk(&manager, ChunkPos::new(0, 0));
        load_chunk(&manager, ChunkPos::new(1, 0));

        let entity = entity(1, 1, DVec3::new(1.0, 64.0, 1.0));
        assert!(
            manager
                .add_live_entity(entity.clone(), EntityOwnership::ManagerOwned)
                .is_ok()
        );

        let new_position = DVec3::new(17.0, 64.0, 1.0);
        assert!(manager.validate_move(entity.id(), new_position).is_ok());
        entity.base().set_position_local(new_position);
        let update = match manager.commit_move(entity.id(), new_position) {
            Ok(update) => update,
            Err(error) => panic!("move into unloaded chunk should commit: {error}"),
        };

        assert!(update.chunk_changed());
        assert!(
            manager
                .live_entities_in_chunk(ChunkPos::new(0, 0))
                .is_empty()
        );
        let new_chunk_entities = manager.live_entities_in_chunk(ChunkPos::new(1, 0));
        assert_eq!(new_chunk_entities.len(), 1);
        assert!(Arc::ptr_eq(&entity, &new_chunk_entities[0]));
    }

    #[test]
    fn validate_move_rejects_manager_owned_unloaded_destination() {
        let manager = WorldEntityManager::new();
        load_chunk(&manager, ChunkPos::new(0, 0));

        let entity = entity(1, 1, DVec3::new(1.0, 64.0, 1.0));
        assert!(
            manager
                .add_live_entity(entity.clone(), EntityOwnership::ManagerOwned)
                .is_ok()
        );

        let new_position = DVec3::new(17.0, 64.0, 1.0);

        assert!(matches!(
            manager.validate_move(entity.id(), new_position),
            Err(EntityMoveError::UnloadedDestination {
                entity_id: 1,
                chunk,
            }) if chunk == ChunkPos::new(1, 0)
        ));
        assert_eq!(manager.live_entities_in_chunk(ChunkPos::new(0, 0)).len(), 1);
        assert!(
            manager
                .live_entities_in_chunk(ChunkPos::new(1, 0))
                .is_empty()
        );
    }

    #[test]
    fn commit_move_rejects_destination_unloaded_after_validation() {
        let manager = WorldEntityManager::new();
        load_chunk(&manager, ChunkPos::new(0, 0));
        load_chunk(&manager, ChunkPos::new(1, 0));

        let entity = entity(1, 1, DVec3::new(1.0, 64.0, 1.0));
        assert!(
            manager
                .add_live_entity(entity.clone(), EntityOwnership::ManagerOwned)
                .is_ok()
        );

        let new_position = DVec3::new(17.0, 64.0, 1.0);
        assert!(manager.validate_move(entity.id(), new_position).is_ok());
        let unload = manager.begin_chunk_unload(ChunkPos::new(1, 0));
        assert!(unload.retained.is_empty());
        assert!(unload.tracking_stopped.is_empty());
        entity.base().set_position_local(new_position);

        assert!(matches!(
            manager.commit_move(entity.id(), new_position),
            Err(EntityMoveError::UnloadedDestination {
                entity_id: 1,
                chunk,
            }) if chunk == ChunkPos::new(1, 0)
        ));
        assert_eq!(manager.live_entities_in_chunk(ChunkPos::new(0, 0)).len(), 1);
        assert!(
            manager
                .live_entities_in_chunk(ChunkPos::new(1, 0))
                .is_empty()
        );
    }

    #[test]
    fn chunk_recovery_restores_same_entity_arc_before_final_unload() {
        let manager = WorldEntityManager::new();
        let chunk = ChunkPos::new(0, 0);
        load_chunk(&manager, chunk);

        let entity = entity(1, 1, DVec3::new(1.0, 64.0, 1.0));
        assert!(
            manager
                .add_live_entity(entity.clone(), EntityOwnership::ManagerOwned)
                .is_ok()
        );

        let unload = manager.begin_chunk_unload(chunk);
        assert_eq!(unload.retained.len(), 1);
        assert!(Arc::ptr_eq(&entity, &unload.retained[0]));
        assert_eq!(unload.tracking_stopped.len(), 1);
        assert!(Arc::ptr_eq(&entity, &unload.tracking_stopped[0]));
        assert!(manager.get_by_id(entity.id()).is_none());

        let result = manager.on_chunk_loaded(chunk);
        assert_eq!(result.restored.len(), 1);
        assert!(Arc::ptr_eq(&entity, &result.restored[0]));
        assert!(!result.needs_save);

        let Some(live_entity) = manager.get_by_id(entity.id()) else {
            panic!("recovered entity should be live again");
        };
        assert!(Arc::ptr_eq(&entity, &live_entity));
        assert!(!entity.is_removed());
    }

    #[test]
    fn chunk_unload_retains_manager_owned_passenger_tree() {
        let manager = WorldEntityManager::new();
        let vehicle_chunk = ChunkPos::new(0, 0);
        let passenger_chunk = ChunkPos::new(1, 0);
        load_chunk(&manager, vehicle_chunk);
        load_chunk(&manager, passenger_chunk);

        let vehicle = entity(1, 1, DVec3::new(1.0, 64.0, 1.0));
        let passenger = entity(2, 2, DVec3::new(17.0, 64.0, 1.0));
        EntityBase::restore_passenger_relationship(&vehicle, &passenger);

        assert!(
            manager
                .add_live_entity(vehicle.clone(), EntityOwnership::ManagerOwned)
                .is_ok()
        );
        assert!(
            manager
                .add_live_entity(passenger.clone(), EntityOwnership::ManagerOwned)
                .is_ok()
        );

        let unload = manager.begin_chunk_unload(vehicle_chunk);
        let mut retained_ids = unload
            .retained
            .iter()
            .map(|entity| entity.id())
            .collect::<Vec<_>>();
        retained_ids.sort_unstable();
        assert_eq!(retained_ids, vec![1, 2]);
        let mut tracking_stopped_ids = unload
            .tracking_stopped
            .iter()
            .map(|entity| entity.id())
            .collect::<Vec<_>>();
        tracking_stopped_ids.sort_unstable();
        assert_eq!(tracking_stopped_ids, vec![1, 2]);
        assert!(manager.get_by_id(vehicle.id()).is_none());
        assert!(manager.get_by_id(passenger.id()).is_none());
        assert!(manager.live_entities_in_chunk(passenger_chunk).is_empty());

        let saveable = manager.get_saveable_entities_for_chunk(vehicle_chunk);
        let mut saveable_ids = saveable
            .iter()
            .map(|entity| entity.id())
            .collect::<Vec<_>>();
        saveable_ids.sort_unstable();
        assert_eq!(saveable_ids, vec![1]);

        manager.finalize_chunk_unload(vehicle_chunk);
        assert!(vehicle.is_removed());
        assert!(passenger.is_removed());
    }

    #[test]
    fn passenger_chunk_unload_hides_passenger_without_unloading_vehicle_tree() {
        let manager = WorldEntityManager::new();
        let vehicle_chunk = ChunkPos::new(0, 0);
        let passenger_chunk = ChunkPos::new(1, 0);
        load_chunk(&manager, vehicle_chunk);
        load_chunk(&manager, passenger_chunk);

        let vehicle = entity(1, 1, DVec3::new(1.0, 64.0, 1.0));
        let passenger = entity(2, 2, DVec3::new(17.0, 64.0, 1.0));
        EntityBase::restore_passenger_relationship(&vehicle, &passenger);

        assert!(
            manager
                .add_live_entity(vehicle.clone(), EntityOwnership::ManagerOwned)
                .is_ok()
        );
        assert!(
            manager
                .add_live_entity(passenger.clone(), EntityOwnership::ManagerOwned)
                .is_ok()
        );

        let passenger_aabb = WorldAabb::new(16.5, 63.0, 0.5, 17.5, 65.0, 1.5);
        assert_eq!(manager.get_entities_in_aabb(&passenger_aabb).len(), 1);

        let unload = manager.begin_chunk_unload(passenger_chunk);

        assert!(unload.retained.is_empty());
        assert_eq!(unload.tracking_stopped.len(), 1);
        assert!(Arc::ptr_eq(&passenger, &unload.tracking_stopped[0]));
        assert!(manager.get_by_id(vehicle.id()).is_some());
        assert!(manager.get_by_id(passenger.id()).is_some());
        assert_eq!(manager.live_entities_in_chunk(passenger_chunk).len(), 1);
        assert!(manager.get_entities_in_aabb(&passenger_aabb).is_empty());
        assert!(
            manager
                .get_saveable_entities_for_chunk(passenger_chunk)
                .is_empty()
        );

        let saveable = manager.get_saveable_entities_for_chunk(vehicle_chunk);
        assert_eq!(saveable.len(), 1);
        assert!(Arc::ptr_eq(&vehicle, &saveable[0]));

        let result = manager.on_chunk_loaded(passenger_chunk);
        assert!(result.restored.is_empty());
        assert_eq!(result.tracking_started.len(), 1);
        assert!(Arc::ptr_eq(&passenger, &result.tracking_started[0]));
        assert_eq!(manager.get_entities_in_aabb(&passenger_aabb).len(), 1);
    }

    #[test]
    fn attached_passenger_can_move_while_its_own_chunk_is_hidden() {
        let manager = WorldEntityManager::new();
        let vehicle_chunk = ChunkPos::new(0, 0);
        let passenger_chunk = ChunkPos::new(1, 0);
        load_chunk(&manager, vehicle_chunk);
        load_chunk(&manager, passenger_chunk);

        let vehicle = entity(1, 1, DVec3::new(1.0, 64.0, 1.0));
        let passenger = entity(2, 2, DVec3::new(17.0, 64.0, 1.0));
        EntityBase::restore_passenger_relationship(&vehicle, &passenger);

        assert!(
            manager
                .add_live_entity(vehicle.clone(), EntityOwnership::ManagerOwned)
                .is_ok()
        );
        assert!(
            manager
                .add_live_entity(passenger.clone(), EntityOwnership::ManagerOwned)
                .is_ok()
        );

        let unload = manager.begin_chunk_unload(passenger_chunk);
        assert!(unload.retained.is_empty());

        let new_position = DVec3::new(18.0, 64.0, 1.0);
        assert!(manager.validate_move(passenger.id(), new_position).is_ok());
        passenger.base().set_position_local(new_position);
        let update = match manager.commit_move(passenger.id(), new_position) {
            Ok(update) => update,
            Err(error) => panic!("attached passenger move should commit: {error}"),
        };
        assert_eq!(update.new_chunk, passenger_chunk);
        assert!(!update.old_accessible);
        assert!(!update.new_accessible);
        assert!(!update.accessibility_changed());
        assert_eq!(manager.live_entities_in_chunk(passenger_chunk).len(), 1);
    }

    #[test]
    fn passenger_move_from_hidden_to_loaded_chunk_becomes_accessible() {
        let manager = WorldEntityManager::new();
        let vehicle_chunk = ChunkPos::new(0, 0);
        let passenger_chunk = ChunkPos::new(1, 0);
        load_chunk(&manager, vehicle_chunk);
        load_chunk(&manager, passenger_chunk);

        let vehicle = entity(1, 1, DVec3::new(1.0, 64.0, 1.0));
        let passenger = entity(2, 2, DVec3::new(17.0, 64.0, 1.0));
        EntityBase::restore_passenger_relationship(&vehicle, &passenger);

        assert!(
            manager
                .add_live_entity(vehicle.clone(), EntityOwnership::ManagerOwned)
                .is_ok()
        );
        assert!(
            manager
                .add_live_entity(passenger.clone(), EntityOwnership::ManagerOwned)
                .is_ok()
        );

        let unload = manager.begin_chunk_unload(passenger_chunk);
        assert!(unload.retained.is_empty());

        let new_position = DVec3::new(2.0, 64.0, 1.0);
        assert!(manager.validate_move(passenger.id(), new_position).is_ok());
        passenger.base().set_position_local(new_position);
        let update = match manager.commit_move(passenger.id(), new_position) {
            Ok(update) => update,
            Err(error) => panic!("attached passenger move should commit: {error}"),
        };

        assert_eq!(update.old_chunk, passenger_chunk);
        assert_eq!(update.new_chunk, vehicle_chunk);
        assert!(!update.old_accessible);
        assert!(update.new_accessible);
        assert!(update.became_accessible());
    }

    #[test]
    fn passenger_move_from_loaded_to_hidden_chunk_becomes_inaccessible() {
        let manager = WorldEntityManager::new();
        let vehicle_chunk = ChunkPos::new(0, 0);
        let passenger_chunk = ChunkPos::new(1, 0);
        load_chunk(&manager, vehicle_chunk);
        load_chunk(&manager, passenger_chunk);

        let vehicle = entity(1, 1, DVec3::new(1.0, 64.0, 1.0));
        let passenger = entity(2, 2, DVec3::new(2.0, 64.0, 1.0));
        EntityBase::restore_passenger_relationship(&vehicle, &passenger);

        assert!(
            manager
                .add_live_entity(vehicle.clone(), EntityOwnership::ManagerOwned)
                .is_ok()
        );
        assert!(
            manager
                .add_live_entity(passenger.clone(), EntityOwnership::ManagerOwned)
                .is_ok()
        );

        let unload = manager.begin_chunk_unload(passenger_chunk);
        assert!(unload.retained.is_empty());
        assert!(unload.tracking_stopped.is_empty());

        let new_position = DVec3::new(17.0, 64.0, 1.0);
        assert!(manager.validate_move(passenger.id(), new_position).is_ok());
        passenger.base().set_position_local(new_position);
        let update = match manager.commit_move(passenger.id(), new_position) {
            Ok(update) => update,
            Err(error) => panic!("attached passenger move should commit: {error}"),
        };

        assert_eq!(update.old_chunk, vehicle_chunk);
        assert_eq!(update.new_chunk, passenger_chunk);
        assert!(update.old_accessible);
        assert!(!update.new_accessible);
        assert!(update.became_inaccessible());
    }

    #[test]
    fn hidden_chunk_passenger_is_not_ticked_by_loaded_vehicle() {
        let manager = WorldEntityManager::new();
        let vehicle_chunk = ChunkPos::new(0, 0);
        let passenger_chunk = ChunkPos::new(1, 0);
        load_chunk(&manager, vehicle_chunk);
        load_chunk(&manager, passenger_chunk);

        let vehicle = entity(1, 1, DVec3::new(1.0, 64.0, 1.0));
        let passenger = entity(2, 2, DVec3::new(17.0, 64.0, 1.0));
        EntityBase::restore_passenger_relationship(&vehicle, &passenger);

        assert!(
            manager
                .add_live_entity(vehicle.clone(), EntityOwnership::ManagerOwned)
                .is_ok()
        );
        assert!(
            manager
                .add_live_entity(passenger.clone(), EntityOwnership::ManagerOwned)
                .is_ok()
        );

        let unload = manager.begin_chunk_unload(passenger_chunk);
        assert!(unload.retained.is_empty());

        manager.tick_entities(0, &[vehicle_chunk]);
        assert_eq!(vehicle.tick_count(), 1);
        assert_eq!(passenger.tick_count(), 0);

        let result = manager.on_chunk_loaded(passenger_chunk);
        assert_eq!(result.tracking_started.len(), 1);
        manager.tick_entities(1, &[vehicle_chunk, passenger_chunk]);
        assert_eq!(vehicle.tick_count(), 2);
        assert_eq!(passenger.tick_count(), 1);
    }

    #[test]
    fn final_chunk_unload_marks_stale_arc_removed_and_allows_same_identity_to_reload() {
        let manager = WorldEntityManager::new();
        let chunk = ChunkPos::new(0, 0);
        let uuid = Uuid::from_u128(9);
        load_chunk(&manager, chunk);

        let stale = ManagerTestEntity::shared(1, uuid, DVec3::new(1.0, 64.0, 1.0));
        assert!(
            manager
                .add_live_entity(stale.clone(), EntityOwnership::ManagerOwned)
                .is_ok()
        );

        let unload = manager.begin_chunk_unload(chunk);
        assert_eq!(unload.retained.len(), 1);
        manager.finalize_chunk_unload(chunk);

        assert!(stale.is_removed());
        assert_eq!(stale.removal_reason(), Some(RemovalReason::UnloadedToChunk));
        assert!(manager.get_by_id(stale.id()).is_none());

        load_chunk(&manager, chunk);
        let reloaded = ManagerTestEntity::shared(1, uuid, DVec3::new(1.0, 64.0, 1.0));
        assert!(
            manager
                .add_live_entity(reloaded.clone(), EntityOwnership::ManagerOwned)
                .is_ok()
        );

        let Some(live_entity) = manager.get_by_id(reloaded.id()) else {
            panic!("reloaded entity should be live");
        };
        assert!(Arc::ptr_eq(&reloaded, &live_entity));
        assert!(!reloaded.is_removed());
    }

    #[test]
    fn saveable_entities_include_manager_owned_live_unloading_and_pending_entities() {
        let manager = WorldEntityManager::new();
        let chunk = ChunkPos::new(0, 0);
        load_chunk(&manager, chunk);

        let live = entity(1, 1, DVec3::new(1.0, 64.0, 1.0));
        let external = entity(2, 2, DVec3::new(2.0, 64.0, 1.0));
        assert!(
            manager
                .add_live_entity(live.clone(), EntityOwnership::ManagerOwned)
                .is_ok()
        );
        assert!(
            manager
                .add_live_entity(external, EntityOwnership::External)
                .is_ok()
        );

        let live_saveable = manager.get_saveable_entities_for_chunk(chunk);
        assert_eq!(live_saveable.len(), 1);
        assert!(Arc::ptr_eq(&live, &live_saveable[0]));

        let unload = manager.begin_chunk_unload(chunk);
        assert_eq!(unload.retained.len(), 1);
        let unloading_saveable = manager.get_saveable_entities_for_chunk(chunk);
        assert_eq!(unloading_saveable.len(), 1);
        assert!(Arc::ptr_eq(&live, &unloading_saveable[0]));

        manager.finalize_chunk_unload(chunk);
        load_chunk(&manager, chunk);

        let pending = entity(3, 3, DVec3::new(3.0, 64.0, 1.0));
        assert!(
            manager
                .add_live_entity(pending.clone(), EntityOwnership::ManagerOwned)
                .is_ok()
        );
        let removed = manager.remove_live_entity(pending.id(), RemovalReason::UnloadedToChunk);
        assert!(removed.is_some());

        let pending_saveable = manager.get_saveable_entities_for_chunk(chunk);
        assert_eq!(pending_saveable.len(), 1);
        assert!(Arc::ptr_eq(&pending, &pending_saveable[0]));
    }

    #[test]
    fn save_pending_acknowledgement_clears_only_persisted_entities() {
        let manager = WorldEntityManager::new();
        let chunk = ChunkPos::new(0, 0);
        load_chunk(&manager, chunk);

        let saved = entity(1, 1, DVec3::new(1.0, 64.0, 1.0));
        let later = entity(2, 2, DVec3::new(2.0, 64.0, 1.0));
        assert!(
            manager
                .add_live_entity(saved.clone(), EntityOwnership::ManagerOwned)
                .is_ok()
        );
        assert!(
            manager
                .add_live_entity(later.clone(), EntityOwnership::ManagerOwned)
                .is_ok()
        );
        assert!(
            manager
                .remove_live_entity(saved.id(), RemovalReason::UnloadedToChunk)
                .is_some()
        );
        assert!(
            manager
                .remove_live_entity(later.id(), RemovalReason::UnloadedToChunk)
                .is_some()
        );
        assert_eq!(manager.get_saveable_entities_for_chunk(chunk).len(), 2);

        manager.on_chunk_saved(chunk, &[saved.id()]);

        let saveable = manager.get_saveable_entities_for_chunk(chunk);
        assert_eq!(saveable.len(), 1);
        assert!(Arc::ptr_eq(&later, &saveable[0]));

        manager.on_chunk_saved(chunk, &[later.id()]);

        assert!(manager.get_saveable_entities_for_chunk(chunk).is_empty());
        assert!(!manager.has_save_pending_for_chunk(chunk));
    }

    #[test]
    fn add_live_entity_rejects_duplicate_uuid_in_save_pending_entities() {
        let manager = WorldEntityManager::new();
        let chunk = ChunkPos::new(0, 0);
        load_chunk(&manager, chunk);

        let uuid = Uuid::from_u128(44);
        let pending = ManagerTestEntity::shared(1, uuid, DVec3::new(1.0, 64.0, 1.0));
        assert!(
            manager
                .add_live_entity(pending, EntityOwnership::ManagerOwned)
                .is_ok()
        );
        assert!(
            manager
                .remove_live_entity(1, RemovalReason::UnloadedToChunk)
                .is_some()
        );

        let duplicate = ManagerTestEntity::shared(2, uuid, DVec3::new(2.0, 64.0, 1.0));

        assert!(matches!(
            manager.add_live_entity(duplicate, EntityOwnership::ManagerOwned),
            Err(AddEntityError::DuplicateUuid {
                entity_id: 2,
                uuid: duplicate_uuid,
            }) if duplicate_uuid == uuid
        ));
    }

    #[test]
    #[should_panic(expected = "entity id 1 is already registered in the world entity manager")]
    fn add_live_entity_panics_on_duplicate_id_in_save_pending_entities() {
        let manager = WorldEntityManager::new();
        let chunk = ChunkPos::new(0, 0);
        load_chunk(&manager, chunk);

        let pending = entity(1, 46, DVec3::new(1.0, 64.0, 1.0));
        assert!(
            manager
                .add_live_entity(pending, EntityOwnership::ManagerOwned)
                .is_ok()
        );
        assert!(
            manager
                .remove_live_entity(1, RemovalReason::UnloadedToChunk)
                .is_some()
        );

        let duplicate = entity(1, 47, DVec3::new(2.0, 64.0, 1.0));
        let _ = manager.add_live_entity(duplicate, EntityOwnership::ManagerOwned);
    }

    #[test]
    fn add_live_entity_rejects_duplicate_uuid_in_unloading_entities() {
        let manager = WorldEntityManager::new();
        let chunk = ChunkPos::new(0, 0);
        load_chunk(&manager, chunk);

        let uuid = Uuid::from_u128(45);
        let unloading = ManagerTestEntity::shared(1, uuid, DVec3::new(1.0, 64.0, 1.0));
        assert!(
            manager
                .add_live_entity(unloading, EntityOwnership::ManagerOwned)
                .is_ok()
        );
        assert_eq!(manager.begin_chunk_unload(chunk).retained.len(), 1);

        let duplicate = ManagerTestEntity::shared(2, uuid, DVec3::new(2.0, 64.0, 1.0));

        assert!(matches!(
            manager.add_live_entity(duplicate, EntityOwnership::ManagerOwned),
            Err(AddEntityError::DuplicateUuid {
                entity_id: 2,
                uuid: duplicate_uuid,
            }) if duplicate_uuid == uuid
        ));
    }

    #[test]
    #[should_panic(expected = "entity id 1 is already registered in the world entity manager")]
    fn add_live_entity_panics_on_duplicate_id_in_unloading_entities() {
        let manager = WorldEntityManager::new();
        let chunk = ChunkPos::new(0, 0);
        load_chunk(&manager, chunk);

        let unloading = entity(1, 48, DVec3::new(1.0, 64.0, 1.0));
        assert!(
            manager
                .add_live_entity(unloading, EntityOwnership::ManagerOwned)
                .is_ok()
        );
        assert_eq!(manager.begin_chunk_unload(chunk).retained.len(), 1);

        let duplicate = entity(1, 49, DVec3::new(2.0, 64.0, 1.0));
        let _ = manager.add_live_entity(duplicate, EntityOwnership::ManagerOwned);
    }

    #[test]
    fn chunk_recovery_does_not_restore_removed_retained_entities() {
        let manager = WorldEntityManager::new();
        let chunk = ChunkPos::new(0, 0);
        load_chunk(&manager, chunk);

        let removed = entity(1, 1, DVec3::new(1.0, 64.0, 1.0));
        assert!(
            manager
                .add_live_entity(removed.clone(), EntityOwnership::ManagerOwned)
                .is_ok()
        );

        let unload = manager.begin_chunk_unload(chunk);
        assert_eq!(unload.retained.len(), 1);
        removed.set_removed(RemovalReason::Discarded);

        let result = manager.on_chunk_loaded(chunk);

        assert!(result.restored.is_empty());
        assert!(!result.needs_save);
        assert!(manager.get_by_id(removed.id()).is_none());
        assert!(manager.get_saveable_entities_for_chunk(chunk).is_empty());
    }

    #[test]
    fn chunk_recovery_keeps_saveable_removed_retained_entities_pending() {
        let manager = WorldEntityManager::new();
        let chunk = ChunkPos::new(0, 0);
        load_chunk(&manager, chunk);

        let pending = entity(1, 1, DVec3::new(1.0, 64.0, 1.0));
        assert!(
            manager
                .add_live_entity(pending.clone(), EntityOwnership::ManagerOwned)
                .is_ok()
        );

        let unload = manager.begin_chunk_unload(chunk);
        assert_eq!(unload.retained.len(), 1);
        pending.set_removed(RemovalReason::UnloadedToChunk);

        let result = manager.on_chunk_loaded(chunk);

        assert!(result.restored.is_empty());
        assert!(result.needs_save);
        assert!(manager.get_by_id(pending.id()).is_none());
        assert!(manager.has_save_pending_for_chunk(chunk));
        let saveable = manager.get_saveable_entities_for_chunk(chunk);
        assert_eq!(saveable.len(), 1);
        assert!(Arc::ptr_eq(&pending, &saveable[0]));
    }

    #[test]
    fn saveable_entities_outside_saved_chunks_reports_only_manager_owned_entities() {
        let manager = WorldEntityManager::new();
        let saved_chunk = ChunkPos::new(0, 0);
        let unsaved_chunk = ChunkPos::new(1, 0);
        load_chunk(&manager, saved_chunk);
        load_chunk(&manager, unsaved_chunk);

        let saved = entity(1, 1, DVec3::new(1.0, 64.0, 1.0));
        let unsaved = entity(2, 2, DVec3::new(17.0, 64.0, 1.0));
        let external = entity(3, 3, DVec3::new(18.0, 64.0, 1.0));
        assert!(
            manager
                .add_live_entity(saved, EntityOwnership::ManagerOwned)
                .is_ok()
        );
        assert!(
            manager
                .add_live_entity(unsaved.clone(), EntityOwnership::ManagerOwned)
                .is_ok()
        );
        assert!(
            manager
                .add_live_entity(external, EntityOwnership::External)
                .is_ok()
        );

        let reports = manager.saveable_entities_outside_chunks(&[saved_chunk]);
        assert_eq!(
            reports,
            vec![UnsavedEntityReport {
                entity_id: unsaved.id(),
                uuid: unsaved.uuid(),
                chunk: unsaved_chunk,
            }]
        );
    }

    #[test]
    fn tick_entities_skips_external_entities() {
        let manager = WorldEntityManager::new();
        let chunk = ChunkPos::new(0, 0);
        load_chunk(&manager, chunk);

        let manager_owned = entity(1, 1, DVec3::new(1.0, 64.0, 1.0));
        let external = entity(2, 2, DVec3::new(2.0, 64.0, 1.0));
        assert!(
            manager
                .add_live_entity(manager_owned.clone(), EntityOwnership::ManagerOwned)
                .is_ok()
        );
        assert!(
            manager
                .add_live_entity(external.clone(), EntityOwnership::External)
                .is_ok()
        );

        let dirty_chunks = manager.tick_entities(12, &[chunk]);

        assert!(dirty_chunks.contains(&chunk));
        assert_eq!(manager_owned.tick_count(), 1);
        assert_eq!(external.tick_count(), 0);
    }
}
