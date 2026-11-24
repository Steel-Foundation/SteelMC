use std::{
    env::temp_dir,
    fs,
    future::Future,
    io::Cursor,
    mem,
    path::PathBuf,
    pin::pin,
    sync::{Arc, Weak},
    task::{Context, Poll, Waker},
    thread,
    time::{Duration, Instant},
};

use serde::Deserialize;
use simdnbt::{borrow::read_compound, owned::NbtTag};
use steel_registry::{
    data_components::vanilla_components::{MAP_DECORATIONS, MAP_ID},
    item_stack::ItemStack,
    structure::StructureRef,
    vanilla_items, vanilla_structures,
};
use steel_utils::{
    BlockPos, ChunkPos, Identifier,
    locks::SyncMutex,
    threading::DEBUG_STACK_SIZE,
    types::{Difficulty, GameType},
};
use tokio::{
    runtime::{Builder, Runtime},
    time::{sleep, timeout},
};
use toml::map::Map;
use uuid::Uuid;

use super::MapStorage;
use crate::maps::MapItemSavedData;
use crate::{
    bootstrap::init_globals,
    chunk::status::ChunkStatus,
    entity::damage::DamageHistory,
    level_data::{GameTimeSource, WorldGenerationSettings},
    loot::{LootParams, name_tests::generate_named_map, vanilla_loot_tables},
    maps::chunk_io::MapChunk,
    server::jobs::ServerJobQueue,
    world::{World, WorldConfig, WorldStorageConfig},
    worldgen::WorldGeneratorRegistry,
};

#[path = "generation_tests/lookahead.rs"]
mod lookahead;

#[derive(Deserialize)]
struct VanillaFixture {
    seed: i64,
    overworld: DimensionFixture,
}

impl VanillaFixture {
    fn buried_treasure(&self) -> (&ChunkFixture, &StartFixture) {
        self.overworld
            .chunks
            .iter()
            .find_map(|chunk| {
                chunk
                    .starts
                    .iter()
                    .find(|start| {
                        start.structure == vanilla_structures::STRUCTURE_BURIED_TREASURE.key
                    })
                    .map(|start| (chunk, start))
            })
            .expect("extracted buried treasure")
    }
}
#[derive(Deserialize)]
struct DimensionFixture {
    chunks: Vec<ChunkFixture>,
}
#[derive(Deserialize)]
struct ChunkFixture {
    x: i32,
    z: i32,
    starts: Vec<StartFixture>,
}
#[derive(Deserialize)]
struct StartFixture {
    structure: Identifier,
    bounding_box: Bounds,
}
#[derive(Deserialize)]
struct Bounds {
    min_x: i32,
    min_y: i32,
    min_z: i32,
    max_x: i32,
    max_y: i32,
    max_z: i32,
}

impl Bounds {
    fn intersection(&self, other: &Self) -> Option<BlockPos> {
        let low = BlockPos::new(
            self.min_x.max(other.min_x),
            self.min_y.max(other.min_y),
            self.min_z.max(other.min_z),
        );
        (low.x() <= self.max_x.min(other.max_x)
            && low.y() <= self.max_y.min(other.max_y)
            && low.z() <= self.max_z.min(other.max_z))
        .then_some(low)
    }
}

struct MapWorld {
    world: Arc<World>,
    runtime: Arc<Runtime>,
    pool: Arc<rayon::ThreadPool>,
    config: WorldConfig,
    root: PathBuf,
    seed: i64,
    jobs: Arc<ServerJobQueue>,
}

