#![feature(portable_simd)]
#![expect(missing_docs, clippy::similar_names, reason = "benchmarks")]

use criterion::{Criterion, Throughput, criterion_group, criterion_main};
use futures::future::join_all;
use glam::IVec3;
use std::cmp::Reverse;
use std::env;
use std::hint::black_box;
use std::sync::{
    Arc, LazyLock, Once, Weak,
    atomic::{AtomicU64, Ordering},
};
use std::time::{Duration, Instant};
use steel_core::behavior::init_behaviors;
use steel_core::block_entity::init_block_entities;
use steel_core::chunk::chunk_access::{ChunkAccess, ChunkStatus};
use steel_core::chunk::chunk_generation_task::StaticCache2D;
use steel_core::chunk::chunk_holder::ChunkHolder;
use steel_core::chunk::chunk_map::ChunkMap;
use steel_core::chunk::chunk_pyramid::{ChunkDependencies, ChunkStep, GENERATION_PYRAMID};
use steel_core::chunk::chunk_status_tasks::ChunkStatusTasks;
use steel_core::chunk::chunk_ticket_manager::{ChunkTicketLevel, MAX_VIEW_DISTANCE};
use steel_core::chunk::proto_chunk::ProtoChunk;
use steel_core::chunk::section::{ChunkSection, Sections};
use steel_core::entity::init_entities;
use steel_core::level_data::WorldGenerationSettings;
use steel_core::world::{World, WorldConfig, WorldStorageConfig};
use steel_core::worldgen::{
    ChunkGenerator, ChunkGeneratorType, EndGenerator, GeneratorOutput, NetherGenerator,
    OverworldGenerator, WorldGenContext, WorldGeneratorRegistry,
};
use steel_registry::dimension_type::DimensionType;
use steel_registry::{REGISTRY, Registry, vanilla_dimension_types};
use steel_utils::locks::SyncMutex;
use steel_utils::types::{Difficulty, GameType};
use steel_utils::{ChunkPos, Identifier};
use steel_worldgen::biomes::{BiomeSourceKind, ChunkBiomeSampler};
use tokio::runtime::{Builder as RuntimeBuilder, Runtime};
use toml::map::Map;

static INIT: Once = Once::new();
static FEATURE_BATCH_PROFILE_LOGS: AtomicU64 = AtomicU64::new(0);
static FEATURE_BATCH_PROFILE_LOG_LIMIT: LazyLock<u64> = LazyLock::new(|| {
    env::var("STEEL_FEATURE_BATCH_PROFILE_LOG_LIMIT")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(16)
});
static FULL_PIPELINE_PROFILE_LOGS: AtomicU64 = AtomicU64::new(0);
static FULL_PIPELINE_PROFILE_LOG_LIMIT: LazyLock<u64> = LazyLock::new(|| {
    env::var("STEEL_FULL_PIPELINE_PROFILE_LOG_LIMIT")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(16)
});
static LIGHT_PROFILE_LOGS: AtomicU64 = AtomicU64::new(0);
static LIGHT_PROFILE_LOG_LIMIT: LazyLock<u64> = LazyLock::new(|| {
    env::var("STEEL_LIGHT_PROFILE_LOG_LIMIT")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(16)
});
const BENCH_HOLDER_LOAD_LEVEL: ChunkTicketLevel =
    ChunkTicketLevel::for_full_chunk_radius(MAX_VIEW_DISTANCE);

fn ensure_registry() {
    INIT.call_once(|| {
        let mut registry = Registry::new_vanilla();
        registry.freeze();
        let _ = REGISTRY.init(registry);
        init_behaviors();
        init_block_entities();
        init_entities();
    });
}

fn create_benchmark_generator(
    generator_key: &Identifier,
    seed: i64,
    context: &str,
) -> GeneratorOutput {
    let generator_config = toml::Value::Table(Map::new());
    let registry = WorldGeneratorRegistry::new_with_builtins()
        .expect("built-in world generators should register");
    let generator_config = registry
        .validate_config(generator_key, &generator_config)
        .expect(context);
    registry.create(&generator_config, seed).expect(context)
}

fn make_proto_chunk(chunk_x: i32, chunk_z: i32, dim: &DimensionType) -> ChunkAccess {
    let section_count = (dim.height / 16) as usize;
    let sections: Box<[ChunkSection]> = (0..section_count)
        .map(|_| ChunkSection::new_empty())
        .collect();
    let sections = Sections::from_owned(sections);
    let pos = ChunkPos::new(chunk_x, chunk_z);
    ChunkAccess::Proto(ProtoChunk::new(
        sections,
        pos,
        dim.min_y,
        dim.height,
        Weak::new(),
    ))
}

/// Build a `neighbor_biomes` closure that reads from the chunk's own sections.
///
/// In a real pipeline this reads from a neighbor cache, but for a single-chunk
/// benchmark the chunk is its own neighbor (biome lookups near edges will
/// wrap but that's fine for timing).
fn self_neighbor_biomes(chunk: &ChunkAccess) -> impl Fn(IVec3) -> u16 + '_ {
    let sections = chunk.sections();
    let min_qy = chunk.min_y() >> 2;
    let total_quarts_y = (sections.sections.len() * 4) as i32;

    move |q: IVec3| -> u16 {
        let local_qx = q.x.rem_euclid(4) as usize;
        let local_qz = q.z.rem_euclid(4) as usize;
        let qy_clamped = (q.y - min_qy).clamp(0, total_quarts_y - 1) as usize;
        let section_idx = qy_clamped / 4;
        let local_qy = qy_clamped % 4;
        sections.sections[section_idx]
            .read()
            .biomes
            .get(local_qx, local_qy, local_qz)
    }
}

/// Sample all biome positions for a chunk using column-major iteration.
///
/// Iterates X → Z → sections → Y so the column cache in the sampler
/// is effective (all Y values for a column are sampled consecutively).
fn sample_chunk_biomes(
    sampler: &mut ChunkBiomeSampler<'_>,
    chunk_x: i32,
    chunk_z: i32,
    min_section_y: i32,
    section_count: i32,
) {
    for lx in 0..4i32 {
        for lz in 0..4i32 {
            for section_index in 0..section_count {
                let section_y = min_section_y + section_index;
                for ly in 0..4i32 {
                    let qx = chunk_x * 4 + lx;
                    let qy = section_y * 4 + ly;
                    let qz = chunk_z * 4 + lz;
                    black_box(sampler.sample(qx, qy, qz));
                }
            }
        }
    }
}

// ── Biome benchmarks ────────────────────────────────────────────────────────

fn bench_overworld_biome(c: &mut Criterion) {
    let dim = &vanilla_dimension_types::OVERWORLD;
    let source = BiomeSourceKind::overworld(0);
    c.bench_function("overworld_biome", |b| {
        b.iter(|| {
            let mut sampler = source.chunk_sampler();
            sample_chunk_biomes(
                &mut sampler,
                black_box(0),
                black_box(0),
                dim.min_y >> 4,
                dim.height / 16,
            );
        });
    });
}

fn bench_nether_biome(c: &mut Criterion) {
    let dim = &vanilla_dimension_types::THE_NETHER;
    let source = BiomeSourceKind::nether(0);
    c.bench_function("nether_biome", |b| {
        b.iter(|| {
            let mut sampler = source.chunk_sampler();
            sample_chunk_biomes(
                &mut sampler,
                black_box(0),
                black_box(0),
                dim.min_y >> 4,
                dim.height / 16,
            );
        });
    });
}

fn bench_end_biome(c: &mut Criterion) {
    let dim = &vanilla_dimension_types::THE_END;
    let source = BiomeSourceKind::end(0);
    c.bench_function("end_biome", |b| {
        b.iter(|| {
            let mut sampler = source.chunk_sampler();
            sample_chunk_biomes(
                &mut sampler,
                black_box(0),
                black_box(0),
                dim.min_y >> 4,
                dim.height / 16,
            );
        });
    });
}

