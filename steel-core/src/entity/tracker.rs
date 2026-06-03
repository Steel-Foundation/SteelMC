//! Entity tracking system for managing which players can see which entities.
//!
//! Keeps the vanilla visibility predicate in block space. Vanilla stores an
//! entity tracking range as client chunks, multiplies it by 16, caps it by the
//! player's view distance, and then checks horizontal squared distance.

use std::sync::Arc;

use glam::DVec3;
use rustc_hash::FxHashSet;
use steel_protocol::packets::game::{CAddEntity, CRemoveEntities, CSetEntityData, to_angle_byte};
use steel_registry::RegistryEntry;
use steel_utils::ChunkPos;
use steel_utils::locks::SyncRwLock;

use crate::chunk::player_chunk_view::PlayerChunkView;
use crate::entity::{Entity, SharedEntity, WeakEntity};
use crate::player::Player;

const BLOCKS_PER_CHUNK: f64 = 16.0;

/// World-level entity tracker.
pub struct EntityTracker {
    /// Maps entity ID to its tracking data.
    entities: scc::HashMap<i32, TrackedEntity>,
}

/// Tracking data for a single entity.
struct TrackedEntity {
    /// Weak reference to the entity. When this fails to upgrade, entity is dead.
    entity: WeakEntity,
    /// Vanilla client tracking range converted to blocks.
    tracking_range: EntityTrackingRange,
    /// Current chunk used by the player-view predicate.
    registered_chunk: ChunkPos,
    /// Players currently tracking this entity (interior mutable for concurrent access).
    seen_by: SyncRwLock<FxHashSet<i32>>,
}

#[derive(Debug, Clone, Copy)]
struct EntityTrackingRange {
    block_radius: f64,
}

impl EntityTrackingRange {
    fn from_client_chunk_range(client_chunk_range: i32) -> Self {
        Self {
            block_radius: f64::from(client_chunk_range) * BLOCKS_PER_CHUNK,
        }
    }

    fn is_disabled(self) -> bool {
        self.block_radius <= 0.0
    }

    fn visible_radius(self, player_view_distance: u8) -> f64 {
        self.block_radius
            .min(f64::from(player_view_distance) * BLOCKS_PER_CHUNK)
    }
}

impl Default for EntityTracker {
    fn default() -> Self {
        Self::new()
    }
}

impl EntityTracker {
    /// Creates a new empty entity tracker.
    #[must_use]
    pub fn new() -> Self {
        Self {
            entities: scc::HashMap::new(),
        }
    }

    /// Starts tracking an entity.
    ///
    /// Sends spawn packets to players already watching the entity chunk and
    /// inside the vanilla tracking range.
    ///
    /// The `get_players_in_chunk` callback should return player IDs in a given chunk
    /// (typically from `PlayerAreaMap::get_tracking_players`).
    /// The `get_player` callback should resolve a player ID to a `Player` reference.
    pub fn add(
        &self,
        entity: &SharedEntity,
        get_players_in_chunk: impl Fn(ChunkPos) -> Vec<i32>,
        get_player: impl Fn(i32) -> Option<Arc<Player>>,
    ) {
        assert!(
            !entity.is_removed(),
            "cannot add removed entity {} to tracker",
            entity.id()
        );

        let entity_id = entity.id();
        let tracking_range = EntityTrackingRange::from_client_chunk_range(
            entity.entity_type().client_tracking_range,
        );
        if tracking_range.is_disabled() {
            return;
        }

        let pos = entity.position();
        let registered_chunk = ChunkPos::from_entity_pos(pos);

        let players_to_notify = Self::visible_players_for_entity(
            entity_id,
            entity.as_ref(),
            registered_chunk,
            tracking_range,
            &get_players_in_chunk,
            &get_player,
        );
        let player_ids_to_notify: Vec<i32> = players_to_notify.iter().copied().collect();

        let tracked = TrackedEntity {
            entity: Arc::downgrade(entity),
            tracking_range,
            registered_chunk,
            seen_by: SyncRwLock::new(players_to_notify),
        };

        if self.entities.insert_sync(entity_id, tracked).is_err() {
            panic!("entity {entity_id} is already tracked");
        }

        // Send spawn packets to all nearby players
        for player_id in player_ids_to_notify {
            if let Some(player) = get_player(player_id) {
                send_spawn_packets(entity, &player);
            }
        }
    }

    /// Stops tracking an entity and sends despawn to all tracking players.
    pub fn remove(&self, entity_id: i32, get_player: impl Fn(i32) -> Option<Arc<Player>>) {
        if let Some((_, tracked)) = self.entities.remove_sync(&entity_id) {
            // Send despawn to all tracking players
            for player_id in tracked.seen_by.read().iter() {
                if let Some(player) = get_player(*player_id) {
                    player.send_packet(CRemoveEntities::single(entity_id));
                }
            }
        }
    }