impl MapWorld {
    fn new(seed: i64, generation_threads: usize) -> Self {
        init_globals();
        let root = temp_dir().join(format!("steel-exploration-map-{}", Uuid::new_v4()));
        let runtime = Arc::new(
            Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("runtime"),
        );
        let pool = Arc::new(
            rayon::ThreadPoolBuilder::new()
                .num_threads(generation_threads)
                .stack_size(DEBUG_STACK_SIZE)
                .build()
                .expect("generation pool"),
        );
        let registry = WorldGeneratorRegistry::new_with_builtins().expect("generators");
        let key = Identifier::vanilla_static("overworld");
        let config = toml::Value::Table(Map::new());
        let validated = registry
            .validate_config(&key, &config)
            .expect("overworld config");
        let generated = registry
            .create(Some(&root), &validated, seed, Arc::clone(&pool))
            .expect("vanilla generator");
        let dimension = generated.dimension_type;
        let config = WorldConfig {
            damage_history: Arc::new(DamageHistory::default()),
            game_time_source: GameTimeSource::Primary,
            storage: WorldStorageConfig::Disk {
                path: root.to_str().expect("test path").to_owned(),
            },
            level_data_path: Some(root.to_str().expect("level path").to_owned()),
            generation_settings: WorldGenerationSettings::from_generator_config(
                key,
                &generated.config,
                dimension.key.clone(),
                dimension.min_y,
                dimension.height,
            ),
            generator: Arc::new(generated.generator),
            view_distance: 2,
            simulation_distance: 2,
            max_chained_neighbor_updates: 1_000_000,
            compression: None,
            is_flat: generated.is_flat,
            sea_level: generated.sea_level,
            default_gamemode: GameType::Survival,
            difficulty: Difficulty::Normal,
        };
        let world = runtime
            .block_on(World::new_with_config(
                Arc::clone(&runtime),
                Identifier::new_static("map_test", "overworld"),
                dimension,
                seed,
                config.clone(),
                Arc::clone(&pool),
            ))
            .expect("world");
        let jobs = Arc::new(ServerJobQueue::new());
        *world.loot_jobs.write() = Arc::downgrade(&jobs);
        world.chunk_map.start_generation_refill_loop();
        Self {
            world,
            runtime,
            pool,
            config,
            root,
            seed,
            jobs,
        }
    }

    async fn drive<T>(&self, future: impl Future<Output = T>) -> T {
        let mut future = pin!(future);
        timeout(Duration::from_secs(120), async {
            let mut tick = 0;
            loop {
                self.world.chunk_map.advance_scheduling();
                self.jobs.tick(Weak::new(), tick, false);
                if let Poll::Ready(result) = future
                    .as_mut()
                    .poll(&mut Context::from_waker(Waker::noop()))
                {
                    return result;
                }
                tick += 1;
                sleep(Duration::from_millis(1)).await;
            }
        })
        .await
        .expect("generation must finish")
    }

    async fn generate_treasure_map(&self, origin: BlockPos) -> ItemStack {
        let output = Arc::new(SyncMutex::new(Vec::new()));
        let deliver = Arc::clone(&output);
        let completion = self
            .world
            .run_loot(
                self.world.prepare_loot(
                    Some(&vanilla_loot_tables::CHESTS_SHIPWRECK_MAP),
                    12345,
                    LootParams {
                        origin: Some(origin.get_center()),
                        ..LootParams::default()
                    },
                ),
                move |result| {
                    *deliver.lock() = result?.into_items();
                    Ok(())
                },
            )
            .expect("exploration loot operation");
        assert!(
            completion.poll_result().is_pending(),
            "unloaded structure lookup must suspend"
        );
        self.drive(completion)
            .await
            .expect("generated exploration loot");
        let items = mem::take(&mut *output.lock());
        items
            .into_iter()
            .find(|item| item.is(&vanilla_items::BURIED_TREASURE_MAP))
            .expect("generated treasure map")
    }

    async fn stop_generation(&self) {
        self.world.chunk_map.stop_generation_refill_loop();
        self.world.chunk_map.task_tracker.close();
        self.world.chunk_map.task_tracker.wait().await;
    }

