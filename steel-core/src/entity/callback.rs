//! Entity lifecycle callbacks for movement and removal tracking.

use std::sync::Weak;

use glam::DVec3;
use steel_utils::ChunkPos;

use super::EntityMoveError;
use crate::{entity::Entity, world::World};

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
    /// Entity is persisted inside a player `RootVehicle` payload.
    StoredWithPlayer,
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
    /// `ChangedWorld` and `StoredWithPlayer` do not save because the entity
    /// is retained by another owner instead of current-world entity storage.
    #[must_use]
    pub const fn should_save(self) -> bool {
        matches!(self, Self::UnloadedToChunk)
    }
}

/// Callback interface for entity lifecycle events.
///
/// Mirrors vanilla's `EntityInLevelCallback`.
pub trait EntityLevelCallback: Send + Sync {
    /// Returns whether direct local position writes may bypass lifecycle callbacks.
    fn allows_local_position_update(&self) -> bool {
        false
    }

    /// Called before an entity position change is committed.
    fn validate_move(&self, old_pos: DVec3, new_pos: DVec3) -> Result<(), EntityMoveError>;

    /// Called after an entity position change has been committed.
    ///
    /// `entity` is the already-locked concrete entity when the move originates from
    /// inside the entity's behavior lock (e.g. during `tick`), so tracker work can
    /// reuse it instead of re-locking. It is `None` for base-direct moves made with
    /// no behavior lock held (construction, loading), where re-locking is safe.
    fn on_move_committed(
        &self,
        entity: Option<&mut dyn Entity>,
        old_pos: DVec3,
        new_pos: DVec3,
    ) -> Result<(), EntityMoveError>;

    /// Called when entity is removed from the world.
    fn on_remove(&self, reason: RemovalReason);
}

/// Null callback for entities not yet in the world.
pub struct NullEntityCallback;

impl EntityLevelCallback for NullEntityCallback {
    fn allows_local_position_update(&self) -> bool {
        true
    }

    fn validate_move(&self, _old_pos: DVec3, _new_pos: DVec3) -> Result<(), EntityMoveError> {
        Ok(())
    }

    fn on_move_committed(
        &self,
        _entity: Option<&mut dyn Entity>,
        _old_pos: DVec3,
        _new_pos: DVec3,
    ) -> Result<(), EntityMoveError> {
        Ok(())
    }

    fn on_remove(&self, _reason: RemovalReason) {}
}

/// Callback for entities retained outside live world membership.
pub struct InactiveEntityCallback {
    entity_id: i32,
}

impl InactiveEntityCallback {
    /// Creates an inactive callback for a retained non-live entity.
    #[must_use]
    pub const fn new(entity_id: i32) -> Self {
        Self { entity_id }
    }
}

impl EntityLevelCallback for InactiveEntityCallback {
    fn validate_move(&self, _old_pos: DVec3, _new_pos: DVec3) -> Result<(), EntityMoveError> {
        Err(EntityMoveError::Inactive {
            entity_id: self.entity_id,
        })
    }

    fn on_move_committed(
        &self,
        _entity: Option<&mut dyn Entity>,
        _old_pos: DVec3,
        _new_pos: DVec3,
    ) -> Result<(), EntityMoveError> {
        Err(EntityMoveError::Inactive {
            entity_id: self.entity_id,
        })
    }

    fn on_remove(&self, _reason: RemovalReason) {}
}

/// Callback for players.
///
/// Players are owned by `World.players`, but the world entity manager still
/// indexes their live position for lookup and tracking updates.
pub struct PlayerEntityCallback {
    entity_id: i32,
    world: Weak<World>,
}

impl PlayerEntityCallback {
    /// Creates a new callback for a player.
    #[must_use]
    pub const fn new(entity_id: i32, world: Weak<World>) -> Self {
        Self { entity_id, world }
    }
}

impl EntityLevelCallback for PlayerEntityCallback {
    fn validate_move(&self, old_pos: DVec3, new_pos: DVec3) -> Result<(), EntityMoveError> {
        let Some(world) = self.world.upgrade() else {
            return Err(EntityMoveError::NotLive {
                entity_id: self.entity_id,
            });
        };

        world
            .entity_manager()
            .validate_move(self.entity_id, new_pos)
            .inspect_err(|error| {
                log::warn!("Rejected player entity move from {old_pos:?} to {new_pos:?}: {error}");
            })
    }