// ── Noise benchmarks ────────────────────────────────────────────────────────

fn bench_overworld_noise(c: &mut Criterion) {
    ensure_registry();
    let dim = &vanilla_dimension_types::OVERWORLD;
    let source = BiomeSourceKind::overworld(0);
    let generator = OverworldGenerator::new(source, 0);

    c.bench_function("overworld_fill_from_noise", |b| {
        b.iter(|| {
            let chunk = make_proto_chunk(black_box(0), black_box(0), dim);
            generator.fill_from_noise(&chunk, None);
        });
    });
}

fn bench_nether_noise(c: &mut Criterion) {
    ensure_registry();
    let dim = &vanilla_dimension_types::THE_NETHER;
    let source = BiomeSourceKind::nether(0);
    let generator = NetherGenerator::new(source, 0);

    c.bench_function("nether_fill_from_noise", |b| {
        b.iter(|| {
            let chunk = make_proto_chunk(black_box(0), black_box(0), dim);
            generator.fill_from_noise(&chunk, None);
        });
    });
}

fn bench_end_noise(c: &mut Criterion) {
    ensure_registry();
    let dim = &vanilla_dimension_types::THE_END;
    let source = BiomeSourceKind::end(0);
    let generator = EndGenerator::new(source, 0);

    c.bench_function("end_fill_from_noise", |b| {
        b.iter(|| {
            let chunk = make_proto_chunk(black_box(0), black_box(0), dim);
            generator.fill_from_noise(&chunk, None);
        });
    });
}

// ── Surface benchmarks ──────────────────────────────────────────────────────

fn bench_overworld_surface(c: &mut Criterion) {
    ensure_registry();
    let dim = &vanilla_dimension_types::OVERWORLD;
    let source = BiomeSourceKind::overworld(0);
    let generator = OverworldGenerator::new(source, 0);

    c.bench_function("overworld_build_surface", |b| {
        b.iter_batched(
            || {
                let chunk = make_proto_chunk(0, 0, dim);
                generator.create_biomes(&chunk);
                generator.fill_from_noise(&chunk, None);
                chunk
            },
            |chunk| {
                let neighbor_biomes = self_neighbor_biomes(&chunk);
                generator.build_surface(black_box(&chunk), &neighbor_biomes);
            },
            criterion::BatchSize::SmallInput,
        );
    });
}

fn bench_nether_surface(c: &mut Criterion) {
    ensure_registry();
    let dim = &vanilla_dimension_types::THE_NETHER;
    let source = BiomeSourceKind::nether(0);
    let generator = NetherGenerator::new(source, 0);

    c.bench_function("nether_build_surface", |b| {
        b.iter_batched(
            || {
                let chunk = make_proto_chunk(0, 0, dim);
                generator.create_biomes(&chunk);
                generator.fill_from_noise(&chunk, None);
                chunk
            },
            |chunk| {
                let neighbor_biomes = self_neighbor_biomes(&chunk);
                generator.build_surface(black_box(&chunk), &neighbor_biomes);
            },
            criterion::BatchSize::SmallInput,
        );
    });
}

fn bench_end_surface(c: &mut Criterion) {
    ensure_registry();
    let dim = &vanilla_dimension_types::THE_END;
    let source = BiomeSourceKind::end(0);
    let generator = EndGenerator::new(source, 0);

    c.bench_function("end_build_surface", |b| {
        b.iter_batched(
            || {
                let chunk = make_proto_chunk(0, 0, dim);
                generator.create_biomes(&chunk);
                generator.fill_from_noise(&chunk, None);
                chunk
            },
            |chunk| {
                let neighbor_biomes = self_neighbor_biomes(&chunk);
                generator.build_surface(black_box(&chunk), &neighbor_biomes);
            },
            criterion::BatchSize::SmallInput,
        );
    });
}

// ── Recalculate-counts benchmarks ──────────────────────────────────────────

/// `recalculate_counts` runs after worldgen writes finalize section palettes and
/// when chunks load from disk. This tracks the palette-counting path over a full
/// overworld chunk's section set.
fn bench_overworld_recalculate_counts(c: &mut Criterion) {
    ensure_registry();
    let dim = &vanilla_dimension_types::OVERWORLD;
    let source = BiomeSourceKind::overworld(0);
    let generator = OverworldGenerator::new(source, 0);

    c.bench_function("overworld_recalculate_counts", |b| {
        b.iter_batched(
            || {
                let chunk = make_proto_chunk(0, 0, dim);
                generator.create_biomes(&chunk);
                generator.fill_from_noise(&chunk, None);
                chunk
            },
            |chunk| {
                for section in &chunk.sections().sections {
                    section.write().recalculate_counts();
                }
            },
            criterion::BatchSize::SmallInput,
        );
    });
}

// ── Carvers benchmarks ──────────────────────────────────────────────────────

fn bench_overworld_carvers(c: &mut Criterion) {
    ensure_registry();
    let dim = &vanilla_dimension_types::OVERWORLD;
    let source = BiomeSourceKind::overworld(0);
    let generator = OverworldGenerator::new(source, 0);

    c.bench_function("overworld_apply_carvers", |b| {
        b.iter_batched(
            || {
                let chunk = make_proto_chunk(0, 0, dim);
                generator.create_biomes(&chunk);
                generator.fill_from_noise(&chunk, None);
                {
                    let neighbor_biomes = self_neighbor_biomes(&chunk);
                    generator.build_surface(&chunk, &neighbor_biomes);
                }
                chunk
            },
            |chunk| {
                generator.apply_carvers(black_box(&chunk));
            },
            criterion::BatchSize::SmallInput,
        );
    });
}

fn bench_nether_carvers(c: &mut Criterion) {
    ensure_registry();
    let dim = &vanilla_dimension_types::THE_NETHER;
    let source = BiomeSourceKind::nether(0);
    let generator = NetherGenerator::new(source, 0);

    c.bench_function("nether_apply_carvers", |b| {
        b.iter_batched(
            || {
                let chunk = make_proto_chunk(0, 0, dim);
                generator.create_biomes(&chunk);
                generator.fill_from_noise(&chunk, None);
                {
                    let neighbor_biomes = self_neighbor_biomes(&chunk);
                    generator.build_surface(&chunk, &neighbor_biomes);
                }
                chunk
            },
            |chunk| {
                generator.apply_carvers(black_box(&chunk));
            },
            criterion::BatchSize::SmallInput,
        );
    });
}

fn bench_end_carvers(c: &mut Criterion) {
    ensure_registry();
    let dim = &vanilla_dimension_types::THE_END;
    let source = BiomeSourceKind::end(0);
    let generator = EndGenerator::new(source, 0);

    c.bench_function("end_apply_carvers", |b| {
        b.iter_batched(
            || {
                let chunk = make_proto_chunk(0, 0, dim);
                generator.create_biomes(&chunk);
                generator.fill_from_noise(&chunk, None);
                {
                    let neighbor_biomes = self_neighbor_biomes(&chunk);
                    generator.build_surface(&chunk, &neighbor_biomes);
                }
                chunk
            },
            |chunk| {
                generator.apply_carvers(black_box(&chunk));
            },
            criterion::BatchSize::SmallInput,
        );
    });
}

// ── Feature benchmarks ─────────────────────────────────────────────────────

fn make_chunk_through_carvers(
    chunk_x: i32,
    chunk_z: i32,
    dim: &DimensionType,
    generator: &ChunkGeneratorType,
) -> ChunkAccess {
    let chunk = make_proto_chunk(chunk_x, chunk_z, dim);
    generator.create_structures(&chunk);
    generator.create_biomes(&chunk);
    generator.fill_from_noise(&chunk, None);
    {
        let neighbor_biomes = self_neighbor_biomes(&chunk);
        generator.build_surface(&chunk, &neighbor_biomes);
    }
    generator.apply_carvers(&chunk);
    chunk
}

