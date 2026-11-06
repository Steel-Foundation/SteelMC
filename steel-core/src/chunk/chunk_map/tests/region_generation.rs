use std::panic;
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use rustc_hash::FxHashMap;
use steel_utils::Identifier;
use steel_utils::types::{Difficulty, GameType};
use steel_worldgen::biomes::BiomeSourceKind;
use steel_worldgen::density_functions::overworld::OverworldNoiseSettings;
use tokio::runtime::Builder as RuntimeBuilder;
use tokio::time::sleep;
use toml::map::Map;

use super::*;
use crate::bootstrap::init_globals;
use crate::chunk::chunk_map::RegionGenerationError;
use crate::chunk::chunk_map::region_generation::region_step_order;
use crate::chunk::chunk_pyramid::GENERATION_PYRAMID;
use crate::chunk::chunk_request::{ChunkRequest, ChunkRequestHandle, ChunkTicketKind, ReadyChunks};
use crate::chunk::light::LightLayer;
use crate::entity::damage::DamageHistory;
use crate::level_data::{GameTimeSource, WorldGenerationSettings};
use crate::world::{WorldConfig, WorldStorageConfig};
use crate::worldgen::{ChunkGeneratorType, OverworldGenerator};

const SEED: i64 = 13_579;

/// A test world and the damage history it holds weakly.
struct GeneratedWorld {
    world: Arc<World>,
    _damage_history: Arc<DamageHistory>,
}

const TEST_STACK_SIZE: usize = 16 * 1024 * 1024;
const TEST_DEADLINE: Duration = Duration::from_secs(600);

/// Every block state, biome and light level of a Full chunk.
#[derive(PartialEq, Eq)]
struct ChunkContents {
    blocks: Vec<u16>,
    biomes: Vec<u16>,
    light: Vec<u8>,
}

impl ChunkContents {
    /// Copies the contents of a Full chunk.
    fn of(holder: &ChunkHolder) -> Self {
        let Some(chunk) = holder.try_chunk(ChunkStatus::Full) else {
            panic!("{:?} should be Full", holder.get_pos());
        };
        let mut blocks = Vec::new();
        let mut biomes = Vec::new();
        for section in &chunk.sections.sections {
            let section = section.read();
            for y in 0..16 {
                for z in 0..16 {
                    for x in 0..16 {
                        blocks.push(section.states.get(x, y, z).0);
                    }
                }
            }
            for y in 0..4 {
                for z in 0..4 {
                    for x in 0..4 {
                        biomes.push(section.biomes.get(x, y, z));
                    }
                }
            }
        }
        let pos = holder.get_pos();
        let light_data = chunk.light();
        let mut light = Vec::new();
        for y in holder.min_y()..holder.min_y() + holder.height() {
            for z in 0..16 {
                for x in 0..16 {
                    let block = BlockPos::new(pos.0.x * 16 + x, y, pos.0.y * 16 + z);
                    light.push(light_data.get_light_value(LightLayer::Sky, block));
                    light.push(light_data.get_light_value(LightLayer::Block, block));
                }
            }
        }
        Self {
            blocks,
            biomes,
            light,
        }
    }
}

/// Opens a RAM-only overworld with `threads` generation workers.
fn overworld(key: &'static str, threads: usize) -> GeneratedWorld {
    init_globals();
    let pool = Arc::new(
        rayon::ThreadPoolBuilder::new()
            .num_threads(threads)
            .stack_size(TEST_STACK_SIZE)
            .build()
            .expect("region generation test pool should build"),
    );
    let runtime = Arc::new(
        RuntimeBuilder::new_multi_thread()
            .thread_stack_size(TEST_STACK_SIZE)
            .enable_all()
            .build()
            .expect("region generation test runtime should build"),
    );
    let generator = Arc::new(ChunkGeneratorType::Overworld(OverworldGenerator::new(
        None,
        BiomeSourceKind::overworld(SEED as u64),
        SEED as u64,
        &pool,
    )));
    let damage_history = Arc::new(DamageHistory::default());
    let config = WorldConfig {
        damage_history: Arc::clone(&damage_history),
        game_time_source: GameTimeSource::Primary,
        storage: WorldStorageConfig::RamOnly,
        level_data_path: None,
        generator,
        generation_settings: WorldGenerationSettings::from_generator_config(
            Identifier::vanilla_static("overworld"),
            &toml::Value::Table(Map::new()),
            OVERWORLD.key.clone(),
            OVERWORLD.min_y,
            OVERWORLD.height,
        ),
        view_distance: 2,
        simulation_distance: 2,
        max_chained_neighbor_updates: 1_000_000,
        compression: None,
        is_flat: false,
        sea_level: OverworldNoiseSettings::SEA_LEVEL,
        default_gamemode: GameType::Survival,
        difficulty: Difficulty::Normal,
    };
    let world = runtime
        .block_on(World::new_with_config(
            Arc::clone(&runtime),
            Identifier::new_static("region_generation", key),
            &OVERWORLD,
            SEED,
            config,
            pool,
        ))
        .expect("region generation test world should build");
    GeneratedWorld {
        world,
        _damage_history: damage_history,
    }
}

