//! This module contains the implementation of the world's entity-related methods.
use std::sync::Arc;

use steel_protocol::packets::game::{CGameEvent, CPlayerInfoUpdate, GameEventType};
use steel_registry::vanilla_entities;
use steel_utils::ChunkPos;
use tokio::time::Instant;

use crate::{
    entity::{
        Entity, EntityOwnership, NullEntityCallback, PlayerEntityCallback, RemovalReason,
        SharedEntity,
    },
    player::connection::NetworkConnection,
    player::player_data::PersistentPlayerData,
    player::{Player, ResetReason, ServerPlayer},
    world::World,
};

impl World {
    fn attach_player_entity_callback(self: &Arc<Self>, player: &Player) {
        let callback = Arc::new(PlayerEntityCallback::new(player.id(), Arc::downgrade(self)));
        player.set_level_callback(callback);
    }

    fn register_player_entity(self: &Arc<Self>, player: &mut Player) {
        self.attach_player_entity_callback(player);

        let entity: SharedEntity = player.shared_entity();
        if let Err(error) = self
            .entity_manager()
            .add_live_entity(entity.clone(), EntityOwnership::External)
        {
            panic!("failed to register player entity: {error}");
        }
        // The caller already holds this player's entity lock; thread it through so the
        // tracker reuses it instead of re-entering the (non-reentrant) behavior mutex.
        self.add_entity_to_tracker_with_entity(&entity, Some(player));
    }

    fn unride_player_for_removal(&self, player: &Player, store_root_vehicle: bool) {
        for passenger in player.passengers() {
            passenger.stop_riding();
            self.mark_chunk_dirty(ChunkPos::from_entity_pos(passenger.position()));
        }

        if store_root_vehicle
            && let Some(root_vehicle) = player.root_vehicle()
            && root_vehicle.id() != player.id()
            && root_vehicle.with_entity(|e| e.has_exactly_one_player_passenger())
        {
            Self::remove_root_vehicle_tree_stored_with_player(root_vehicle);
            return;
        }

        if let Some(vehicle) = player.vehicle() {
            player.stop_riding();
            self.mark_chunk_dirty(ChunkPos::from_entity_pos(vehicle.position()));
        }
    }

    fn remove_root_vehicle_tree_stored_with_player(entity: SharedEntity) {
        let passengers = entity.passengers();
        entity.set_removed(RemovalReason::StoredWithPlayer);

        for passenger in passengers {
            if passenger.entity_type() == &vanilla_entities::PLAYER {
                continue;
            }
            Self::remove_root_vehicle_tree_stored_with_player(passenger);
        }
    }

    pub(crate) fn unregister_player_entity(&self, player: &Player) {
        let entity_id = player.id();
        self.remove_entity_from_tracker(entity_id);

        self.entity_manager()
            .remove_live_entity(entity_id, RemovalReason::ChangedWorld);
        player.set_level_callback(Arc::new(NullEntityCallback));
    }

    pub(crate) fn register_respawned_player_entity(self: &Arc<Self>, player: &Arc<ServerPlayer>) {
        let mut player = player.entity.lock();
        self.register_player_entity(&mut player);
        self.chunk_map.update_player_status(&player);
    }

    /// Removes a player from the world.
    pub async fn remove_player(self: &Arc<Self>, player: Arc<ServerPlayer>) {
        let Some(player) = self.players.remove_player(&player).await else {
            if player.entity.lock().has_won_game() {
                self.remove_detached_end_credits_player(player).await;
            }
            return;
        };
        let domain = self.domain().to_owned();

        // Synchronous teardown under a single lock (no `.await` inside).
        let (uuid, player_data, server) = {
            let guard = player.entity.lock();
            let uuid = guard.gameprofile.id;
            let entity_id = guard.id();
            // Ender pearls persist with the player, so hand them over before snapshotting.
            guard.store_ender_pearls_with_player();
            let player_data = PersistentPlayerData::from_player(&guard);
            let server = guard.server();

            self.unride_player_for_removal(&guard, true);
            self.unregister_player_entity(&guard);

            // Remove player from entity tracking (stop tracking all entities for this player)
            self.entity_tracker().on_player_leave(entity_id);

            self.player_area_map.on_player_leave(&guard);

            (uuid, player_data, server)
        };
        self.chunk_map.remove_player(&player);

        let start = Instant::now();

        // Save after world indexes are cleared so a fast reconnect cannot collide
        // with this player's stale entity ID/UUID cache entries. This also drops the
        // player from the server-wide online set and the tab list.
        server
            .remove_online_player_after_disconnect(player, domain, player_data)
            .await;
        log::info!("Player {uuid} removed in {:?}", start.elapsed());
    }

    /// Removes a player who left while detached in the End credits flow.
    ///
    /// The player is no longer attached to a live world, so there is no world
    /// index teardown to do — only persistence and session cleanup.
    async fn remove_detached_end_credits_player(self: &Arc<Self>, player: Arc<ServerPlayer>) {
        let domain = self.domain().to_owned();
        let start = Instant::now();

        let (uuid, player_data, server) = {
            let guard = player.entity.lock();
            guard.store_ender_pearls_with_player();
            (
                guard.gameprofile.id,
                PersistentPlayerData::from_player(&guard),
                guard.server(),
            )
        };

        server
            .remove_online_player_after_disconnect(player, domain, player_data)
            .await;
        log::info!(
            "Detached End credits player {uuid} removed in {:?}",
            start.elapsed()
        );
    }

