#![expect(
    clippy::unwrap_used,
    reason = "missing spawn fixture data is a test failure"
)]

use std::{borrow::Cow, collections::BTreeSet, panic, sync::Arc, thread};

use glam::DVec3;
use rustc_hash::{FxHashMap, FxHashSet};
use serde::Deserialize;
use steel_registry::{
    DyeColor, dimension_type::DimensionTypeRef, vanilla_biomes, vanilla_dimension_types,
    vanilla_entities, vanilla_game_rules::SPAWN_MOBS, vanilla_world_clocks,
};
use steel_utils::{
    BlockPos, ChunkPos, Downcast as _,
    random::{Random, RandomSource, legacy_random::LegacyRandom},
};
use steel_worldgen::biomes::BiomeSourceKind;
use tokio::runtime::Runtime;

use crate::{
    bootstrap::init_globals,
    chunk::{
        chunk_generation_task::StaticCache2D, chunk_holder::ChunkHolder,
        chunk_pyramid::GENERATION_PYRAMID, chunk_ticket_manager::ChunkTicketLevel,
        status::ChunkStatus,
    },
    chunk_saver::ChunkStorage,
    entity::{
        ENTITIES, EntityRegistry, MobSpawnState, SharedEntity, damage::DamageHistory,
        entities::mobs::passive::SheepEntity,
    },
    world::World,
    worldgen::{
        ChunkGeneratorType, NetherGenerator, OverworldGenerator, WorldGenRegion,
        chunk_stage_hashes::create_test_world,
    },
};

use super::{
    SpawnSite,
    finalize::{
        SpawnContext, SpawnData, SpawnInitError, SpawnInitializer, SpawnOverrides, SpawnState,
        WorldgenSpawnGroup, typed_initializer,
    },
    rules::{WorldgenSpawnPredicate, WorldgenSpawnRule},
};

type Position = (i32, i32);

const SPAWN_TEST_STACK_BYTES: usize = 32 * 1024 * 1024;
const CHUNKS_PER_DIMENSION: usize = 200;
const DIMENSIONS: [&str; 2] = ["minecraft:overworld", "minecraft:the_nether"];

#[derive(Deserialize)]
struct Fixture {
    seed: u64,
    minecraft_version: String,
    cluster_origins: Option<Vec<Position>>,
    cluster_size: Option<i32>,
    dimensions: FxHashMap<String, DimensionTrace>,
}

impl Fixture {
    fn cluster_layout(&self, chunks: &[ChunkTrace]) -> (Cow<'_, [Position]>, i32) {
        let (origins, size) = match (self.cluster_origins.as_deref(), self.cluster_size) {
            (Some(origins), Some(size)) => (Cow::Borrowed(origins), size),
            (None, None) => {
                let min_x = chunks.iter().map(|chunk| chunk.x).min().unwrap();
                let min_z = chunks.iter().map(|chunk| chunk.z).min().unwrap();
                let width = chunks.iter().map(|chunk| chunk.x).max().unwrap() - min_x + 1;
                let height = chunks.iter().map(|chunk| chunk.z).max().unwrap() - min_z + 1;
                assert_eq!(
                    width, height,
                    "legacy fixture must contain one square cluster"
                );

                (Cow::Owned(vec![(min_x, min_z)]), width)
            }
            _ => panic!("fixture cluster origins and size must be provided together"),
        };

        let cluster_width = usize::try_from(size).unwrap();
        assert!(
            cluster_width > 0 && !origins.is_empty(),
            "spawn fixture has no cluster"
        );
        assert_eq!(
            chunks.len(),
            origins.len() * cluster_width * cluster_width,
            "incomplete spawn fixture clusters"
        );

        (origins, size)
    }
}

#[derive(Deserialize)]
struct DimensionTrace {
    chunks: Vec<ChunkTrace>,
}

#[derive(Deserialize)]
struct ChunkTrace {
    x: i32,
    z: i32,
    clock_time: i64,
    difficulty: String,
    spawn_mobs: bool,
}

#[derive(Debug, PartialEq, Eq)]
enum RandomState {
    Legacy { seed: String },
    Xoroshiro { seed_lo: String, seed_hi: String },
}

impl RandomState {
    fn from_legacy_seed(seed: i64) -> Self {
        Self::Legacy {
            seed: format!("{seed:016x}"),
        }
    }

    fn from_random(random: &RandomSource) -> Self {
        match random {
            RandomSource::Legacy(random) => Self::from_legacy_seed(random.get_seed()),
            RandomSource::Xoroshiro(random) => {
                let [seed_lo, seed_hi] = random.state();
                Self::Xoroshiro {
                    seed_lo: format!("{seed_lo:016x}"),
                    seed_hi: format!("{seed_hi:016x}"),
                }
            }
        }
    }
}

