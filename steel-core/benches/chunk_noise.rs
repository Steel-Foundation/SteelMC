#![allow(missing_docs)]

use criterion::{Criterion, criterion_group, criterion_main};
use std::hint::black_box;
use std::sync::Once;
use steel_core::chunk::chunk_access::ChunkAccess;
use steel_core::chunk::chunk_generator::ChunkGenerator;
use steel_core::chunk::proto_chunk::ProtoChunk;
use steel_core::chunk::section::{ChunkSection, Sections};
use steel_core::chunk::world_gen_context::OverworldGenerator;
use steel_core::worldgen::BiomeSourceKind;
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

criterion_group!(
    benches,
    bench_fill_from_noise_single,
    bench_fill_from_noise_grid
);
criterion_main!(benches);
