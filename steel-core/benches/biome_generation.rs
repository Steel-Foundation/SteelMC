#![allow(missing_docs)]

use criterion::{BenchmarkId, Criterion, criterion_group, criterion_main};
use std::hint::black_box;
use steel_core::worldgen::{BiomeSourceKind, ChunkBiomeSampler};

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

// ── Overworld ───────────────────────────────────────────────────────────────

fn bench_overworld_single_chunk(c: &mut Criterion) {
    let source = BiomeSourceKind::overworld(0);

    c.bench_function("overworld_biome_single_chunk", |b| {
        b.iter(|| {
            let mut sampler = source.chunk_sampler();
            sample_chunk_biomes(&mut sampler, black_box(0), black_box(0), -4, 24);
        });
    });
}

fn bench_overworld_chunk_grid(c: &mut Criterion) {
    let source = BiomeSourceKind::overworld(0);

    let mut group = c.benchmark_group("overworld_biome_grid");
    for radius in [3, 5] {
        let side = radius * 2 + 1;
        let chunk_count = side * side;
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
        group.throughput(criterion::Throughput::Elements(chunk_count as u64));
    }
    group.finish();
}

// ── Nether ──────────────────────────────────────────────────────────────────

fn bench_nether_single_chunk(c: &mut Criterion) {
    let source = BiomeSourceKind::nether(0);

    c.bench_function("nether_biome_single_chunk", |b| {
        b.iter(|| {
            let mut sampler = source.chunk_sampler();
            sample_chunk_biomes(&mut sampler, black_box(0), black_box(0), 0, 16);
        });
    });
}

// ── End ─────────────────────────────────────────────────────────────────────

fn bench_end_single_chunk(c: &mut Criterion) {
    let source = BiomeSourceKind::end(0);

    c.bench_function("end_biome_single_chunk", |b| {
        b.iter(|| {
            let mut sampler = source.chunk_sampler();
            sample_chunk_biomes(&mut sampler, black_box(0), black_box(0), 0, 16);
        });
    });
}

fn bench_end_outer_islands(c: &mut Criterion) {
    let source = BiomeSourceKind::end(0);

    // Sample far from origin where the End island noise is active
    c.bench_function("end_biome_outer_islands", |b| {
        b.iter(|| {
            let mut sampler = source.chunk_sampler();
            sample_chunk_biomes(&mut sampler, black_box(100), black_box(100), 0, 16);
        });
    });
}

// ── Source creation ─────────────────────────────────────────────────────────

fn bench_overworld_source_creation(c: &mut Criterion) {
    c.bench_function("overworld_source_creation", |b| {
        b.iter(|| {
            black_box(BiomeSourceKind::overworld(black_box(0)));
        });
    });
}

criterion_group!(
    benches,
    bench_overworld_single_chunk,
    bench_overworld_chunk_grid,
    bench_nether_single_chunk,
    bench_end_single_chunk,
    bench_end_outer_islands,
    bench_overworld_source_creation,
);
criterion_main!(benches);
