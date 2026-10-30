//! Test world implementation using the real steel-core World.
//!
//! This module provides a test world that wraps the real `Arc<World>` from steel-core,
//! configured with RAM-only storage for instant chunk creation without disk I/O.

use std::io::Cursor;
use std::mem;
use std::slice;
use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};
use std::thread;
use std::time::{Duration, Instant};

use crate::convert::{
    flint_block_to_state_id, flint_gamerule_value, flint_pos_to_steel, state_id_to_block,
};
use crate::player::SteelTestPlayer;
use crate::runtime;
use flint_core::Block;
use flint_core::test_spec::{EntityNbt, GameruleValue};
use flint_core::traits::EntityState;
use flint_core::{BlockPos as FlintBlockPos, FlintPlayer, FlintWorld};
use glam::DVec3;
use rustc_hash::FxHashMap;
use simdnbt::borrow::read_compound;
use simdnbt::owned::{NbtCompound, NbtTag};
use steel_core::chunk::chunk_request::{ChunkRequestHandle, ChunkRequestState, ChunkTicketKind};
use steel_core::chunk::status::ChunkStatus;
use steel_core::config::{ResolvedDomainConfig, RuntimeConfig};
use steel_core::entity::damage::DamageHistory;
use steel_core::entity::{ENTITIES, Entity, SharedEntity, next_entity_id};
use steel_core::level_data::{GameTimeSource, WorldGenerationSettings};
use steel_core::permission::PermissionSubjectIndex;
use steel_core::server::{Server, test_server_with_worlds_and_config};
use steel_core::world::{LevelReader, World, WorldConfig, WorldStorageConfig};
use steel_core::worldgen::{ChunkGeneratorType, EmptyChunkGenerator};
use steel_registry::blocks::block_state_ext::BlockStateExt;
use steel_registry::vanilla_dimension_types::OVERWORLD;
use steel_registry::{REGISTRY, RegistryExt};
use steel_utils::Identifier;
use steel_utils::locks::SyncMutex;
use steel_utils::nbt::{parse_nbt_path, parse_snbt_compound, to_canonical_snbt};
use steel_utils::types::{Difficulty, GameType};
use steel_utils::{BlockPos, ChunkPos, WorldAabb, types::UpdateFlags};
use toml::Value;
use toml::value::Table;
use uuid::Uuid;

/// Test world implementation using the real steel-core World.
///
/// This wraps an `Arc<World>` configured with RAM-only storage:
/// - Chunks are created empty (all air) on-demand
/// - No disk I/O, no chunk generation delay
/// - Full block behavior system (neighbors, shapes, etc.)
/// - Real tick processing
pub struct SteelTestWorld {
    /// The underlying steel-core world.
    world: Arc<World>,
    /// Server owning the world; players hold a weak reference to it.
    server: Arc<Server>,
    /// Owner of the damage history; the world only holds a weak reference.
    _damage_history: Arc<DamageHistory>,
    /// Current tick count (for `FlintWorld` trait).
    tick: AtomicU64,
    /// Active chunk requests, keyed by chunk position.
    ///
    /// Each handle owns the chunk's tickets; retaining it for the world's
    /// lifetime keeps the chunk permanently loaded (it unloads on drop).
    chunk_requests: SyncMutex<FxHashMap<ChunkPos, ChunkRequestHandle>>,

    entities: FxHashMap<String, Uuid>,
}

