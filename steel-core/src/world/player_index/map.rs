//! Thread-safe player storage with dual indexing by UUID and entity ID.

use std::sync::Arc;

use arc_swap::ArcSwap;
use scc::HashMap;
use steel_utils::locks::SyncMutex;
use uuid::Uuid;

use crate::{entity::Entity, player::Player};

struct PlayerSlot {
    player: ArcSwap<Player>,
}

impl PlayerSlot {
    fn new(player: Arc<Player>) -> Self {
        Self {
            player: ArcSwap::new(player),
        }
    }

    fn load(&self) -> Arc<Player> {
        self.player.load_full()
    }

    fn replace(&self, expected: &Arc<Player>, replacement: Arc<Player>) -> bool {
        let previous = self.player.compare_and_swap(expected, replacement);
        Arc::ptr_eq(&previous, expected)
    }
}

/// Thread-safe player storage with dual indexing.
///
/// Both indexes point to the same player slot, so replacing a player updates
/// UUID and entity ID lookups as one operation.
pub struct PlayerMap {
    /// Primary index by UUID (persistent identifier)
    by_uuid: HashMap<Uuid, Arc<PlayerSlot>>,
    /// Secondary index by entity ID (session-local identifier)
    by_entity_id: HashMap<i32, Arc<PlayerSlot>>,
    /// Player UUIDs in insertion order for vanilla-visible iteration.
    order: SyncMutex<Vec<Uuid>>,
    /// Serializes changes to the indexes and their shared slots.
    mutations: SyncMutex<()>,
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
            mutations: SyncMutex::new(()),
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
        let _mutation = self.mutations.lock();
        let slot = Arc::new(PlayerSlot::new(player));

        if self.by_uuid.insert_sync(uuid, Arc::clone(&slot)).is_err() {
            return false;
        }

        if self.by_entity_id.insert_sync(entity_id, slot).is_err() {
            let _ = self.by_uuid.remove_sync(&uuid);
            panic!("player entity id {entity_id} is already registered");
        }
        self.order.lock().push(uuid);
        true
    }

    /// Removes a player by UUID from both maps.
    ///
    /// Returns the removed player if found.
    #[expect(
        clippy::unused_async,
        clippy::unused_async_trait_impl,
        reason = "keeps the existing asynchronous PlayerMap API while mutation is synchronized internally"
    )]
    pub async fn remove(&self, uuid: &Uuid) -> Option<Arc<Player>> {
        self.remove_sync(uuid)
    }

    /// Removes this exact player from both maps.
    ///
    /// Returns the removed player if the UUID still maps to this same player
    /// handle. A stale duplicate-login cleanup must not remove the accepted
    /// player that owns the UUID.
    #[expect(
        clippy::unused_async,
        clippy::unused_async_trait_impl,
        reason = "keeps the existing asynchronous PlayerMap API while mutation is synchronized internally"
    )]
    pub async fn remove_player(&self, player: &Arc<Player>) -> Option<Arc<Player>> {
        self.remove_player_sync(player)
    }

    /// Removes a player by UUID from both maps synchronously.
    ///
    /// Returns the removed player if found. Use this when async is not available
    /// (e.g., during world changes on the tick thread).
    pub fn remove_sync(&self, uuid: &Uuid) -> Option<Arc<Player>> {
        let _mutation = self.mutations.lock();
        let (_, slot) = self.by_uuid.remove_sync(uuid)?;
        let player = slot.load();
        let _ = self
            .by_entity_id
            .remove_if_sync(&player.id(), |current| Arc::ptr_eq(current, &slot));
        self.order.lock().retain(|player_uuid| player_uuid != uuid);
        Some(player)
    }

    /// Removes this exact player from both maps synchronously.
    pub fn remove_player_sync(&self, player: &Arc<Player>) -> Option<Arc<Player>> {
        let uuid = player.gameprofile.id;
        let _mutation = self.mutations.lock();
        let slot = self
            .by_uuid
            .read_sync(&uuid, |_, current| Arc::clone(current))?;
        let current = slot.load();
        if !Arc::ptr_eq(&current, player) {
            return None;
        }

        let (_, removed_slot) = self
            .by_uuid
            .remove_if_sync(&uuid, |current| Arc::ptr_eq(current, &slot))?;
        let _ = self
            .by_entity_id
            .remove_if_sync(&current.id(), |indexed| Arc::ptr_eq(indexed, &removed_slot));
        self.order
            .lock()
            .retain(|indexed_uuid| *indexed_uuid != uuid);
        Some(current)
    }

    /// Replaces this exact player while retaining its UUID, entity ID, and
    /// insertion-order position.
    ///
    /// Returns `false` if the replacement has different index keys, either
    /// index no longer points to the same slot, or `expected` is stale. The
    /// pointer comparison and replacement form one compare-and-swap operation,
    /// so concurrent attempts using the same expected player cannot both
    /// succeed.
    pub fn replace_player(&self, expected: &Arc<Player>, replacement: Arc<Player>) -> bool {
        let uuid = expected.gameprofile.id;
        let entity_id = expected.id();
        if replacement.gameprofile.id != uuid || replacement.id() != entity_id {
            return false;
        }

        let _mutation = self.mutations.lock();
        let Some(slot) = self
            .by_uuid
            .read_sync(&uuid, |_, current| Arc::clone(current))
        else {
            return false;
        };
        let Some(entity_slot) = self
            .by_entity_id
            .read_sync(&entity_id, |_, current| Arc::clone(current))
        else {
            return false;
        };
        if !Arc::ptr_eq(&slot, &entity_slot) {
            return false;
        }

        slot.replace(expected, replacement)
    }

    /// Gets a player by UUID.
    #[must_use]
    pub fn get_by_uuid(&self, uuid: &Uuid) -> Option<Arc<Player>> {
        self.by_uuid.read_sync(uuid, |_, slot| slot.load())
    }

    /// Gets a player by entity ID.
    #[must_use]
    pub fn get_by_entity_id(&self, entity_id: i32) -> Option<Arc<Player>> {
        self.by_entity_id
            .read_sync(&entity_id, |_, slot| slot.load())
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

#[cfg(test)]
mod tests;
