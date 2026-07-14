//! Entity tracking system for managing which players can see which entities.
//!
//! Keeps the vanilla visibility predicate in block space. Vanilla stores an
//! entity tracking range as client chunks, multiplies it by 16, caps it by the
//! player's view distance, and then checks horizontal squared distance.

use std::sync::Arc;

use glam::DVec3;
use rustc_hash::FxHashSet;
use steel_protocol::packets::game::{
    AttributeSnapshot, CAddEntity, CRemoveEntities, CSetEntityData, CSetEntityLink,
    CSetEntityMotion, CSetEquipment, CSetPassengers, CUpdateAttributes, EquipmentSlotItem,
    to_angle_byte,
};
use steel_registry::entity_data::DataValue;
use steel_registry::{RegistryEntry, vanilla_entities};
use steel_utils::ChunkPos;
use steel_utils::locks::{SyncMutex, SyncRwLock};

use crate::chunk::player_chunk_view::PlayerChunkView;
use crate::entity::{
    Entity, EntityMovementSyncPacket, MobEffectSyncPacket, ServerEntityMovementSyncState,
    ServerEntityMovementSyncUpdate, SharedEntity, WeakEntity,
};
use crate::player::Player;

const BLOCKS_PER_CHUNK: f64 = 16.0;

/// World-level entity tracker.
pub struct EntityTracker {
    /// Maps entity ID to its tracking data.
    entities: scc::HashMap<i32, TrackedEntity>,
}

/// Packet sinks used by [`EntityTracker::send_changes`].
pub struct EntityChangeSenders<
    Movement,
    SelfMovement,
    EntityData,
    Attributes,
    MobEffects,
    Equipment,
    Passengers,
    EntityLink,
> {
    /// Broadcasts movement and velocity sync packets.
    pub movement: Movement,
    /// Sends vanilla self-inclusive hurt motion sync to one player.
    pub self_movement: SelfMovement,
    /// Broadcasts entity data watcher changes.
    pub entity_data: EntityData,
    /// Broadcasts dirty syncable attributes.
    pub attributes: Attributes,
    /// Sends mob-effect add/remove packets to a specific player.
    pub mob_effects: MobEffects,
    /// Broadcasts dirty equipment slots.
    pub equipment: Equipment,
    /// Sends passenger updates to a specific player.
    pub passengers: Passengers,
    /// Broadcasts leash/link holder updates.
    pub entity_link: EntityLink,
}

/// Tracking data for a single entity.
struct TrackedEntity {
    /// Weak reference to the entity. When this fails to upgrade, entity is dead.
    entity: WeakEntity,
    /// Vanilla `ServerEntity` movement sync state owned by the tracker.
    server_entity: SyncMutex<ServerEntityMovementSyncState>,
    /// Last direct passenger ids sent to tracking clients.
    last_passenger_ids: SyncMutex<Vec<i32>>,
    /// Last leash holder id sent to tracking clients.
    last_leash_holder_id: SyncMutex<Option<i32>>,
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
    ///
    /// # Panics
    /// Panics if the entity is removed or is already tracked.
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

        let tracked_entity = TrackedEntity {
            entity: Arc::downgrade(entity),
            server_entity: SyncMutex::new(ServerEntityMovementSyncState::new(
                pos,
                entity.velocity(),
                entity.on_ground(),
                entity.rotation(),
                entity.head_yaw(),
                entity.entity_type().update_interval,
                entity.entity_type().track_deltas,
            )),
            last_passenger_ids: SyncMutex::new(self.direct_tracked_passenger_ids(entity.as_ref())),
            last_leash_holder_id: SyncMutex::new(leash_holder_id(entity.as_ref())),
            tracking_range,
            registered_chunk,
            seen_by: SyncRwLock::new(players_to_notify),
        };

        assert!(
            self.entities.insert_sync(entity_id, tracked_entity).is_ok(),
            "entity {entity_id} is already tracked"
        );