    /// Removes a player from the world during a world change.
    ///
    /// Unlike `remove_player`, this is synchronous and skips player data saving and tab list
    /// removal — the player stays in the global tab list since they are only switching worlds.
    pub fn remove_player_for_world_change(self: &Arc<Self>, player: &Arc<ServerPlayer>) {
        let Some(player) = self.players.remove_player_sync(player) else {
            return;
        };
        let guard = player.entity.lock();
        let entity_id = guard.id();

        self.unride_player_for_removal(&guard, false);
        self.unregister_player_entity(&guard);
        self.entity_tracker().on_player_leave(entity_id);
        self.player_area_map.on_player_leave(&guard);
        // Note: no CRemovePlayerInfo — player stays in the global tab list
        self.chunk_map.remove_player(&player);
    }

    /// Removes a player during a domain switch after the caller has saved
    /// the player's current-domain data.
    pub fn remove_player_for_domain_switch(self: &Arc<Self>, player: &Arc<ServerPlayer>) {
        let Some(player) = self.players.remove_player_sync(player) else {
            return;
        };
        let guard = player.entity.lock();
        let entity_id = guard.id();

        self.unride_player_for_removal(&guard, true);
        self.unregister_player_entity(&guard);
        self.entity_tracker().on_player_leave(entity_id);
        self.player_area_map.on_player_leave(&guard);
        self.chunk_map.remove_player(&player);
    }

    /// Adds a player to the world.
    ///
    /// On `InitialJoin`, sends full tab list + entity spawn synchronization to/from all
    /// players. On `WorldChange`, this is skipped — the player already exists in all
    /// clients' tab lists and the entity tracker handles spawning as chunks load.
    #[must_use]
    /// Re-inserts a respawning player into this world.
    pub(crate) fn add_respawned_player(self: &Arc<Self>, player: Arc<ServerPlayer>) -> bool {
        if !self.players.insert(player.clone()) {
            player.connection.close();
            return false;
        }

        self.register_respawned_player_entity(&player);
        player.entity.lock().send_packet(CGameEvent {
            event: GameEventType::LevelChunksLoadStart,
            data: 0.0,
        });
        true
    }

    pub fn add_player(self: &Arc<Self>, player: Arc<ServerPlayer>, reason: ResetReason) -> bool {
        if !self.players.insert(player.clone()) {
            player.connection.close();
            return false;
        }

        // Tab-list sync only needs the initial login path; world changes keep
        // the player in the global tab list. Done before locking the new player,
        // since it iterates (and briefly locks) every player including this one.
        if reason == ResetReason::InitialJoin {
            self.sync_tab_list(&player);
        }

        let mut guard = player.entity.lock();

        self.register_player_entity(&mut guard);
        self.chunk_map.update_player_status(&guard);

        guard.send_packet(CGameEvent {
            event: GameEventType::LevelChunksLoadStart,
            data: 0.0,
        });

        let game_mode = guard.game_mode();
        guard.send_packet(CGameEvent {
            event: GameEventType::ChangeGameMode,
            data: game_mode.into(),
        });

        true
    }

    /// Sends full tab list synchronization for a newly joined player.
    ///
    /// Sends all existing players' info to the new player, then broadcasts the
    /// new player's info to everyone. Entity spawn pairing is owned by
    /// `EntityTracker`, matching vanilla `ChunkMap`.
    fn sync_tab_list(self: &Arc<Self>, player: &Arc<ServerPlayer>) {
        let new_uuid = player.entity.lock().gameprofile.id;

        // Collect existing players' tab-list packets (locking each briefly), then
        // send them to the new player. Avoids holding two player locks at once.
        let mut packets = Vec::new();
        self.players.iter_players(|_, existing_player| {
            let latency = existing_player.connection.latency();
            let guard = existing_player.entity.lock();
            if guard.gameprofile.id == new_uuid {
                return true;
            }

            packets.push(CPlayerInfoUpdate::create_player_initializing(
                guard.gameprofile.id,
                guard.gameprofile.name.clone(),
                guard.gameprofile.properties.clone(),
                guard.game_mode().into(),
                latency,
                None, // display_name
                true, // show_hat
            ));

            if let Some(session) = guard.chat_session()
                && let Ok(protocol_data) = session.as_data().to_protocol_data()
            {
                packets.push(CPlayerInfoUpdate::update_chat_session(
                    guard.gameprofile.id,
                    protocol_data,
                ));
            }

            true
        });

        // Broadcast the new player's tab list entry, then send the collected
        // existing-player entries to the new player.
        let player_info_packet = {
            let latency = player.connection.latency();
            let guard = player.entity.lock();
            for packet in packets {
                guard.send_packet(packet);
            }
            CPlayerInfoUpdate::create_player_initializing(
                guard.gameprofile.id,
                guard.gameprofile.name.clone(),
                guard.gameprofile.properties.clone(),
                guard.game_mode().into(),
                latency,
                None, // display_name
                true, // show_hat
            )
        };
        self.broadcast_to_all(player_info_packet);
    }
}
