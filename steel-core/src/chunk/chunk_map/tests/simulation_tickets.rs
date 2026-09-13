use super::*;
use crate::chunk::chunk_ticket_storage::PORTAL_TICKET_RADIUS;
use crate::level_data::WorldGenerationSettings;
use crate::world::{WorldConfig, WorldStorageConfig};
use std::{
    env::temp_dir,
    fs,
    io::ErrorKind,
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};
use steel_utils::{
    Identifier,
    saved_data::{SavedDataManager, names as saved_data_names},
    types::{Difficulty, GameType},
};
use toml::map::Map;
use uuid::Uuid;

const TEST_WORLD_SEED: i64 = 0;
const TEST_VIEW_AND_SIMULATION_DISTANCE_CHUNKS: u8 = 2;

struct TemporaryWorldDirectory(PathBuf);

impl TemporaryWorldDirectory {
    fn new(test_name: &str) -> Self {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time should be after the Unix epoch")
            .as_nanos();
        Self(temp_dir().join(format!("steel-{test_name}-{unique}")))
    }

    fn path_string(&self) -> String {
        self.0.to_string_lossy().into_owned()
    }
}

impl Drop for TemporaryWorldDirectory {
    fn drop(&mut self) {
        if let Err(error) = fs::remove_dir_all(&self.0)
            && error.kind() != ErrorKind::NotFound
        {
            panic!("temporary world directory should be removable: {error}");
        }
    }
}

#[test]
fn restored_portal_ticket_initializes_both_levels_in_the_first_source_phase() {
    init_vanilla_registry();
    init_behaviors();
    let directory = TemporaryWorldDirectory::new("restored-portal-ticket");
    let runtime = Arc::new(Runtime::new().expect("test runtime should initialize"));
    let center = ChunkPos::new(-4, 7);
    let mut ticket_storage = ChunkTicketStorage::new();
    let _ = ticket_storage.add_or_refresh_portal_ticket(center);
    let persistent_tickets = ticket_storage.to_persistent();
    runtime
        .block_on(
            SavedDataManager::new(Some(&directory.0))
                .save(saved_data_names::CHUNK_TICKETS, &persistent_tickets),
        )
        .expect("portal ticket data should persist");

    let generator = Arc::new(ChunkGeneratorType::Empty(EmptyChunkGenerator::new()));
    let generation_settings = WorldGenerationSettings::from_generator_config(
        Identifier::vanilla_static("empty"),
        &toml::Value::Table(Map::new()),
        OVERWORLD.key.clone(),
        OVERWORLD.min_y,
        OVERWORLD.height,
    );
    let generation_pool = Arc::new(
        rayon::ThreadPoolBuilder::new()
            .num_threads(1)
            .build()
            .expect("test generation pool should initialize"),
    );
    let world = runtime
        .block_on(World::new_with_config(
            Arc::clone(&runtime),
            Identifier::vanilla_static("restored_portal_ticket"),
            &OVERWORLD,
            TEST_WORLD_SEED,
            WorldConfig {
                storage: WorldStorageConfig::RamOnly,
                level_data_path: Some(directory.path_string()),
                generator,
                generation_settings,
                view_distance: TEST_VIEW_AND_SIMULATION_DISTANCE_CHUNKS,
                simulation_distance: TEST_VIEW_AND_SIMULATION_DISTANCE_CHUNKS,
                max_chained_neighbor_updates: 1_000_000,
                compression: None,
                is_flat: false,
                sea_level: 63,
                default_gamemode: GameType::Survival,
                difficulty: Difficulty::Normal,
            },
            generation_pool,
        ))
        .expect("world should restore persisted portal tickets");

    world.chunk_map.advance_scheduling();
    let holder = world
        .chunk_map
        .chunks
        .read_sync(&center, |_, holder| Arc::clone(holder))
        .expect("restored portal ticket should load its center holder");
    let expected_level = ChunkTicketLevel::for_full_chunk_radius(PORTAL_TICKET_RADIUS);
    assert_eq!(holder.load_level(), Some(expected_level));
    assert_eq!(
        holder.simulation_level(),
        Some(expected_level),
        "restored simulation must be authoritative when loading creates the holder"
    );
    stop_chunk_tasks(&world);
}

#[test]
fn unified_source_phase_updates_an_existing_holder_and_commits_its_receipt() {
    let world = fresh_test_world("unified_ticket_source_phase");
    let pos = ChunkPos::new(7, -5);
    let holder = insert_active_full_holder(&world, pos, ChunkTicketLevel::FULL_CHUNK, Vec::new());
    let player_id = Uuid::from_u128(1);

    let receipt = world.chunk_map.queue_test_player_ticket_add(pos, player_id);
    assert_eq!(holder.simulation_level(), None);
    assert!(!world.chunk_map.is_ticket_receipt_committed(receipt));

    advance_until_receipt(&world.chunk_map, receipt);

    assert_eq!(
        holder.simulation_level(),
        Some(ChunkTicketLevel::for_entity_ticking_radius(
            TEST_VIEW_AND_SIMULATION_DISTANCE_CHUNKS
        ))
    );
    assert_eq!(
        holder.load_level(),
        Some(ChunkTicketLevel::ENTITY_TICKING_CHUNK)
    );
    assert!(world.chunk_map.chunks.contains_sync(&pos));
    stop_chunk_tasks(&world);
}

#[test]
fn simulation_changes_do_not_create_holders() {
    let world = fresh_test_world("simulation_change_without_load");
    let pos = ChunkPos::new(11, -3);

    let _ = world
        .chunk_map
        .apply_simulation_changes(&[SimulationLevelChange {
            pos,
            new_level: Some(ChunkTicketLevel::ENTITY_TICKING_CHUNK),
        }]);

    assert!(!world.chunk_map.chunks.contains_sync(&pos));
    assert!(!world.chunk_map.unloading_chunks.contains_sync(&pos));
    stop_chunk_tasks(&world);
}

#[test]
fn removing_simulation_ticket_keeps_holder_with_load_only_ticket() {
    let world = fresh_test_world("simulation_ticket_removal_keeps_loaded");
    let pos = ChunkPos::new(-8, 6);
    let load_level = ChunkTicketLevel::FULL_CHUNK;
    let player_id = Uuid::from_u128(2);

    let _ = world
        .chunk_map
        .acquire_chunk_request_leases(&[pos], load_level);
    let addition_receipt = world.chunk_map.queue_test_player_ticket_add(pos, player_id);
    advance_until_receipt(&world.chunk_map, addition_receipt);

    let holder = world
        .chunk_map
        .chunks
        .read_sync(&pos, |_, holder| Arc::clone(holder))
        .expect("committed tickets should create an active holder");
    assert_eq!(
        holder.simulation_level(),
        Some(ChunkTicketLevel::for_entity_ticking_radius(
            TEST_VIEW_AND_SIMULATION_DISTANCE_CHUNKS
        ))
    );

    let removal_receipt = world
        .chunk_map
        .queue_test_player_ticket_remove(pos, player_id);
    advance_until_receipt(&world.chunk_map, removal_receipt);

    assert!(
        world
            .chunk_map
            .chunks
            .read_sync(&pos, |_, active| Arc::ptr_eq(active, &holder))
            .unwrap_or(false),
        "the load-only ticket should keep the same holder active"
    );
    assert_eq!(holder.load_level(), Some(ChunkTicketLevel::FULL_CHUNK));
    assert_eq!(holder.simulation_level(), None);
    let _ = world
        .chunk_map
        .release_chunk_request_leases(&[pos], load_level);
    stop_chunk_tasks(&world);
}