    /// Refreshes the tracked-entity set for one player.
    ///
    /// Mirrors vanilla `TrackedEntity.updatePlayer`: each tracked entity checks
    /// whether the player tracks the entity chunk, passes the entity-specific
    /// broadcast predicate, and is inside the effective horizontal range.
    pub fn update_player(&self, player: &Player, view: &PlayerChunkView) {
        let player_id = player.id();
        let player_pos = player.position();
        let player_view_distance = view.view_distance;

        let mut entity_ids_to_despawn = Vec::new();
        let mut entities_to_spawn = Vec::new();
        let mut dead_entities = Vec::new();

        self.entities.iter_sync(|entity_id, tracked| {
            let entity_id = *entity_id;
            let Some(entity) = tracked.entity.upgrade() else {
                dead_entities.push(entity_id);
                return true;
            };

            let visible = !entity.is_removed()
                && entity_id != player_id
                && view.contains(tracked.registered_chunk)
                && entity.broadcast_to_player(player)
                && is_within_tracking_distance(
                    entity.position(),
                    player_pos,
                    tracked.tracking_range,
                    player_view_distance,
                );

            let mut seen_by = tracked.seen_by.write();
            if visible {
                if seen_by.insert(player_id) {
                    entities_to_spawn.push(entity);
                }
            } else if seen_by.remove(&player_id) {
                entity_ids_to_despawn.push(entity_id);
            }

            true
        });

        for entity_id in entity_ids_to_despawn {
            player.send_packet(CRemoveEntities::single(entity_id));
        }

        for entity in entities_to_spawn {
            send_spawn_packets(&entity, player);
        }

        // Clean up dead entities we encountered
        for entity_id in dead_entities {
            self.remove_dead_entity(entity_id);
        }
    }

    /// Called when a player leaves - removes them from all entity tracking.
    pub fn on_player_leave(&self, player_id: i32) {
        // We need to iterate all entities to remove this player
        // This is acceptable since player leave is infrequent
        let mut dead_entities = Vec::new();

        self.entities.iter_sync(|entity_id, tracked| {
            tracked.seen_by.write().remove(&player_id);
            if tracked.entity.strong_count() == 0 {
                dead_entities.push(*entity_id);
            }
            true // continue iteration
        });

        // Clean up any dead entities we found
        for entity_id in dead_entities {
            self.remove_dead_entity(entity_id);
        }
    }

    /// Updates an entity's current chunk and visible players after a section move.
    ///
    /// Vanilla refreshes tracked players when an entity's section position changes.
    /// The old and new chunks may be the same for purely vertical section moves.
    pub fn on_entity_section_change(
        &self,
        entity_id: i32,
        old_chunk: ChunkPos,
        new_chunk: ChunkPos,
        get_players_in_chunk: impl Fn(ChunkPos) -> Vec<i32>,
        get_player: impl Fn(i32) -> Option<Arc<Player>>,
    ) {
        let mut players_to_remove = Vec::new();
        let mut players_to_add = Vec::new();
        let mut entity_to_spawn = None;

        self.entities.update_sync(&entity_id, |_, tracked| {
            if old_chunk != new_chunk {
                tracked.registered_chunk = new_chunk;
            }

            let Some(entity) = tracked.entity.upgrade() else {
                return;
            };

            let new_seen_by = Self::visible_players_for_entity(
                entity_id,
                entity.as_ref(),
                new_chunk,
                tracked.tracking_range,
                &get_players_in_chunk,
                &get_player,
            );

            let mut seen_by = tracked.seen_by.write();
            players_to_remove.extend(seen_by.difference(&new_seen_by).copied());
            players_to_add.extend(new_seen_by.difference(&seen_by).copied());
            *seen_by = new_seen_by;
            entity_to_spawn = Some(entity);
        });

        for player_id in players_to_remove {
            if let Some(player) = get_player(player_id) {
                player.send_packet(CRemoveEntities::single(entity_id));
            }
        }

        let Some(entity) = entity_to_spawn else {
            return;
        };
        for player_id in players_to_add {
            if let Some(player) = get_player(player_id) {
                send_spawn_packets(&entity, &player);
            }
        }
    }

    /// Gets the number of tracked entities.
    #[must_use]
    pub fn count(&self) -> usize {
        self.entities.len()
    }

    /// Returns players currently tracking an entity.
    #[must_use]
    pub fn tracking_player_ids(&self, entity_id: i32) -> Vec<i32> {
        self.entities
            .read_sync(&entity_id, |_, tracked| {
                tracked.seen_by.read().iter().copied().collect()
            })
            .unwrap_or_default()
    }