fn make_holder_for_features(
    center: ChunkPos,
    chunk_x: i32,
    chunk_z: i32,
    dim: &DimensionType,
    generator: &ChunkGeneratorType,
) -> Arc<ChunkHolder> {
    let holder = Arc::new(ChunkHolder::new(
        ChunkPos::new(chunk_x, chunk_z),
        BENCH_HOLDER_LOAD_LEVEL,
        None,
        dim.min_y,
        dim.height,
    ));

    let distance = (chunk_x - center.0.x)
        .abs()
        .max((chunk_z - center.0.y).abs());
    if distance <= 1 {
        holder.insert_chunk(
            make_chunk_through_carvers(chunk_x, chunk_z, dim, generator),
            ChunkStatus::Carvers,
        );
    } else {
        let chunk = make_proto_chunk(chunk_x, chunk_z, dim);
        generator.create_structures(&chunk);
        holder.insert_chunk(chunk, ChunkStatus::StructureStarts);
    }

    holder
}

fn make_holder_for_feature_centers(
    centers: &[ChunkPos],
    chunk_x: i32,
    chunk_z: i32,
    dim: &DimensionType,
    generator: &ChunkGeneratorType,
) -> Arc<ChunkHolder> {
    let holder = Arc::new(ChunkHolder::new(
        ChunkPos::new(chunk_x, chunk_z),
        BENCH_HOLDER_LOAD_LEVEL,
        None,
        dim.min_y,
        dim.height,
    ));

    let needs_carvers = centers.iter().any(|center| {
        (chunk_x - center.0.x)
            .abs()
            .max((chunk_z - center.0.y).abs())
            <= 1
    });
    if needs_carvers {
        holder.insert_chunk(
            make_chunk_through_carvers(chunk_x, chunk_z, dim, generator),
            ChunkStatus::Carvers,
        );
    } else {
        let chunk = make_proto_chunk(chunk_x, chunk_z, dim);
        generator.create_structures(&chunk);
        holder.insert_chunk(chunk, ChunkStatus::StructureStarts);
    }

    holder
}

struct FeatureFixture {
    context: Arc<WorldGenContext>,
    cache: Arc<StaticCache2D<Arc<ChunkHolder>>>,
    target: Arc<ChunkHolder>,
    _world: Arc<World>,
}

fn build_feature_fixture(generator_key: Identifier) -> FeatureFixture {
    build_feature_fixture_at(generator_key, 0, ChunkPos::new(0, 0))
}

fn build_feature_fixture_at(
    generator_key: Identifier,
    seed: i64,
    center: ChunkPos,
) -> FeatureFixture {
    let output = create_benchmark_generator(
        &generator_key,
        seed,
        "feature benchmark should use a built-in generator",
    );
    let dim = output.dimension_type;
    let generator = Arc::new(output.generator);
    let generation_settings = WorldGenerationSettings::from_generator_config(
        generator_key.clone(),
        &output.config,
        dim.key.clone(),
        dim.min_y,
        dim.height,
    );
    let chunk_runtime = Arc::new(
        RuntimeBuilder::new_current_thread()
            .enable_all()
            .build()
            .expect("feature benchmark runtime should build"),
    );
    let generation_pool = Arc::new(
        rayon::ThreadPoolBuilder::new()
            .num_threads(1)
            .build()
            .expect("feature benchmark generation pool should build"),
    );
    let world_config = WorldConfig {
        storage: WorldStorageConfig::RamOnly,
        level_data_path: None,
        generator: generator.clone(),
        generation_settings,
        view_distance: 10,
        simulation_distance: 10,
        compression: None,
        is_flat: false,
        sea_level: output.sea_level,
        default_gamemode: GameType::Survival,
        difficulty: Difficulty::Normal,
    };
    let world_key = Identifier::new("bench", format!("{}_features", generator_key.path));
    let world = chunk_runtime
        .block_on(World::new_with_config(
            chunk_runtime.clone(),
            world_key,
            dim,
            seed,
            world_config,
            generation_pool,
        ))
        .expect("feature benchmark world should build");
    let context = world.chunk_map.world_gen_context.clone();

    let generator_for_factory = generator.clone();
    let cache = Arc::new(StaticCache2D::create(
        center.0.x,
        center.0.y,
        8,
        move |x, z| make_holder_for_features(center, x, z, dim, generator_for_factory.as_ref()),
    ));
    let target = cache.get(center.0.x, center.0.y).clone();

    FeatureFixture {
        context,
        cache,
        target,
        _world: world,
    }
}

struct ConcurrentFeatureFixture {
    context: Arc<WorldGenContext>,
    cache: Arc<StaticCache2D<Arc<ChunkHolder>>>,
    targets: Vec<Arc<ChunkHolder>>,
    generation_pool: Arc<rayon::ThreadPool>,
    _world: Arc<World>,
}

#[derive(Clone, Copy)]
struct FeatureTaskWallTime {
    pos: ChunkPos,
    elapsed: Duration,
}

struct FullPipelineStage {
    step: &'static ChunkStep,
    holders: Vec<Arc<ChunkHolder>>,
}

struct ConcurrentFullPipelineFixture {
    chunk_runtime: Arc<Runtime>,
    chunk_map: Arc<ChunkMap>,
    cache: Arc<StaticCache2D<Arc<ChunkHolder>>>,
    stages: Vec<FullPipelineStage>,
    generation_pool: Arc<rayon::ThreadPool>,
    targets: Vec<Arc<ChunkHolder>>,
    _world: Arc<World>,
}

struct ConcurrentLightFixture {
    chunk_runtime: Arc<Runtime>,
    chunk_map: Arc<ChunkMap>,
    cache: Arc<StaticCache2D<Arc<ChunkHolder>>>,
    setup_stages: Vec<FullPipelineStage>,
    light_stage: FullPipelineStage,
    generation_pool: Arc<rayon::ThreadPool>,
    targets: Vec<Arc<ChunkHolder>>,
    _world: Arc<World>,
}

#[derive(Clone, Copy)]
struct FullPipelineStageWallTime {
    status: ChunkStatus,
    chunks: usize,
    elapsed: Duration,
}

fn bench_features(c: &mut Criterion, name: &str, generator_key: Identifier) {
    let step = GENERATION_PYRAMID.get_step_to(ChunkStatus::Features);

    c.bench_function(name, |b| {
        b.iter_batched(
            {
                let generator_key = generator_key.clone();
                move || build_feature_fixture(generator_key.clone())
            },
            |fixture| {
                ChunkStatusTasks::generate_features(
                    fixture.context,
                    step,
                    &fixture.cache,
                    fixture.target,
                );
            },
            criterion::BatchSize::SmallInput,
        );
    });
}

fn bench_overworld_features(c: &mut Criterion) {
    ensure_registry();
    bench_features(
        c,
        "overworld_generate_features",
        Identifier::vanilla_static("overworld"),
    );
}

const CONCURRENT_OVERWORLD_SEED: i64 = 2_965_282_071_327_931_563;
const CONCURRENT_FEATURE_GRID_MIN: i32 = -1;
const CONCURRENT_FEATURE_GRID_MAX: i32 = 2;
const CONCURRENT_FEATURE_THREAD_COUNT: usize = 8;
const FULL_PIPELINE_THREAD_COUNT: usize = CONCURRENT_FEATURE_THREAD_COUNT;
const LIGHT_THREAD_COUNT: usize = CONCURRENT_FEATURE_THREAD_COUNT;
const FULL_PIPELINE_STATUSES: [ChunkStatus; 12] = [
    ChunkStatus::Empty,
    ChunkStatus::StructureStarts,
    ChunkStatus::StructureReferences,
    ChunkStatus::Biomes,
    ChunkStatus::Noise,
    ChunkStatus::Surface,
    ChunkStatus::Carvers,
    ChunkStatus::Features,
    ChunkStatus::InitializeLight,
    ChunkStatus::Light,
    ChunkStatus::Spawn,
    ChunkStatus::Full,
];
const LIGHT_SETUP_STATUSES: [ChunkStatus; 9] = [
    ChunkStatus::Empty,
    ChunkStatus::StructureStarts,
    ChunkStatus::StructureReferences,
    ChunkStatus::Biomes,
    ChunkStatus::Noise,
    ChunkStatus::Surface,
    ChunkStatus::Carvers,
    ChunkStatus::Features,
    ChunkStatus::InitializeLight,
];

