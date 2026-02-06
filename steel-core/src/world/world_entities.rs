//! This module contains the implementation of the world's entity-related methods.
use std::sync::Arc;

use steel_protocol::packets::game::{
    CAddEntity, CGameEvent, CPlayerInfoUpdate, CRemoveEntities, CRemovePlayerInfo, GameEventType,
};
use steel_registry::{REGISTRY, vanilla_entities};
use tokio::time::Instant;

use crate::{
    entity::{Entity, PlayerEntityCallback, SharedEntity},
    player::Player,
    world::World,
};

impl World {
    /// Removes a player from the world.
    pub async fn remove_player(self: &Arc<Self>, player: Arc<Player>) {
        let uuid = player.gameprofile.id;
        let entity_id = player.id;

        if self.players.remove(&uuid).await.is_some() {
            let start = Instant::now();

            // Save player data before removal
            if let Some(server) = player.server.upgrade()
                && let Err(e) = server.player_data_storage.save(&player).await
            {
                log::error!("Failed to save player data for {uuid}: {e}");
            }

            // Unregister from entity cache
            let pos = player.position();
            let section = steel_utils::SectionPos::new(
                (pos.x as i32) >> 4,
                (pos.y as i32) >> 4,
                (pos.z as i32) >> 4,
            );
            self.entity_cache.unregister(entity_id, uuid, section);

            // Remove player from entity tracking (stop tracking all entities for this player)
            self.entity_tracker().on_player_leave(entity_id);

            self.player_area_map.on_player_leave(&player);
            self.broadcast_to_all(CRemoveEntities::single(entity_id));
            self.broadcast_to_all(CRemovePlayerInfo::single(uuid));

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

        // Set up level callback for section tracking
        let pos = player.position();
        let callback = Arc::new(PlayerEntityCallback::new(
            player.id,
            pos,
            Arc::downgrade(self),
        ));
        player.set_level_callback(callback);

        // Register player in entity cache for unified entity lookups
        self.entity_cache
            .register(&(player.clone() as SharedEntity));

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

                // Spawn existing player entity for new player (bundled for atomic processing)
                let existing_pos = *existing_player.position.lock();
                let (existing_yaw, existing_pitch) = existing_player.rotation.load();
                let player_type_id = *REGISTRY.entity_types.get_id(vanilla_entities::PLAYER) as i32;
                player.connection.send_bundle(|bundle| {
                    bundle.add(CAddEntity::player(
                        existing_player.id,
                        existing_player.gameprofile.id,
                        player_type_id,
                        existing_pos.x,
                        existing_pos.y,
                        existing_pos.z,
                        existing_yaw,
                        existing_pitch,
                    ));
                    // TODO: Add entity metadata and equipment packets here when implemented
                });
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
        let player_type_id = *REGISTRY.entity_types.get_id(vanilla_entities::PLAYER) as i32;
        let spawn_packet = CAddEntity::player(
            player.id,
            player.gameprofile.id,
            player_type_id,
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
                // Bundle spawn packet for atomic processing
                p.connection.send_bundle(|bundle| {
                    bundle.add(spawn_packet.clone());
                    // TODO: Add entity metadata and equipment packets here when implemented
                });
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
