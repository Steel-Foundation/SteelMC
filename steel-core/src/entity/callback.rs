//! Entity lifecycle callbacks for movement and removal tracking.

use std::sync::Weak;

use glam::DVec3;
use steel_utils::locks::SyncMutex;
use steel_utils::{ChunkPos, SectionPos};

use super::SharedEntity;
use crate::world::World;

/// Reasons an entity can be removed from the world.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RemovalReason {
    /// Entity was killed/destroyed.
    Killed,
    /// Entity was discarded (e.g., too far from players).
    Discarded,
    /// Entity unloaded with chunk.
    UnloadedToChunk,
    /// Entity moved to another loaded world.
    ChangedWorld,
}

impl RemovalReason {
    /// Returns true if entity data should be destroyed (not saved).
    #[must_use]
    pub const fn should_destroy(self) -> bool {
        matches!(self, Self::Killed | Self::Discarded)
    }

    /// Returns true if the entity should be saved when removed.
    ///
    /// In vanilla, only `UnloadedToChunk` saves - the entity persists in chunk storage.
    /// `ChangedWorld` does NOT save because the entity moves to a different world
    /// rather than being stored in the current world's entity storage.
    #[must_use]
    pub const fn should_save(self) -> bool {
        matches!(self, Self::UnloadedToChunk)
    }
}

/// Callback interface for entity lifecycle events.
///
/// Mirrors vanilla's `EntityInLevelCallback`.
pub trait EntityLevelCallback: Send + Sync {
    /// Called when entity position changes - may trigger section/chunk migration.
    fn on_move(&self, old_pos: DVec3, new_pos: DVec3);

    /// Called when entity is removed from the world.
    fn on_remove(&self, reason: RemovalReason);
}

/// Null callback for entities not yet in the world.
pub struct NullEntityCallback;

impl EntityLevelCallback for NullEntityCallback {
    fn on_move(&self, _old_pos: DVec3, _new_pos: DVec3) {}
    fn on_remove(&self, _reason: RemovalReason) {}
}

/// Callback for players - only tracks section changes for the entity cache.
///
/// Players are stored in `World.players`, not in chunk entity storage,
/// so this callback doesn't handle chunk movement - only section index updates.
pub struct PlayerEntityCallback {
    entity_id: i32,
    world: Weak<World>,
    state: SyncMutex<PlayerEntityCallbackState>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct PlayerEntityCallbackState {
    last_section: SectionPos,
}

impl PlayerEntityCallbackState {
    fn new(position: DVec3) -> Self {
        Self {
            last_section: SectionPos::from_entity_pos(position),
        }
    }

    fn replace_section(&mut self, new_section: SectionPos) -> Option<SectionPos> {
        if self.last_section == new_section {
            return None;
        }

        let old_section = self.last_section;
        self.last_section = new_section;
        Some(old_section)
    }
}

impl PlayerEntityCallback {
    /// Creates a new callback for a player.
    #[must_use]
    pub fn new(entity_id: i32, position: DVec3, world: Weak<World>) -> Self {
        Self {
            entity_id,
            world,
            state: SyncMutex::new(PlayerEntityCallbackState::new(position)),
        }
    }
}

impl EntityLevelCallback for PlayerEntityCallback {
    fn on_move(&self, _old_pos: DVec3, new_pos: DVec3) {
        let Some(world) = self.world.upgrade() else {
            return;
        };

        let new_section = SectionPos::from_entity_pos(new_pos);
        let old_section = self.state.lock().replace_section(new_section);

        // Update section cache if section changed
        if let Some(old_section) = old_section {
            world
                .entity_cache()
                .on_section_change(self.entity_id, old_section, new_section);

            let old_chunk = ChunkPos::new(old_section.x(), old_section.z());
            let new_chunk = ChunkPos::new(new_section.x(), new_section.z());
            world.entity_tracker().on_entity_section_change(
                self.entity_id,
                old_chunk,
                new_chunk,
                |chunk| world.player_area_map.get_tracking_players(chunk),
                |player_id| world.players.get_by_entity_id(player_id),
            );

            if let Some(player) = world.players.get_by_entity_id(self.entity_id)
                && let Some(view) = *player.last_tracking_view.lock()
            {
                world.entity_tracker().update_player(&player, &view);
            }
        }
    }

    fn on_remove(&self, _reason: RemovalReason) {
        // Player removal is handled by World::remove_player, not through this callback
    }
}

/// Callback attached to each entity for tracking chunk/section movement.
///
/// Mirrors vanilla's `PersistentEntitySectionManager.Callback`.
pub struct EntityChunkCallback {
    entity_id: i32,
    world: Weak<World>,
    state: SyncMutex<EntityChunkCallbackState>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct EntityChunkCallbackState {
    last_chunk: ChunkPos,
    last_section: SectionPos,
    removed: bool,
}

impl EntityChunkCallbackState {
    fn new(position: DVec3) -> Self {
        Self {
            last_chunk: ChunkPos::from_entity_pos(position),
            last_section: SectionPos::from_entity_pos(position),
            removed: false,
        }
    }