    #[expect(
        clippy::print_stdout,
        reason = "manual timing harness reports measurements under --nocapture"
    )]
    async fn profile_saved_structure_lookups(&mut self, origin: BlockPos, target: ChunkPos) {
        self.reopen().await;
        let count = self.world.chunk_map.chunks.len();
        let start = Instant::now();
        let found = self
            .drive(self.world.find_nearest_map_structure(
                &[&vanilla_structures::STRUCTURE_BURIED_TREASURE],
                origin,
                0,
                false,
            ))
            .await
            .expect("saved lookup")
            .expect("saved treasure");
        println!(
            "[map-profile] saved structure lookup: {:?}; active chunks: {}",
            start.elapsed(),
            self.world.chunk_map.chunks.len()
        );
        assert_eq!(self.world.chunk_map.chunks.len(), count);
        assert_eq!((found.x(), found.z()), (origin.x(), origin.z()));

        self.reopen().await;
        let start = Instant::now();
        let loaded = self
            .drive(MapChunk::load(
                &self.world,
                target,
                ChunkStatus::StructureStarts,
            ))
            .await
            .expect("full-load reference");
        assert!(
            loaded
                .holder
                .try_chunk(ChunkStatus::StructureStarts)
                .expect("starts")
                .structure_starts()
                .contains_key(&vanilla_structures::STRUCTURE_BURIED_TREASURE.key)
        );
        println!(
            "[map-profile] saved start full-load reference: {:?}; active chunks: {}",
            start.elapsed(),
            self.world.chunk_map.chunks.len()
        );
    }

    async fn reopen(&mut self) {
        self.stop_generation().await;
        self.world
            .chunk_map
            .save_all_chunks()
            .await
            .expect("save generated starts");
        let maps = Arc::clone(&self.world.maps.read());
        maps.save(&self.world.saved_data)
            .await
            .expect("save map data");
        let world = World::new_with_config(
            Arc::clone(&self.runtime),
            self.world.key.clone(),
            self.world.dimension_type,
            self.seed,
            self.config.clone(),
            Arc::clone(&self.pool),
        )
        .await
        .expect("reopened world");
        let maps = MapStorage::load(&world.saved_data)
            .await
            .expect("reload maps");
        *world.maps.write() = Arc::new(maps);
        *world.loot_jobs.write() = Arc::downgrade(&self.jobs);
        self.world = world;
        self.world.chunk_map.start_generation_refill_loop();
    }
}

#[test]
fn generated_exploration_map_and_structure_claims_survive_save_reload() {
    thread::Builder::new()
        .stack_size(DEBUG_STACK_SIZE)
        .spawn(run_generation_and_reload)
        .expect("map test thread")
        .join()
        .expect("map generation test");
}

#[test]
#[ignore = "manual timing harness; reports structure lookup and preview separately"]
#[expect(
    clippy::print_stdout,
    reason = "manual timing harness reports measurements under --nocapture"
)]
fn profile_exploration_map_stages() {
    thread::Builder::new()
        .stack_size(DEBUG_STACK_SIZE)
        .spawn(|| {
            let fixture: VanillaFixture =
                serde_json::from_str(include_str!("../../test_assets/structure_starts.json"))
                    .expect("vanilla structures");
            let (target_chunk, target) = fixture.buried_treasure();
            let origin = BlockPos::new(
                target.bounding_box.min_x,
                target.bounding_box.min_y,
                target.bounding_box.min_z,
            );
            let mut test = MapWorld::new(fixture.seed, 1);
            let runtime = Arc::clone(&test.runtime);
            runtime.block_on(async {
                let start = Instant::now();
                let target = test
                    .drive(test.world.find_nearest_map_structure(
                        &[&vanilla_structures::STRUCTURE_BURIED_TREASURE],
                        origin,
                        50,
                        false,
                    ))
                    .await
                    .expect("structure lookup")
                    .expect("treasure");
                println!(
                    "[map-profile] structure lookup: {:?}; active chunks: {}",
                    start.elapsed(),
                    test.world.chunk_map.chunks.len()
                );
                let data = Arc::new(SyncMutex::new(MapItemSavedData::create_fresh(
                    f64::from(target.x()),
                    f64::from(target.z()),
                    1,
                    true,
                    true,
                    test.world.key.clone(),
                )));
                let start = Instant::now();
                test.drive(MapItemSavedData::render_biome_preview(&data, &test.world))
                    .await
                    .expect("preview");
                println!(
                    "[map-profile] biome preview: {:?}; active chunks: {}",
                    start.elapsed(),
                    test.world.chunk_map.chunks.len()
                );
                assert!(data.lock().colors().iter().any(|color| *color != 0));
                test.profile_saved_structure_lookups(
                    origin,
                    ChunkPos::new(target_chunk.x, target_chunk.z),
                )
                .await;
                test.stop_generation().await;
            });
            let root = test.root.clone();
            drop(test);
            fs::remove_dir_all(root).expect("remove timing world");
        })
        .expect("profiling thread")
        .join()
        .expect("map profile");
}