fn concurrent_feature_centers() -> Vec<ChunkPos> {
    let side = CONCURRENT_FEATURE_GRID_MAX - CONCURRENT_FEATURE_GRID_MIN + 1;
    let mut positions = Vec::with_capacity((side * side) as usize);

    for z in CONCURRENT_FEATURE_GRID_MIN..=CONCURRENT_FEATURE_GRID_MAX {
        for x in CONCURRENT_FEATURE_GRID_MIN..=CONCURRENT_FEATURE_GRID_MAX {
            positions.push(ChunkPos::new(x, z));
        }
    }

    positions
}

fn concurrent_feature_cache_radius(centers: &[ChunkPos]) -> i32 {
    centers
        .iter()
        .map(|center| center.0.x.abs().max(center.0.y.abs()) + 8)
        .max()
        .unwrap_or(8)
}

fn concurrent_pipeline_cache_radius(centers: &[ChunkPos], target_status: ChunkStatus) -> i32 {
    let target_step = GENERATION_PYRAMID.get_step_to(target_status);
    let dependency_radius = target_step.get_accumulated_radius_of(ChunkStatus::Empty) as i32;

    centers
        .iter()
        .map(|center| center.0.x.abs().max(center.0.y.abs()) + dependency_radius)
        .max()
        .unwrap_or(dependency_radius)
}

fn pipeline_positions_for_status(
    centers: &[ChunkPos],
    target_step: &ChunkStep,
    status: ChunkStatus,
) -> Vec<ChunkPos> {
    let radius = target_step.get_accumulated_radius_of(status) as i32;
    let mut positions = Vec::new();

    for center in centers {
        for z in (center.0.y - radius)..=(center.0.y + radius) {
            for x in (center.0.x - radius)..=(center.0.x + radius) {
                positions.push(ChunkPos::new(x, z));
            }
        }
    }

    positions.sort_by_key(|pos| (pos.0.y, pos.0.x));
    positions.dedup();
    positions
}

fn pipeline_stages_for_statuses(
    cache: &Arc<StaticCache2D<Arc<ChunkHolder>>>,
    centers: &[ChunkPos],
    target_status: ChunkStatus,
    statuses: &[ChunkStatus],
) -> Vec<FullPipelineStage> {
    let target_step = GENERATION_PYRAMID.get_step_to(target_status);

    statuses
        .iter()
        .copied()
        .map(|status| {
            let holders = pipeline_positions_for_status(centers, target_step, status)
                .into_iter()
                .map(|pos| cache.get(pos.0.x, pos.0.y).clone())
                .collect();

            FullPipelineStage {
                step: GENERATION_PYRAMID.get_step_to(status),
                holders,
            }
        })
        .collect()
}

fn full_pipeline_stages(
    cache: &Arc<StaticCache2D<Arc<ChunkHolder>>>,
    centers: &[ChunkPos],
) -> Vec<FullPipelineStage> {
    pipeline_stages_for_statuses(cache, centers, ChunkStatus::Full, &FULL_PIPELINE_STATUSES)
}

fn build_concurrent_feature_fixture(
    generator_key: Identifier,
    seed: i64,
) -> ConcurrentFeatureFixture {
    let output = create_benchmark_generator(
        &generator_key,
        seed,
        "feature benchmark should use a built-in generator",
    );
    let dim = output.dimension_type;
    let generator = Arc::new(output.generator);
    let generation_settings = WorldGenerationSettings::from_generator_config(
        generator_key.clone(),
        &output.config,
        dim.key.clone(),
        dim.min_y,
        dim.height,
    );
    let chunk_runtime = Arc::new(
        RuntimeBuilder::new_current_thread()
            .enable_all()
            .build()
            .expect("feature benchmark runtime should build"),
    );
    let generation_pool = Arc::new(
        rayon::ThreadPoolBuilder::new()
            .num_threads(CONCURRENT_FEATURE_THREAD_COUNT)
            .thread_name(|index| format!("bench-feature-{index}"))
            .build()
            .expect("feature benchmark generation pool should build"),
    );
    let world_config = WorldConfig {
        storage: WorldStorageConfig::RamOnly,
        level_data_path: None,
        generator: generator.clone(),
        generation_settings,
        view_distance: 10,
        simulation_distance: 10,
        compression: None,
        is_flat: false,
        sea_level: output.sea_level,
        default_gamemode: GameType::Survival,
        difficulty: Difficulty::Normal,
    };
    let world_key = Identifier::new(
        "bench",
        format!("{}_features_concurrent", generator_key.path),
    );
    let world = chunk_runtime
        .block_on(World::new_with_config(
            chunk_runtime.clone(),
            world_key,
            dim,
            seed,
            world_config,
            generation_pool.clone(),
        ))
        .expect("feature benchmark world should build");
    let context = world.chunk_map.world_gen_context.clone();

    let centers: Arc<[ChunkPos]> = concurrent_feature_centers().into();
    let cache_radius = concurrent_feature_cache_radius(&centers);
    let generator_for_factory = generator.clone();
    let centers_for_factory = centers.clone();
    let cache = Arc::new(StaticCache2D::create(0, 0, cache_radius, move |x, z| {
        make_holder_for_feature_centers(
            &centers_for_factory,
            x,
            z,
            dim,
            generator_for_factory.as_ref(),
        )
    }));
    let targets = centers
        .iter()
        .map(|center| cache.get(center.0.x, center.0.y).clone())
        .collect();

    ConcurrentFeatureFixture {
        context,
        cache,
        targets,
        generation_pool,
        _world: world,
    }
}

fn build_concurrent_full_pipeline_fixture(
    generator_key: Identifier,
    seed: i64,
    centers: Vec<ChunkPos>,
    thread_count: usize,
) -> ConcurrentFullPipelineFixture {
    let output = create_benchmark_generator(
        &generator_key,
        seed,
        "full-pipeline benchmark should use a built-in generator",
    );
    let dim = output.dimension_type;
    let generator = Arc::new(output.generator);
    let generation_settings = WorldGenerationSettings::from_generator_config(
        generator_key.clone(),
        &output.config,
        dim.key.clone(),
        dim.min_y,
        dim.height,
    );
    let chunk_runtime = Arc::new(
        RuntimeBuilder::new_current_thread()
            .enable_all()
            .build()
            .expect("full-pipeline benchmark runtime should build"),
    );
    let generation_pool = Arc::new(
        rayon::ThreadPoolBuilder::new()
            .num_threads(thread_count)
            .thread_name(|index| format!("bench-full-pipeline-{index}"))
            .build()
            .expect("full-pipeline benchmark generation pool should build"),
    );
    let world_config = WorldConfig {
        storage: WorldStorageConfig::RamOnly,
        level_data_path: None,
        generator: generator.clone(),
        generation_settings,
        view_distance: 10,
        simulation_distance: 10,
        compression: None,
        is_flat: false,
        sea_level: output.sea_level,
        default_gamemode: GameType::Survival,
        difficulty: Difficulty::Normal,
    };
    let world_key = Identifier::new(
        "bench",
        format!("{}_full_pipeline_concurrent", generator_key.path),
    );
    let world = chunk_runtime
        .block_on(World::new_with_config(
            chunk_runtime.clone(),
            world_key,
            dim,
            seed,
            world_config,
            generation_pool.clone(),
        ))
        .expect("full-pipeline benchmark world should build");
    let chunk_map = world.chunk_map.clone();

    let cache_radius = concurrent_pipeline_cache_radius(&centers, ChunkStatus::Full);
    let chunk_map_for_factory = chunk_map.clone();
    let cache = Arc::new(StaticCache2D::create(0, 0, cache_radius, move |x, z| {
        let pos = ChunkPos::new(x, z);
        let holder = Arc::new(ChunkHolder::new(
            pos,
            BENCH_HOLDER_LOAD_LEVEL,
            None,
            dim.min_y,
            dim.height,
        ));
        let _ = chunk_map_for_factory
            .chunks
            .insert_sync(pos, holder.clone());
        holder
    }));
    let stages = full_pipeline_stages(&cache, &centers);
    let targets = centers
        .iter()
        .map(|center| cache.get(center.0.x, center.0.y).clone())
        .collect();

    ConcurrentFullPipelineFixture {
        chunk_runtime,
        chunk_map,
        cache,
        stages,
        generation_pool,
        targets,
        _world: world,
    }
}

