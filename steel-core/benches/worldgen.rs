#![allow(missing_docs)]

use criterion::{BenchmarkId, Criterion, criterion_group, criterion_main};
use std::hint::black_box;
use std::sync::Once;
use steel_core::chunk::chunk_access::ChunkAccess;
use steel_core::chunk::chunk_generator::ChunkGenerator;
use steel_core::chunk::proto_chunk::ProtoChunk;
use steel_core::chunk::section::{ChunkSection, Sections};
use steel_core::chunk::world_gen_context::OverworldGenerator;
use steel_core::worldgen::{BiomeSourceKind, ChunkBiomeSampler};
use steel_registry::{REGISTRY, Registry};
use steel_utils::ChunkPos;

static INIT: Once = Once::new();

fn ensure_registry() {
    INIT.call_once(|| {
        let mut registry = Registry::new_vanilla();
        registry.freeze();
        let _ = REGISTRY.init(registry);
    });
}

fn make_proto_chunk(chunk_x: i32, chunk_z: i32) -> ChunkAccess {
    let section_count = 24; // overworld: 384 / 16
    let sections: Box<[ChunkSection]> = (0..section_count)
        .map(|_| ChunkSection::new_empty())
        .collect();
    let sections = Sections::from_owned(sections);
    let pos = ChunkPos::new(chunk_x, chunk_z);
    ChunkAccess::Proto(ProtoChunk::new(sections, pos, -64, 384))
}

