use std::sync::{Arc, Weak};

use crate::config::RuntimeConfig;
use crate::player::{
    ClientInformation, GameProfile, Player, PlayerConnection, ResetReason, is_valid_player_name,
};
use crate::world::World;
use uuid::Uuid;

use super::{RecordingConnection, TestHarnessError};

/// An attached production [`Player`] and its observable connection.
pub struct TestPlayer {
    player: Arc<Player>,
    connection: RecordingConnection,
}

impl TestPlayer {
    pub(super) fn attach(
        world: &Arc<World>,
        uuid: Uuid,
        name: String,
        entity_id: i32,
    ) -> Result<Self, TestHarnessError> {
        if !is_valid_player_name(&name) {
            return Err(TestHarnessError::InvalidPlayerName { name });
        }
        if world.players.get_by_uuid(&uuid).is_some() || world.get_entity_by_uuid(&uuid).is_some() {
            return Err(TestHarnessError::DuplicatePlayerUuid { uuid });
        }
        if world.players.get_by_entity_id(entity_id).is_some()
            || world.get_entity_by_id(entity_id).is_some()
        {
            return Err(TestHarnessError::DuplicateEntityId { entity_id });
        }

        let connection = RecordingConnection::default();
        let player_connection = Arc::new(PlayerConnection::Other(Box::new(connection.clone())));
        let player = Arc::new(Player::new(
            GameProfile {
                id: uuid,
                name: name.clone(),
                properties: Vec::new(),
                profile_actions: None,
            },
            player_connection,
            Arc::clone(world),
            Weak::new(),
            test_runtime_config(),
            entity_id,
            ClientInformation::default(),
        ));
        if !world.add_player(Arc::clone(&player), ResetReason::InitialJoin) {
            return Err(TestHarnessError::PlayerRegistrationRejected { name });
        }

        Ok(Self { player, connection })
    }

    /// Returns the production player used by Steel gameplay code.
    #[must_use]
    pub const fn player(&self) -> &Arc<Player> {
        &self.player
    }

    /// Returns the connection observer receiving this player's client-bound packets.
    #[must_use]
    pub const fn connection(&self) -> &RecordingConnection {
        &self.connection
    }
}

impl Drop for TestPlayer {
    fn drop(&mut self) {
        self.player
            .get_world()
            .remove_player_for_world_change(&self.player);
    }
}

fn test_runtime_config() -> Arc<RuntimeConfig> {
    Arc::new(RuntimeConfig {
        max_players: 1,
        view_distance: 2,
        simulation_distance: 2,
        max_chained_neighbor_updates: 1_000_000,
        online_mode: false,
        auth_server: None,
        profile_server: None,
        services_server: None,
        encryption: false,
        allow_flight: false,
        motd: String::new(),
        use_favicon: false,
        favicon: String::new(),
        enforce_secure_chat: false,
        chat_spam_threshold_seconds: 10,
        command_spam_threshold_seconds: 10,
        compression: None,
        server_links: None,
        packet_workers: Some(1),
        chunk_generation_threads: Some(1),
        chunk_encoding_threads: Some(1),
    })
}
