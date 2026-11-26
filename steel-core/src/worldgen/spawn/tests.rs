#![expect(
    clippy::unwrap_used,
    reason = "missing parity fixture data is a test failure"
)]

use std::{borrow::Cow, collections::BTreeSet, panic, sync::Arc, thread};

use glam::DVec3;
use rustc_hash::{FxHashMap, FxHashSet};
use serde::Deserialize;
use steel_registry::{
    DyeColor, REGISTRY, dimension_type::DimensionTypeRef, vanilla_biomes, vanilla_dimension_types,
    vanilla_entities, vanilla_game_rules::SPAWN_MOBS, vanilla_world_clocks,
};
use steel_utils::{
    BlockPos, ChunkPos, Downcast as _, SectionPos,
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
        ChunkGenerator, ChunkGeneratorType, NetherGenerator, OverworldGenerator, WorldGenRegion,
        chunk_stage_hashes::create_test_world,
    },
};

use super::{
    SpawnMode, biome_at, creature_spawn_probability,
    finalize::{
        SpawnContext, SpawnData, SpawnInitError, SpawnInitializer, SpawnOverrides, SpawnState,
        WorldgenSpawnGroup, typed_initializer,
    },
    generate, noise_biome_at,
    placement::Placement,
};

type Position = (i32, i32);

const CHUNK_HASH_TEST_STACK_BYTES: usize = 32 * 1024 * 1024;
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
    source_biome: Option<String>,
    probability_biome: Option<String>,
    creature_probability_bits: Option<String>,
    selection_random_state: RandomState,
    region_random_state: RandomState,
    spawns: Vec<SpawnTrace>,
    hash: String,
}

#[derive(Debug, PartialEq, Eq, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
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

#[derive(Debug, PartialEq, Eq, PartialOrd, Ord, Deserialize)]
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

    fn hash(spawns: &[Self]) -> String {
        let mut digest = md5::Context::new();
        digest.consume(u32::try_from(spawns.len()).unwrap().to_be_bytes());

        for spawn in spawns {
            let entity = spawn.entity.as_bytes();
            digest.consume(u32::try_from(entity.len()).unwrap().to_be_bytes());
            digest.consume(entity);

            for bits in [&spawn.x_bits, &spawn.y_bits, &spawn.z_bits] {
                digest.consume(u64::from_str_radix(bits, 16).unwrap().to_be_bytes());
            }

            digest.consume(
                u32::from_str_radix(&spawn.yaw_bits, 16)
                    .unwrap()
                    .to_be_bytes(),
            );
        }

        format!("{:x}", digest.finalize())
    }
}

struct SpawnTestWorld {
    world: Arc<World>,
    generator: Arc<ChunkGeneratorType>,
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
            Arc::clone(&generator),
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

        Self {
            world,
            generator,
            cache,
        }
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

    fn verify_shadow(&self, expected: &ChunkTrace, biome_seed: i64) -> Vec<SpawnTrace> {
        let center = ChunkPos::new(expected.x, expected.z);
        let step = GENERATION_PYRAMID.get_step_to(ChunkStatus::Spawn);
        let rng = self
            .generator
            .create_worldgen_region_random(self.world.seed(), center);
        let mut region = WorldGenRegion::new(
            &self.world.chunk_map.world_gen_context,
            step,
            &self.cache,
            center,
            rng,
        );

        if expected.spawn_mobs {
            let pos = BlockPos::new(
                SectionPos::section_to_block_coord(expected.x, 0),
                region.max_y_exclusive() - 1,
                SectionPos::section_to_block_coord(expected.z, 0),
            );

            let biome = biome_at(&region, biome_seed, pos);
            let probability_biome = noise_biome_at(&region, pos);
            assert_eq!(
                Some(biome.key.to_string()),
                expected.source_biome,
                "source biome at {center:?}"
            );
            assert_eq!(
                Some(probability_biome.key.to_string()),
                expected.probability_biome,
                "probability biome at {center:?}"
            );

            let probability = creature_spawn_probability(probability_biome);
            assert_eq!(
                Some(format!("{:08x}", probability.to_bits())),
                expected.creature_probability_bits,
                "creature spawn probability at {center:?}"
            );
        }

        let (seed, trace) = trace_without_factories(&mut region, &self.world, biome_seed);
        assert_eq!(
            RandomState::from_legacy_seed(seed),
            expected.selection_random_state,
            "selection RNG state at {center:?}"
        );
        assert_eq!(
            RandomState::from_random(region.random_mut()),
            expected.region_random_state,
            "region RNG state at {center:?}"
        );
        assert_eq!(
            trace.len(),
            expected.spawns.len(),
            "spawn count at {center:?}"
        );
        for (index, ((_, actual), reference)) in trace.iter().zip(&expected.spawns).enumerate() {
            assert_eq!(actual, reference, "spawn {index} at {center:?}");
        }

        let (materializes, spawns): (Vec<_>, Vec<_>) = trace.into_iter().unzip();
        assert_eq!(
            SpawnTrace::hash(&spawns),
            expected.hash,
            "spawn hash at {center:?}"
        );
        materializes
            .into_iter()
            .zip(spawns)
            .filter_map(|(materializes, spawn)| materializes.then_some(spawn))
            .collect()
    }