#[derive(Debug, PartialEq, Eq, PartialOrd, Ord)]
struct SpawnTrace {
    entity: String,
    x_bits: String,
    y_bits: String,
    z_bits: String,
    yaw_bits: String,
}

impl SpawnTrace {
    fn new(entity: String, pos: DVec3, yaw: f32) -> Self {
        Self {
            entity,
            x_bits: format!("{:016x}", pos.x.to_bits()),
            y_bits: format!("{:016x}", pos.y.to_bits()),
            z_bits: format!("{:016x}", pos.z.to_bits()),
            yaw_bits: format!("{:08x}", yaw.to_bits()),
        }
    }

    fn from_entity(entity: &SharedEntity) -> Self {
        Self::new(
            entity.entity_type().key.to_string(),
            entity.position(),
            entity.rotation().0,
        )
    }
}

struct SpawnTestWorld {
    world: Arc<World>,
    cache: Arc<StaticCache2D<Arc<ChunkHolder>>>,
}

impl SpawnTestWorld {
    fn new(dim_key: &str, seed: u64, origin: Position, size: i32) -> Self {
        let pool = Arc::new(
            rayon::ThreadPoolBuilder::new()
                .num_threads(1)
                .build()
                .unwrap(),
        );

        let (dim, generator): (DimensionTypeRef, ChunkGeneratorType) = match dim_key {
            "minecraft:overworld" => (
                &vanilla_dimension_types::OVERWORLD,
                ChunkGeneratorType::Overworld(OverworldGenerator::new(
                    None,
                    BiomeSourceKind::overworld(seed),
                    seed,
                    &pool,
                )),
            ),
            "minecraft:the_nether" => (
                &vanilla_dimension_types::THE_NETHER,
                ChunkGeneratorType::Nether(NetherGenerator::new(
                    None,
                    BiomeSourceKind::nether(seed),
                    seed,
                    &pool,
                )),
            ),
            _ => panic!("Unknown fixture dimension {dim_key}"),
        };

        let generator = Arc::new(generator);
        let world = create_test_world(
            dim_key,
            dim,
            seed,
            generator,
            pool,
            &Arc::new(DamageHistory::default()),
        );

        let radius = size / 2 + preparation_radius(ChunkStatus::Empty);
        let cache = Arc::new(StaticCache2D::create(
            origin.0 + size / 2,
            origin.1 + size / 2,
            radius,
            |x, z| {
                Arc::new(ChunkHolder::new(
                    ChunkPos::new(x, z),
                    ChunkTicketLevel::STRONGEST,
                    None,
                    dim.min_y,
                    dim.height,
                ))
            },
        ));

        Self { world, cache }
    }

    async fn apply(&self, status: ChunkStatus, positions: impl IntoIterator<Item = Position>) {
        let map = &self.world.chunk_map;
        let step = GENERATION_PYRAMID.get_step_to(status);

        for (x, z) in positions {
            let holder = self.cache.get(x, z);
            if let Some(ready) =
                holder.apply_step(step, map, &self.cache, Arc::clone(&map.generation_pool))
            {
                assert!(
                    ready.await.is_some(),
                    "failed to generate {status:?} at ({x}, {z})"
                );
            }
            assert!(
                holder.try_chunk(status).is_some(),
                "missing {status:?} at ({x}, {z})"
            );
        }
    }

    async fn prepare(&self, tracked: &BTreeSet<Position>) {
        for index in ChunkStatus::Empty.get_index()..ChunkStatus::Features.get_index() {
            let status = ChunkStatus::from_index(index).unwrap();
            self.apply(status, expanded(tracked, preparation_radius(status)))
                .await;
        }

        let features = expanded(tracked, preparation_radius(ChunkStatus::Features));
        self.apply(ChunkStatus::Features, tracked.iter().copied())
            .await;
        self.apply(ChunkStatus::Features, features.difference(tracked).copied())
            .await;

        let init = preparation_radius(ChunkStatus::InitializeLight);
        self.apply(ChunkStatus::InitializeLight, expanded(tracked, init))
            .await;
        let light = preparation_radius(ChunkStatus::Light);
        self.apply(ChunkStatus::Light, expanded(tracked, light))
            .await;
    }

    async fn verify_chunk(&self, expected: &ChunkTrace) -> bool {
        let pos = ChunkPos::new(expected.x, expected.z);
        let holder = self.cache.get(expected.x, expected.z);
        let chunk = holder.try_chunk(ChunkStatus::Light).unwrap();
        assert_eq!(self.world.difficulty().name(), expected.difficulty);
        self.world
            .set_clock_total_ticks(&vanilla_world_clocks::OVERWORLD, expected.clock_time)
            .unwrap();
        self.world.set_game_rule(&SPAWN_MOBS, expected.spawn_mobs);
        let before = chunk
            .get_entities()
            .iter()
            .map(|entity| entity.id())
            .collect::<FxHashSet<_>>();
        self.materialize_and_reload(pos, &before).await
    }

