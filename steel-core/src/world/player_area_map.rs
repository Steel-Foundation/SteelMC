//! Spatial data structure for efficient player proximity queries.
//!
//! Based on VMP (Very Many Players) implementation pattern:
//! Maps chunk coordinates to sets of players for O(1) nearby player lookup.

use std::sync::Arc;

use rustc_hash::FxHashSet;
use steel_utils::ChunkPos;

use crate::chunk::player_chunk_view::PlayerChunkView;
use crate::player::Player;

/// Spatial index for player proximity queries.
///
/// Uses packed `ChunkPos` chunk coordinates as keys for efficient hashing.
/// Thread-safe via `scc::HashMap` for concurrent access.
///
/// The map maintains a dual index:
/// - `chunks`: Maps chunk coords to players whose tracking area includes that chunk
/// - `player_chunks`: Maps player entity IDs to the set of chunks they're registered in
///
/// This enables O(1) lookup of nearby players and O(tracking area) removal.
///
/// Entity IDs are used instead of UUIDs because:
/// - They are globally unique within a server session (vanilla uses a static atomic counter)
/// - They are smaller (4 bytes vs 16 bytes) and faster to hash
/// - `PlayerAreaMap` only tracks players during a session, not persisted
pub struct PlayerAreaMap {
    /// Maps packed chunk coords (`ChunkPos`) to set of player entity IDs
    chunks: scc::HashMap<ChunkPos, FxHashSet<i32>>,

    /// Maps player entity ID to its current set of tracked chunks (for efficient removal)
    player_chunks: scc::HashMap<i32, FxHashSet<ChunkPos>>,
}

impl Default for PlayerAreaMap {
    fn default() -> Self {
        Self::new()
    }
}

impl PlayerAreaMap {
    /// Creates a new player area map.
    #[must_use]
    pub fn new() -> Self {
        Self {
            chunks: scc::HashMap::new(),
            player_chunks: scc::HashMap::new(),
        }
    }

    /// Registers a player at their current position using their chunk view.
    pub fn on_player_join(&self, player: &Player, view: &PlayerChunkView) {
        let entity_id = player.id;
        let mut player_set = FxHashSet::default();

        view.for_each(|chunk| {
            player_set.insert(chunk);
            self.add_to_chunk(chunk, entity_id);
        });

        let _ = self.player_chunks.insert_sync(entity_id, player_set);
    }

    /// Removes a player from all tracked chunks.
    pub fn on_player_leave(&self, player: &Arc<Player>) {
        let entity_id = player.id;
        if let Some((_, chunks)) = self.player_chunks.remove_sync(&entity_id) {
            for chunk in chunks {
                self.remove_from_chunk(chunk, entity_id);
            }
        }
    }

    /// Updates a player's tracked chunks using pre-computed view differences.
    ///
    /// Call this after computing the difference via `PlayerChunkView::difference()`.
    pub fn on_player_view_change(
        &self,
        entity_id: i32,
        added_chunks: &[ChunkPos],
        removed_chunks: &[ChunkPos],
    ) {
        if added_chunks.is_empty() && removed_chunks.is_empty() {
            return;
        }

        for &chunk in removed_chunks {
            self.remove_from_chunk(chunk, entity_id);
        }

        for &chunk in added_chunks {
            self.add_to_chunk(chunk, entity_id);
        }

        // Update the player's chunk set
        self.player_chunks.update_sync(&entity_id, |_, set| {
            for &chunk in removed_chunks {
                set.remove(&chunk);
            }
            for &chunk in added_chunks {
                set.insert(chunk);
            }
        });
    }

    /// Gets all players tracking the given chunk.
    #[must_use]
    pub fn get_tracking_players(&self, chunk: ChunkPos) -> Vec<i32> {
        self.chunks
            .read_sync(&chunk, |_, set| set.iter().copied().collect())
            .unwrap_or_default()
    }

    /// Returns the number of tracked players.
    #[must_use]
    pub fn len(&self) -> usize {
        self.player_chunks.len()
    }

