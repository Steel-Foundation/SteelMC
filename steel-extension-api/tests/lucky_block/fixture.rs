use std::{
    io::Cursor,
    slice,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

use glam::DVec3;
use steel_core::{
    chunk::test_support::{insert_entity_ticking_full_chunk, insert_full_chunk},
    config::{ResolvedDomainConfig, RuntimeConfig},
    entity::{Entity, damage::DamageHistory, next_entity_id},
    level_data::{GameTimeSource, WorldGenerationSettings},
    permission::PermissionSubjectIndex,
    player::{
        ClientInformation, Player, PlayerConnection, PlayerSession, connection::NetworkConnection,
    },
    server::{Server, test_attach_player, test_detach_player, test_server_with_worlds_and_config},
    world::{World, WorldConfig, WorldStorageConfig},
    worldgen::{ChunkGeneratorType, EmptyChunkGenerator},
};
use steel_protocol::packet_traits::{CompressionInfo, EncodedPacket};
use steel_registry::{dimension_type::DimensionTypeRef, game_profile::GameProfile, vanilla_blocks};
use steel_utils::{
    ChunkPos, Identifier,
    codec::VarInt,
    locks::SyncMutex,
    serial::ReadFrom,
    types::{Difficulty, GameType, UpdateFlags},
};
use text_components::TextComponent;
use tokio::runtime::{Builder, Runtime};
use uuid::Uuid;

#[derive(Default)]
struct RecordingConnection {
    packets: Arc<SyncMutex<Vec<EncodedPacket>>>,
    closed: AtomicBool,
}

impl NetworkConnection for RecordingConnection {
    fn compression(&self) -> Option<CompressionInfo> {
        None
    }
    fn send_encoded(&self, packet: EncodedPacket) {
        self.packets.lock().push(packet);
    }
    fn send_encoded_bundle(&self, packets: Vec<EncodedPacket>) {
        self.packets.lock().extend(packets);
    }
    fn disconnect_with_reason(&self, _: TextComponent) {
        self.close();
    }
    fn tick(&self) {}
    fn latency(&self) -> i32 {
        0
    }
    fn close(&self) {
        self.closed.store(true, Ordering::Release);
    }
    fn closed(&self) -> bool {
        self.closed.load(Ordering::Acquire)
    }
}

pub struct Fixture {
    pub world: Arc<World>,
    pub player: Arc<Player>,
    pub packets: Arc<SyncMutex<Vec<EncodedPacket>>>,
    pub pool: Arc<rayon::ThreadPool>,
    pub runtime: Arc<Runtime>,
    _history: Arc<DamageHistory>,
    pub server: Arc<Server>,
    joined: bool,
}

impl Fixture {
    #[expect(
        clippy::too_many_lines,
        reason = "keep the real server/world/player fixture and its ownership explicit"
    )]
    pub fn with_dimension(
        dimension: DimensionTypeRef,
        entity_ticking: bool,
    ) -> Result<Self, String> {
        let runtime = Arc::new(
            Builder::new_multi_thread()
                .worker_threads(2)
                .enable_all()
                .build()
                .map_err(|e| e.to_string())?,
        );
        let pool = Arc::new(
            rayon::ThreadPoolBuilder::new()
                .num_threads(1)
                .build()
                .map_err(|e| e.to_string())?,
        );
        let history = Arc::new(DamageHistory::default());
        let settings = WorldGenerationSettings::from_generator_config(
            Identifier::from_steel("empty"),
            &toml::Value::Table(toml::Table::from_iter([(
                "dimension_type".into(),
                toml::Value::String(dimension.key.to_string()),
            )])),
            dimension.key.clone(),
            dimension.min_y,
            dimension.height,
        );
        let world = runtime
            .block_on(World::new_with_config(
                Arc::clone(&runtime),
                Identifier::new_static("lucky_test", "overworld"),
                dimension,
                0,
                WorldConfig {
                    damage_history: Arc::clone(&history),
                    game_time_source: GameTimeSource::Primary,
                    storage: WorldStorageConfig::RamOnly,
                    level_data_path: None,
                    generator: Arc::new(ChunkGeneratorType::Empty(EmptyChunkGenerator::new())),
                    generation_settings: settings,
                    view_distance: 2,
                    simulation_distance: 2,
                    max_chained_neighbor_updates: 1_000_000,
                    compression: None,
                    is_flat: false,
                    sea_level: 0,
                    default_gamemode: GameType::Survival,
                    difficulty: Difficulty::Normal,
                },
                Arc::clone(&pool),
            ))
            .map_err(|e| e.to_string())?;
        if entity_ticking {
            let _ = insert_entity_ticking_full_chunk(&world, ChunkPos::new(0, 0));
        } else {
            let _ = insert_full_chunk(&world, ChunkPos::new(0, 0), true);
        }
        let config = Arc::new(RuntimeConfig {
            online_mode: false,
            encryption: false,
            compression: None,
            ..Default::default()
        });
        let domain = ResolvedDomainConfig {
            name: world.domain().to_owned(),
            default_world: world.key.clone(),
            worlds: vec![world.key.clone()],
        };
        let server = runtime.block_on(test_server_with_worlds_and_config(
            domain.name.clone(),
            slice::from_ref(&domain),
            slice::from_ref(&world),
            PermissionSubjectIndex::default(),
            Arc::clone(&config),
            None,
        ))?;
        let packets = Arc::new(SyncMutex::new(Vec::new()));
        let session = Arc::new(PlayerSession::new(10, 10));
        let player = Arc::new(Player::new(
            GameProfile::new(Uuid::new_v4(), "LuckyTest".into(), Vec::new())
                .map_err(|e| e.to_string())?,
            Arc::new(PlayerConnection::Other(Box::new(RecordingConnection {
                packets: Arc::clone(&packets),
                ..Default::default()
            }))),
            Arc::clone(&session),
            Arc::clone(&world),
            Arc::downgrade(&server),
            config,
            next_entity_id(),
            ClientInformation::default(),
        ));
        assert!(session.bind_initial_player(&player));
        player
            .try_set_position(DVec3::new(6.5, 65.0, 8.5))
            .map_err(|e| e.to_string())?;
        world.set_block(
            player.block_position().below(),
            vanilla_blocks::STONE.default_state(),
            UpdateFlags::UPDATE_ALL,
        );
        player.set_on_ground(true);
        Ok(Self {
            world,
            player,
            packets,
            pool,
            runtime,
            _history: history,
            server,
            joined: false,
        })
    }

    pub fn join_player(&mut self) -> Result<(), String> {
        test_attach_player(&self.server, Arc::clone(&self.player))?;
        self.joined = true;
        self.player.set_client_loaded(true);
        Ok(())
    }

    pub fn display_ids(&self) -> Result<Vec<i32>, String> {
        use steel_registry::{RegistryEntry, packets::play::C_ADD_ENTITY, vanilla_entities};
        self.packets
            .lock()
            .iter()
            .filter_map(|packet| {
                let mut data = packet_payload(packet, C_ADD_ENTITY)?;
                Some((|| {
                    let id = VarInt::read(&mut data).map_err(|e| e.to_string())?.0;
                    let _ = Uuid::read(&mut data).map_err(|e| e.to_string())?;
                    let kind = VarInt::read(&mut data).map_err(|e| e.to_string())?.0;
                    assert_eq!(kind, vanilla_entities::ITEM_DISPLAY.id() as i32);
                    Ok(id)
                })())
            })
            .collect()
    }

    pub fn removed_ids(&self) -> Result<Vec<i32>, String> {
        use steel_registry::packets::play::C_REMOVE_ENTITIES;
        let mut ids = Vec::new();
        for packet in self.packets.lock().iter() {
            if let Some(mut data) = packet_payload(packet, C_REMOVE_ENTITIES) {
                let count = VarInt::read(&mut data).map_err(|e| e.to_string())?.0;
                for _ in 0..count {
                    ids.push(VarInt::read(&mut data).map_err(|e| e.to_string())?.0);
                }
            }
        }
        Ok(ids)
    }
}

fn packet_payload(packet: &EncodedPacket, id: i32) -> Option<Cursor<&[u8]>> {
    let mut cursor = Cursor::new(packet.encoded_data.as_slice());
    let _length = VarInt::read(&mut cursor).ok()?;
    (VarInt::read(&mut cursor).ok()?.0 == id).then_some(cursor)
}

impl Drop for Fixture {
    fn drop(&mut self) {
        if self.joined {
            test_detach_player(&self.server, &self.player);
        }
        self.world.chunk_map.stop_generation_refill_loop();
        self.world.chunk_map.task_tracker.close();
        self.runtime
            .block_on(self.world.chunk_map.task_tracker.wait());
    }
}