fn build_concurrent_light_fixture(
    generator_key: Identifier,
    seed: i64,
    centers: Vec<ChunkPos>,
    thread_count: usize,
) -> ConcurrentLightFixture {
    let output = create_benchmark_generator(
        &generator_key,
        seed,
        "light benchmark should use a built-in generator",
    );
    let dim = output.dimension_type;
    let generator = Arc::new(output.generator);
    let generation_settings = WorldGenerationSettings::from_generator_config(
        generator_key.clone(),
        &output.config,
        dim.key.clone(),
        dim.min_y,
        dim.height,
    );
    let chunk_runtime = Arc::new(
        RuntimeBuilder::new_current_thread()
            .enable_all()
            .build()
            .expect("light benchmark runtime should build"),
    );
    let generation_pool = Arc::new(
        rayon::ThreadPoolBuilder::new()
            .num_threads(thread_count)
            .thread_name(|index| format!("bench-light-{index}"))
            .build()
            .expect("light benchmark generation pool should build"),
    );
    let world_config = WorldConfig {
        storage: WorldStorageConfig::RamOnly,
        level_data_path: None,
        generator: generator.clone(),
        generation_settings,
        view_distance: 10,
        simulation_distance: 10,
        compression: None,
        is_flat: false,
        sea_level: output.sea_level,
        default_gamemode: GameType::Survival,
        difficulty: Difficulty::Normal,
    };
    let world_key = Identifier::new("bench", format!("{}_light_concurrent", generator_key.path));
    let world = chunk_runtime
        .block_on(World::new_with_config(
            chunk_runtime.clone(),
            world_key,
            dim,
            seed,
            world_config,
            generation_pool.clone(),
        ))
        .expect("light benchmark world should build");
    let chunk_map = world.chunk_map.clone();

    let cache_radius = concurrent_pipeline_cache_radius(&centers, ChunkStatus::Light);
    let chunk_map_for_factory = chunk_map.clone();
    let cache = Arc::new(StaticCache2D::create(0, 0, cache_radius, move |x, z| {
        let pos = ChunkPos::new(x, z);
        let holder = Arc::new(ChunkHolder::new(
            pos,
            BENCH_HOLDER_LOAD_LEVEL,
            None,
            dim.min_y,
            dim.height,
        ));
        let _ = chunk_map_for_factory
            .chunks
            .insert_sync(pos, holder.clone());
        holder
    }));
    let setup_stages =
        pipeline_stages_for_statuses(&cache, &centers, ChunkStatus::Light, &LIGHT_SETUP_STATUSES);
    let target_step = GENERATION_PYRAMID.get_step_to(ChunkStatus::Light);
    let light_stage = FullPipelineStage {
        step: target_step,
        holders: pipeline_positions_for_status(&centers, target_step, ChunkStatus::Light)
            .into_iter()
            .map(|pos| cache.get(pos.0.x, pos.0.y).clone())
            .collect(),
    };
    let targets = centers
        .iter()
        .map(|center| cache.get(center.0.x, center.0.y).clone())
        .collect();

    let fixture = ConcurrentLightFixture {
        chunk_runtime,
        chunk_map,
        cache,
        setup_stages,
        light_stage,
        generation_pool,
        targets,
        _world: world,
    };
    run_concurrent_light_setup(&fixture);
    fixture
}

fn bench_overworld_features_concurrent_overlap(c: &mut Criterion) {
    ensure_registry();
    let step = GENERATION_PYRAMID.get_step_to(ChunkStatus::Features);

    c.bench_function("overworld_generate_features_concurrent_overlap", |b| {
        b.iter_batched(
            || {
                build_concurrent_feature_fixture(
                    Identifier::vanilla_static("overworld"),
                    CONCURRENT_OVERWORLD_SEED,
                )
            },
            |fixture| {
                if env::var_os("STEEL_FEATURE_BATCH_PROFILE").is_some() {
                    run_concurrent_feature_batch_profiled(fixture, step);
                } else {
                    fixture.generation_pool.scope(|scope| {
                        for target in &fixture.targets {
                            let context = fixture.context.clone();
                            let cache = fixture.cache.clone();
                            let target = target.clone();
                            scope.spawn(move |_| {
                                ChunkStatusTasks::generate_features(context, step, &cache, target);
                            });
                        }
                    });
                }
            },
            criterion::BatchSize::SmallInput,
        );
    });
}

fn bench_overworld_full_pipeline_concurrent_overlap(c: &mut Criterion) {
    ensure_registry();

    c.bench_function("overworld_full_pipeline_concurrent_overlap", |b| {
        b.iter_batched(
            || {
                build_concurrent_full_pipeline_fixture(
                    Identifier::vanilla_static("overworld"),
                    CONCURRENT_OVERWORLD_SEED,
                    concurrent_feature_centers(),
                    FULL_PIPELINE_THREAD_COUNT,
                )
            },
            run_concurrent_full_pipeline_batch,
            criterion::BatchSize::SmallInput,
        );
    });
}

/// A single overworld chunk taken Empty → Full on one thread.
///
/// Finishing one chunk requires generating the surrounding neighborhood of
/// dependency chunks (each pipeline step pulls a radius of lower-status
/// neighbors), so this measures the honest end-to-end cost of producing one
/// finished chunk rather than a single isolated step.
fn bench_overworld_full_chunk(c: &mut Criterion) {
    ensure_registry();

    c.bench_function("overworld_full_chunk", |b| {
        b.iter_batched(
            || {
                build_concurrent_full_pipeline_fixture(
                    Identifier::vanilla_static("overworld"),
                    CONCURRENT_OVERWORLD_SEED,
                    vec![ChunkPos::new(0, 0)],
                    1,
                )
            },
            run_concurrent_full_pipeline_batch,
            criterion::BatchSize::SmallInput,
        );
    });
}

/// A 4×4 batch of overworld chunks taken Empty → Full concurrently, reported as
/// per-chunk throughput (`x16`).
///
/// Same workload shape as `overworld_full_pipeline_concurrent_overlap`, but
/// expressed as a throughput group so criterion reports elements/sec per chunk.
fn bench_overworld_full_chunk_concurrent(c: &mut Criterion) {
    ensure_registry();
    let chunk_count = concurrent_feature_centers().len() as u64;

    let mut group = c.benchmark_group("overworld_full_chunk_concurrent");
    group.throughput(Throughput::Elements(chunk_count));
    group.bench_function(format!("x{chunk_count}"), |b| {
        b.iter_batched(
            || {
                build_concurrent_full_pipeline_fixture(
                    Identifier::vanilla_static("overworld"),
                    CONCURRENT_OVERWORLD_SEED,
                    concurrent_feature_centers(),
                    FULL_PIPELINE_THREAD_COUNT,
                )
            },
            run_concurrent_full_pipeline_batch,
            criterion::BatchSize::SmallInput,
        );
    });
    group.finish();
}