impl SteelTestWorld {
    /// Creates a new test world with RAM-only storage.
    ///
    /// The world uses the overworld dimension type and starts with seed 0.
    /// All chunks are created empty on-demand.
    ///
    /// # Panic
    /// shouldn't panic only something is completely broken and then it is ok
    #[allow(clippy::missing_panics_doc)]
    #[must_use]
    pub fn new() -> Self {
        let rt = runtime();

        let dim_id = Identifier::vanilla_static("overworld");

        let damage_history = Arc::new(DamageHistory::default());
        let config = WorldConfig {
            damage_history: Arc::clone(&damage_history),
            storage: WorldStorageConfig::RamOnly,
            level_data_path: None,
            generator: Arc::new(ChunkGeneratorType::Empty(EmptyChunkGenerator::new())),
            generation_settings: WorldGenerationSettings {
                generator: Identifier::new("steel", "empty"),
                config: Value::Table(Table::new()),
                dimension_type: dim_id.clone(),
                min_y: OVERWORLD.min_y,
                height: OVERWORLD.height,
            },
            view_distance: 10,
            simulation_distance: 10,
            max_chained_neighbor_updates: -1,
            compression: None,
            is_flat: false,
            sea_level: 63,
            default_gamemode: GameType::Survival,
            difficulty: Difficulty::Normal,
            game_time_source: GameTimeSource::Primary,
        };

        let generation_pool = Arc::new(
            rayon::ThreadPoolBuilder::new()
                .build()
                .expect("Failed to create rayon thread pool"),
        );

        let runtime_config = Arc::new(RuntimeConfig {
            max_players: 20,
            view_distance: 10,
            simulation_distance: 10,
            max_chained_neighbor_updates: -1,
            online_mode: false,
            encryption: false,
            motd: String::new(),
            use_favicon: false,
            favicon: String::new(),
            enforce_secure_chat: false,
            compression: None,
            allow_flight: true,
            chat_spam_threshold_seconds: 10,
            command_spam_threshold_seconds: 10,
            packet_workers: Some(1),
            chunk_generation_threads: Some(1),
            chunk_encoding_threads: Some(1),
            ..RuntimeConfig::default()
        });

        let (world, server) = rt.block_on(async {
            let world =
                World::new_with_config(rt.clone(), dim_id, &OVERWORLD, 0, config, generation_pool)
                    .await
                    .expect("Failed to create test world");
            let domain = ResolvedDomainConfig {
                name: world.domain().to_owned(),
                default_world: world.key.clone(),
                worlds: vec![world.key.clone()],
            };
            let server = test_server_with_worlds_and_config(
                domain.name.clone(),
                slice::from_ref(&domain),
                slice::from_ref(&world),
                PermissionSubjectIndex::default(),
                runtime_config,
            )
            .await
            .expect("Failed to create test server");
            (world, server)
        });

        Self {
            world,
            server,
            _damage_history: damage_history,
            tick: AtomicU64::new(0),
            chunk_requests: SyncMutex::new(FxHashMap::default()),
            entities: FxHashMap::default(),
        }
    }

    /// Ensures the chunk containing `pos` is loaded, retaining its ticket handle.
    fn ensure_chunk_at(&self, pos: &BlockPos) {
        let chunk_pos = ChunkPos::new(pos.x() >> 4, pos.z() >> 4);
        if let Some(handle) = self.chunk_requests.lock().get(&chunk_pos)
            && handle.poll() == ChunkRequestState::Ready
        {
            return;
        }

        let handle = self.drive_chunk_request(chunk_pos);
        self.chunk_requests.lock().insert(chunk_pos, handle);
    }

    /// Requests the chunk at `chunk_pos` with a simulation ticket and blocks
    /// until it becomes tickable.
    fn drive_chunk_request(&self, chunk_pos: ChunkPos) -> ChunkRequestHandle {
        let chunk_map = &self.world.chunk_map;

        let handle =
            chunk_map.request_chunk(chunk_pos, ChunkStatus::Full, ChunkTicketKind::Command);
        chunk_map.place_ender_pearl_ticket(chunk_pos);

        let deadline = Instant::now() + Duration::from_secs(30);

        while Instant::now() < deadline {
            chunk_map.flint_advance_scheduling();

            match handle.poll() {
                ChunkRequestState::Ready => break,
                ChunkRequestState::Cancelled => {
                    panic!("chunk {chunk_pos:?} request was cancelled before reaching Full")
                }
                ChunkRequestState::Pending { .. } => thread::sleep(Duration::from_millis(1)),
            }
        }
        assert_eq!(
            handle.poll(),
            ChunkRequestState::Ready,
            "chunk {chunk_pos:?} did not reach Full status within 30s"
        );

        while Instant::now() < deadline {
            if chunk_map.is_block_ticking_full_chunk_simulated(chunk_pos)
                && chunk_map.is_entity_ticking_full_chunk_loaded(chunk_pos)
            {
                return handle;
            }
            chunk_map.flint_advance_scheduling();
            thread::sleep(Duration::from_millis(1));
        }

        panic!("chunk {chunk_pos:?} did not enter tickable snapshot within 30s");
    }

