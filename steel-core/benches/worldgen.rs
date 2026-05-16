#![expect(missing_docs, clippy::similar_names, reason = "benchmarks")]

use criterion::{Criterion, criterion_group, criterion_main};
use std::hint::black_box;
use std::sync::{Arc, Once, Weak};
use steel_core::chunk::chunk_access::{ChunkAccess, ChunkStatus};
use steel_core::chunk::chunk_generation_task::StaticCache2D;
use steel_core::chunk::chunk_holder::ChunkHolder;
use steel_core::chunk::chunk_pyramid::{ChunkDependencies, ChunkStep};
use steel_core::chunk::chunk_status_tasks::ChunkStatusTasks;
use steel_core::chunk::proto_chunk::ProtoChunk;
use steel_core::chunk::section::{ChunkSection, Sections};
use steel_core::worldgen::{
    BiomeSourceKind, ChunkBiomeSampler, ChunkGenerator, ChunkGeneratorType, EndGenerator,
    NetherGenerator, OverworldGenerator, WorldGenContext,
};
use steel_registry::dimension_type::DimensionType;
use steel_registry::{REGISTRY, Registry, vanilla_dimension_types};
use steel_utils::ChunkPos;

static INIT: Once = Once::new();

fn ensure_registry() {
    INIT.call_once(|| {
        let mut registry = Registry::new_vanilla();
        registry.freeze();
        let _ = REGISTRY.init(registry);
    });
}

fn make_proto_chunk(chunk_x: i32, chunk_z: i32, dim: &DimensionType) -> ChunkAccess {
    let section_count = (dim.height / 16) as usize;
    let sections: Box<[ChunkSection]> = (0..section_count)
        .map(|_| ChunkSection::new_empty())
        .collect();
    let sections = Sections::from_owned(sections);
    let pos = ChunkPos::new(chunk_x, chunk_z);
    ChunkAccess::Proto(ProtoChunk::new(sections, pos, dim.min_y, dim.height))
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
criterion_main!(benches);