fn bench_overworld_light(c: &mut Criterion) {
    ensure_registry();

    c.bench_function("overworld_light", |b| {
        b.iter_batched(
            || {
                build_concurrent_light_fixture(
                    Identifier::vanilla_static("overworld"),
                    CONCURRENT_OVERWORLD_SEED,
                    vec![ChunkPos::new(0, 0)],
                    1,
                )
            },
            run_concurrent_light_batch,
            criterion::BatchSize::SmallInput,
        );
    });
}

fn bench_overworld_light_concurrent(c: &mut Criterion) {
    ensure_registry();
    let chunk_count = concurrent_feature_centers().len() as u64;

    let mut group = c.benchmark_group("overworld_light_concurrent");
    group.throughput(Throughput::Elements(chunk_count));
    group.bench_function(format!("x{chunk_count}"), |b| {
        b.iter_batched(
            || {
                build_concurrent_light_fixture(
                    Identifier::vanilla_static("overworld"),
                    CONCURRENT_OVERWORLD_SEED,
                    concurrent_feature_centers(),
                    LIGHT_THREAD_COUNT,
                )
            },
            run_concurrent_light_batch,
            criterion::BatchSize::SmallInput,
        );
    });
    group.finish();
}

fn run_concurrent_feature_batch_profiled(fixture: ConcurrentFeatureFixture, step: &ChunkStep) {
    let task_times = Arc::new(SyncMutex::new(Vec::with_capacity(fixture.targets.len())));
    let batch_started_at = Instant::now();
    fixture.generation_pool.scope(|scope| {
        for target in &fixture.targets {
            let context = fixture.context.clone();
            let cache = fixture.cache.clone();
            let target = target.clone();
            let task_times = task_times.clone();
            scope.spawn(move |_| {
                let pos = target.get_pos();
                let started_at = Instant::now();
                ChunkStatusTasks::generate_features(context, step, &cache, target);
                task_times.lock().push(FeatureTaskWallTime {
                    pos,
                    elapsed: started_at.elapsed(),
                });
            });
        }
    });
    let batch_elapsed = batch_started_at.elapsed();
    let task_times = task_times.lock();
    log_feature_batch_profile(batch_elapsed, &task_times);
}

fn log_feature_batch_profile(batch_elapsed: Duration, task_times: &[FeatureTaskWallTime]) {
    if FEATURE_BATCH_PROFILE_LOGS.fetch_add(1, Ordering::Relaxed)
        >= *FEATURE_BATCH_PROFILE_LOG_LIMIT
    {
        return;
    }

    if task_times.is_empty() {
        eprintln!(
            "feature batch profile tasks=0 batch_ms={:.3}",
            duration_ms(batch_elapsed)
        );
        return;
    }

    let sum = task_times
        .iter()
        .map(|record| record.elapsed)
        .fold(Duration::ZERO, |left, right| left + right);
    let min = task_times
        .iter()
        .map(|record| record.elapsed)
        .min()
        .unwrap_or(Duration::ZERO);
    let max = task_times
        .iter()
        .map(|record| record.elapsed)
        .max()
        .unwrap_or(Duration::ZERO);
    let utilization = duration_ms(sum)
        / (duration_ms(batch_elapsed) * CONCURRENT_FEATURE_THREAD_COUNT as f64).max(f64::EPSILON);

    let mut slowest = task_times.to_vec();
    slowest.sort_by_key(|record| Reverse(record.elapsed));
    let slowest = slowest
        .iter()
        .take(4)
        .map(|record| {
            format!(
                "({},{}):{:.3}ms",
                record.pos.0.x,
                record.pos.0.y,
                duration_ms(record.elapsed)
            )
        })
        .collect::<Vec<_>>()
        .join(",");

    eprintln!(
        "feature batch profile tasks={} batch_ms={:.3} sum_task_ms={:.3} min_task_ms={:.3} \
         max_task_ms={:.3} utilization={:.3} slowest=[{}]",
        task_times.len(),
        duration_ms(batch_elapsed),
        duration_ms(sum),
        duration_ms(min),
        duration_ms(max),
        utilization,
        slowest
    );
}

fn run_concurrent_full_pipeline_batch(fixture: ConcurrentFullPipelineFixture) {
    let mut stage_times = env::var_os("STEEL_FULL_PIPELINE_PROFILE")
        .is_some()
        .then(Vec::new);
    let batch_started_at = Instant::now();

    for stage in &fixture.stages {
        let stage_started_at = Instant::now();
        run_full_pipeline_stage(&fixture, stage);

        if let Some(stage_times) = &mut stage_times {
            stage_times.push(FullPipelineStageWallTime {
                status: stage.step.target_status,
                chunks: stage.holders.len(),
                elapsed: stage_started_at.elapsed(),
            });
        }
    }

    for target in &fixture.targets {
        assert!(
            target.try_chunk(ChunkStatus::Full).is_some(),
            "full-pipeline target chunk did not reach Full"
        );
    }

    if let Some(stage_times) = stage_times {
        log_full_pipeline_profile(batch_started_at.elapsed(), &stage_times);
    }
}

fn run_full_pipeline_stage(fixture: &ConcurrentFullPipelineFixture, stage: &FullPipelineStage) {
    run_pipeline_stage(
        &fixture.chunk_runtime,
        &fixture.chunk_map,
        &fixture.cache,
        &fixture.generation_pool,
        stage,
    );
}

fn run_concurrent_light_setup(fixture: &ConcurrentLightFixture) {
    for stage in &fixture.setup_stages {
        run_light_fixture_stage(fixture, stage);
    }

    for target in &fixture.targets {
        assert!(
            target.try_chunk(ChunkStatus::InitializeLight).is_some(),
            "light benchmark target chunk did not reach InitializeLight"
        );
    }
}

fn run_concurrent_light_batch(fixture: ConcurrentLightFixture) {
    let profile = env::var_os("STEEL_LIGHT_PROFILE").is_some();
    let batch_started_at = Instant::now();
    let stage_started_at = Instant::now();

    run_light_fixture_stage(&fixture, &fixture.light_stage);
    let light_elapsed = stage_started_at.elapsed();

    for target in &fixture.targets {
        assert!(
            target.try_chunk(ChunkStatus::Light).is_some(),
            "light benchmark target chunk did not reach Light"
        );
    }

    if profile {
        log_light_profile(
            batch_started_at.elapsed(),
            light_elapsed,
            fixture.light_stage.holders.len(),
        );
    }
}

fn run_light_fixture_stage(fixture: &ConcurrentLightFixture, stage: &FullPipelineStage) {
    run_pipeline_stage(
        &fixture.chunk_runtime,
        &fixture.chunk_map,
        &fixture.cache,
        &fixture.generation_pool,
        stage,
    );
}

fn run_pipeline_stage(
    chunk_runtime: &Arc<Runtime>,
    chunk_map: &Arc<ChunkMap>,
    cache: &Arc<StaticCache2D<Arc<ChunkHolder>>>,
    generation_pool: &Arc<rayon::ThreadPool>,
    stage: &FullPipelineStage,
) {
    chunk_runtime.block_on(async {
        let futures = stage
            .holders
            .iter()
            .filter_map(|holder| {
                holder.apply_step(stage.step, chunk_map, cache, generation_pool.clone())
            })
            .collect::<Vec<_>>();

        let results = join_all(futures).await;
        assert!(
            results.iter().all(Option::is_some),
            "pipeline stage {:?} did not complete",
            stage.step.target_status
        );
    });
}

