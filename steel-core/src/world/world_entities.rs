//! This module contains the implementation of the world's entity-related methods.
use std::sync::Arc;

use steel_protocol::packets::game::{
    CGameEvent, CPlayerInfoUpdate, CRemovePlayerInfo, GameEventType,
};
use steel_utils::SectionPos;
use tokio::time::Instant;

use crate::{
    entity::{Entity, NullEntityCallback, PlayerEntityCallback, SharedEntity},
    player::connection::NetworkConnection,
    player::{Player, ResetReason},
    world::World,
};

impl World {
    fn attach_player_entity_callback(self: &Arc<Self>, player: &Arc<Player>) {
        let callback = Arc::new(PlayerEntityCallback::new(
            player.id(),
            player.position(),
            Arc::downgrade(self),
        ));
        player.set_level_callback(callback);
    }

    fn register_player_entity(self: &Arc<Self>, player: &Arc<Player>) {
        self.attach_player_entity_callback(player);

        let entity: SharedEntity = player.clone();
        self.entity_cache.register(&entity);
        self.entity_tracker.add(
            &entity,
            |chunk| self.player_area_map.get_tracking_players(chunk),
            |id| self.players.get_by_entity_id(id),
        );
    }

    pub(crate) fn unregister_player_entity(&self, player: &Player) {
        let entity_id = player.id();
        self.entity_tracker
            .remove(entity_id, |id| self.players.get_by_entity_id(id));

        let section = SectionPos::from_entity_pos(player.position());
        self.entity_cache
            .unregister(entity_id, player.uuid(), section);
        player.set_level_callback(Arc::new(NullEntityCallback));
    }

    pub(crate) fn register_respawned_player_entity(self: &Arc<Self>, player: &Arc<Player>) {
        self.register_player_entity(player);
        self.chunk_map.update_player_status(player);
    }

    /// Removes a player from the world.
    pub async fn remove_player(self: &Arc<Self>, player: Arc<Player>) {
        let uuid = player.gameprofile.id;
        let entity_id = player.id();

        if self.players.remove(&uuid).await.is_none() {
            return;
        }

        self.unregister_player_entity(&player);

        // Remove player from entity tracking (stop tracking all entities for this player)
        self.entity_tracker().on_player_leave(entity_id);

        self.player_area_map.on_player_leave(&player);
        self.chunk_map.remove_player(&player);

        let start = Instant::now();

        // Save after world indexes are cleared so a fast reconnect cannot collide
        // with this player's stale entity ID/UUID cache entries.
        let server = player.server();
        if let Err(e) = server.player_data_storage.save(&player).await {
            log::error!("Failed to save player data for {uuid}: {e}");
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
        let uuid = player.gameprofile.id;
        let entity_id = player.id();

        if self.players.remove_sync(&uuid).is_none() {
            return;
        }

        self.unregister_player_entity(player);
        self.entity_tracker().on_player_leave(entity_id);
        self.player_area_map.on_player_leave(player);
        // Note: no CRemovePlayerInfo — player stays in the global tab list
        self.chunk_map.remove_player(player);
    }

    /// Adds a player to the world.
    ///
    /// On `InitialJoin`, sends full tab list + entity spawn synchronization to/from all
    /// players. On `WorldChange`, this is skipped — the player already exists in all
    /// clients' tab lists and the entity tracker handles spawning as chunks load.
    pub fn add_player(self: &Arc<Self>, player: Arc<Player>, reason: ResetReason) {
        if !self.players.insert(player.clone()) {
            player.connection.close();
            return;
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