    async fn materialize_and_reload(&self, pos: ChunkPos, before: &FxHashSet<i32>) -> bool {
        self.apply(ChunkStatus::Spawn, [(pos.0.x, pos.0.y)]).await;
        let holder = self.cache.get(pos.0.x, pos.0.y);
        let chunk = holder.try_chunk(ChunkStatus::Spawn).unwrap();
        let entities = chunk.get_saveable_entities();
        let new_entity_count = entities
            .iter()
            .filter(|entity| !before.contains(&entity.id()))
            .count();
        assert_eq!(
            chunk.get_entities().len(),
            before.len() + new_entity_count,
            "unsaveable or missing mobs at {pos:?}"
        );
        if new_entity_count == 0 {
            return false;
        }

        let map = &self.world.chunk_map;
        let storage = &map.storage;
        let prepared =
            ChunkStorage::prepare_chunk_save(chunk, ChunkStatus::Spawn, &[], true).unwrap();
        assert!(
            storage
                .save_chunk_data(prepared, &map.generation_pool)
                .await
                .unwrap()
        );
        let loaded = storage
            .load_chunk(
                pos,
                holder.min_y(),
                holder.height(),
                Arc::downgrade(&self.world),
                &map.generation_pool,
            )
            .await
            .unwrap()
            .unwrap();
        assert_eq!(loaded.status, ChunkStatus::Spawn);
        assert!(loaded.pending_entities.is_empty());
        let saved_trace = |entities: &[SharedEntity]| {
            let mut trace = entities
                .iter()
                .map(SpawnTrace::from_entity)
                .collect::<Vec<_>>();
            trace.sort_unstable();
            trace
        };
        assert_eq!(
            saved_trace(&loaded.chunk.get_saveable_entities()),
            saved_trace(&entities),
            "mobs after save/load at {pos:?}"
        );
        true
    }
}

fn preparation_radius(status: ChunkStatus) -> i32 {
    let spawn = GENERATION_PYRAMID.get_step_to(ChunkStatus::Spawn);
    let light = GENERATION_PYRAMID.get_step_to(ChunkStatus::Light);
    let spawn_radius = spawn.get_accumulated_radius_of(status);
    let light_radius =
        light.direct_dependencies.get_radius() + light.get_accumulated_radius_of(status);
    spawn_radius.max(light_radius) as i32
}

fn expanded(positions: &BTreeSet<Position>, radius: i32) -> BTreeSet<Position> {
    positions
        .iter()
        .flat_map(|&(x, z)| {
            (x - radius..=x + radius)
                .flat_map(move |x| (z - radius..=z + radius).map(move |z| (x, z)))
        })
        .collect()
}

#[test]
fn custom_spawn_predicate_advances_region_rng_on_rejection() {
    const SEED: u64 = 42;
    const POS: BlockPos = BlockPos::new(1, 64, 1);

    init_globals();
    let setup = SpawnTestWorld::new("minecraft:overworld", SEED, (0, 0), 1);
    let mut region = WorldGenRegion::new(
        &setup.world.chunk_map.world_gen_context,
        GENERATION_PYRAMID.get_step_to(ChunkStatus::Spawn),
        &setup.cache,
        ChunkPos::new(0, 0),
        RandomSource::Legacy(LegacyRandom::from_seed(SEED)),
    );
    let rule = WorldgenSpawnRule::new(WorldgenSpawnPredicate::Custom(|context| {
        assert_eq!(context.site.pos, POS);
        let _ = context.random_mut().next_f32();
        false
    }));

    assert!(!rule.check(
        &mut region,
        0,
        SpawnSite {
            entity_type: &vanilla_entities::SHEEP,
            pos: POS,
        },
    ));
    let mut expected = LegacyRandom::from_seed(SEED);
    let _ = expected.next_f32();
    assert_eq!(
        RandomState::from_random(region.random_mut()),
        RandomState::from_legacy_seed(expected.get_seed()),
    );
}

