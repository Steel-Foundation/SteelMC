//! This module contains the implementation of the world's entity-related methods.
use std::sync::Arc;

use steel_protocol::packets::game::{
    CAddEntity, CGameEvent, CPlayerInfoUpdate, CRemoveEntities, CRemovePlayerInfo, GameEventType,
};
use tokio::time::Instant;

use crate::{player::Player, world::World};

impl World {
    /// Removes a player from the world.
    pub async fn remove_player(self: &Arc<Self>, player: Arc<Player>) {
        let uuid = player.gameprofile.id;
        let entity_id = player.entity_id;

        if self.players.remove(&uuid).await.is_some() {
            let start = Instant::now();

            self.player_area_map.on_player_leave(&player);
            let remove_entity = CRemoveEntities::single(entity_id);
            let remove_info = CRemovePlayerInfo::single(uuid);
            self.players.iter_players(|_, p| {
                p.connection.send_packet(remove_entity.clone());
                p.connection.send_packet(remove_info.clone());
                true
            });

            self.chunk_map.remove_player(&player);
            player.cleanup();
            log::info!("Player {uuid} removed in {:?}", start.elapsed());
        }
    }

    /// Adds a player to the world.
    pub fn add_player(self: &Arc<Self>, player: Arc<Player>) {
        if !self.players.insert(player.clone()) {
            player.connection.close();
            return;
        }

        // Note: player_area_map.on_player_join is called in chunk_map.update_player_status
        // when the player's view is first computed

        let pos = *player.position.lock();
        let (yaw, pitch) = player.rotation.load();

        // Send existing players to the new player (tab list + entity spawn)
        self.players.iter_players(|_, existing_player| {
            if existing_player.gameprofile.id != player.gameprofile.id {
                // Add to tab list with full player info
                let add_existing = CPlayerInfoUpdate::create_player_initializing(
                    existing_player.gameprofile.id,
                    existing_player.gameprofile.name.clone(),
                    existing_player.gameprofile.properties.clone(),
                    existing_player.game_mode.load().into(),
                    existing_player.connection.latency(),
                    None, // display_name
                    true, // show_hat
                );
                player.connection.send_packet(add_existing);

                // Send chat session if available
                if let Some(session) = existing_player.chat_session()
                    && let Ok(protocol_data) = session.as_data().to_protocol_data()
                {
                    let session_packet = CPlayerInfoUpdate::update_chat_session(
                        existing_player.gameprofile.id,
                        protocol_data,
                    );
                    player.connection.send_packet(session_packet);
                }

                // Spawn existing player entity for new player
                let existing_pos = *existing_player.position.lock();
                let (existing_yaw, existing_pitch) = existing_player.rotation.load();
                player.connection.send_packet(CAddEntity::player(
                    existing_player.entity_id,
                    existing_player.gameprofile.id,
                    existing_pos.x,
                    existing_pos.y,
                    existing_pos.z,
                    existing_yaw,
                    existing_pitch,
                ));
            }
            true
        });

        // Broadcast new player to all existing players (tab list + entity spawn)
        let player_info_packet = CPlayerInfoUpdate::create_player_initializing(
            player.gameprofile.id,
            player.gameprofile.name.clone(),
            player.gameprofile.properties.clone(),
            player.game_mode.load().into(),
            player.connection.latency(),
            None, // display_name
            true, // show_hat
        );
        let spawn_packet = CAddEntity::player(
            player.entity_id,
            player.gameprofile.id,
            pos.x,
            pos.y,
            pos.z,
            yaw,
            pitch,
        );

        self.players.iter_players(|_, p| {
            p.connection.send_packet(player_info_packet.clone());
            // Don't send spawn packet to self
            if p.gameprofile.id != player.gameprofile.id {
                p.connection.send_packet(spawn_packet.clone());
            }
            true
        });

        player.connection.send_packet(CGameEvent {
            event: GameEventType::LevelChunksLoadStart,
            data: 0.0,
        });

        player.connection.send_packet(CGameEvent {
            event: GameEventType::ChangeGameMode,
            data: player.game_mode.load().into(),
        });
    }
}