    async fn verify_chunk(&self, expected: &ChunkTrace) {
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
        let biome_seed = self.generator.original_mob_spawn_biome_seed().unwrap();
        let materialized = self.verify_shadow(expected, biome_seed);
        assert_eq!(
            chunk.get_entities().len(),
            before.len(),
            "shadow entities leaked at {pos:?}"
        );

        self.materialize_and_reload(pos, &before, materialized)
            .await;
    }

    async fn materialize_and_reload(
        &self,
        pos: ChunkPos,
        before: &FxHashSet<i32>,
        mut materialized: Vec<SpawnTrace>,
    ) {
        self.apply(ChunkStatus::Spawn, [(pos.0.x, pos.0.y)]).await;
        let holder = self.cache.get(pos.0.x, pos.0.y);
        let chunk = holder.try_chunk(ChunkStatus::Spawn).unwrap();
        let entities = chunk.get_saveable_entities();
        let mut actual = entities
            .iter()
            .filter(|entity| !before.contains(&entity.id()))
            .map(SpawnTrace::from_entity)
            .collect::<Vec<_>>();
        actual.sort_unstable();
        materialized.sort_unstable();
        assert_eq!(actual, materialized, "materialized mobs at {pos:?}");
        assert_eq!(
            chunk.get_entities().len(),
            before.len() + actual.len(),
            "unsaveable or missing mobs at {pos:?}"
        );
        if !actual.is_empty() {
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
        }
    }
}

fn trace_without_factories(
    region: &mut WorldGenRegion<'_>,
    world: &World,
    biome_seed: i64,
) -> (i64, Vec<(bool, SpawnTrace)>) {
    let mut trace = Vec::new();
    let seed = generate(
        region,
        world,
        world.seed(),
        biome_seed,
        SpawnMode::Trace {
            spawn_mobs: world.get_game_rule(&SPAWN_MOBS),
        },
        |pos, yaw, state| {
            let entity = state.data.entity_type;
            let materializes = ENTITIES.has_factory(entity);
            trace.push((
                materializes,
                SpawnTrace::new(entity.key.to_string(), pos, yaw),
            ));
            if let Some(passenger) = &state.passenger {
                let rider = passenger.entity_type;
                trace.push((
                    materializes && ENTITIES.has_factory(rider),
                    SpawnTrace::new(rider.key.to_string(), pos, yaw),
                ));
            }
        },
    );
    (seed, trace)
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
fn worldgen_spawn_lists_have_registered_rules_and_groups() {
    init_globals();
    for (_, biome) in REGISTRY.biomes.iter() {
        for entry in biome.worldgen_creature_spawns {
            assert!(
                Placement::for_entity(entry.entity_type).is_some(),
                "Missing placement for {} in {}",
                entry.entity_type.key,
                biome.key,
            );
            assert!(
                ENTITIES.worldgen_spawn_group(entry.entity_type).is_some(),
                "Missing spawn group for {} in {}",
                entry.entity_type.key,
                biome.key,
            );
        }
    }
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
                    context.entity_type,
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
                    entity_type: sheep_type,
                    biome: &vanilla_biomes::PLAINS,
                    world: &world,
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
            entity_type: sheep_type,
            biome: &vanilla_biomes::PLAINS,
            world: &world,
            random: &mut random,
        });
        assert!(matches!(
            spawn.data.apply(cow.as_ref()),
            Err(SpawnInitError::WrongEntityType)
        ));
    });
}

#[test]
#[ignore = "Generates the Vanilla fixture clusters through Light; run with --release"]
fn chunk_spawn_hashes() {
    on_worldgen_test_stack(verify_chunk_spawn_hashes);
}

fn on_worldgen_test_stack(run: fn()) {
    let result = thread::Builder::new()
        .stack_size(CHUNK_HASH_TEST_STACK_BYTES)
        .spawn(run)
        .unwrap()
        .join();

    if let Err(payload) = result {
        panic::resume_unwind(payload);
    }
}

fn verify_chunk_spawn_hashes() {
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
    for dimension in DIMENSIONS {
        let trace = &fixture.dimensions[dimension];
        assert_eq!(trace.chunks.len(), CHUNKS_PER_DIMENSION, "{dimension}");
        assert!(
            trace.chunks.iter().any(|chunk| !chunk.spawns.is_empty()),
            "fixture needs successful spawns in {dimension}"
        );
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
                    setup.verify_chunk(expected).await;
                }
            });
        }
    }
}