    fn entity_by_alias(&self, alias: &str) -> anyhow::Result<SharedEntity> {
        let uuid = self
            .entities
            .get(alias)
            .ok_or_else(|| anyhow::anyhow!("Unknown entity alias '{alias}'"))?;
        self.world
            .get_entity_by_uuid(uuid)
            .ok_or_else(|| anyhow::anyhow!("Entity '{alias}' is no longer in the world"))
    }

    /// Resolves each requested NBT path (e.g. `Items[0].count`) against `compound`,
    /// keyed by the path as written. Paths that don't resolve are omitted.
    fn nbt_values_for_paths(
        compound: NbtCompound,
        requested_nbt: &[String],
    ) -> Vec<(String, String)> {
        let root = NbtTag::Compound(compound);
        requested_nbt
            .iter()
            .filter_map(|path| {
                let value = parse_nbt_path(path).ok()?.get(&root).into_iter().next()?;
                Some((path.clone(), to_canonical_snbt(&value)?))
            })
            .collect()
    }

    fn entity_state(entity: &dyn Entity, requested_nbt: &[String]) -> EntityState {
        let pos = entity.position();
        let (yaw, pitch) = entity.rotation();
        let nbt = Self::nbt_values_for_paths(entity.nbt_for_data_compare(), requested_nbt)
            .into_iter()
            .collect();
        EntityState {
            entity_type: Some(entity.entity_type().key.to_string()),
            pos: Some([pos.x, pos.y, pos.z]),
            rot: Some([yaw, pitch]),
            nbt,
        }
    }
}

impl Drop for SteelTestWorld {
    fn drop(&mut self) {
        mem::forget(Arc::clone(&self.world));
    }
}

impl Default for SteelTestWorld {
    fn default() -> Self {
        Self::new()
    }
}

impl FlintWorld for SteelTestWorld {
    fn do_tick(&mut self) -> Result<(), anyhow::Error> {
        let tick_count = self.tick.fetch_add(1, Ordering::SeqCst);

        for &pos in self.chunk_requests.lock().keys() {
            self.world.chunk_map.place_ender_pearl_ticket(pos);
        }

        self.server.worlds.advance_domain_game_times();
        self.world.tick_game(tick_count, true);
        Ok(())
    }

    fn current_tick(&self) -> u64 {
        self.tick.load(Ordering::SeqCst)
    }

    fn get_time(&self) -> Result<u64, anyhow::Error> {
        Ok(self.world.game_time() as u64)
    }

    fn get_block(
        &self,
        pos: FlintBlockPos,
        requested_nbt: &[String],
    ) -> Result<Block, anyhow::Error> {
        let steel_pos = flint_pos_to_steel(pos);

        self.ensure_chunk_at(&steel_pos);

        let state = self.world.get_block_state(steel_pos);
        let mut block = state_id_to_block(state);
        if let Some(entity) = self.world.get_block_entity(steel_pos) {
            let compound = entity.save_with_full_metadata();
            block.nbt = Some(EntityNbt::from_string_values(Self::nbt_values_for_paths(
                compound,
                requested_nbt,
            )));
        }
        Ok(block)
    }

    fn set_block(&mut self, pos: FlintBlockPos, block: &Block) -> Result<(), anyhow::Error> {
        let Some(state_id) = flint_block_to_state_id(block) else {
            tracing::warn!("Unknown block: {} - skipping placement", block.id);
            return Ok(());
        };

        let steel_pos = flint_pos_to_steel(pos);

        self.ensure_chunk_at(&steel_pos);
        self.world.chunk_map.propagate_queued_light_changes();

        let mut state = self.world.update_from_neighbor_shapes(state_id, steel_pos);
        if state.is_air() {
            state = state_id;
        }
        for (name, value) in &block.properties {
            state = REGISTRY
                .blocks
                .try_set_property_by_name(state, name, value)
                .unwrap_or(state);
        }

        self.world
            .set_block(steel_pos, state, UpdateFlags::UPDATE_ALL);
        if let Some(nbt) = &block.nbt
            && let Some(entity) = self.world.get_block_entity(steel_pos)
        {
            let compound = parse_snbt_compound(&nbt.to_snbt())?;
            let mut bytes = Vec::new();
            compound.write(&mut bytes);
            let borrowed = read_compound(&mut Cursor::new(bytes.as_slice()))?;
            entity.load_additional(&borrowed);
            entity.set_changed();
        }
        Ok(())
    }