fn run_generation_and_reload() {
    let fixture: VanillaFixture =
        serde_json::from_str(include_str!("../../test_assets/structure_starts.json"))
            .expect("extracted vanilla structures");
    let (target_chunk, target) = fixture.buried_treasure();
    let target_pos = BlockPos::new(
        target.bounding_box.min_x,
        target.bounding_box.min_y,
        target.bounding_box.min_z,
    );
    let mut test = MapWorld::new(fixture.seed, 1);
    let runtime = Arc::clone(&test.runtime);
    runtime.block_on(async {
        let stack = test.generate_treasure_map(target_pos).await;
        let id = *stack.get(MAP_ID).expect("saved map identity");
        let decorations = stack.get(MAP_DECORATIONS).expect("target marker");
        let marker = decorations
            .decorations()
            .get("+")
            .expect("exploration target");
        assert_eq!(
            (marker.x(), marker.z()),
            (f64::from(target_pos.x()), f64::from(target_pos.z()))
        );
        let data = test.world.get_map_data(id).expect("created map data");
        let pixels = data.lock().colors().to_vec();
        assert!(
            pixels.iter().any(|color| *color != 0),
            "coastal vanilla target must render a biome preview"
        );
        test.drive(test.world.claim_map_structure_at(
            &[&vanilla_structures::STRUCTURE_BURIED_TREASURE],
            target_pos,
        ))
        .await
        .expect("claim generated start");
        let target_start = test
            .drive(MapChunk::load(
                &test.world,
                ChunkPos::new(target_chunk.x, target_chunk.z),
                ChunkStatus::StructureStarts,
            ))
            .await
            .expect("target chunk");
        assert_eq!(
            target_start
                .holder
                .try_chunk(ChunkStatus::StructureStarts)
                .expect("starts")
                .structure_starts()[&vanilla_structures::STRUCTURE_BURIED_TREASURE.key]
                .references,
            1
        );
        drop(target_start);
        verify_overlapping_claims(&test, &fixture).await;
        let named_stack = test
            .drive(generate_named_map(&test.world, target_pos))
            .await;
        let stacks = [stack, named_stack];
        let saved = stacks.iter().map(encode_stack).collect::<Vec<_>>();
        test.reopen().await;
        verify_saved_structure_fast_path(&test, target_chunk, target_pos).await;
        for (bytes, stack) in saved.iter().zip(&stacks) {
            let borrowed =
                read_compound(&mut Cursor::new(bytes.as_slice())).expect("saved map item");
            let restored =
                ItemStack::from_borrowed_compound(&(&borrowed).into()).expect("restored map stack");
            assert_eq!(restored, *stack);
            let map = test
                .world
                .get_map_data(*restored.get(MAP_ID).expect("restored identity"))
                .expect("reloaded pixels");
            assert_eq!(map.lock().colors().as_slice(), pixels);
        }
        let loaded = test
            .drive(MapChunk::load(
                &test.world,
                ChunkPos::new(target_chunk.x, target_chunk.z),
                ChunkStatus::StructureStarts,
            ))
            .await
            .expect("persisted start");
        assert_eq!(
            loaded
                .holder
                .try_chunk(ChunkStatus::StructureStarts)
                .expect("restored starts")
                .structure_starts()[&vanilla_structures::STRUCTURE_BURIED_TREASURE.key]
                .references,
            1
        );
        drop(loaded);
        test.stop_generation().await;
    });
    let root = test.root.clone();
    drop(test);
    fs::remove_dir_all(root).expect("remove generated test world");
}

