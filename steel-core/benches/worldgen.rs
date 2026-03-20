#![expect(missing_docs, clippy::similar_names, reason = "benchmarks")]

use criterion::{Criterion, criterion_group, criterion_main};
use std::hint::black_box;
use std::sync::Once;
use steel_core::chunk::chunk_access::ChunkAccess;
use steel_core::chunk::chunk_generator::ChunkGenerator;
use steel_core::chunk::proto_chunk::ProtoChunk;
use steel_core::chunk::section::{ChunkSection, Sections};
use steel_core::chunk::world_gen_context::{EndGenerator, NetherGenerator, OverworldGenerator};
use steel_core::worldgen::{BiomeSourceKind, ChunkBiomeSampler};
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
    let dim = vanilla_dimension_types::OVERWORLD;
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
    let dim = vanilla_dimension_types::THE_NETHER;
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
    let dim = vanilla_dimension_types::THE_END;
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
    let dim = vanilla_dimension_types::OVERWORLD;
    let source = BiomeSourceKind::overworld(0);
    let generator = OverworldGenerator::new(source, 0);

    c.bench_function("overworld_fill_from_noise", |b| {
        b.iter(|| {
            let chunk = make_proto_chunk(black_box(0), black_box(0), dim);
            generator.fill_from_noise(&chunk);
        });
    });
}

fn bench_nether_noise(c: &mut Criterion) {
    ensure_registry();
    let dim = vanilla_dimension_types::THE_NETHER;
    let source = BiomeSourceKind::nether(0);
    let generator = NetherGenerator::new(source, 0);

    c.bench_function("nether_fill_from_noise", |b| {
        b.iter(|| {
            let chunk = make_proto_chunk(black_box(0), black_box(0), dim);
            generator.fill_from_noise(&chunk);
        });
    });
}

fn bench_end_noise(c: &mut Criterion) {
    ensure_registry();
    let dim = vanilla_dimension_types::THE_END;
    let source = BiomeSourceKind::end(0);
    let generator = EndGenerator::new(source, 0);

    c.bench_function("end_fill_from_noise", |b| {
        b.iter(|| {
            let chunk = make_proto_chunk(black_box(0), black_box(0), dim);
            generator.fill_from_noise(&chunk);
        });
    });
}

// ── Surface benchmarks ──────────────────────────────────────────────────────

fn bench_overworld_surface(c: &mut Criterion) {
    ensure_registry();
    let dim = vanilla_dimension_types::OVERWORLD;
    let source = BiomeSourceKind::overworld(0);
    let generator = OverworldGenerator::new(source, 0);

    c.bench_function("overworld_build_surface", |b| {
        b.iter_batched(
            || {
                let chunk = make_proto_chunk(0, 0, dim);
                generator.create_biomes(&chunk);
                generator.fill_from_noise(&chunk);
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
    let dim = vanilla_dimension_types::THE_NETHER;
    let source = BiomeSourceKind::nether(0);
    let generator = NetherGenerator::new(source, 0);

    c.bench_function("nether_build_surface", |b| {
        b.iter_batched(
            || {
                let chunk = make_proto_chunk(0, 0, dim);
                generator.create_biomes(&chunk);
                generator.fill_from_noise(&chunk);
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
    let dim = vanilla_dimension_types::THE_END;
    let source = BiomeSourceKind::end(0);
    let generator = EndGenerator::new(source, 0);

    c.bench_function("end_build_surface", |b| {
        b.iter_batched(
            || {
                let chunk = make_proto_chunk(0, 0, dim);
                generator.create_biomes(&chunk);
                generator.fill_from_noise(&chunk);
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
);
criterion_main!(benches);
