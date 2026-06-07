//! This module contains the implementation of the world's entity-related methods.
use std::sync::Arc;

use steel_protocol::packets::game::{
    CGameEvent, CPlayerInfoUpdate, CRemovePlayerInfo, GameEventType,
};
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
    player::player_data_storage::GlobalPlayerData,
    player::{Player, ResetReason},
    world::World,
};

impl World {
    fn attach_player_entity_callback(self: &Arc<Self>, player: &Arc<Player>) {
        let callback = Arc::new(PlayerEntityCallback::new(player.id(), Arc::downgrade(self)));
        player.set_level_callback(callback);
    }

    fn register_player_entity(self: &Arc<Self>, player: &Arc<Player>) {
        self.attach_player_entity_callback(player);

        let entity: SharedEntity = player.clone();
        if let Err(error) = self
            .entity_manager()
            .add_live_entity(entity.clone(), EntityOwnership::External)
        {
            panic!("failed to register player entity: {error}");
        }
        self.add_entity_to_tracker(&entity);
    }

    fn unride_player_for_removal(&self, player: &Player, store_root_vehicle: bool) {
        for passenger in player.passengers() {
            passenger.stop_riding();
            self.mark_chunk_dirty(ChunkPos::from_entity_pos(passenger.position()));
        }

        if store_root_vehicle
            && let Some(root_vehicle) = player.root_vehicle()
            && root_vehicle.id() != player.id()
            && root_vehicle.has_exactly_one_player_passenger()
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
        self.entity_tracker
            .remove(entity_id, |id| self.players.get_by_entity_id(id));

        self.entity_manager()
            .remove_live_entity(entity_id, RemovalReason::ChangedWorld);
        player.set_level_callback(Arc::new(NullEntityCallback));
    }

    pub(crate) fn register_respawned_player_entity(self: &Arc<Self>, player: &Arc<Player>) {
        self.register_player_entity(player);
        self.chunk_map.update_player_status(player);
    }

    /// Removes a player from the world.
    pub async fn remove_player(self: &Arc<Self>, player: Arc<Player>) {
        let Some(player) = self.players.remove_player(&player).await else {
            return;
        };
        let uuid = player.gameprofile.id;
        let entity_id = player.id();
        let domain = self.domain().to_owned();
        let player_data = PersistentPlayerData::from_player(&player);

        self.unride_player_for_removal(&player, true);
        self.unregister_player_entity(&player);

        // Remove player from entity tracking (stop tracking all entities for this player)
        self.entity_tracker().on_player_leave(entity_id);

        self.player_area_map.on_player_leave(&player);
        self.chunk_map.remove_player(&player);

        let start = Instant::now();

        // Save after world indexes are cleared so a fast reconnect cannot collide
        // with this player's stale entity ID/UUID cache entries.
        let server = player.server();
        if let Err(e) = server
            .player_data_storage
            .save_domain_data(&domain, uuid, &player_data)
            .await
        {
            log::error!("Failed to save player domain data for {uuid}: {e}");
        }
        if let Err(e) = server
            .player_data_storage
            .save_global(
                uuid,
                &GlobalPlayerData {
                    last_active_domain: domain,
                },
            )
            .await
        {
            log::error!("Failed to save global player data for {uuid}: {e}");
        }

        self.broadcast_to_all(CRemovePlayerInfo::single(uuid));

        player.cleanup();
        log::info!("Player {uuid} removed in {:?}", start.elapsed());
    }

    /// Removes a player from the world during a world change.
    ///
    /// Unlike `remove_player`, this is synchronous and skips player data saving and tab list
    /// removal — the player stays in the global tab list since they are only switching worlds.
    pub fn remove_player_for_world_change(self: &Arc<Self>, player: &Arc<Player>) {
        let Some(player) = self.players.remove_player_sync(player) else {
            return;
        };
        let entity_id = player.id();

        self.unride_player_for_removal(&player, false);
        self.unregister_player_entity(&player);
        self.entity_tracker().on_player_leave(entity_id);
        self.player_area_map.on_player_leave(&player);
        // Note: no CRemovePlayerInfo — player stays in the global tab list
        self.chunk_map.remove_player(&player);
    }

    /// Removes a player during a domain switch after the caller has saved
    /// the player's current-domain data.
    pub fn remove_player_for_domain_switch(self: &Arc<Self>, player: &Arc<Player>) {
        let Some(player) = self.players.remove_player_sync(player) else {
            return;
        };
        let entity_id = player.id();

        self.unride_player_for_removal(&player, true);
        self.unregister_player_entity(&player);
        self.entity_tracker().on_player_leave(entity_id);
        self.player_area_map.on_player_leave(&player);
        self.chunk_map.remove_player(&player);
    }

    /// Adds a player to the world.
    ///
    /// On `InitialJoin`, sends full tab list + entity spawn synchronization to/from all
    /// players. On `WorldChange`, this is skipped — the player already exists in all
    /// clients' tab lists and the entity tracker handles spawning as chunks load.
    #[must_use]
    pub fn add_player(self: &Arc<Self>, player: Arc<Player>, reason: ResetReason) -> bool {
        if !self.players.insert(player.clone()) {
            player.connection.close();
            return false;
        }

        // Tab-list sync only needs the initial login path; world changes keep
        // the player in the global tab list.
        if reason == ResetReason::InitialJoin {
            self.sync_tab_list(&player);
        }

        self.register_player_entity(&player);
        self.chunk_map.update_player_status(&player);

        player.send_packet(CGameEvent {
            event: GameEventType::LevelChunksLoadStart,
            data: 0.0,
        });

        player.send_packet(CGameEvent {
            event: GameEventType::ChangeGameMode,
            data: player.game_mode().into(),
        });

        true
    }

    /// Sends full tab list synchronization for a newly joined player.
    ///
    /// Sends all existing players' info to the new player, then broadcasts the
    /// new player's info to everyone. Entity spawn pairing is owned by
    /// `EntityTracker`, matching vanilla `ChunkMap`.
    fn sync_tab_list(self: &Arc<Self>, player: &Arc<Player>) {
        // Send existing players to the new player.
        self.players.iter_players(|_, existing_player| {
            if existing_player.gameprofile.id == player.gameprofile.id {
                return true;
            }

            // Add to tab list with full player info
            let add_existing = CPlayerInfoUpdate::create_player_initializing(
                existing_player.gameprofile.id,
                existing_player.gameprofile.name.clone(),
                existing_player.gameprofile.properties.clone(),
                existing_player.game_mode().into(),
                existing_player.connection.latency(),
                None, // display_name
                true, // show_hat
            );
            player.send_packet(add_existing);

            // Send chat session if available
            if let Some(session) = existing_player.chat_session()
                && let Ok(protocol_data) = session.as_data().to_protocol_data()
            {
                let session_packet = CPlayerInfoUpdate::update_chat_session(
                    existing_player.gameprofile.id,
                    protocol_data,
                );
                player.send_packet(session_packet);
            }

            true
        });

        // Broadcast new player's tab list entry to all players
        let player_info_packet = CPlayerInfoUpdate::create_player_initializing(
            player.gameprofile.id,
            player.gameprofile.name.clone(),
            player.gameprofile.properties.clone(),
            player.game_mode().into(),
            player.connection.latency(),
            None, // display_name
            true, // show_hat
        );
        self.broadcast_to_all(player_info_packet);
    }
}