#[test]
fn registered_group_initializes_entities_without_changing_spawn_rng() {
    struct CustomSheepGroup {
        remaining_red: i32,
    }

    impl WorldgenSpawnGroup for CustomSheepGroup {
        fn finalize(&mut self, context: SpawnContext<'_>) -> SpawnState<'_> {
            let initialize: &SpawnInitializer<'_> = if self.remaining_red > 0 {
                self.remaining_red -= 1;
                &|entity| {
                    typed_initializer(|sheep: &SheepEntity| sheep.set_color(DyeColor::Red))(entity)
                }
            } else {
                let _ = context.random.next_f32();
                &|entity| {
                    typed_initializer(|sheep: &SheepEntity| sheep.set_color(DyeColor::Blue))(entity)
                }
            };
            SpawnState {
                data: SpawnData::new(
                    context.site.entity_type,
                    MobSpawnState::sample(context.random),
                    SpawnOverrides::default(),
                    Some(initialize),
                ),
                passenger: None,
            }
        }
    }

    init_globals();
    let world = SpawnTestWorld::new("minecraft:overworld", 0, (0, 0), 1).world;
    let mut registry = EntityRegistry::new();
    let sheep_type = &vanilla_entities::SHEEP;
    registry.register_worldgen_spawn_group(sheep_type, |run| {
        run(&mut CustomSheepGroup { remaining_red: 1 });
    });
    let with_group = registry.worldgen_spawn_group(sheep_type).unwrap();

    let run = |materialize| {
        let mut random = RandomSource::Legacy(LegacyRandom::from_seed(0));
        with_group(&mut |group| {
            for color in [DyeColor::Red, DyeColor::Blue] {
                let spawn = group.finalize(SpawnContext {
                    site: SpawnSite {
                        entity_type: sheep_type,
                        pos: BlockPos::ZERO,
                    },
                    biome: &vanilla_biomes::PLAINS,
                    random: &mut random,
                });
                if materialize {
                    let sheep = ENTITIES
                        .create(sheep_type, DVec3::ZERO, Arc::downgrade(&world))
                        .unwrap();
                    spawn.data.apply(sheep.as_ref()).unwrap();
                    assert_eq!(sheep.downcast_ref::<SheepEntity>().unwrap().color(), color);
                }
            }
        });
        RandomState::from_random(&random)
    };

    assert_eq!(run(false), run(true));
    let cow = ENTITIES
        .create(&vanilla_entities::COW, DVec3::ZERO, Arc::downgrade(&world))
        .unwrap();
    with_group(&mut |group| {
        let mut random = RandomSource::Legacy(LegacyRandom::from_seed(0));
        let spawn = group.finalize(SpawnContext {
            site: SpawnSite {
                entity_type: sheep_type,
                pos: BlockPos::ZERO,
            },
            biome: &vanilla_biomes::PLAINS,
            random: &mut random,
        });
        assert!(matches!(
            spawn.data.apply(cow.as_ref()),
            Err(SpawnInitError::WrongEntityType)
        ));
    });
}

#[test]
#[ignore = "Generates spawn fixture clusters through Light; run with --release"]
fn chunk_spawn_save_reload() {
    on_worldgen_test_stack(verify_chunk_spawn_save_reload);
}

fn on_worldgen_test_stack(run: fn()) {
    let result = thread::Builder::new()
        .stack_size(SPAWN_TEST_STACK_BYTES)
        .spawn(run)
        .unwrap()
        .join();

    if let Err(payload) = result {
        panic::resume_unwind(payload);
    }
}

fn verify_chunk_spawn_save_reload() {
    init_globals();
    let fixture: Fixture =
        serde_json::from_str(include_str!("../../../test_assets/chunk_spawn_hashes.json")).unwrap();
    let version = env!("CARGO_PKG_VERSION");
    assert_eq!(
        version.split_once("+mc").unwrap().1,
        fixture.minecraft_version
    );
    assert_eq!(fixture.dimensions.len(), DIMENSIONS.len());
    let runtime = Runtime::new().unwrap();
    let mut verified_spawns = false;
    for dimension in DIMENSIONS {
        let trace = &fixture.dimensions[dimension];
        assert_eq!(trace.chunks.len(), CHUNKS_PER_DIMENSION, "{dimension}");
        let (origins, size) = fixture.cluster_layout(&trace.chunks);
        let chunks = trace
            .chunks
            .iter()
            .map(|trace| ((trace.x, trace.z), trace))
            .collect::<FxHashMap<_, _>>();
        assert_eq!(
            chunks.len(),
            trace.chunks.len(),
            "duplicate {dimension} traces"
        );
        for &origin in origins.as_ref() {
            let tracked = (origin.0..origin.0 + size)
                .flat_map(|x| (origin.1..origin.1 + size).map(move |z| (x, z)))
                .collect::<BTreeSet<_>>();
            let setup = SpawnTestWorld::new(dimension, fixture.seed, origin, size);
            runtime.block_on(async {
                setup.prepare(&tracked).await;
                for pos in tracked {
                    let expected = chunks
                        .get(&pos)
                        .unwrap_or_else(|| panic!("missing {dimension} trace for {pos:?}"));
                    verified_spawns |= setup.verify_chunk(expected).await;
                }
            });
        }
    }
    assert!(
        verified_spawns,
        "fixture generated no mobs to save and reload"
    );
}