    /// Returns true if no players are tracked.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.player_chunks.is_empty()
    }

    fn add_to_chunk(&self, chunk: ChunkPos, entity_id: i32) {
        if self
            .chunks
            .update_sync(&chunk, |_, set| {
                set.insert(entity_id);
            })
            .is_none()
        {
            let mut set = FxHashSet::default();
            set.insert(entity_id);
            let _ = self.chunks.insert_sync(chunk, set);
        }
    }

    fn remove_from_chunk(&self, chunk: ChunkPos, entity_id: i32) {
        let should_remove = self
            .chunks
            .update_sync(&chunk, |_, set| {
                set.remove(&entity_id);
                set.is_empty()
            })
            .unwrap_or(false);

        if should_remove {
            let _ = self.chunks.remove_if_sync(&chunk, |set| set.is_empty());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_add_and_get() {
        let map = PlayerAreaMap::new();
        let entity_id = 42;
        let center = ChunkPos::new(0, 0);
        let view = PlayerChunkView::new(center, 2);

        // Manually add since we don't have a Player in tests
        let mut player_set = FxHashSet::default();
        view.for_each(|chunk| {
            player_set.insert(chunk);
            map.add_to_chunk(chunk, entity_id);
        });
        let _ = map.player_chunks.insert_sync(entity_id, player_set);

        assert!(map.get_tracking_players(center).contains(&entity_id));
        assert!(
            map.get_tracking_players(ChunkPos::new(1, 1))
                .contains(&entity_id)
        );
        // ChunkPos(3,0) should be in view with distance 2 (due to buffer logic)
        assert!(
            map.get_tracking_players(ChunkPos::new(3, 0))
                .contains(&entity_id)
        );
        // ChunkPos(5,5) should be outside
        assert!(
            !map.get_tracking_players(ChunkPos::new(5, 5))
                .contains(&entity_id)
        );
    }

    #[test]
    fn test_remove() {
        let map = PlayerAreaMap::new();
        let entity_id = 42;
        let center = ChunkPos::new(0, 0);
        let view = PlayerChunkView::new(center, 2);

        // Manually add
        let mut player_set = FxHashSet::default();
        view.for_each(|chunk| {
            player_set.insert(chunk);
            map.add_to_chunk(chunk, entity_id);
        });
        let _ = map.player_chunks.insert_sync(entity_id, player_set);
        assert_eq!(map.len(), 1);

        // Manually remove
        if let Some((_, chunks)) = map.player_chunks.remove_sync(&entity_id) {
            for chunk in chunks {
                map.remove_from_chunk(chunk, entity_id);
            }
        }
        assert_eq!(map.len(), 0);
        assert!(map.get_tracking_players(center).is_empty());
    }

    #[test]
    fn test_view_change() {
        let map = PlayerAreaMap::new();
        let entity_id = 42;
        let old_center = ChunkPos::new(0, 0);
        let new_center = ChunkPos::new(5, 5);
        let old_view = PlayerChunkView::new(old_center, 1);
        let new_view = PlayerChunkView::new(new_center, 1);

        // Manually add
        let mut player_set = FxHashSet::default();
        old_view.for_each(|chunk| {
            player_set.insert(chunk);
            map.add_to_chunk(chunk, entity_id);
        });
        let _ = map.player_chunks.insert_sync(entity_id, player_set);
        assert!(map.get_tracking_players(old_center).contains(&entity_id));

        // Compute diff using PlayerChunkView::difference
        let mut diff = (Vec::new(), Vec::new());
        PlayerChunkView::difference(
            &old_view,
            &new_view,
            |pos, (added, _): &mut (Vec<ChunkPos>, Vec<ChunkPos>)| added.push(pos),
            |pos, (_, removed): &mut (Vec<ChunkPos>, Vec<ChunkPos>)| removed.push(pos),
            &mut diff,
        );
        let (added, removed) = diff;

        map.on_player_view_change(entity_id, &added, &removed);

        assert!(!map.get_tracking_players(old_center).contains(&entity_id));
        assert!(map.get_tracking_players(new_center).contains(&entity_id));
    }

    #[test]
    fn test_multiple_players() {
        let map = PlayerAreaMap::new();
        let entity_id1 = 42;
        let entity_id2 = 43;

        let view1 = PlayerChunkView::new(ChunkPos::new(0, 0), 2);
        let view2 = PlayerChunkView::new(ChunkPos::new(1, 1), 2);

        // Manually add both
        let mut set1 = FxHashSet::default();
        view1.for_each(|chunk| {
            set1.insert(chunk);
            map.add_to_chunk(chunk, entity_id1);
        });
        let _ = map.player_chunks.insert_sync(entity_id1, set1);

        let mut set2 = FxHashSet::default();
        view2.for_each(|chunk| {
            set2.insert(chunk);
            map.add_to_chunk(chunk, entity_id2);
        });
        let _ = map.player_chunks.insert_sync(entity_id2, set2);

        let players = map.get_tracking_players(ChunkPos::new(0, 0));
        assert!(players.contains(&entity_id1));
        assert!(players.contains(&entity_id2));
        assert_eq!(map.len(), 2);
    }
}