    fn set_gamerule(&mut self, rule: String, value: GameruleValue) -> anyhow::Result<()> {
        let key = rule
            .parse::<Identifier>()
            .map_err(|e| anyhow::anyhow!("Invalid gamerule identifier '{rule}': {e}"))?;
        let rule = REGISTRY
            .game_rules
            .by_key(&key)
            .ok_or_else(|| anyhow::anyhow!("Unknown gamerule '{key}'"))?;
        let value = flint_gamerule_value(rule, value)?;
        if !self.world.set_erased_game_rule(rule, value) {
            anyhow::bail!("Value rejected for gamerule '{key}' (outside allowed range)");
        }
        Ok(())
    }

    fn summon_entity(
        &mut self,
        alias: &str,
        entity_type: &str,
        pos: [f64; 3],
        nbt: Option<&EntityNbt>,
    ) -> anyhow::Result<()> {
        let key = entity_type
            .parse::<Identifier>()
            .map_err(|e| anyhow::anyhow!("Invalid entity type identifier '{entity_type}': {e}"))?;
        let entity_type = REGISTRY
            .entity_types
            .by_key(&key)
            .ok_or_else(|| anyhow::anyhow!("Unknown entity type '{key}'"))?;
        let entity = ENTITIES
            .create(
                entity_type,
                next_entity_id(),
                DVec3::new(pos[0], pos[1], pos[2]),
                Arc::downgrade(&self.world),
            )
            .ok_or_else(|| anyhow::anyhow!("No entity factory registered for '{key}'"))?;
        if let Some(nbt) = nbt {
            let compound = parse_snbt_compound(&nbt.to_snbt())?;
            let mut bytes = Vec::new();
            compound.write(&mut bytes);
            let borrowed = read_compound(&mut Cursor::new(bytes.as_slice()))?;
            entity.apply_spawn_data((&borrowed).into());
        }
        let uuid = entity.uuid();
        self.world
            .try_add_entity(entity)
            .map_err(|e| anyhow::anyhow!("Failed to add '{key}' to world: {e:?}"))?;
        self.entities.insert(alias.to_string(), uuid);
        Ok(())
    }

    fn teleport_entity(
        &mut self,
        alias: &str,
        pos: [f64; 3],
        rot: Option<[f32; 2]>,
    ) -> anyhow::Result<()> {
        let entity = self.entity_by_alias(alias)?;
        entity
            .teleport_to(DVec3::new(pos[0], pos[1], pos[2]))
            .map_err(|e| anyhow::anyhow!("Failed to teleport '{alias}': {e:?}"))?;
        if let Some([yaw, pitch]) = rot {
            entity.set_rotation((yaw, pitch));
        }
        Ok(())
    }

    fn get_entity(
        &self,
        alias: &str,
        requested_nbt: &[String],
    ) -> anyhow::Result<Vec<EntityState>> {
        let entity = self.entity_by_alias(alias)?;
        Ok(vec![Self::entity_state(entity.as_ref(), requested_nbt)])
    }
    fn find_entity(
        &self,
        entity_type: &str,
        requested_nbt: &[String],
    ) -> Result<Vec<EntityState>, anyhow::Error> {
        const WORLD_EXTENT: f64 = 3.0e7;
        let aabb = WorldAabb::new(
            -WORLD_EXTENT,
            f64::from(OVERWORLD.min_y),
            -WORLD_EXTENT,
            WORLD_EXTENT,
            f64::from(OVERWORLD.min_y + OVERWORLD.height),
            WORLD_EXTENT,
        );

        let wanted = entity_type
            .strip_prefix("minecraft:")
            .unwrap_or(entity_type);
        let entities = self.world.get_entities_in_aabb_matching(&aabb, |entity| {
            let key = &entity.entity_type().key;
            key.namespace == "minecraft" && key.path == wanted
        });

        Ok(entities
            .iter()
            .map(|entity| Self::entity_state(entity.as_ref(), requested_nbt))
            .collect())
    }

    fn create_player(&mut self) -> Box<dyn FlintPlayer> {
        Box::new(SteelTestPlayer::new(self.world.clone(), &self.server))
    }
}