fn log_full_pipeline_profile(batch_elapsed: Duration, stage_times: &[FullPipelineStageWallTime]) {
    if FULL_PIPELINE_PROFILE_LOGS.fetch_add(1, Ordering::Relaxed)
        >= *FULL_PIPELINE_PROFILE_LOG_LIMIT
    {
        return;
    }

    let total_stage_time = stage_times
        .iter()
        .map(|record| record.elapsed)
        .fold(Duration::ZERO, |left, right| left + right);
    let stage_summary = stage_times
        .iter()
        .map(|record| {
            format!(
                "{:?}:{}:{:.3}ms",
                record.status,
                record.chunks,
                duration_ms(record.elapsed)
            )
        })
        .collect::<Vec<_>>()
        .join(",");

    eprintln!(
        "full pipeline profile batch_ms={:.3} sum_stage_ms={:.3} stages=[{}]",
        duration_ms(batch_elapsed),
        duration_ms(total_stage_time),
        stage_summary
    );
}

fn log_light_profile(batch_elapsed: Duration, light_elapsed: Duration, chunks: usize) {
    if LIGHT_PROFILE_LOGS.fetch_add(1, Ordering::Relaxed) >= *LIGHT_PROFILE_LOG_LIMIT {
        return;
    }

    eprintln!(
        "light profile chunks={} batch_ms={:.3} light_ms={:.3}",
        chunks,
        duration_ms(batch_elapsed),
        duration_ms(light_elapsed)
    );
}

fn duration_ms(duration: Duration) -> f64 {
    duration.as_secs_f64() * 1000.0
}

fn bench_nether_features(c: &mut Criterion) {
    ensure_registry();
    bench_features(
        c,
        "nether_generate_features",
        Identifier::vanilla_static("the_nether"),
    );
}

fn bench_end_features(c: &mut Criterion) {
    ensure_registry();
    bench_features(
        c,
        "end_generate_features",
        Identifier::vanilla_static("the_end"),
    );
}

// ── Structure benchmarks ────────────────────────────────────────────────────

/// A 20×20 grid hits structure sets with different spacings (villages at 32,
/// shipwrecks at 24, mineshafts at 1, ...), so the timings include cheap-reject,
/// full-placement, and jigsaw paths.
const STRUCTURE_GRID_SIDE: i32 = 20;

fn structure_grid_chunks(dim: &'static DimensionType) -> Vec<ChunkAccess> {
    (0..STRUCTURE_GRID_SIDE)
        .flat_map(|x| (0..STRUCTURE_GRID_SIDE).map(move |z| make_proto_chunk(x, z, dim)))
        .collect()
}

fn run_grid<G: ChunkGenerator>(generator: &G, chunks: &[ChunkAccess]) {
    for chunk in chunks {
        generator.create_structures(black_box(chunk));
    }
}

fn bench_overworld_structure_starts(c: &mut Criterion) {
    ensure_registry();
    let dim = &vanilla_dimension_types::OVERWORLD;
    let source = BiomeSourceKind::overworld(0);
    let generator = OverworldGenerator::new(source, 0);

    c.bench_function("overworld_create_structures", |b| {
        b.iter_batched(
            || structure_grid_chunks(dim),
            |chunks| run_grid(&generator, &chunks),
            criterion::BatchSize::SmallInput,
        );
    });
}

fn bench_nether_structure_starts(c: &mut Criterion) {
    ensure_registry();
    let dim = &vanilla_dimension_types::THE_NETHER;
    let source = BiomeSourceKind::nether(0);
    let generator = NetherGenerator::new(source, 0);

    c.bench_function("nether_create_structures", |b| {
        b.iter_batched(
            || structure_grid_chunks(dim),
            |chunks| run_grid(&generator, &chunks),
            criterion::BatchSize::SmallInput,
        );
    });
}

fn bench_end_structure_starts(c: &mut Criterion) {
    ensure_registry();
    let dim = &vanilla_dimension_types::THE_END;
    let source = BiomeSourceKind::end(0);
    let generator = EndGenerator::new(source, 0);

    c.bench_function("end_create_structures", |b| {
        b.iter_batched(
            || structure_grid_chunks(dim),
            |chunks| run_grid(&generator, &chunks),
            criterion::BatchSize::SmallInput,
        );
    });
}

/// No-op filler for `ChunkStep::task`; `generate_structure_references` never dispatches through it.
fn noop_task(
    _ctx: Arc<WorldGenContext>,
    _step: &ChunkStep,
    _cache: &Arc<StaticCache2D<Arc<ChunkHolder>>>,
    _holder: Arc<ChunkHolder>,
) {
}

fn dummy_step() -> ChunkStep {
    ChunkStep {
        target_status: ChunkStatus::StructureReferences,
        direct_dependencies: ChunkDependencies::EMPTY,
        accumulated_dependencies: ChunkDependencies::EMPTY,
        block_state_write_radius: -1,
        task: noop_task,
    }
}

/// Builds a `ChunkHolder` at `(chunk_x, chunk_z)` containing a proto chunk
/// with structure starts generated and the holder advanced to `StructureStarts`.
fn make_holder_with_starts(
    chunk_x: i32,
    chunk_z: i32,
    dim: &DimensionType,
    generator: &ChunkGeneratorType,
) -> Arc<ChunkHolder> {
    let holder = Arc::new(ChunkHolder::new(
        ChunkPos::new(chunk_x, chunk_z),
        BENCH_HOLDER_LOAD_LEVEL,
        None,
        dim.min_y,
        dim.height,
    ));
    let chunk = make_proto_chunk(chunk_x, chunk_z, dim);
    generator.create_structures(&chunk);
    holder.insert_chunk(chunk, ChunkStatus::StructureStarts);
    holder
}

fn build_references_fixture(
    dim: &'static DimensionType,
    generator: ChunkGeneratorType,
) -> (
    Arc<WorldGenContext>,
    Arc<StaticCache2D<Arc<ChunkHolder>>>,
    Arc<ChunkHolder>,
) {
    let generator_arc = Arc::new(generator);
    let context = Arc::new(WorldGenContext::new(
        generator_arc.clone(),
        Weak::new(),
        dim.min_y,
        dim.height,
        63, // overworld sea level (bench is overworld)
    ));

    let gen_for_factory = generator_arc.clone();
    let cache = Arc::new(StaticCache2D::create(0, 0, 8, move |x, z| {
        make_holder_with_starts(x, z, dim, &gen_for_factory)
    }));
    let target = cache.get(0, 0).clone();
    (context, cache, target)
}

fn bench_references(c: &mut Criterion, name: &str, context_fixture: ReferencesFixture) {
    let ReferencesFixture {
        context,
        cache,
        target,
    } = context_fixture;
    let step = dummy_step();

    c.bench_function(name, |b| {
        b.iter_batched(
            || {
                let chunk = target
                    .try_chunk(ChunkStatus::StructureStarts)
                    .expect("target chunk missing");
                chunk.structure_references_mut().clear();
            },
            |()| {
                ChunkStatusTasks::generate_structure_references(
                    context.clone(),
                    &step,
                    &cache,
                    target.clone(),
                );
            },
            criterion::BatchSize::SmallInput,
        );
    });
}

struct ReferencesFixture {
    context: Arc<WorldGenContext>,
    cache: Arc<StaticCache2D<Arc<ChunkHolder>>>,
    target: Arc<ChunkHolder>,
}

fn bench_overworld_structure_references(c: &mut Criterion) {
    ensure_registry();
    let dim = &vanilla_dimension_types::OVERWORLD;
    let generator = OverworldGenerator::new(BiomeSourceKind::overworld(0), 0).into();
    let (context, cache, target) = build_references_fixture(dim, generator);
    bench_references(
        c,
        "overworld_structure_references",
        ReferencesFixture {
            context,
            cache,
            target,
        },
    );
}

fn bench_nether_structure_references(c: &mut Criterion) {
    ensure_registry();
    let dim = &vanilla_dimension_types::THE_NETHER;
    let generator = NetherGenerator::new(BiomeSourceKind::nether(0), 0).into();
    let (context, cache, target) = build_references_fixture(dim, generator);
    bench_references(
        c,
        "nether_structure_references",
        ReferencesFixture {
            context,
            cache,
            target,
        },
    );
}