async fn verify_saved_structure_fast_path(test: &MapWorld, chunk: &ChunkFixture, origin: BlockPos) {
    let count = test.world.chunk_map.chunks.len();
    let info = test
        .world
        .chunk_map
        .storage
        .read_structure_info(ChunkPos::new(chunk.x, chunk.z), &test.pool)
        .await
        .expect("saved start scan")
        .expect("completed saved starts");
    assert!(info.contains(&vanilla_structures::STRUCTURE_BURIED_TREASURE.key, false));
    assert!(!info.contains(&vanilla_structures::STRUCTURE_BURIED_TREASURE.key, true));
    assert!(!info.contains(&vanilla_structures::STRUCTURE_MONUMENT.key, false));
    let found = test
        .drive(test.world.find_nearest_map_structure(
            &[&vanilla_structures::STRUCTURE_BURIED_TREASURE],
            origin,
            0,
            false,
        ))
        .await
        .expect("saved structure lookup")
        .expect("saved treasure");
    assert_eq!((found.x(), found.z()), (origin.x(), origin.z()));
    assert_eq!(
        test.world.chunk_map.chunks.len(),
        count,
        "saved presence must not load gameplay chunks"
    );
}

fn encode_stack(stack: &ItemStack) -> Vec<u8> {
    let NbtTag::Compound(encoded) = stack.to_nbt_tag_ref() else {
        panic!("map stack NBT")
    };
    let mut bytes = Vec::new();
    encoded.write(&mut bytes);
    bytes
}

async fn verify_overlapping_claims(test: &MapWorld, fixture: &VanillaFixture) {
    let starts = fixture
        .overworld
        .chunks
        .iter()
        .flat_map(|chunk| chunk.starts.iter().map(move |start| (chunk, start)))
        .collect::<Vec<_>>();
    let (first, second, origin) = starts
        .iter()
        .find_map(|first| {
            starts.iter().find_map(|second| {
                if first.1.structure == second.1.structure {
                    return None;
                }
                first
                    .1
                    .bounding_box
                    .intersection(&second.1.bounding_box)
                    .map(|origin| (*first, *second, origin))
            })
        })
        .expect("overlapping vanilla starts");
    let structures: [StructureRef; 2] = [
        &vanilla_structures::STRUCTURE_MINESHAFT,
        &vanilla_structures::STRUCTURE_TRIAL_CHAMBERS,
    ];
    assert!(
        structures
            .iter()
            .any(|structure| structure.key == first.1.structure)
    );
    assert!(
        structures
            .iter()
            .any(|structure| structure.key == second.1.structure)
    );
    let origin_chunk = test
        .drive(MapChunk::load(
            &test.world,
            ChunkPos::from_block_pos(origin),
            ChunkStatus::StructureReferences,
        ))
        .await
        .expect("origin references");
    test.drive(test.world.claim_map_structure_at(&structures, origin))
        .await
        .expect("first claim");
    test.drive(test.world.claim_map_structure_at(&structures, origin))
        .await
        .expect("already claimed origin");
    let mut references = 0;
    for (chunk, start) in [first, second] {
        let loaded = test
            .drive(MapChunk::load(
                &test.world,
                ChunkPos::new(chunk.x, chunk.z),
                ChunkStatus::StructureStarts,
            ))
            .await
            .expect("overlapping start");
        references += loaded
            .holder
            .try_chunk(ChunkStatus::StructureStarts)
            .expect("starts")
            .structure_starts()[&start.structure]
            .references;
    }
    assert_eq!(
        references, 1,
        "repeated claims must stop at the same first containing start"
    );
    drop(origin_chunk);
}