/// Lists `min..=max` in ascending X-then-Z order.
fn rectangle(min: ChunkPos, max: ChunkPos) -> Vec<ChunkPos> {
    (min.0.x..=max.0.x)
        .flat_map(|x| (min.0.y..=max.0.y).map(move |z| ChunkPos::new(x, z)))
        .collect()
}

/// Advances scheduling until every chunk of `request` is ready.
async fn await_request(chunk_map: &Arc<ChunkMap>, request: &ChunkRequestHandle) -> ReadyChunks {
    loop {
        chunk_map.advance_scheduling();
        if let Some(ready) = request.ready_chunks() {
            return ready;
        }
        sleep(Duration::from_millis(1)).await;
    }
}

/// Generates through tickets, decorating one chunk at a time in region generation's order.
///
/// Ticket generation otherwise runs neighboring Features steps in scheduling order, and
/// overlapping decorations then differ between runs.
fn generate_with_tickets(
    world: &Arc<World>,
    min: ChunkPos,
    max: ChunkPos,
) -> FxHashMap<ChunkPos, ChunkContents> {
    let chunk_map = &world.chunk_map;
    let feature_radius = GENERATION_PYRAMID
        .get_step_to(ChunkStatus::Full)
        .get_accumulated_radius_of(ChunkStatus::Features) as i32;
    let contents = chunk_map.chunk_runtime.block_on(async {
        let mut decorated = Vec::new();
        let feature_positions = rectangle(
            ChunkPos::new(min.0.x - feature_radius, min.0.y - feature_radius),
            ChunkPos::new(max.0.x + feature_radius, max.0.y + feature_radius),
        );
        for pos in region_step_order(
            GENERATION_PYRAMID.get_step_to(ChunkStatus::Features),
            feature_positions,
        ) {
            let request =
                chunk_map.request_chunk(pos, ChunkStatus::Features, ChunkTicketKind::Command);
            await_request(chunk_map, &request).await;
            decorated.push(request);
        }
        let request = chunk_map.request_chunks(ChunkRequest {
            status: ChunkStatus::Full,
            positions: rectangle(min, max),
            ticket_kind: ChunkTicketKind::Command,
        });
        await_request(chunk_map, &request)
            .await
            .holders
            .iter()
            .map(|holder| (holder.get_pos(), ChunkContents::of(holder)))
            .collect()
    });
    stop_chunk_tasks(world);
    contents
}

/// Generates through [`ChunkMap::generate_region`] and checks the returned order.
fn generate_region(
    world: &Arc<World>,
    min: ChunkPos,
    max: ChunkPos,
) -> FxHashMap<ChunkPos, ChunkContents> {
    stop_chunk_tasks(world);
    let holders = match world
        .chunk_map
        .chunk_runtime
        .block_on(world.chunk_map.generate_region(min, max))
    {
        Ok(holders) => holders,
        Err(error) => panic!("region generation failed: {error}"),
    };
    assert_eq!(
        holders
            .iter()
            .map(|holder| holder.get_pos())
            .collect::<Vec<_>>(),
        rectangle(min, max)
    );
    holders
        .iter()
        .map(|holder| (holder.get_pos(), ChunkContents::of(holder)))
        .collect()
}