fn bench_end_structure_references(c: &mut Criterion) {
    ensure_registry();
    let dim = &vanilla_dimension_types::THE_END;
    let generator = EndGenerator::new(BiomeSourceKind::end(0), 0).into();
    let (context, cache, target) = build_references_fixture(dim, generator);
    bench_references(
        c,
        "end_structure_references",
        ReferencesFixture {
            context,
            cache,
            target,
        },
    );
}

// ── Full-pipeline benchmarks (biomes + noise + surface + carvers) ──────────

fn bench_overworld_full(c: &mut Criterion) {
    ensure_registry();
    let dim = &vanilla_dimension_types::OVERWORLD;
    let source = BiomeSourceKind::overworld(0);
    let generator = OverworldGenerator::new(source, 0);

    c.bench_function("overworld_full_through_carvers", |b| {
        b.iter(|| {
            let chunk = make_proto_chunk(black_box(0), black_box(0), dim);
            generator.create_biomes(&chunk);
            generator.fill_from_noise(&chunk, None);
            {
                let neighbor_biomes = self_neighbor_biomes(&chunk);
                generator.build_surface(&chunk, &neighbor_biomes);
            }
            generator.apply_carvers(&chunk);
        });
    });
}

fn bench_nether_full(c: &mut Criterion) {
    ensure_registry();
    let dim = &vanilla_dimension_types::THE_NETHER;
    let source = BiomeSourceKind::nether(0);
    let generator = NetherGenerator::new(source, 0);

    c.bench_function("nether_full_through_carvers", |b| {
        b.iter(|| {
            let chunk = make_proto_chunk(black_box(0), black_box(0), dim);
            generator.create_biomes(&chunk);
            generator.fill_from_noise(&chunk, None);
            {
                let neighbor_biomes = self_neighbor_biomes(&chunk);
                generator.build_surface(&chunk, &neighbor_biomes);
            }
            generator.apply_carvers(&chunk);
        });
    });
}

fn bench_end_full(c: &mut Criterion) {
    ensure_registry();
    let dim = &vanilla_dimension_types::THE_END;
    let source = BiomeSourceKind::end(0);
    let generator = EndGenerator::new(source, 0);

    c.bench_function("end_full_through_carvers", |b| {
        b.iter(|| {
            let chunk = make_proto_chunk(black_box(0), black_box(0), dim);
            generator.create_biomes(&chunk);
            generator.fill_from_noise(&chunk, None);
            {
                let neighbor_biomes = self_neighbor_biomes(&chunk);
                generator.build_surface(&chunk, &neighbor_biomes);
            }
            generator.apply_carvers(&chunk);
        });
    });
}

/// Isolated `ImprovedNoise::sample_and_lerp` kernel throughput at 4-wide vs
/// 8-wide SIMD. Measures whether widening to AVX-512 (`f64x8`) pays off given
/// the kernel's heavy scalar permutation-gather floor. Each variant computes
/// the same 8 Y samples per column across a fixed set of columns.
fn bench_noise_kernel(c: &mut Criterion) {
    use std::simd::{f64x4, f64x8};
    use steel_utils::random::xoroshiro::Xoroshiro;
    use steel_worldgen::noise::ImprovedNoise;

    let mut rng = Xoroshiro::from_seed(0x5713_2026);
    let noise = ImprovedNoise::new(&mut rng);

    // Realistic working set: many distinct (x, z) columns, 8 stacked Ys each.
    let columns: Vec<(f64, f64)> = (0..256)
        .map(|i| (f64::from(i) * 1.3, f64::from(255 - i) * 0.7))
        .collect();
    let ys8 = f64x8::from_array([0.0, 4.0, 8.0, 12.0, 16.0, 20.0, 24.0, 28.0]);
    let ys_lo = f64x4::from_array([0.0, 4.0, 8.0, 12.0]);
    let ys_hi = f64x4::from_array([16.0, 20.0, 24.0, 28.0]);
    let y_scale = 8.0;

    let mut group = c.benchmark_group("noise_kernel");
    group.throughput(Throughput::Elements((columns.len() * 8) as u64));

    // Current production path: generic 4 lanes, two calls per 8 Ys.
    group.bench_function("y_scale_4x_generic", |b| {
        b.iter(|| {
            let mut acc = f64x4::splat(0.0);
            for &(x, z) in &columns {
                acc += noise.noise_with_y_scale_simd(
                    black_box(x),
                    ys_lo,
                    black_box(z),
                    y_scale,
                    ys_lo,
                );
                acc += noise.noise_with_y_scale_simd(
                    black_box(x),
                    ys_hi,
                    black_box(z),
                    y_scale,
                    ys_hi,
                );
            }
            black_box(acc)
        });
    });

    // 8-wide (AVX-512 on Zen 5), one call per 8 Ys.
    group.bench_function("y_scale_8x_generic", |b| {
        b.iter(|| {
            let mut acc = f64x8::splat(0.0);
            for &(x, z) in &columns {
                acc += noise.noise_with_y_scale_simd::<8>(
                    black_box(x),
                    ys8,
                    black_box(z),
                    y_scale,
                    ys8,
                );
            }
            black_box(acc)
        });
    });

    group.finish();
}

criterion_group!(
    benches,
    // Biome
    bench_overworld_biome,
    bench_nether_biome,
    bench_end_biome,
    // Noise
    bench_overworld_noise,
    bench_nether_noise,
    bench_end_noise,
    // Surface
    bench_overworld_surface,
    bench_nether_surface,
    bench_end_surface,
    // Recalc counts (chunk-finalize / chunk-load path)
    bench_overworld_recalculate_counts,
    // Carvers
    bench_overworld_carvers,
    bench_nether_carvers,
    bench_end_carvers,
    // Features
    bench_overworld_features,
    bench_nether_features,
    bench_end_features,
    // Structure starts
    bench_overworld_structure_starts,
    bench_nether_structure_starts,
    bench_end_structure_starts,
    // Structure references
    bench_overworld_structure_references,
    bench_nether_structure_references,
    bench_end_structure_references,
    // Full pipeline (biomes → noise → surface → carvers)
    bench_overworld_full,
    bench_nether_full,
    bench_end_full,
);
criterion_group! {
    name = feature_distribution_benches;
    config = Criterion::default()
        .sample_size(30)
        .warm_up_time(Duration::from_secs(2))
        .measurement_time(Duration::from_secs(20));
    targets = bench_overworld_features_concurrent_overlap
}
criterion_group! {
    name = full_pipeline_benches;
    config = Criterion::default()
        .sample_size(10)
        .warm_up_time(Duration::from_secs(1))
        .measurement_time(Duration::from_secs(10));
    targets = bench_overworld_full_pipeline_concurrent_overlap
}
criterion_group! {
    name = full_chunk_benches;
    config = Criterion::default()
        .sample_size(10)
        .warm_up_time(Duration::from_secs(1))
        .measurement_time(Duration::from_secs(10));
    targets = bench_overworld_full_chunk, bench_overworld_full_chunk_concurrent
}
criterion_group! {
    name = light_benches;
    config = Criterion::default()
        .sample_size(10)
        .warm_up_time(Duration::from_secs(1))
        .measurement_time(Duration::from_secs(10));
    targets = bench_overworld_light, bench_overworld_light_concurrent
}
criterion_group! {
    name = noise_kernel_benches;
    config = Criterion::default()
        .sample_size(50)
        .warm_up_time(Duration::from_secs(1))
        .measurement_time(Duration::from_secs(5));
    targets = bench_noise_kernel
}
criterion_main!(
    benches,
    feature_distribution_benches,
    full_pipeline_benches,
    full_chunk_benches,
    light_benches,
    noise_kernel_benches,
);