        // Send spawn packets to all nearby players
        for player_id in player_ids_to_notify {
            if let Some(player) = get_player(player_id) {
                self.send_spawn_packets(entity, &player);
            }
        }
    }

    /// Stops tracking an entity and sends despawn to all tracking players.
    pub fn remove(&self, entity_id: i32, get_player: impl Fn(i32) -> Option<Arc<Player>>) {
        if let Some((_, tracked)) = self.entities.remove_sync(&entity_id) {
            let entity = tracked.entity.upgrade();
            // Send despawn to all tracking players
            for player_id in tracked.seen_by.read().iter() {
                if let Some(player) = get_player(*player_id) {
                    if let Some(entity) = &entity
                        && let Some(packet) =
                            self.vehicle_passenger_packet_for_player(entity.as_ref(), *player_id)
                    {
                        player.send_packet(packet);
                    }
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
    pub fn update_player(
        &self,
        player: &Player,
        view: &PlayerChunkView,
        is_chunk_sent: impl Fn(ChunkPos) -> bool,
    ) {
        let player_id = player.id();
        let player_pos = player.position();
        let player_view_distance = view.view_distance;

        let mut entities_to_despawn = Vec::new();
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
                && is_chunk_sent(tracked.registered_chunk)
                && entity.broadcast_to_player(player)
                && is_within_tracking_distance(
                    entity.position(),
                    player_pos,
                    effective_tracking_range(entity.as_ref(), tracked.tracking_range),
                    player_view_distance,
                );

            let mut despawn = false;
            {
                let mut seen_by = tracked.seen_by.write();
                if visible {
                    if seen_by.insert(player_id) {
                        entities_to_spawn.push(entity.clone());
                    }
                } else if seen_by.remove(&player_id) {
                    despawn = true;
                }
            }

            if despawn {
                let vehicle_packet =
                    self.vehicle_passenger_packet_for_player(entity.as_ref(), player_id);
                entities_to_despawn.push((entity_id, vehicle_packet));
            }

            true
        });

        for (entity_id, vehicle_packet) in entities_to_despawn {
            if let Some(packet) = vehicle_packet {
                player.send_packet(packet);
            }
            player.send_packet(CRemoveEntities::single(entity_id));
        }

        for entity in entities_to_spawn {
            self.send_spawn_packets(&entity, player);
        }

        // Clean up dead entities we encountered
        for entity_id in dead_entities {
            self.remove_dead_entity(entity_id);
        }
    }

    /// Sends tracker-owned movement changes for all tracked entities.
    ///
    /// Mirrors vanilla `ChunkMap.tick` driving `ServerEntity.sendChanges`.
    #[expect(
        clippy::too_many_lines,
        reason = "entity tracker fanout mirrors vanilla sendChanges ordering"
    )]
    pub fn send_changes<
        Movement,
        SelfMovement,
        EntityData,
        Attributes,
        MobEffects,
        Equipment,
        Passengers,
        EntityLink,
    >(
        &self,
        get_players_in_chunk: impl Fn(ChunkPos) -> Vec<i32>,
        get_player: impl Fn(i32) -> Option<Arc<Player>>,
        mut senders: EntityChangeSenders<
            Movement,
            SelfMovement,
            EntityData,
            Attributes,
            MobEffects,
            Equipment,
            Passengers,
            EntityLink,
        >,
    ) where
        Movement: FnMut(i32, EntityMovementSyncPacket),
        SelfMovement: FnMut(i32, EntityMovementSyncPacket),
        EntityData: FnMut(i32, Vec<DataValue>),
        Attributes: FnMut(i32, Vec<AttributeSnapshot>),
        MobEffects: FnMut(i32, MobEffectSyncPacket),
        Equipment: FnMut(i32, CSetEquipment),
        Passengers: FnMut(i32, CSetPassengers),
        EntityLink: FnMut(i32, CSetEntityLink),
    {
        let mut dead_entities = Vec::new();
        let mut entities_to_refresh = Vec::new();
        let mut passenger_packets_to_send = Vec::new();
        let mut packets_to_broadcast = Vec::new();
        let mut self_movement_packets = Vec::new();
        let mut entity_data_to_broadcast = Vec::new();
        let mut attributes_to_broadcast = Vec::new();
        let mut mob_effect_packets_to_send = Vec::new();
        let mut equipment_to_broadcast = Vec::new();
        let mut entity_links_to_broadcast = Vec::new();

        self.entities.iter_sync(|entity_id, tracked| {
            let entity_id = *entity_id;
            let Some(entity) = tracked.entity.upgrade() else {
                dead_entities.push(entity_id);
                return true;
            };

            if entity.is_removed() {
                return true;
            }

            entity.update_data_before_sync();

            let passenger_ids = self.direct_tracked_passenger_ids(entity.as_ref());
            {
                let mut last_passenger_ids = tracked.last_passenger_ids.lock();
                if *last_passenger_ids != passenger_ids {
                    let changed_player_passenger_ids = direct_player_passenger_delta(
                        &last_passenger_ids,
                        &passenger_ids,
                        &get_player,
                    );
                    let seen_by = tracked.seen_by.read();
                    for player_id in seen_by.iter() {
                        if changed_player_passenger_ids.contains(player_id) {
                            continue;
                        }
                        passenger_packets_to_send.push((
                            *player_id,
                            CSetPassengers::new(
                                entity_id,
                                self.direct_passenger_ids_seen_by_player(
                                    entity.as_ref(),
                                    *player_id,
                                ),
                            ),
                        ));
                    }
                    *last_passenger_ids = passenger_ids;
                    entities_to_refresh.push(entity_id);
                }
            }

            let dirty_entity_data = entity.pack_dirty_entity_data();
            let has_dirty_entity_data = dirty_entity_data.is_some();
            let result =
                tracked
                    .server_entity
                    .lock()
                    .record_send_changes(ServerEntityMovementSyncUpdate {
                        entity_id,
                        is_passenger: entity.is_passenger(),
                        position: entity.position(),
                        velocity: entity.velocity(),
                        body_rotation: entity.rotation(),
                        head_yaw: entity.head_yaw(),
                        on_ground: entity.on_ground(),
                        needs_velocity_sync: entity.needs_velocity_sync(),
                        has_dirty_entity_data,
                        force_velocity_sync: entity.forces_fall_flying_velocity_sync(),
                    });
            if result.should_clear_velocity_sync() {
                entity.clear_velocity_sync();
            }
            result.for_each_packet(|packet| packets_to_broadcast.push((entity_id, packet)));
            if entity.hurt_marked() {
                let velocity = entity.velocity();
                packets_to_broadcast.push((
                    entity_id,
                    EntityMovementSyncPacket::from(CSetEntityMotion::new(entity_id, velocity)),
                ));
                if entity.entity_type() == &vanilla_entities::PLAYER {
                    self_movement_packets.push((
                        entity_id,
                        EntityMovementSyncPacket::from(CSetEntityMotion::new(entity_id, velocity)),
                    ));
                }
                entity.clear_hurt_mark();
            }
            if let Some(dirty_entity_data) = dirty_entity_data {
                entity_data_to_broadcast.push((entity_id, dirty_entity_data));
            }
            let dirty_attributes = entity.drain_dirty_syncable_attributes();
            if !dirty_attributes.is_empty() {
                attributes_to_broadcast.push((entity_id, dirty_attributes));
            }
            let dirty_mob_effects = entity.drain_dirty_mob_effects();
            if !dirty_mob_effects.is_empty() {
                let mut recipient_ids = FxHashSet::default();
                if entity.entity_type() == &vanilla_entities::PLAYER
                    && get_player(entity_id).is_some()
                {
                    recipient_ids.insert(entity_id);
                }
                for passenger in entity.passengers() {
                    let passenger_id = passenger.id();
                    if passenger.entity_type() == &vanilla_entities::PLAYER
                        && get_player(passenger_id).is_some()
                    {
                        recipient_ids.insert(passenger_id);
                    }
                }
                for recipient_id in recipient_ids {
                    for change in &dirty_mob_effects {
                        mob_effect_packets_to_send.push((
                            recipient_id,
                            change.packet(entity_id, recipient_id == entity_id),
                        ));
                    }
                }
            }
            let dirty_equipment = entity.drain_dirty_equipment();
            if !dirty_equipment.is_empty() {
                equipment_to_broadcast.push((entity_id, dirty_equipment));
            }

            let leash_holder_id = leash_holder_id(entity.as_ref());
            {
                let mut last_leash_holder_id = tracked.last_leash_holder_id.lock();
                if *last_leash_holder_id != leash_holder_id {
                    entity_links_to_broadcast.push((
                        entity_id,
                        CSetEntityLink::new(entity_id, leash_holder_id.unwrap_or_default()),
                    ));
                    *last_leash_holder_id = leash_holder_id;
                }
            }

            true
        });

        for entity_id in dead_entities {
            self.remove_dead_entity(entity_id);
        }

        for (player_id, packet) in passenger_packets_to_send {
            (senders.passengers)(player_id, packet);
        }

        for entity_id in entities_to_refresh {
            self.refresh_entity_players(entity_id, &get_players_in_chunk, &get_player);
        }

        for (entity_id, packet) in packets_to_broadcast {
            (senders.movement)(entity_id, packet);
        }

        for (player_id, packet) in self_movement_packets {
            (senders.self_movement)(player_id, packet);
        }

        for (entity_id, dirty_entity_data) in entity_data_to_broadcast {
            (senders.entity_data)(entity_id, dirty_entity_data);
        }

        for (entity_id, dirty_attributes) in attributes_to_broadcast {
            (senders.attributes)(entity_id, dirty_attributes);
        }

        for (player_id, packet) in mob_effect_packets_to_send {
            (senders.mob_effects)(player_id, packet);
        }

        for (entity_id, dirty_equipment) in equipment_to_broadcast {
            (senders.equipment)(entity_id, CSetEquipment::new(entity_id, dirty_equipment));
        }

        for (entity_id, packet) in entity_links_to_broadcast {
            (senders.entity_link)(entity_id, packet);
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

        let Some(entity) = entity_to_spawn else {
            return;
        };

        for player_id in players_to_remove {
            if let Some(player) = get_player(player_id) {
                if let Some(packet) =
                    self.vehicle_passenger_packet_for_player(entity.as_ref(), player_id)
                {
                    player.send_packet(packet);
                }
                player.send_packet(CRemoveEntities::single(entity_id));
            }
        }

        for player_id in players_to_add {
            if let Some(player) = get_player(player_id) {
                self.send_spawn_packets(&entity, &player);
            }
        }
    }

    fn refresh_entity_players(
        &self,
        entity_id: i32,
        get_players_in_chunk: &impl Fn(ChunkPos) -> Vec<i32>,
        get_player: &impl Fn(i32) -> Option<Arc<Player>>,
    ) {
        let mut players_to_remove = Vec::new();
        let mut players_to_add = Vec::new();
        let mut entity_to_spawn = None;

        self.entities.update_sync(&entity_id, |_, tracked| {
            let Some(entity) = tracked.entity.upgrade() else {
                return;
            };

            let new_seen_by = Self::visible_players_for_entity(
                entity_id,
                entity.as_ref(),
                tracked.registered_chunk,
                tracked.tracking_range,
                get_players_in_chunk,
                get_player,
            );

            let mut seen_by = tracked.seen_by.write();
            players_to_remove.extend(seen_by.difference(&new_seen_by).copied());
            players_to_add.extend(new_seen_by.difference(&seen_by).copied());
            *seen_by = new_seen_by;
            entity_to_spawn = Some(entity);
        });

        let Some(entity) = entity_to_spawn else {
            return;
        };

        for player_id in players_to_remove {
            if let Some(player) = get_player(player_id) {
                if let Some(packet) =
                    self.vehicle_passenger_packet_for_player(entity.as_ref(), player_id)
                {
                    player.send_packet(packet);
                }
                player.send_packet(CRemoveEntities::single(entity_id));
            }
        }

        for player_id in players_to_add {
            if let Some(player) = get_player(player_id) {
                self.send_spawn_packets(&entity, &player);
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
        let tracking_range = effective_tracking_range(entity, tracking_range);
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

    fn send_spawn_packets(&self, entity: &SharedEntity, player: &Player) {
        let entity_id = entity.id();
        self.spawn_pairing(entity, player.id())
            .send_to(entity_id, player);
    }

    fn spawn_pairing(&self, entity: &SharedEntity, player_id: i32) -> EntitySpawnPairing {
        EntitySpawnPairing::from_entity(
            entity,
            self.passenger_pairing_packets_for_player(entity.as_ref(), player_id),
        )
    }

    fn passenger_pairing_packets_for_player(
        &self,
        entity: &dyn Entity,
        player_id: i32,
    ) -> Vec<CSetPassengers> {
        let mut packets = Vec::new();
        let passenger_ids = self.direct_passenger_ids_seen_by_player(entity, player_id);
        if !passenger_ids.is_empty() {
            packets.push(CSetPassengers::new(entity.id(), passenger_ids));
        }

        if let Some(vehicle) = entity.vehicle()
            && self.entity_seen_by_player(vehicle.id(), player_id)
        {
            packets.push(CSetPassengers::new(
                vehicle.id(),
                self.direct_passenger_ids_seen_by_player(vehicle.as_ref(), player_id),
            ));
        }

        packets
    }

    fn vehicle_passenger_packet_for_player(
        &self,
        entity: &dyn Entity,
        player_id: i32,
    ) -> Option<CSetPassengers> {
        let vehicle = entity.vehicle()?;
        if !self.entity_seen_by_player(vehicle.id(), player_id) {
            return None;
        }
        Some(CSetPassengers::new(
            vehicle.id(),
            self.direct_passenger_ids_seen_by_player(vehicle.as_ref(), player_id),
        ))
    }

    fn direct_tracked_passenger_ids(&self, entity: &dyn Entity) -> Vec<i32> {
        entity
            .passengers()
            .into_iter()
            .filter(|passenger| self.is_entity_tracked(passenger.id()))
            .map(|passenger| passenger.id())
            .collect()
    }

    fn direct_passenger_ids_seen_by_player(&self, entity: &dyn Entity, player_id: i32) -> Vec<i32> {
        entity
            .passengers()
            .into_iter()
            .filter(|passenger| {
                passenger.id() == player_id || self.entity_seen_by_player(passenger.id(), player_id)
            })
            .map(|passenger| passenger.id())
            .collect()
    }

    fn is_entity_tracked(&self, entity_id: i32) -> bool {
        self.entities.read_sync(&entity_id, |_, _| ()).is_some()
    }

    fn entity_seen_by_player(&self, entity_id: i32, player_id: i32) -> bool {
        self.entities
            .read_sync(&entity_id, |_, tracked| {
                tracked.seen_by.read().contains(&player_id)
            })
            .unwrap_or(false)
    }
}

fn leash_holder_id(entity: &dyn Entity) -> Option<i32> {
    entity
        .as_mob()
        .and_then(super::mob::Mob::leash_holder)
        .map(|holder| holder.id())
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

fn effective_tracking_range(
    entity: &dyn Entity,
    base_range: EntityTrackingRange,
) -> EntityTrackingRange {
    let mut range = base_range;
    let mut visited = FxHashSet::default();
    visited.insert(entity.id());
    add_passenger_tracking_ranges(entity, &mut range, &mut visited);
    range
}

fn add_passenger_tracking_ranges(
    entity: &dyn Entity,
    range: &mut EntityTrackingRange,
    visited: &mut FxHashSet<i32>,
) {
    for passenger in entity.passengers() {
        if !visited.insert(passenger.id()) {
            continue;
        }
        let passenger_range = EntityTrackingRange::from_client_chunk_range(
            passenger.entity_type().client_tracking_range,
        );
        range.block_radius = range.block_radius.max(passenger_range.block_radius);
        add_passenger_tracking_ranges(passenger.as_ref(), range, visited);
    }
}

struct EntitySpawnPairing {
    spawn_packet: CAddEntity,
    entity_data: Vec<DataValue>,
    attributes: Vec<AttributeSnapshot>,
    equipment: Vec<EquipmentSlotItem>,
    passenger_packets: Vec<CSetPassengers>,
    entity_link_packet: Option<CSetEntityLink>,
}

impl EntitySpawnPairing {
    fn from_entity(entity: &SharedEntity, passenger_packets: Vec<CSetPassengers>) -> Self {
        entity.update_data_before_sync();

        let pos = entity.spawn_position();
        let vel = entity.velocity();
        let (yaw, pitch) = entity.rotation();
        let head_yaw = entity.head_yaw();
        let entity_type_id = entity.entity_type().id() as i32;

        // Convert rotation from degrees to protocol byte format (256th of a full rotation)
        // Uses to_angle_byte which matches vanilla's Mth.packDegrees
        let x_rot = to_angle_byte(pitch);
        let y_rot = to_angle_byte(yaw);
        let head_y_rot = to_angle_byte(head_yaw);

        Self {
            spawn_packet: CAddEntity {
                id: entity.id(),
                uuid: entity.uuid(),
                entity_type: entity_type_id,
                position: pos,
                velocity: vel,
                x_rot,
                y_rot,
                head_y_rot,
                data: entity.spawn_data(),
            },
            entity_data: entity.pack_all_entity_data(),
            attributes: entity.pack_syncable_attributes(),
            equipment: entity.pack_all_equipment(),
            passenger_packets,
            entity_link_packet: leash_holder_id(entity.as_ref())
                .map(|holder_id| CSetEntityLink::new(entity.id(), holder_id)),
        }
    }

    fn send_to(self, entity_id: i32, player: &Player) {
        player.send_bundle(|bundle| {
            bundle.add(self.spawn_packet);

            if !self.entity_data.is_empty() {
                bundle.add(CSetEntityData::new(entity_id, self.entity_data));
            }

            if !self.attributes.is_empty() {
                bundle.add(CUpdateAttributes::new(entity_id, self.attributes));
            }

            if !self.equipment.is_empty() {
                bundle.add(CSetEquipment::new(entity_id, self.equipment));
            }

            for packet in self.passenger_packets {
                bundle.add(packet);
            }

            if let Some(packet) = self.entity_link_packet {
                bundle.add(packet);
            }
        });
    }
}

fn direct_player_passenger_delta(
    old_passenger_ids: &[i32],
    new_passenger_ids: &[i32],
    get_player: &impl Fn(i32) -> Option<Arc<Player>>,
) -> Vec<i32> {
    let old_passenger_ids = old_passenger_ids.iter().copied().collect::<FxHashSet<_>>();
    let new_passenger_ids = new_passenger_ids.iter().copied().collect::<FxHashSet<_>>();
    old_passenger_ids
        .symmetric_difference(&new_passenger_ids)
        .copied()
        .filter(|entity_id| get_player(*entity_id).is_some())
        .collect()
}

#[cfg(test)]
mod tests {
    use std::{
        mem,
        sync::{Arc, Weak},
    };

    use steel_protocol::packets::game::{AttributeSnapshot, EquipmentSlotItem};
    use steel_registry::item_stack::ItemStack;
    use steel_registry::{
        entity_type::EntityTypeRef, test_support, vanilla_entities, vanilla_items,
    };
    use steel_utils::BlockPos;

    use super::*;
    use crate::entity::{
        EntityBase, Mob,
        entities::{LeashFenceKnotEntity, PigEntity},
    };
    use crate::inventory::equipment::EquipmentSlot;

    struct PairingTestEntity {
        base: EntityBase,
        entity_type: EntityTypeRef,
        attributes: Vec<AttributeSnapshot>,
        dirty_attributes: SyncMutex<Vec<AttributeSnapshot>>,
        equipment: SyncMutex<Vec<EquipmentSlotItem>>,
        dirty_equipment: SyncMutex<Vec<EquipmentSlotItem>>,
        passengers: SyncMutex<Vec<WeakEntity>>,
        vehicle: SyncMutex<Option<WeakEntity>>,
    }

    impl PairingTestEntity {
        fn new(id: i32, attributes: Vec<AttributeSnapshot>) -> Arc<Self> {
            Self::new_with_type(id, &vanilla_entities::ITEM, attributes)
        }

        fn new_with_type(
            id: i32,
            entity_type: EntityTypeRef,
            attributes: Vec<AttributeSnapshot>,
        ) -> Arc<Self> {
            Arc::new(Self {
                base: EntityBase::new(id, DVec3::ZERO, entity_type.dimensions, Weak::new()),
                entity_type,
                attributes,
                dirty_attributes: SyncMutex::new(Vec::new()),
                equipment: SyncMutex::new(Vec::new()),
                dirty_equipment: SyncMutex::new(Vec::new()),
                passengers: SyncMutex::new(Vec::new()),
                vehicle: SyncMutex::new(None),
            })
        }

        fn shared(attributes: Vec<AttributeSnapshot>) -> SharedEntity {
            Self::new(1, attributes)
        }

        fn add_passenger(&self, passenger: &SharedEntity) {
            self.passengers.lock().push(Arc::downgrade(passenger));
        }

        fn clear_passengers(&self) {
            self.passengers.lock().clear();
        }

        fn set_vehicle(&self, vehicle: &SharedEntity) {
            *self.vehicle.lock() = Some(Arc::downgrade(vehicle));
        }

        fn set_dirty_attributes(&self, attributes: Vec<AttributeSnapshot>) {
            *self.dirty_attributes.lock() = attributes;
        }

        fn set_equipment(&self, equipment: Vec<EquipmentSlotItem>) {
            *self.equipment.lock() = equipment;
        }

        fn set_dirty_equipment(&self, equipment: Vec<EquipmentSlotItem>) {
            *self.dirty_equipment.lock() = equipment;
        }
    }

    crate::entity::impl_test_downcast_type!(PairingTestEntity);

    impl Entity for PairingTestEntity {
        fn base(&self) -> &EntityBase {
            &self.base
        }

        fn entity_type(&self) -> EntityTypeRef {
            self.entity_type
        }

        fn pack_syncable_attributes(&self) -> Vec<AttributeSnapshot> {
            self.attributes.clone()
        }

        fn drain_dirty_syncable_attributes(&self) -> Vec<AttributeSnapshot> {
            mem::take(&mut *self.dirty_attributes.lock())
        }

        fn pack_all_equipment(&self) -> Vec<EquipmentSlotItem> {
            self.equipment.lock().clone()
        }

        fn drain_dirty_equipment(&self) -> Vec<EquipmentSlotItem> {
            mem::take(&mut *self.dirty_equipment.lock())
        }

        fn vehicle(&self) -> Option<SharedEntity> {
            self.vehicle.lock().as_ref().and_then(Weak::upgrade)
        }

        fn passengers(&self) -> Vec<SharedEntity> {
            let mut live_passengers = Vec::new();
            self.passengers.lock().retain(|passenger| {
                let Some(entity) = passenger.upgrade() else {
                    return false;
                };
                live_passengers.push(entity);
                true
            });
            live_passengers
        }
    }

    fn track_entity_for_player(tracker: &EntityTracker, entity: &SharedEntity, player_id: i32) {
        let pos = entity.position();
        let mut seen_by = FxHashSet::default();
        seen_by.insert(player_id);
        let tracked_entity = TrackedEntity {
            entity: Arc::downgrade(entity),
            server_entity: SyncMutex::new(ServerEntityMovementSyncState::new(
                pos,
                entity.velocity(),
                entity.on_ground(),
                entity.rotation(),
                entity.head_yaw(),
                entity.entity_type().update_interval,
                entity.entity_type().track_deltas,
            )),
            last_passenger_ids: SyncMutex::new(
                tracker.direct_tracked_passenger_ids(entity.as_ref()),
            ),
            last_leash_holder_id: SyncMutex::new(leash_holder_id(entity.as_ref())),
            tracking_range: EntityTrackingRange::from_client_chunk_range(
                entity.entity_type().client_tracking_range,
            ),
            registered_chunk: ChunkPos::from_entity_pos(pos),
            seen_by: SyncRwLock::new(seen_by),
        };
        assert!(
            tracker
                .entities
                .insert_sync(entity.id(), tracked_entity)
                .is_ok()
        );
    }

    fn mark_seen_by_player(tracker: &EntityTracker, entity_id: i32, player_id: i32) {
        tracker.entities.update_sync(&entity_id, |_, tracked| {
            tracked.seen_by.write().insert(player_id);
        });
    }

    fn assert_has_velocity_packet(
        updates: &[(i32, EntityMovementSyncPacket)],
        entity_id: i32,
        velocity: DVec3,
    ) {
        let has_packet = updates.iter().any(|(sent_entity_id, packet)| {
            let EntityMovementSyncPacket::Velocity(packet) = packet else {
                return false;
            };
            *sent_entity_id == entity_id
                && packet.entity_id == entity_id
                && packet.vel.x.to_bits() == velocity.x.to_bits()
                && packet.vel.y.to_bits() == velocity.y.to_bits()
                && packet.vel.z.to_bits() == velocity.z.to_bits()
        });
        assert!(
            has_packet,
            "expected velocity packet for entity {entity_id} with velocity {velocity:?}, got {updates:?}"
        );
    }

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

    #[test]
    fn vehicle_effective_tracking_range_uses_widest_passenger_range() {
        test_support::init_test_registry();

        let vehicle_typed =
            PairingTestEntity::new_with_type(1, &vanilla_entities::ITEM, Vec::new());
        let passenger_typed =
            PairingTestEntity::new_with_type(2, &vanilla_entities::PLAYER, Vec::new());
        assert!(
            passenger_typed.entity_type().client_tracking_range
                > vehicle_typed.entity_type().client_tracking_range
        );

        let passenger: SharedEntity = passenger_typed;
        vehicle_typed.add_passenger(&passenger);
        let vehicle: SharedEntity = vehicle_typed;
        let base_range = EntityTrackingRange::from_client_chunk_range(
            vehicle.entity_type().client_tracking_range,
        );

        let effective = effective_tracking_range(vehicle.as_ref(), base_range);

        assert_eq!(
            effective.block_radius.to_bits(),
            (f64::from(passenger.entity_type().client_tracking_range) * BLOCKS_PER_CHUNK).to_bits()
        );
    }

    #[test]
    fn spawn_pairing_includes_syncable_attributes() {
        test_support::init_test_registry();

        let entity = PairingTestEntity::shared(vec![AttributeSnapshot {
            attribute_id: 7,
            base_value: 1.25,
            modifiers: Vec::new(),
        }]);
        let pairing = EntitySpawnPairing::from_entity(&entity, Vec::new());

        assert_eq!(pairing.spawn_packet.id, entity.id());
        assert_eq!(pairing.attributes.len(), 1);
        assert_eq!(pairing.attributes[0].attribute_id, 7);
        assert_eq!(
            pairing.attributes[0].base_value.to_bits(),
            1.25_f64.to_bits()
        );
    }

    #[test]
    fn spawn_pairing_includes_non_empty_equipment() {
        test_support::init_test_registry();

        let entity_typed = PairingTestEntity::new(1, Vec::new());
        let stack = ItemStack::new(&vanilla_items::ITEMS.elytra);
        entity_typed.set_equipment(vec![EquipmentSlotItem {
            slot: EquipmentSlot::Chest,
            item_stack: stack.clone(),
        }]);
        let entity: SharedEntity = entity_typed;
        let pairing = EntitySpawnPairing::from_entity(&entity, Vec::new());

        assert_eq!(pairing.spawn_packet.id, entity.id());
        assert_eq!(pairing.equipment.len(), 1);
        assert_eq!(pairing.equipment[0].slot, EquipmentSlot::Chest);
        assert_eq!(pairing.equipment[0].item_stack, stack);
    }

    #[test]
    fn spawn_pairing_uses_entity_spawn_packet_position() {
        test_support::init_test_registry();

        let entity: SharedEntity = Arc::new(LeashFenceKnotEntity::new_attached(
            &vanilla_entities::LEASH_KNOT,
            1,
            BlockPos::new(4, 65, -9),
            Weak::new(),
        ));
        let pairing = EntitySpawnPairing::from_entity(&entity, Vec::new());

        assert_eq!(pairing.spawn_packet.position.x.to_bits(), 4.0_f64.to_bits());
        assert_eq!(
            pairing.spawn_packet.position.y.to_bits(),
            65.0_f64.to_bits()
        );
        assert_eq!(
            pairing.spawn_packet.position.z.to_bits(),
            (-9.0_f64).to_bits()
        );
    }

    #[test]
    fn send_changes_broadcasts_dirty_attributes_once() {
        test_support::init_test_registry();

        let tracker = EntityTracker::new();
        let entity_typed = PairingTestEntity::new(1, Vec::new());
        let entity: SharedEntity = entity_typed.clone();
        tracker.add(&entity, |_| Vec::new(), |_| None);

        entity_typed.set_dirty_attributes(vec![AttributeSnapshot {
            attribute_id: 7,
            base_value: 2.5,
            modifiers: Vec::new(),
        }]);

        let mut updates = Vec::new();
        tracker.send_changes(
            |_| Vec::new(),
            |_| None,
            EntityChangeSenders {
                movement: |_, _| {},
                self_movement: |_, _| {},
                entity_data: |_, _| {},
                attributes: |entity_id, attributes| updates.push((entity_id, attributes)),
                mob_effects: |_, _| {},
                equipment: |_, _| {},
                passengers: |_, _| {},
                entity_link: |_, _| {},
            },
        );

        assert_eq!(updates.len(), 1);
        assert_eq!(updates[0].0, 1);
        assert_eq!(updates[0].1.len(), 1);
        assert_eq!(updates[0].1[0].attribute_id, 7);
        assert_eq!(updates[0].1[0].base_value.to_bits(), 2.5_f64.to_bits());

        updates.clear();
        tracker.send_changes(
            |_| Vec::new(),
            |_| None,
            EntityChangeSenders {
                movement: |_, _| {},
                self_movement: |_, _| {},
                entity_data: |_, _| {},
                attributes: |entity_id, attributes| updates.push((entity_id, attributes)),
                mob_effects: |_, _| {},
                equipment: |_, _| {},
                passengers: |_, _| {},
                entity_link: |_, _| {},
            },
        );
        assert!(updates.is_empty());
    }

    #[test]
    fn send_changes_broadcasts_dirty_equipment_once() {
        test_support::init_test_registry();

        let tracker = EntityTracker::new();
        let entity_typed = PairingTestEntity::new(1, Vec::new());
        let entity: SharedEntity = entity_typed.clone();
        tracker.add(&entity, |_| Vec::new(), |_| None);

        let stack = ItemStack::new(&vanilla_items::ITEMS.elytra);
        entity_typed.set_dirty_equipment(vec![EquipmentSlotItem {
            slot: EquipmentSlot::Chest,
            item_stack: stack.clone(),
        }]);

        let mut updates = Vec::new();
        tracker.send_changes(
            |_| Vec::new(),
            |_| None,
            EntityChangeSenders {
                movement: |_, _| {},
                self_movement: |_, _| {},
                entity_data: |_, _| {},
                attributes: |_, _| {},
                mob_effects: |_, _| {},
                equipment: |entity_id, packet| updates.push((entity_id, packet)),
                passengers: |_, _| {},
                entity_link: |_, _| {},
            },
        );

        assert_eq!(updates.len(), 1);
        assert_eq!(updates[0].0, 1);
        assert_eq!(updates[0].1.entity_id, 1);
        assert_eq!(updates[0].1.slots.len(), 1);
        assert_eq!(updates[0].1.slots[0].slot, EquipmentSlot::Chest);
        assert_eq!(updates[0].1.slots[0].item_stack, stack);

        updates.clear();
        tracker.send_changes(
            |_| Vec::new(),
            |_| None,
            EntityChangeSenders {
                movement: |_, _| {},
                self_movement: |_, _| {},
                entity_data: |_, _| {},
                attributes: |_, _| {},
                mob_effects: |_, _| {},
                equipment: |entity_id, packet| updates.push((entity_id, packet)),
                passengers: |_, _| {},
                entity_link: |_, _| {},
            },
        );
        assert!(updates.is_empty());
    }

    #[test]
    fn send_changes_syncs_hurt_marked_player_motion_to_self() {
        test_support::init_test_registry();

        let tracker = EntityTracker::new();
        let entity_typed =
            PairingTestEntity::new_with_type(1, &vanilla_entities::PLAYER, Vec::new());
        let entity: SharedEntity = entity_typed.clone();
        track_entity_for_player(&tracker, &entity, 99);

        entity_typed.set_velocity(DVec3::new(0.25, 0.4, -0.125));
        entity_typed.mark_hurt();

        let mut tracker_updates = Vec::new();
        let mut self_updates = Vec::new();
        tracker.send_changes(
            |_| Vec::new(),
            |_| None,
            EntityChangeSenders {
                movement: |entity_id, packet| tracker_updates.push((entity_id, packet)),
                self_movement: |player_id, packet| self_updates.push((player_id, packet)),
                entity_data: |_, _| {},
                attributes: |_, _| {},
                mob_effects: |_, _| {},
                equipment: |_, _| {},
                passengers: |_, _| {},
                entity_link: |_, _| {},
            },
        );

        assert_has_velocity_packet(&tracker_updates, 1, DVec3::new(0.25, 0.4, -0.125));

        assert_eq!(self_updates.len(), 1);
        assert_eq!(self_updates[0].0, 1);
        let EntityMovementSyncPacket::Velocity(packet) = &self_updates[0].1 else {
            panic!(
                "expected velocity self-motion packet, got {:?}",
                self_updates[0].1
            );
        };
        assert_eq!(packet.entity_id, 1);
        assert_eq!(packet.vel.x.to_bits(), 0.25_f64.to_bits());
        assert_eq!(packet.vel.y.to_bits(), 0.4_f64.to_bits());
        assert_eq!(packet.vel.z.to_bits(), (-0.125_f64).to_bits());
        assert!(!entity_typed.hurt_marked());
    }

    #[test]
    fn send_changes_broadcasts_hurt_marked_non_player_motion() {
        test_support::init_test_registry();

        let tracker = EntityTracker::new();
        let entity_typed = PairingTestEntity::new(1, Vec::new());
        let entity: SharedEntity = entity_typed.clone();
        track_entity_for_player(&tracker, &entity, 99);

        entity_typed.set_velocity(DVec3::new(-0.25, 0.2, 0.125));
        entity_typed.mark_hurt();

        let mut tracker_updates = Vec::new();
        let mut self_updates = Vec::new();
        tracker.send_changes(
            |_| Vec::new(),
            |_| None,
            EntityChangeSenders {
                movement: |entity_id, packet| tracker_updates.push((entity_id, packet)),
                self_movement: |player_id, packet| self_updates.push((player_id, packet)),
                entity_data: |_, _| {},
                attributes: |_, _| {},
                mob_effects: |_, _| {},
                equipment: |_, _| {},
                passengers: |_, _| {},
                entity_link: |_, _| {},
            },
        );

        assert_has_velocity_packet(&tracker_updates, 1, DVec3::new(-0.25, 0.2, 0.125));
        assert!(self_updates.is_empty());
        assert!(!entity_typed.hurt_marked());
    }

    #[test]
    fn spawn_pairing_omits_untracked_passenger_for_vehicle() {
        test_support::init_test_registry();

        let tracker = EntityTracker::new();
        let vehicle_typed = PairingTestEntity::new(1, Vec::new());
        let passenger_typed = PairingTestEntity::new(2, Vec::new());
        let passenger: SharedEntity = passenger_typed;
        vehicle_typed.add_passenger(&passenger);
        let vehicle: SharedEntity = vehicle_typed;

        let pairing = tracker.spawn_pairing(&vehicle, 99);

        assert!(pairing.passenger_packets.is_empty());
    }

    #[test]
    fn spawn_pairing_includes_tracked_passenger_packet_for_vehicle() {
        test_support::init_test_registry();

        let tracker = EntityTracker::new();
        let vehicle_typed = PairingTestEntity::new(1, Vec::new());
        let passenger_typed = PairingTestEntity::new(2, Vec::new());
        let passenger: SharedEntity = passenger_typed;
        vehicle_typed.add_passenger(&passenger);
        track_entity_for_player(&tracker, &passenger, 99);
        let vehicle: SharedEntity = vehicle_typed;

        let pairing = tracker.spawn_pairing(&vehicle, 99);

        assert_eq!(pairing.passenger_packets.len(), 1);
        assert_eq!(pairing.passenger_packets[0].vehicle_id, 1);
        assert_eq!(pairing.passenger_packets[0].passenger_ids, vec![2]);
    }

    #[test]
    fn spawn_pairing_for_passenger_omits_untracked_vehicle_packet() {
        test_support::init_test_registry();

        let tracker = EntityTracker::new();
        let vehicle_typed = PairingTestEntity::new(1, Vec::new());
        let passenger_typed = PairingTestEntity::new(2, Vec::new());
        let passenger: SharedEntity = passenger_typed.clone();
        vehicle_typed.add_passenger(&passenger);
        let vehicle: SharedEntity = vehicle_typed;
        passenger_typed.set_vehicle(&vehicle);

        let pairing = tracker.spawn_pairing(&passenger, 99);

        assert!(pairing.passenger_packets.is_empty());
    }

    #[test]
    fn spawn_pairing_for_passenger_includes_tracked_vehicle_passenger_packet() {
        test_support::init_test_registry();

        let tracker = EntityTracker::new();
        let vehicle_typed = PairingTestEntity::new(1, Vec::new());
        let passenger_typed = PairingTestEntity::new(2, Vec::new());
        let passenger: SharedEntity = passenger_typed.clone();
        vehicle_typed.add_passenger(&passenger);
        let vehicle: SharedEntity = vehicle_typed;
        passenger_typed.set_vehicle(&vehicle);
        track_entity_for_player(&tracker, &vehicle, 99);
        track_entity_for_player(&tracker, &passenger, 99);

        let pairing = tracker.spawn_pairing(&passenger, 99);

        assert_eq!(pairing.passenger_packets.len(), 1);
        assert_eq!(pairing.passenger_packets[0].vehicle_id, 1);
        assert_eq!(pairing.passenger_packets[0].passenger_ids, vec![2]);
    }

    #[test]
    fn spawn_pairing_includes_live_mob_leash_link_packet() {
        test_support::init_test_registry();

        let tracker = EntityTracker::new();
        let pig_typed = Arc::new(PigEntity::new(
            &vanilla_entities::PIG,
            1,
            DVec3::ZERO,
            Weak::new(),
        ));
        let holder: SharedEntity = PairingTestEntity::new(2, Vec::new());
        assert!(pig_typed.set_leashed_to(&holder));
        let pig: SharedEntity = pig_typed;

        let pairing = tracker.spawn_pairing(&pig, 99);

        assert_eq!(pairing.entity_link_packet, Some(CSetEntityLink::new(1, 2)));
    }

    #[test]
    fn send_changes_broadcasts_leash_link_changes_once() {
        test_support::init_test_registry();

        let tracker = EntityTracker::new();
        let pig: SharedEntity = Arc::new(PigEntity::new(
            &vanilla_entities::PIG,
            1,
            DVec3::ZERO,
            Weak::new(),
        ));
        let holder: SharedEntity = PairingTestEntity::new(2, Vec::new());
        track_entity_for_player(&tracker, &pig, 99);
        let Some(pig_mob) = pig.as_mob() else {
            panic!("pig should expose mob behavior");
        };

        let mut updates = Vec::new();
        tracker.send_changes(
            |_| Vec::new(),
            |_| None,
            EntityChangeSenders {
                movement: |_, _| {},
                self_movement: |_, _| {},
                entity_data: |_, _| {},
                attributes: |_, _| {},
                mob_effects: |_, _| {},
                equipment: |_, _| {},
                passengers: |_, _| {},
                entity_link: |entity_id, packet| updates.push((entity_id, packet)),
            },
        );
        assert!(updates.is_empty());

        assert!(pig_mob.set_leashed_to(&holder));
        tracker.send_changes(
            |_| Vec::new(),
            |_| None,
            EntityChangeSenders {
                movement: |_, _| {},
                self_movement: |_, _| {},
                entity_data: |_, _| {},
                attributes: |_, _| {},
                mob_effects: |_, _| {},
                equipment: |_, _| {},
                passengers: |_, _| {},
                entity_link: |entity_id, packet| updates.push((entity_id, packet)),
            },
        );
        assert_eq!(updates, vec![(1, CSetEntityLink::new(1, 2))]);

        updates.clear();
        tracker.send_changes(
            |_| Vec::new(),
            |_| None,
            EntityChangeSenders {
                movement: |_, _| {},
                self_movement: |_, _| {},
                entity_data: |_, _| {},
                attributes: |_, _| {},
                mob_effects: |_, _| {},
                equipment: |_, _| {},
                passengers: |_, _| {},
                entity_link: |entity_id, packet| updates.push((entity_id, packet)),
            },
        );
        assert!(updates.is_empty());

        pig_mob.remove_leash_state();
        tracker.send_changes(
            |_| Vec::new(),
            |_| None,
            EntityChangeSenders {
                movement: |_, _| {},
                self_movement: |_, _| {},
                entity_data: |_, _| {},
                attributes: |_, _| {},
                mob_effects: |_, _| {},
                equipment: |_, _| {},
                passengers: |_, _| {},
                entity_link: |entity_id, packet| updates.push((entity_id, packet)),
            },
        );
        assert_eq!(updates, vec![(1, CSetEntityLink::new(1, 0))]);
    }

    #[test]
    fn send_changes_broadcasts_passenger_changes_once() {
        test_support::init_test_registry();

        let tracker = EntityTracker::new();
        let vehicle_typed = PairingTestEntity::new(1, Vec::new());
        let vehicle: SharedEntity = vehicle_typed.clone();
        let passenger_typed = PairingTestEntity::new(2, Vec::new());
        let passenger: SharedEntity = passenger_typed;
        track_entity_for_player(&tracker, &vehicle, 99);
        track_entity_for_player(&tracker, &passenger, 99);

        let mut updates = Vec::new();
        tracker.send_changes(
            |_| Vec::new(),
            |_| None,
            EntityChangeSenders {
                movement: |_, _| {},
                self_movement: |_, _| {},
                entity_data: |_, _| {},
                attributes: |_, _| {},
                mob_effects: |_, _| {},
                equipment: |_, _| {},
                passengers: |player_id, packet| {
                    updates.push((player_id, packet));
                },
                entity_link: |_, _| {},
            },
        );
        assert!(updates.is_empty());

        vehicle_typed.add_passenger(&passenger);
        tracker.send_changes(
            |_| Vec::new(),
            |_| None,
            EntityChangeSenders {
                movement: |_, _| {},
                self_movement: |_, _| {},
                entity_data: |_, _| {},
                attributes: |_, _| {},
                mob_effects: |_, _| {},
                equipment: |_, _| {},
                passengers: |player_id, packet| {
                    updates.push((player_id, packet));
                },
                entity_link: |_, _| {},
            },
        );
        assert_eq!(updates.len(), 1);
        assert_eq!(updates[0].0, 99);
        assert_eq!(updates[0].1.vehicle_id, 1);
        assert_eq!(updates[0].1.passenger_ids, vec![2]);

        updates.clear();
        tracker.send_changes(
            |_| Vec::new(),
            |_| None,
            EntityChangeSenders {
                movement: |_, _| {},
                self_movement: |_, _| {},
                entity_data: |_, _| {},
                attributes: |_, _| {},
                mob_effects: |_, _| {},
                equipment: |_, _| {},
                passengers: |player_id, packet| {
                    updates.push((player_id, packet));
                },
                entity_link: |_, _| {},
            },
        );
        assert!(updates.is_empty());

        vehicle_typed.clear_passengers();
        mark_seen_by_player(&tracker, 1, 99);
        tracker.send_changes(
            |_| Vec::new(),
            |_| None,
            EntityChangeSenders {
                movement: |_, _| {},
                self_movement: |_, _| {},
                entity_data: |_, _| {},
                attributes: |_, _| {},
                mob_effects: |_, _| {},
                equipment: |_, _| {},
                passengers: |player_id, packet| {
                    updates.push((player_id, packet));
                },
                entity_link: |_, _| {},
            },
        );
        assert_eq!(updates.len(), 1);
        assert_eq!(updates[0].0, 99);
        assert_eq!(updates[0].1.vehicle_id, 1);
        assert!(updates[0].1.passenger_ids.is_empty());
    }

    #[test]
    fn send_changes_removes_untracked_passenger_from_vehicle_packet() {
        test_support::init_test_registry();

        let tracker = EntityTracker::new();
        let vehicle_typed = PairingTestEntity::new(1, Vec::new());
        let passenger_typed = PairingTestEntity::new(2, Vec::new());
        let passenger: SharedEntity = passenger_typed;
        vehicle_typed.add_passenger(&passenger);
        let vehicle: SharedEntity = vehicle_typed;
        track_entity_for_player(&tracker, &passenger, 99);
        track_entity_for_player(&tracker, &vehicle, 99);

        let _ = tracker.entities.remove_sync(&passenger.id());

        let mut updates = Vec::new();
        tracker.send_changes(
            |_| Vec::new(),
            |_| None,
            EntityChangeSenders {
                movement: |_, _| {},
                self_movement: |_, _| {},
                entity_data: |_, _| {},
                attributes: |_, _| {},
                mob_effects: |_, _| {},
                equipment: |_, _| {},
                passengers: |player_id, packet| {
                    updates.push((player_id, packet));
                },
                entity_link: |_, _| {},
            },
        );

        assert_eq!(updates.len(), 1);
        assert_eq!(updates[0].0, 99);
        assert_eq!(updates[0].1.vehicle_id, 1);
        assert!(updates[0].1.passenger_ids.is_empty());
    }
}
