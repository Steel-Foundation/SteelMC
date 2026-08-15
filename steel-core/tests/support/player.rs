use std::sync::{Arc, Weak};

use uuid::Uuid;

use super::TestConnection;
use crate::config::RuntimeConfig;
use crate::player::{ClientInformation, GameProfile, Player, PlayerConnection};
use crate::server::Server;
use crate::world::World;

pub(crate) fn test_runtime_config(max_players: u32) -> Arc<RuntimeConfig> {
    Arc::new(RuntimeConfig {
        max_players,
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

pub(crate) struct TestPlayerBuilder {
    profile: GameProfile,
    connection: Arc<PlayerConnection>,
    world: Arc<World>,
    context: TestPlayerContext,
    entity_id: i32,
}

enum TestPlayerContext {
    Detached(Arc<RuntimeConfig>),
    Server {
        server: Weak<Server>,
        config: Arc<RuntimeConfig>,
    },
}

impl TestPlayerBuilder {
    pub(crate) fn new(world: Arc<World>, name: impl Into<String>, entity_id: i32) -> Self {
        Self {
            profile: GameProfile {
                id: Uuid::new_v4(),
                name: name.into(),
                properties: Vec::new(),
                profile_actions: None,
            },
            connection: Arc::new(PlayerConnection::Other(Box::new(TestConnection::default()))),
            world,
            context: TestPlayerContext::Detached(test_runtime_config(1)),
            entity_id,
        }
    }

    pub(crate) fn uuid(mut self, uuid: Uuid) -> Self {
        self.profile.id = uuid;
        self
    }

    pub(crate) fn connection(mut self, connection: Arc<PlayerConnection>) -> Self {
        self.connection = connection;
        self
    }

    pub(crate) fn detached_config(mut self, config: Arc<RuntimeConfig>) -> Self {
        self.context = TestPlayerContext::Detached(config);
        self
    }

    pub(crate) fn server(mut self, server: &Arc<Server>) -> Self {
        self.context = TestPlayerContext::Server {
            server: Arc::downgrade(server),
            config: Arc::clone(&server.config),
        };
        self
    }

    pub(crate) fn build(self) -> Arc<Player> {
        let (server, config) = match self.context {
            TestPlayerContext::Detached(config) => (Weak::new(), config),
            TestPlayerContext::Server { server, config } => (server, config),
        };
        Arc::new(Player::new(
            self.profile,
            self.connection,
            self.world,
            server,
            config,
            self.entity_id,
            ClientInformation::default(),
        ))
    }
}
