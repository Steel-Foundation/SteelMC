//! Thread-safe player storage with dual indexing by UUID and entity ID.

use std::sync::Arc;

use scc::HashMap;
use uuid::Uuid;

use crate::player::Player;

/// Thread-safe player storage with dual indexing.
///
/// Maintains two synchronized maps for O(1) lookup by either UUID or entity ID.
/// All operations keep both maps in sync automatically.
pub struct PlayerMap {
    /// Primary index by UUID (persistent identifier)
    by_uuid: HashMap<Uuid, Arc<Player>>,
    /// Secondary index by entity ID (session-local identifier)
    by_entity_id: HashMap<i32, Arc<Player>>,
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
        }
    }

    /// Inserts a player into both maps.
    ///
    /// Returns `true` if the player was inserted, `false` if a player with the same UUID already exists.
    pub fn insert(&self, player: Arc<Player>) -> bool {
        let uuid = player.gameprofile.id;
        let entity_id = player.id;

        if self.by_uuid.insert_sync(uuid, player.clone()).is_err() {
            return false;
        }

        let _ = self.by_entity_id.insert_sync(entity_id, player);
        true
    }

    /// Removes a player by UUID from both maps.
    ///
    /// Returns the removed player if found.
    pub async fn remove(&self, uuid: &Uuid) -> Option<Arc<Player>> {
        if let Some((_, player)) = self.by_uuid.remove_async(uuid).await {
            let _ = self.by_entity_id.remove_async(&player.id).await;
            Some(player)
        } else {
            None
        }
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
        self.by_uuid.iter_sync(|uuid, player| f(uuid, player));
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