/// Pre-populate a chunk through biomes + noise so it's ready for surface.
fn prepare_chunk_for_surface(
    generator: &OverworldGenerator,
    chunk_x: i32,
    chunk_z: i32,
) -> ChunkAccess {
    let chunk = make_proto_chunk(chunk_x, chunk_z);
    generator.create_biomes(&chunk);
    generator.fill_from_noise(&chunk);
    chunk
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

fn bench_overworld_biome_single_chunk(c: &mut Criterion) {
    let source = BiomeSourceKind::overworld(0);

    c.bench_function("overworld_biome_single_chunk", |b| {
        b.iter(|| {
            let mut sampler = source.chunk_sampler();
            sample_chunk_biomes(&mut sampler, black_box(0), black_box(0), -4, 24);
        });
    });
}

fn bench_overworld_biome_grid(c: &mut Criterion) {
    let source = BiomeSourceKind::overworld(0);

    let mut group = c.benchmark_group("overworld_biome_grid");
    for radius in [3, 5] {
        let side = radius * 2 + 1;
        let chunk_count = side * side;
        group.throughput(criterion::Throughput::Elements(chunk_count as u64));
        group.bench_with_input(
            BenchmarkId::from_parameter(format!("{side}x{side}")),
            &radius,
            |b, &r| {
                b.iter(|| {
                    for cx in -r..=r {
                        for cz in -r..=r {
                            let mut sampler = source.chunk_sampler();
                            sample_chunk_biomes(&mut sampler, cx, cz, -4, 24);
                        }
                    }
                });
            },
        );
    }
    group.finish();
}

fn bench_nether_biome_single_chunk(c: &mut Criterion) {
    let source = BiomeSourceKind::nether(0);

    c.bench_function("nether_biome_single_chunk", |b| {
        b.iter(|| {
            let mut sampler = source.chunk_sampler();
            sample_chunk_biomes(&mut sampler, black_box(0), black_box(0), 0, 16);
        });
    });
}

fn bench_end_biome_single_chunk(c: &mut Criterion) {
    let source = BiomeSourceKind::end(0);

    c.bench_function("end_biome_single_chunk", |b| {
        b.iter(|| {
            let mut sampler = source.chunk_sampler();
            sample_chunk_biomes(&mut sampler, black_box(0), black_box(0), 0, 16);
        });
    });
}

fn bench_end_biome_outer_islands(c: &mut Criterion) {
    let source = BiomeSourceKind::end(0);

    c.bench_function("end_biome_outer_islands", |b| {
        b.iter(|| {
            let mut sampler = source.chunk_sampler();
            sample_chunk_biomes(&mut sampler, black_box(100), black_box(100), 0, 16);
        });
    });
}

fn bench_overworld_source_creation(c: &mut Criterion) {
    c.bench_function("overworld_source_creation", |b| {
        b.iter(|| {
            black_box(BiomeSourceKind::overworld(black_box(0)));
        });
    });
}

// ── Noise benchmarks ────────────────────────────────────────────────────────

fn bench_fill_from_noise_single(c: &mut Criterion) {
    ensure_registry();
    let source = BiomeSourceKind::overworld(0);
    let generator = OverworldGenerator::new(source, 0);

    c.bench_function("overworld_fill_from_noise", |b| {
        b.iter(|| {
            let chunk = make_proto_chunk(black_box(0), black_box(0));
            generator.fill_from_noise(&chunk);
        });
    });
}

fn bench_fill_from_noise_grid(c: &mut Criterion) {
    ensure_registry();
    let source = BiomeSourceKind::overworld(0);
    let generator = OverworldGenerator::new(source, 0);

    let mut group = c.benchmark_group("overworld_fill_from_noise_grid");
    for radius in [1, 2] {
        let side = radius * 2 + 1;
        let chunk_count = side * side;
        group.throughput(criterion::Throughput::Elements(chunk_count as u64));
        group.bench_function(format!("{side}x{side}"), |b| {
            b.iter(|| {
                for cx in -radius..=radius {
                    for cz in -radius..=radius {
                        let chunk = make_proto_chunk(black_box(cx), black_box(cz));
                        generator.fill_from_noise(&chunk);
                    }
                }
            });
        });
    }
    group.finish();
}

// ── Surface benchmarks ──────────────────────────────────────────────────────

fn bench_build_surface_single(c: &mut Criterion) {
    ensure_registry();
    let source = BiomeSourceKind::overworld(0);
    let generator = OverworldGenerator::new(source, 0);

    c.bench_function("overworld_build_surface", |b| {
        b.iter_batched(
            || prepare_chunk_for_surface(&generator, 0, 0),
            |chunk| {
                let neighbor_biomes = self_neighbor_biomes(&chunk);
                generator.build_surface(black_box(&chunk), &neighbor_biomes);
            },
            criterion::BatchSize::SmallInput,
        );
    });
}

fn bench_build_surface_grid(c: &mut Criterion) {
    ensure_registry();
    let source = BiomeSourceKind::overworld(0);
    let generator = OverworldGenerator::new(source, 0);

    let mut group = c.benchmark_group("overworld_build_surface_grid");
    for radius in [1, 2] {
        let side = radius * 2 + 1;
        let chunk_count = side * side;
        group.throughput(criterion::Throughput::Elements(chunk_count as u64));

        let coords: Vec<_> = (-radius..=radius)
            .flat_map(|cx| (-radius..=radius).map(move |cz| (cx, cz)))
            .collect();

        group.bench_function(format!("{side}x{side}"), |b| {
            b.iter_batched(
                || {
                    coords
                        .iter()
                        .map(|&(cx, cz)| prepare_chunk_for_surface(&generator, cx, cz))
                        .collect::<Vec<_>>()
                },
                |chunks| {
                    for chunk in &chunks {
                        let neighbor_biomes = self_neighbor_biomes(chunk);
                        generator.build_surface(black_box(chunk), &neighbor_biomes);
                    }
                },
                criterion::BatchSize::LargeInput,
            );
        });
    }
    group.finish();
}

criterion_group!(
    benches,
    // Biome
    bench_overworld_biome_single_chunk,
    bench_overworld_biome_grid,
    bench_nether_biome_single_chunk,
    bench_end_biome_single_chunk,
    bench_end_biome_outer_islands,
    bench_overworld_source_creation,
    // Noise
    bench_fill_from_noise_single,
    bench_fill_from_noise_grid,
    // Surface
    bench_build_surface_single,
    bench_build_surface_grid,
);
criterion_main!(benches);