    fn replace_section(&mut self, new_section: SectionPos) -> Option<SectionPos> {
        if self.last_section == new_section {
            return None;
        }

        let old_section = self.last_section;
        self.last_section = new_section;
        Some(old_section)
    }

    fn replace_chunk(&mut self, new_chunk: ChunkPos) -> Option<ChunkPos> {
        if self.last_chunk == new_chunk {
            return None;
        }

        let old_chunk = self.last_chunk;
        self.last_chunk = new_chunk;
        Some(old_chunk)
    }

    const fn mark_removed(&mut self) -> Option<ChunkPos> {
        if self.removed {
            return None;
        }

        self.removed = true;
        Some(self.last_chunk)
    }
}

impl EntityChunkCallback {
    /// Creates a new callback for an entity.
    #[must_use]
    pub fn new(entity: &SharedEntity, world: Weak<World>) -> Self {
        let pos = entity.position();

        Self {
            entity_id: entity.id(),
            world,
            state: SyncMutex::new(EntityChunkCallbackState::new(pos)),
        }
    }
}

impl EntityLevelCallback for EntityChunkCallback {
    fn on_move(&self, _old_pos: DVec3, new_pos: DVec3) {
        let Some(world) = self.world.upgrade() else {
            return;
        };

        let new_section = SectionPos::from_entity_pos(new_pos);
        let new_chunk = ChunkPos::from_entity_pos(new_pos);

        let (old_section_pos, old_chunk_pos) = {
            let state = self.state.lock();
            (state.last_section, state.last_chunk)
        };

        let section_changed = old_section_pos != new_section;
        let chunk_changed = old_chunk_pos != new_chunk;

        if chunk_changed
            && !world.move_entity_between_chunks(self.entity_id, old_chunk_pos, new_chunk)
        {
            log::warn!(
                "Could not move entity {} from chunk {:?} to non-full chunk {:?}",
                self.entity_id,
                old_chunk_pos,
                new_chunk
            );
            return;
        }

        {
            let mut state = self.state.lock();
            if section_changed {
                let _ = state.replace_section(new_section);
            }
            if chunk_changed {
                let _ = state.replace_chunk(new_chunk);
            }
        }

        // Update section cache if section changed
        if section_changed {
            world
                .entity_cache()
                .on_section_change(self.entity_id, old_section_pos, new_section);
        }

        if chunk_changed {
            // Mark both old and new chunks dirty for saving
            // (within-chunk movement is handled by LevelChunk::tick marking dirty after entity ticks)
            world.mark_chunk_dirty(old_chunk_pos);
            world.mark_chunk_dirty(new_chunk);
        }

        if section_changed {
            world.entity_tracker().on_entity_section_change(
                self.entity_id,
                old_chunk_pos,
                new_chunk,
                |chunk| world.player_area_map.get_tracking_players(chunk),
                |player_id| world.players.get_by_entity_id(player_id),
            );
        }
    }

    fn on_remove(&self, reason: RemovalReason) {
        let Some(chunk_pos) = self.state.lock().mark_removed() else {
            return;
        };

        let Some(world) = self.world.upgrade() else {
            return;
        };

        // Mark chunk dirty so removal is persisted
        world.mark_chunk_dirty(chunk_pos);

        world.remove_entity_internal(self.entity_id, chunk_pos, reason);
    }
}

#[cfg(test)]
mod tests {
    use glam::DVec3;
    use steel_utils::{ChunkPos, SectionPos};

    use super::{EntityChunkCallbackState, PlayerEntityCallbackState};

    #[test]
    fn player_callback_state_reports_section_changes_once() {
        let mut state = PlayerEntityCallbackState::new(DVec3::new(1.0, 64.0, 1.0));
        let start_section = SectionPos::from_entity_pos(DVec3::new(1.0, 64.0, 1.0));
        let next_section = SectionPos::from_entity_pos(DVec3::new(1.0, 80.0, 1.0));

        assert_eq!(state.replace_section(start_section), None);
        assert_eq!(state.replace_section(next_section), Some(start_section));
        assert_eq!(state.replace_section(next_section), None);
    }

    #[test]
    fn chunk_callback_state_tracks_chunk_section_and_removal() {
        let mut state = EntityChunkCallbackState::new(DVec3::new(1.0, 64.0, 1.0));
        let start_chunk = ChunkPos::from_entity_pos(DVec3::new(1.0, 64.0, 1.0));
        let next_chunk = ChunkPos::from_entity_pos(DVec3::new(17.0, 64.0, 1.0));
        let start_section = SectionPos::from_entity_pos(DVec3::new(1.0, 64.0, 1.0));
        let next_section = SectionPos::from_entity_pos(DVec3::new(17.0, 80.0, 1.0));

        assert_eq!(state.replace_chunk(start_chunk), None);
        assert_eq!(state.replace_section(start_section), None);
        assert_eq!(state.replace_chunk(next_chunk), Some(start_chunk));
        assert_eq!(state.replace_section(next_section), Some(start_section));
        assert_eq!(state.mark_removed(), Some(next_chunk));
        assert_eq!(state.mark_removed(), None);
    }
}