/// Runs a test with room for debug-build worldgen recursion, failing it after a deadline.
fn run_generation_test(test: fn()) {
    let (finished, result) = mpsc::channel();
    thread::Builder::new()
        .stack_size(TEST_STACK_SIZE)
        .spawn(move || {
            let _ = finished.send(panic::catch_unwind(test));
        })
        .expect("region generation test thread should spawn");
    match result.recv_timeout(TEST_DEADLINE) {
        Ok(Ok(())) => {}
        Ok(Err(payload)) => panic::resume_unwind(payload),
        Err(error) => panic!("region generation test did not finish: {error}"),
    }
}

/// Unordered ticket generation decorates overlapping neighbors in scheduling order, so the
/// reference requests Features one chunk at a time; region output must match at any thread count.
#[test]
fn region_generation_matches_ordered_ticket_generation() {
    run_generation_test(|| {
        let min = ChunkPos::new(-1, 2);
        let max = ChunkPos::new(1, 3);

        let ticketed = overworld("ticketed", 4);
        let expected = generate_with_tickets(&ticketed.world, min, max);
        for threads in [1, 4] {
            let region = overworld("region", threads);
            let actual = generate_region(&region.world, min, max);
            for pos in rectangle(min, max) {
                assert!(
                    expected[&pos] == actual[&pos],
                    "{threads}-thread region generation diverged from tickets at {pos:?}"
                );
            }
        }
    });
}

/// Returns the largest chunk coordinate whose dependency halo stays within the valid range.
fn region_coordinate_limit() -> i32 {
    let halo = GENERATION_PYRAMID
        .get_step_to(ChunkStatus::Full)
        .get_accumulated_radius_of(ChunkStatus::Empty) as i32;
    ChunkPos::MAX_COORDINATE_VALUE - halo
}

#[test]
fn region_generation_accepts_the_coordinate_limit() {
    let fixture = fresh_test_world("region_generation_limit");
    stop_chunk_tasks(&fixture.world);
    let chunk_map = &fixture.world.chunk_map;
    let edge = ChunkPos::new(region_coordinate_limit(), -region_coordinate_limit());

    let generated = chunk_map
        .chunk_runtime
        .block_on(chunk_map.generate_region(edge, edge));
    assert!(generated.is_ok_and(|holders| holders.len() == 1));
}

#[test]
fn region_generation_rejects_unusable_requests() {
    let fixture = fresh_test_world("region_generation_rejects");
    let chunk_map = &fixture.world.chunk_map;
    let pos = ChunkPos::new(0, 0);

    let running = chunk_map
        .chunk_runtime
        .block_on(chunk_map.generate_region(pos, pos));
    assert_eq!(running.err(), Some(RegionGenerationError::MapInUse));

    stop_chunk_tasks(&fixture.world);
    let inverted = chunk_map
        .chunk_runtime
        .block_on(chunk_map.generate_region(ChunkPos::new(1, 0), pos));
    assert!(matches!(
        inverted,
        Err(RegionGenerationError::InvalidBounds { .. })
    ));
    let beyond_limit = ChunkPos::new(-region_coordinate_limit() - 1, 0);
    let overflowing = chunk_map
        .chunk_runtime
        .block_on(chunk_map.generate_region(beyond_limit, pos));
    assert!(matches!(
        overflowing,
        Err(RegionGenerationError::InvalidBounds { .. })
    ));
    assert!(chunk_map.chunks.is_empty());

    let level = ChunkTicketLevel::MAX;
    let Some(added) = chunk_map.acquire_chunk_request_leases(&[pos], level) else {
        panic!("a request lease should produce a receipt");
    };
    advance_until_receipt(chunk_map, added);
    let Some(removed) = chunk_map.release_chunk_request_leases(&[pos], level) else {
        panic!("a request lease release should produce a receipt");
    };
    advance_until_receipt(chunk_map, removed);
    assert!(chunk_map.chunks.is_empty() && chunk_map.unloading_chunks.contains_sync(&pos));
    let unloading = chunk_map
        .chunk_runtime
        .block_on(chunk_map.generate_region(pos, pos));
    assert_eq!(unloading.err(), Some(RegionGenerationError::MapInUse));
}