    fn on_move_committed(
        &self,
        entity: Option<&mut dyn Entity>,
        old_pos: DVec3,
        new_pos: DVec3,
    ) -> Result<(), EntityMoveError> {
        let Some(world) = self.world.upgrade() else {
            return Err(EntityMoveError::NotLive {
                entity_id: self.entity_id,
            });
        };

        let update = world
            .entity_manager()
            .commit_move(self.entity_id, new_pos)
            .inspect_err(|error| {
                log::warn!(
                    "Failed to commit player entity move from {old_pos:?} to {new_pos:?}: {error}"
                );
            })?;

        if update.section_changed() {
            // This player's tracking view lives on the `ServerPlayer` (Arc-level lock),
            // not behind the entity lock, so it is safe to read while the entity is held.
            let self_view = world
                .players
                .get_by_entity_id(self.entity_id)
                .and_then(|sp| *sp.last_tracking_view.lock());

            // Refresh THIS player's own tracked-entity set. When the move holds the
            // behavior lock (entity threaded in as `Some`), reuse it — re-locking self
            // here deadlocks under the tick. Only lock when no behavior lock is held.
            if let Some(view) = self_view {
                // The sent-chunk set lives on the session, so it is readable without
                // the behavior lock.
                let sent_chunks = world
                    .players
                    .get_by_entity_id(self.entity_id)
                    .map(|session| session.chunk_sender.lock().sent_chunks_snapshot())
                    .unwrap_or_default();
                let is_chunk_sent = |chunk| sent_chunks.contains(&chunk);

                match entity.as_deref() {
                    Some(e) => {
                        if let Some(player) = e.as_player_ref() {
                            world
                                .entity_tracker()
                                .update_player(player, &view, is_chunk_sent);
                        }
                    }
                    None => {
                        if let Some(player) = world.players.get_by_entity_id(self.entity_id) {
                            world.entity_tracker().update_player(
                                &player.entity.lock(),
                                &view,
                                is_chunk_sent,
                            );
                        }
                    }
                }
            }

            world.entity_tracker().on_entity_section_change(
                self.entity_id,
                entity,
                update.old_chunk,
                update.new_chunk,
                |chunk| world.get_packet_tracking_players(chunk),
                |player_id| world.players.get_by_entity_id(player_id),
                |player_id| {
                    world
                        .entity_manager()
                        .get_by_id(player_id)
                        .map(|e| e.position())
                },
            );
        }

        Ok(())
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
}

impl EntityChunkCallback {
    /// Creates a new callback for an entity.
    #[must_use]
    pub const fn new(entity_id: i32, world: Weak<World>) -> Self {
        Self { entity_id, world }
    }
}

impl EntityLevelCallback for EntityChunkCallback {
    fn validate_move(&self, old_pos: DVec3, new_pos: DVec3) -> Result<(), EntityMoveError> {
        let Some(world) = self.world.upgrade() else {
            return Err(EntityMoveError::NotLive {
                entity_id: self.entity_id,
            });
        };

        world
            .entity_manager()
            .validate_move(self.entity_id, new_pos)
            .inspect_err(|error| {
                log::warn!("Rejected entity move from {old_pos:?} to {new_pos:?}: {error}");
            })
    }

    fn on_move_committed(
        &self,
        entity: Option<&mut dyn Entity>,
        old_pos: DVec3,
        new_pos: DVec3,
    ) -> Result<(), EntityMoveError> {
        let Some(world) = self.world.upgrade() else {
            return Err(EntityMoveError::NotLive {
                entity_id: self.entity_id,
            });
        };

        let update = world
            .entity_manager()
            .commit_move(self.entity_id, new_pos)
            .inspect_err(|error| {
                log::warn!("Failed to commit entity move from {old_pos:?} to {new_pos:?}: {error}");
            })?;

        world.mark_chunk_dirty(update.new_chunk);
        if update.chunk_changed() {
            world.mark_chunk_dirty(update.old_chunk);
        }

        if update.section_changed() {
            if update.became_inaccessible() {
                world.remove_entity_from_tracker(self.entity_id);
            } else if update.became_accessible() {
                if let Some(shared) = world.entity_manager().get_by_id(self.entity_id) {
                    world.add_entity_to_tracker_with_entity(&shared, entity);
                }
            } else if update.new_accessible {
                world.entity_tracker().on_entity_section_change(
                    self.entity_id,
                    entity,
                    update.old_chunk,
                    update.new_chunk,
                    |chunk| world.get_packet_tracking_players(chunk),
                    |player_id| world.players.get_by_entity_id(player_id),
                    |player_id| {
                        world
                            .entity_manager()
                            .get_by_id(player_id)
                            .map(|e| e.position())
                    },
                );
            }
        }

        Ok(())
    }

    fn on_remove(&self, reason: RemovalReason) {
        let Some(world) = self.world.upgrade() else {
            return;
        };

        let entity = world
            .entity_manager()
            .remove_live_entity(self.entity_id, reason);
        if let Some(entity) = entity {
            world.mark_chunk_dirty(ChunkPos::from_entity_pos(entity.position()));
        }

        world.remove_entity_from_tracker(self.entity_id);
    }
}