    fn remove_dead_entity(&self, entity_id: i32) {
        // Note: We don't send despawn packets here because the players
        // will get updated via player view changes or explicit removals.
        let _ = self.entities.remove_sync(&entity_id);
    }

    fn visible_players_for_entity(
        entity_id: i32,
        entity: &dyn Entity,
        entity_chunk: ChunkPos,
        tracking_range: EntityTrackingRange,
        get_players_in_chunk: &impl Fn(ChunkPos) -> Vec<i32>,
        get_player: &impl Fn(i32) -> Option<Arc<Player>>,
    ) -> FxHashSet<i32> {
        let entity_pos = entity.position();
        let mut players = FxHashSet::default();
        if entity.is_removed() {
            return players;
        }

        for player_id in get_players_in_chunk(entity_chunk) {
            if player_id == entity_id {
                continue;
            }

            let Some(player) = get_player(player_id) else {
                continue;
            };

            if entity.broadcast_to_player(&player)
                && is_within_tracking_distance(
                    entity_pos,
                    player.position(),
                    tracking_range,
                    player.view_distance(),
                )
            {
                players.insert(player_id);
            }
        }

        players
    }
}

fn is_within_tracking_distance(
    entity_pos: DVec3,
    player_pos: DVec3,
    tracking_range: EntityTrackingRange,
    player_view_distance: u8,
) -> bool {
    let visible_radius = tracking_range.visible_radius(player_view_distance);
    let x = player_pos.x - entity_pos.x;
    let z = player_pos.z - entity_pos.z;
    x * x + z * z <= visible_radius * visible_radius
}

/// Sends spawn packets for an entity to a player.
///
/// Uses packet bundling to ensure all spawn-related packets (add entity, metadata, etc.)
/// are processed atomically by the client in a single tick.
fn send_spawn_packets(entity: &SharedEntity, player: &Player) {
    let pos = entity.position();
    let vel = entity.velocity();
    let (yaw, pitch) = entity.rotation();
    let entity_type_id = entity.entity_type().id() as i32;

    // Convert rotation from degrees to protocol byte format (256th of a full rotation)
    // Uses to_angle_byte which matches vanilla's Mth.packDegrees
    let x_rot = to_angle_byte(pitch);
    let y_rot = to_angle_byte(yaw);

    let spawn_packet = CAddEntity {
        id: entity.id(),
        uuid: entity.uuid(),
        entity_type: entity_type_id,
        x: pos.x,
        y: pos.y,
        z: pos.z,
        velocity_x: vel.x,
        velocity_y: vel.y,
        velocity_z: vel.z,
        x_rot,
        y_rot,
        head_y_rot: y_rot,
        data: entity.spawn_data(),
    };

    // Collect entity data before entering the bundle closure
    let entity_data = entity.pack_all_entity_data();
    let entity_id = entity.id();

    // Send all spawn packets in a bundle so client processes them atomically
    player.send_bundle(|bundle| {
        bundle.add(spawn_packet);

        // Send entity data if any
        if !entity_data.is_empty() {
            bundle.add(CSetEntityData::new(entity_id, entity_data));
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn client_tracking_range_is_converted_to_blocks() {
        let range = EntityTrackingRange::from_client_chunk_range(4);

        assert!((range.visible_radius(10) - 64.0).abs() < f64::EPSILON);
    }

    #[test]
    fn zero_client_tracking_range_disables_tracking() {
        let range = EntityTrackingRange::from_client_chunk_range(0);

        assert!(range.is_disabled());
    }

    #[test]
    fn tracking_distance_uses_horizontal_circle() {
        let range = EntityTrackingRange::from_client_chunk_range(4);
        let entity_pos = DVec3::ZERO;

        assert!(is_within_tracking_distance(
            entity_pos,
            DVec3::new(64.0, 300.0, 0.0),
            range,
            8,
        ));
        assert!(!is_within_tracking_distance(
            entity_pos,
            DVec3::new(64.0, 0.0, 64.0),
            range,
            8,
        ));
        assert!(!is_within_tracking_distance(
            entity_pos,
            DVec3::new(64.1, 0.0, 0.0),
            range,
            8,
        ));
    }

    #[test]
    fn tracking_distance_is_capped_by_player_view_distance() {
        let range = EntityTrackingRange::from_client_chunk_range(10);
        let entity_pos = DVec3::ZERO;

        assert!(is_within_tracking_distance(
            entity_pos,
            DVec3::new(32.0, 0.0, 0.0),
            range,
            2,
        ));
        assert!(!is_within_tracking_distance(
            entity_pos,
            DVec3::new(32.1, 0.0, 0.0),
            range,
            2,
        ));
    }
}
