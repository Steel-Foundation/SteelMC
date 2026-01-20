use std::sync::Arc;
use steel_core::config::ServerLinks;
use steel_protocol::packets::common::CCustomPayload;
use steel_protocol::packets::common::{SClientInformation, SCustomPayload};
use steel_protocol::packets::config::CFinishConfiguration;

use steel_protocol::packets::config::CSelectKnownPacks;
use steel_protocol::packets::config::SSelectKnownPacks;
use steel_protocol::packets::shared_implementation::KnownPack;
use steel_protocol::utils::ConnectionProtocol;

use steel_core::player::networking::JavaConnection;
use steel_core::player::{ClientInformation, Player};
use steel_utils::Identifier;

use crate::MC_VERSION;
use crate::network::JavaTcpClient;
use crate::network::java_tcp_client::ConnectionUpdate;

const BRAND_PAYLOAD: [u8; 5] = *b"Steel";

impl JavaTcpClient {
    /// Handles a custom payload packet during the configuration state.
    pub fn handle_config_custom_payload(&self, packet: SCustomPayload) {
        println!("Custom payload packet: {packet:?}");
    }

    /// Handles the client information packet during the configuration state.
    pub async fn handle_client_information(&self, packet: SClientInformation) {
        log::debug!("Client information packet: {packet:?}");

        // Convert packet to our ClientInformation struct and store it
        let info = ClientInformation {
            language: packet.language,
            view_distance: packet.view_distance.clamp(2, 32) as u8,
            chat_visibility: packet.chat_visibility,
            chat_colors: packet.chat_colors,
            model_customisation: packet.model_customisation,
            main_hand: packet.main_hand,
            text_filtering_enabled: packet.text_filtering_enabled,
            allows_listing: packet.allows_listing,
            particle_status: packet.particle_status,
        };

        *self.client_information.lock().await = info;
    }

    /// Starts the configuration process by sending initial packets.
    pub async fn start_configuration(&self) {
        self.send_bare_packet_now(CCustomPayload::new(
            Identifier::vanilla_static("brand"),
            Box::new(BRAND_PAYLOAD),
        ))
        .await;

        // Send server links if enabled and configured
        if let Some(server_links) = ServerLinks::from_config() {
            self.send_bare_packet_now(server_links).await;
        }

        self.send_bare_packet_now(CSelectKnownPacks::new(vec![KnownPack::new(
            "minecraft".to_string(),
            "core".to_string(),
            MC_VERSION.to_string(),
        )]))
        .await;
    }

    /// Handles the select known packs packet during the configuration state.
    pub async fn handle_select_known_packs(&self, packet: SSelectKnownPacks) {
        println!("Select known packs packet: {packet:?}");

        let registry_cache = self.server.registry_cache.registry_packets.clone();
        for encoded_packet in registry_cache.iter() {
            self.send_packet_now(encoded_packet).await;
        }

        // Send the packet for tags
        self.send_packet_now(&self.server.registry_cache.tags_packet)
            .await;

        // Finish configuration with CFinishConfigurationPacket
        self.send_bare_packet_now(CFinishConfiguration {}).await;
    }

    /// Finishes the configuration process and transitions to the play state.
    ///
    /// # Panics
    /// This function will panic if the game profile is empty, should be impossible at this point.
    pub async fn finish_configuration(&self) {
        self.protocol.store(ConnectionProtocol::Play);

        let gameprofile = self
            .gameprofile
            .lock()
            .await
            .clone()
            .expect("Game profile is empty");

        let client_info = self.client_information.lock().await.clone();

        let world = self.server.worlds[0].clone();
        let entity_id = self.server.next_entity_id();

        let player = Arc::new_cyclic(|player_weak| {
            let connection = Arc::new(JavaConnection::new(
                self.outgoing_queue.clone(),
                self.cancel_token.clone(),
                self.compression.load(),
                self.network_writer.clone(),
                self.id,
                player_weak.clone(),
            ));

            Player::new(
                gameprofile,
                connection,
                world,
                entity_id,
                player_weak,
                client_info,
            )
        });

        self.connection_updates
            .send(ConnectionUpdate::Upgrade(player.connection.clone()))
            .expect("Failed to send connection update");

        self.server.add_player(player);
    }
}
