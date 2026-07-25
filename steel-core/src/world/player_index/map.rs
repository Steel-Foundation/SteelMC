//! Thread-safe player storage with dual indexing by UUID and entity ID.

use std::sync::Arc;

use scc::HashMap;
use steel_utils::locks::SyncMutex;
use uuid::Uuid;

use crate::{entity::Entity, player::Player};

/// Thread-safe player storage with dual indexing.
///
/// Maintains two synchronized maps for O(1) lookup by either UUID or entity ID.
/// All operations keep both maps in sync automatically.
pub struct PlayerMap {
    /// Primary index by UUID (persistent identifier)
    by_uuid: HashMap<Uuid, Arc<Player>>,
    /// Secondary index by entity ID (session-local identifier)
    by_entity_id: HashMap<i32, Arc<Player>>,
    /// Player UUIDs in insertion order for vanilla-visible iteration.
    order: SyncMutex<Vec<Uuid>>,
}

impl Default for PlayerMap {
    fn default() -> Self {
        Self::new()
    }
}

impl PlayerMap {
    /// Creates a new empty player map.
    #[must_use]
    pub fn new() -> Self {
        Self {
            by_uuid: HashMap::new(),
            by_entity_id: HashMap::new(),
            order: SyncMutex::new(Vec::new()),
        }
    }

    /// Inserts a player into both maps.
    ///
    /// Returns `true` if the player was inserted, `false` if a player with the same UUID already exists.
    ///
    /// # Panics
    ///
    /// Panics if another player already has the same entity ID. Entity IDs are
    /// session-unique; accepting a duplicate would break entity lookup and
    /// packet routing invariants.
    pub fn insert(&self, player: Arc<Player>) -> bool {
        let uuid = player.gameprofile.id;
        let entity_id = player.id();

        if self.by_uuid.insert_sync(uuid, player.clone()).is_err() {
            return false;
        }

        if self.by_entity_id.insert_sync(entity_id, player).is_err() {
            let _ = self.by_uuid.remove_sync(&uuid);
            panic!("player entity id {entity_id} is already registered");
        }
        self.order.lock().push(uuid);
        true
    }

    /// Removes a player by UUID from both maps.
    ///
    /// Returns the removed player if found.
    pub async fn remove(&self, uuid: &Uuid) -> Option<Arc<Player>> {
        if let Some((_, player)) = self.by_uuid.remove_async(uuid).await {
            let _ = self.by_entity_id.remove_async(&player.id()).await;
            self.order.lock().retain(|player_uuid| player_uuid != uuid);
            Some(player)
        } else {
            None
        }
    }

    /// Removes this exact player from both maps.
    ///
    /// Returns the removed player if the UUID still maps to this same player
    /// handle. A stale duplicate-login cleanup must not remove the accepted
    /// player that owns the UUID.
    pub async fn remove_player(&self, player: &Arc<Player>) -> Option<Arc<Player>> {
        let uuid = player.gameprofile.id;
        let (_, removed) = self
            .by_uuid
            .remove_if_async(&uuid, |current| Arc::ptr_eq(current, player))
            .await?;
        let _ = self
            .by_entity_id
            .remove_if_async(&removed.id(), |current| Arc::ptr_eq(current, &removed))
            .await;
        self.order
            .lock()
            .retain(|uuid| *uuid != removed.gameprofile.id);
        Some(removed)
    }

    /// Removes a player by UUID from both maps synchronously.
    ///
    /// Returns the removed player if found. Use this when async is not available
    /// (e.g., during world changes on the tick thread).
    pub fn remove_sync(&self, uuid: &Uuid) -> Option<Arc<Player>> {
        if let Some((_, player)) = self.by_uuid.remove_sync(uuid) {
            let _ = self.by_entity_id.remove_sync(&player.id());
            self.order.lock().retain(|player_uuid| player_uuid != uuid);
            Some(player)
        } else {
            None
        }
    }

    /// Removes this exact player from both maps synchronously.
    pub fn remove_player_sync(&self, player: &Arc<Player>) -> Option<Arc<Player>> {
        let uuid = player.gameprofile.id;
        let (_, removed) = self
            .by_uuid
            .remove_if_sync(&uuid, |current| Arc::ptr_eq(current, player))?;
        let _ = self
            .by_entity_id
            .remove_if_sync(&removed.id(), |current| Arc::ptr_eq(current, &removed));
        self.order
            .lock()
            .retain(|uuid| *uuid != removed.gameprofile.id);
        Some(removed)
    }

    /// Gets a player by UUID.
    #[must_use]
    pub fn get_by_uuid(&self, uuid: &Uuid) -> Option<Arc<Player>> {
        self.by_uuid.read_sync(uuid, |_, p| p.clone())
    }

    /// Gets a player by entity ID.
    #[must_use]
    pub fn get_by_entity_id(&self, entity_id: i32) -> Option<Arc<Player>> {
        self.by_entity_id.read_sync(&entity_id, |_, p| p.clone())
    }

    /// Iterates over all players.
    ///
    /// The callback returns `true` to continue iteration, `false` to stop.
    pub fn iter_players<F>(&self, mut f: F)
    where
        F: FnMut(&Uuid, &Arc<Player>) -> bool,
    {
        let order = self.order.lock().iter().copied().collect::<Vec<_>>();
        for uuid in order {
            let Some(player) = self.get_by_uuid(&uuid) else {
                continue;
            };
            if !f(&uuid, &player) {
                return;
            }
        }
    }

    /// Returns the number of players.
    #[must_use]
    pub fn len(&self) -> usize {
        self.by_uuid.len()
    }

    /// Returns true if there are no players.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.by_uuid.is_empty()
    }
}
