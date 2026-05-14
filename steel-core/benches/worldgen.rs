#![expect(missing_docs, clippy::similar_names, reason = "benchmarks")]

use criterion::{Criterion, criterion_group, criterion_main};
use std::hint::black_box;
use std::sync::{Arc, Once, Weak};
use std::time::Duration;
use steel_core::behavior::init_behaviors;
use steel_core::chunk::chunk_access::{ChunkAccess, ChunkStatus};
use steel_core::chunk::chunk_generation_task::StaticCache2D;
use steel_core::chunk::chunk_holder::ChunkHolder;
use steel_core::chunk::chunk_pyramid::{ChunkDependencies, ChunkStep, GENERATION_PYRAMID};
use steel_core::chunk::chunk_status_tasks::ChunkStatusTasks;
use steel_core::chunk::proto_chunk::ProtoChunk;
use steel_core::chunk::section::{ChunkSection, Sections};
use steel_core::level_data::WorldGenerationSettings;
use steel_core::world::{World, WorldConfig, WorldStorageConfig};
use steel_core::worldgen::{
    BiomeSourceKind, ChunkBiomeSampler, ChunkGenerator, ChunkGeneratorType, EndGenerator,
    NetherGenerator, OverworldGenerator, WorldGenContext, WorldGeneratorRegistry,
};
use steel_registry::dimension_type::DimensionType;
use steel_registry::{REGISTRY, Registry, vanilla_dimension_types};
use steel_utils::types::{Difficulty, GameType};
use steel_utils::{ChunkPos, Identifier};
use tokio::runtime::Builder as RuntimeBuilder;

static INIT: Once = Once::new();

fn ensure_registry() {
    INIT.call_once(|| {
        let mut registry = Registry::new_vanilla();
        registry.freeze();
        let _ = REGISTRY.init(registry);
        init_behaviors();
    });
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
        std::sync::Weak::new(),
    ))
}

/// Build a `neighbor_biomes` closure that reads from the chunk's own sections.
///
/// In a real pipeline this reads from a neighbor cache, but for a single-chunk
/// benchmark the chunk is its own neighbor (biome lookups near edges will
/// wrap but that's fine for timing).
fn self_neighbor_biomes(chunk: &ChunkAccess) -> impl Fn(i32, i32, i32) -> u16 + '_ {
    let sections = chunk.sections();
    let min_qy = chunk.min_y() >> 2;
    let total_quarts_y = (sections.sections.len() * 4) as i32;

    move |qx: i32, qy: i32, qz: i32| -> u16 {
        let local_qx = qx.rem_euclid(4) as usize;
        let local_qz = qz.rem_euclid(4) as usize;
        let qy_clamped = (qy - min_qy).clamp(0, total_quarts_y - 1) as usize;
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
        0,
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
    let generator_config = toml::Value::Table(toml::map::Map::new());
    let output = WorldGeneratorRegistry::new_with_builtins()
        .expect("built-in world generators should register")
        .create(&generator_key, &generator_config, seed)
        .expect("feature benchmark should use a built-in generator");
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

const PROFILE_FEATURE_SEED: i64 = 2_965_282_071_327_931_563;
const FEATURE_SAMPLE_GRID_RADIUS: i32 = 8;
const FEATURE_SAMPLE_GRID_STRIDE: i32 = 4;

fn feature_sample_positions() -> Vec<ChunkPos> {
    let side = FEATURE_SAMPLE_GRID_RADIUS * 2 + 1;
    let mut positions = Vec::with_capacity((side * side) as usize);

    for z in -FEATURE_SAMPLE_GRID_RADIUS..=FEATURE_SAMPLE_GRID_RADIUS {
        for x in -FEATURE_SAMPLE_GRID_RADIUS..=FEATURE_SAMPLE_GRID_RADIUS {
            positions.push(ChunkPos::new(
                x * FEATURE_SAMPLE_GRID_STRIDE,
                z * FEATURE_SAMPLE_GRID_STRIDE,
            ));
        }
    }

    positions
}

fn bench_overworld_features_profile_range(c: &mut Criterion) {
    ensure_registry();
    let step = GENERATION_PYRAMID.get_step_to(ChunkStatus::Features);
    let positions = feature_sample_positions();
    let mut next_position_index = 0usize;

    c.bench_function("overworld_generate_features_profile_range", |b| {
        b.iter_batched(
            || {
                let center = positions[next_position_index % positions.len()];
                next_position_index = next_position_index.wrapping_add(1);
                build_feature_fixture_at(
                    Identifier::vanilla_static("overworld"),
                    PROFILE_FEATURE_SEED,
                    center,
                )
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
        0,
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
    let context = Arc::new(WorldGenContext::new(generator_arc.clone(), Weak::new()));

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
    targets = bench_overworld_features_profile_range
}
criterion_main!(benches, feature_distribution_benches);
