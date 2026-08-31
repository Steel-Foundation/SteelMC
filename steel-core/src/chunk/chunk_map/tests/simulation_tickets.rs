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

fn snapshot_membership(chunk_map: &ChunkMap, pos: ChunkPos) -> (bool, bool, bool) {
    let snapshot = chunk_map.ticking_chunks.load();
    let Some(&index) = snapshot.layout.slot_by_pos.get(&pos) else {
        return (false, false, false);
    };

    (
        snapshot.block.contains(index),
        snapshot.random.contains(index),
        snapshot.entity.contains(index),
    )
}

#[test]
fn restored_portal_ticket_initializes_loading_and_simulation_before_first_flush() {
    init_vanilla_registry();
    init_behaviors();
    let directory = TemporaryWorldDirectory::new("restored-portal-ticket");
    let runtime = Arc::new(Runtime::new().expect("test runtime should initialize"));
    let center = ChunkPos::new(-4, 7);
    let mut ticket_storage = ChunkTicketStorage::new(0);
    assert!(
        ticket_storage
            .add_or_refresh_portal_ticket(center)
            .load_domain_affected
    );
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
            0,
            WorldConfig {
                storage: WorldStorageConfig::RamOnly,
                level_data_path: Some(directory.path_string()),
                generator,
                generation_settings,
                view_distance: 2,
                simulation_distance: 2,
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

    let mut holder = None;
    for _ in 0..10_000 {
        world.chunk_map.advance_scheduling();
        holder = world
            .chunk_map
            .chunks
            .read_sync(&center, |_, holder| Arc::clone(holder));
        if holder.is_some() {
            break;
        }
        thread::sleep(Duration::from_millis(1));
    }
    let holder = holder.expect("restored portal ticket should load its center holder");
    let expected_level = ChunkTicketLevel::for_full_chunk_radius(PORTAL_TICKET_RADIUS);
    assert_eq!(holder.load_level(), Some(expected_level));
    assert_eq!(
        holder.simulation_level(),
        Some(expected_level),
        "restored simulation must be authoritative when loading creates the holder"
    );

    world.chunk_map.flush_simulation_updates();
    assert_eq!(holder.simulation_level(), Some(expected_level));
    stop_chunk_tasks(&world);
}

#[test]
fn simulation_level_classes_publish_immutable_masks_on_a_shared_layout() {
    let world = fresh_test_world("simulation_snapshot_class_transitions");
    let pos = ChunkPos::new(0, 0);
    let adjacent = ChunkPos::new(1, 0);
    let holder = insert_ready_full_chunk(&world, pos);
    holder.swap_load_level(ChunkTicketLevel::ENTITY_TICKING_CHUNK);
    holder.set_simulation_level(None);
    assert_eq!(
        holder.transition_ticking_readiness(TickingReadiness::EntityTicking),
        Some(TickingReadiness::BlockTicking)
    );
    world.chunk_map.rebuild_ticking_chunk_snapshot();

    let entity_a = ChunkTicket::full_chunks_with_entity_ticking(1, 1);
    let entity_b = ChunkTicket::full_chunks_with_entity_ticking(0, 0);
    let _ = world.chunk_map.add_chunk_ticket(pos, entity_b);
    let _ = world.chunk_map.add_chunk_ticket(pos, entity_a);
    let before_initial = world.chunk_map.ticking_chunks.load_full();
    let initial_slot = *before_initial
        .layout
        .slot_by_pos
        .get(&pos)
        .expect("ready holder should have a stable layout slot");
    assert!(!before_initial.block.contains(initial_slot));
    let initial = world.chunk_map.flush_simulation_updates();
    let after_initial = world.chunk_map.ticking_chunks.load_full();
    assert_eq!(
        holder.simulation_level(),
        Some(ChunkTicketLevel::for_entity_ticking_radius(1))
    );
    assert_eq!(
        snapshot_membership(&world.chunk_map, pos),
        (true, true, true)
    );
    assert!(!Arc::ptr_eq(&before_initial, &after_initial));
    assert!(Arc::ptr_eq(&before_initial.layout, &after_initial.layout));
    assert!(
        !before_initial.block.contains(initial_slot),
        "publishing new masks must not mutate a retained snapshot"
    );
    assert_eq!(initial.rebuilt_ticking_chunk_count, 1);

    let _ = world.chunk_map.remove_chunk_ticket(pos, entity_a);
    let before_same_class = world.chunk_map.ticking_chunks.load_full();
    let same_class = world.chunk_map.flush_simulation_updates();
    let after_same_class = world.chunk_map.ticking_chunks.load_full();
    assert_eq!(
        holder.simulation_level(),
        Some(ChunkTicketLevel::ENTITY_TICKING_CHUNK)
    );
    assert_eq!(
        snapshot_membership(&world.chunk_map, pos),
        (true, true, true)
    );
    assert!(Arc::ptr_eq(&before_same_class, &after_same_class));
    assert_eq!(same_class.ticking_snapshot_rebuild, Duration::ZERO);
    assert_eq!(same_class.rebuilt_ticking_chunk_count, 0);

    let _ = world.chunk_map.remove_chunk_ticket(pos, entity_b);
    let _ = world.chunk_map.add_chunk_ticket(adjacent, entity_b);
    let before_entity_to_block = world.chunk_map.ticking_chunks.load_full();
    let entity_to_block = world.chunk_map.flush_simulation_updates();
    let after_entity_to_block = world.chunk_map.ticking_chunks.load_full();
    assert_eq!(
        holder.simulation_level(),
        Some(ChunkTicketLevel::BLOCK_TICKING_CHUNK)
    );
    assert_eq!(
        snapshot_membership(&world.chunk_map, pos),
        (true, false, false)
    );
    assert!(!Arc::ptr_eq(
        &before_entity_to_block,
        &after_entity_to_block
    ));
    assert!(Arc::ptr_eq(
        &before_entity_to_block.layout,
        &after_entity_to_block.layout
    ));
    let old_slot = before_entity_to_block.layout.slot_by_pos[&pos];
    assert!(before_entity_to_block.random.contains(old_slot));
    assert!(before_entity_to_block.entity.contains(old_slot));
    assert_eq!(entity_to_block.rebuilt_ticking_chunk_count, 1);

    let _ = world.chunk_map.remove_chunk_ticket(adjacent, entity_b);
    let before_block_to_absent = world.chunk_map.ticking_chunks.load_full();
    let block_to_absent = world.chunk_map.flush_simulation_updates();
    let after_block_to_absent = world.chunk_map.ticking_chunks.load_full();
    assert_eq!(holder.simulation_level(), None);
    assert_eq!(
        snapshot_membership(&world.chunk_map, pos),
        (false, false, false)
    );
    assert!(!Arc::ptr_eq(
        &before_block_to_absent,
        &after_block_to_absent
    ));
    assert!(Arc::ptr_eq(
        &before_block_to_absent.layout,
        &after_block_to_absent.layout
    ));
    let old_slot = before_block_to_absent.layout.slot_by_pos[&pos];
    assert!(before_block_to_absent.block.contains(old_slot));
    assert_eq!(block_to_absent.rebuilt_ticking_chunk_count, 0);
}

#[test]
fn only_layout_eligibility_changes_replace_the_shared_layout() {
    let world = fresh_test_world("ticking_layout_membership_boundaries");
    let ticking_pos = ChunkPos::new(0, 0);
    let unready_pos = ChunkPos::new(8, 8);
    let ticking_holder = insert_ready_full_chunk(&world, ticking_pos);
    ticking_holder.set_simulation_level(None);
    world.chunk_map.rebuild_ticking_chunk_snapshot();
    let initial = world.chunk_map.ticking_chunks.load_full();

    let unready_holder = world
        .chunk_map
        .update_chunk_level(unready_pos, Some(ChunkTicketLevel::MAX))
        .expect("load churn should create an Unready holder");
    assert_eq!(
        unready_holder.ticking_readiness_snapshot().readiness(),
        TickingReadiness::Unready
    );
    world.chunk_map.update_chunk_level(unready_pos, None);

    let _ = world.chunk_map.add_chunk_ticket(
        ticking_pos,
        ChunkTicket::full_chunks_with_entity_ticking(0, 0),
    );
    world.chunk_map.flush_simulation_updates();
    let after_unready_churn = world.chunk_map.ticking_chunks.load_full();
    assert!(Arc::ptr_eq(&initial.layout, &after_unready_churn.layout));

    let candidate = TickingReadinessCandidate {
        pos: ticking_pos,
        holder: Arc::clone(&ticking_holder),
        desired: TickingReadiness::Unready,
        target: TickingReadiness::Unready,
    };
    assert!(world.chunk_map.apply_readiness_demotions(&[candidate]));
    world.chunk_map.rebuild_ticking_chunk_snapshot();
    let after_eligibility_change = world.chunk_map.ticking_chunks.load_full();
    assert!(!Arc::ptr_eq(
        &after_unready_churn.layout,
        &after_eligibility_change.layout
    ));
    assert!(
        !after_eligibility_change
            .layout
            .slot_by_pos
            .contains_key(&ticking_pos)
    );
}

#[test]
fn simulation_ticket_updates_existing_holder_before_load_epoch_commits() {
    let world = fresh_test_world("simulation_ticket_before_load_epoch");
    let pos = ChunkPos::new(7, -5);
    let holder = insert_active_full_holder(&world, pos, ChunkTicketLevel::FULL_CHUNK, Vec::new());
    let ticket = ChunkTicket::full_chunks_with_entity_ticking(0, 0);

    let receipt = world.chunk_map.add_chunk_ticket(pos, ticket);
    world.chunk_map.flush_simulation_updates();

    assert_eq!(
        holder.simulation_level(),
        Some(ChunkTicketLevel::ENTITY_TICKING_CHUNK)
    );
    assert_eq!(holder.load_level(), Some(ChunkTicketLevel::FULL_CHUNK));
    assert!(world.chunk_map.chunks.contains_sync(&pos));
    assert!(
        !world.chunk_map.is_ticket_receipt_committed(receipt),
        "simulation propagation must not wait for or commit the load epoch"
    );
}

#[test]
fn removing_simulation_ticket_keeps_holder_with_load_only_ticket() {
    let world = fresh_test_world("simulation_ticket_removal_keeps_loaded");
    let pos = ChunkPos::new(-8, 6);
    let load_only_ticket = ChunkTicket::full_chunks(0);
    let simulation_ticket = ChunkTicket::full_chunks_with_entity_ticking(0, 0);

    world.chunk_map.add_chunk_ticket(pos, load_only_ticket);
    let addition_receipt = world.chunk_map.add_chunk_ticket(pos, simulation_ticket);
    advance_until_receipt(&world.chunk_map, addition_receipt);

    let holder = world
        .chunk_map
        .chunks
        .read_sync(&pos, |_, holder| Arc::clone(holder))
        .expect("committed tickets should create an active holder");
    assert_eq!(
        holder.simulation_level(),
        Some(ChunkTicketLevel::ENTITY_TICKING_CHUNK)
    );

    let removal_receipt = world.chunk_map.remove_chunk_ticket(pos, simulation_ticket);
    world.chunk_map.flush_simulation_updates();

    assert_eq!(holder.simulation_level(), None);
    assert!(world.chunk_map.chunks.contains_sync(&pos));

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
    stop_chunk_tasks(&world);
}

#[test]
fn simulation_ticket_waits_for_load_epoch_before_holder_creation() {
    let world = fresh_test_world("load_creation_samples_simulation");
    let pos = ChunkPos::new(-13, -9);
    let ticket = ChunkTicket::full_chunks_with_entity_ticking(0, 0);

    let receipt = world.chunk_map.add_chunk_ticket(pos, ticket);
    world.chunk_map.flush_simulation_updates();
    assert!(!world.chunk_map.chunks.contains_sync(&pos));
    assert!(!world.chunk_map.unloading_chunks.contains_sync(&pos));
    assert!(
        !world.chunk_map.is_ticket_receipt_committed(receipt),
        "simulation propagation must leave the load operation for a background epoch"
    );

    advance_until_receipt(&world.chunk_map, receipt);

    let holder = world
        .chunk_map
        .chunks
        .read_sync(&pos, |_, holder| Arc::clone(holder))
        .expect("the committed load ticket should create a holder");
    assert_eq!(
        holder.simulation_level(),
        Some(ChunkTicketLevel::ENTITY_TICKING_CHUNK)
    );
    stop_chunk_tasks(&world);
}
