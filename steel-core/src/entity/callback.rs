//! Entity lifecycle callbacks for movement and removal tracking.

use std::sync::{
    Weak,
    atomic::{AtomicBool, AtomicI64, Ordering},
};

use steel_utils::{ChunkPos, SectionPos, math::Vector3};

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
    /// Entity changed dimension.
    ChangedDimension,
}

impl RemovalReason {
    /// Returns true if entity data should be destroyed (not saved).
    #[must_use]
    pub fn should_destroy(self) -> bool {
        matches!(self, Self::Killed | Self::Discarded)
    }

    /// Returns true if the entity should be saved when removed.
    ///
    /// In vanilla, only `UnloadedToChunk` saves - the entity persists in chunk storage.
    /// `ChangedDimension` does NOT save because the entity moves to a different world
    /// rather than being stored in the current world's entity storage.
    #[must_use]
    pub fn should_save(self) -> bool {
        matches!(self, Self::UnloadedToChunk)
    }
}

/// Callback interface for entity lifecycle events.
///
/// Mirrors vanilla's `EntityInLevelCallback`.
pub trait EntityLevelCallback: Send + Sync {
    /// Called when entity position changes - may trigger section/chunk migration.
    fn on_move(&self, old_pos: Vector3<f64>, new_pos: Vector3<f64>);

    /// Called when entity is removed from the world.
    fn on_remove(&self, reason: RemovalReason);
}

/// Null callback for entities not yet in the world.
pub struct NullEntityCallback;

impl EntityLevelCallback for NullEntityCallback {
    fn on_move(&self, _old_pos: Vector3<f64>, _new_pos: Vector3<f64>) {}
    fn on_remove(&self, _reason: RemovalReason) {}
}

/// Callback for players - only tracks section changes for the entity cache.
///
/// Players are stored in `World.players`, not in chunk entity storage,
/// so this callback doesn't handle chunk movement - only section index updates.
pub struct PlayerEntityCallback {
    entity_id: i32,
    world: Weak<World>,
    /// Packed last known section position (for cache updates).
    last_section: AtomicI64,
}

impl PlayerEntityCallback {
    /// Creates a new callback for a player.
    #[must_use]
    pub fn new(entity_id: i32, position: Vector3<f64>, world: Weak<World>) -> Self {
        let section_pos = SectionPos::new(
            (position.x as i32) >> 4,
            (position.y as i32) >> 4,
            (position.z as i32) >> 4,
        );

        Self {
            entity_id,
            world,
            last_section: AtomicI64::new(section_pos.as_i64()),
        }
    }
}

impl EntityLevelCallback for PlayerEntityCallback {
    fn on_move(&self, _old_pos: Vector3<f64>, new_pos: Vector3<f64>) {
        let Some(world) = self.world.upgrade() else {
            return;
        };

        let new_section = SectionPos::new(
            (new_pos.x as i32) >> 4,
            (new_pos.y as i32) >> 4,
            (new_pos.z as i32) >> 4,
        );

        let old_packed = self
            .last_section
            .swap(new_section.as_i64(), Ordering::AcqRel);
        let old_section = SectionPos::from_i64(old_packed);

        // Update section cache if section changed
        if old_section != new_section {
            world
                .entity_cache()
                .on_section_change(self.entity_id, old_section, new_section);
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
    /// Packed last known chunk position (for detecting chunk transitions).
    last_chunk: AtomicI64,
    /// Packed last known section position (for cache updates).
    last_section: AtomicI64,
    /// Whether we've already processed a removal.
    removed: AtomicBool,
}

impl EntityChunkCallback {
    /// Creates a new callback for an entity.
    #[must_use]
    pub fn new(entity: &SharedEntity, world: Weak<World>) -> Self {
        let pos = entity.position();
        let chunk_pos = ChunkPos::new((pos.x as i32) >> 4, (pos.z as i32) >> 4);
        let section_pos = SectionPos::new(
            (pos.x as i32) >> 4,
            (pos.y as i32) >> 4,
            (pos.z as i32) >> 4,
        );

        Self {
            entity_id: entity.id(),
            world,
            last_chunk: AtomicI64::new(chunk_pos.as_i64()),
            last_section: AtomicI64::new(section_pos.as_i64()),
            removed: AtomicBool::new(false),
        }
    }

    /// Gets the current chunk position from stored state.
    fn current_chunk(&self) -> ChunkPos {
        ChunkPos::from_i64(self.last_chunk.load(Ordering::Acquire))
    }
}

impl EntityLevelCallback for EntityChunkCallback {
    fn on_move(&self, old_pos: Vector3<f64>, new_pos: Vector3<f64>) {
        let Some(world) = self.world.upgrade() else {
            return;
        };

        // Calculate section positions
        let old_section = SectionPos::new(
            (old_pos.x as i32) >> 4,
            (old_pos.y as i32) >> 4,
            (old_pos.z as i32) >> 4,
        );
        let new_section = SectionPos::new(
            (new_pos.x as i32) >> 4,
            (new_pos.y as i32) >> 4,
            (new_pos.z as i32) >> 4,
        );

        // Update section cache if section changed
        if old_section != new_section {
            let old_packed = self
                .last_section
                .swap(new_section.as_i64(), Ordering::AcqRel);
            let actual_old_section = SectionPos::from_i64(old_packed);

            world
                .entity_cache()
                .on_section_change(self.entity_id, actual_old_section, new_section);
        }

        // Calculate chunk positions
        let old_chunk = ChunkPos::new((old_pos.x as i32) >> 4, (old_pos.z as i32) >> 4);
        let new_chunk = ChunkPos::new((new_pos.x as i32) >> 4, (new_pos.z as i32) >> 4);

        // Move Arc between chunks if chunk changed
        if old_chunk != new_chunk {
            let old_packed = self.last_chunk.swap(new_chunk.as_i64(), Ordering::AcqRel);
            let actual_old_chunk = ChunkPos::from_i64(old_packed);

            world.move_entity_between_chunks(self.entity_id, actual_old_chunk, new_chunk);

            // Mark both old and new chunks dirty for saving
            // (within-chunk movement is handled by LevelChunk::tick marking dirty after entity ticks)
            world.mark_chunk_dirty(actual_old_chunk);
            world.mark_chunk_dirty(new_chunk);
        }
    }

    fn on_remove(&self, reason: RemovalReason) {
        // Prevent double removal
        if self.removed.swap(true, Ordering::AcqRel) {
            return;
        }

        let Some(world) = self.world.upgrade() else {
            return;
        };

        let chunk_pos = self.current_chunk();

        // Mark chunk dirty so removal is persisted
        world.mark_chunk_dirty(chunk_pos);

        world.remove_entity_internal(self.entity_id, chunk_pos, reason);
    }
}
