#![expect(missing_docs, reason = "benchmarks")]

use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use std::hint::black_box;
use steel_core::entity::entity_manager_benchmark_support::manager_with_entities;

const ENTITY_COUNTS: [usize; 3] = [100, 1_000, 5_000];

fn tick_entities(c: &mut Criterion) {
    let mut group = c.benchmark_group("entity_manager_tick");

    for count in ENTITY_COUNTS {
        group.throughput(Throughput::Elements(count as u64));
        group.bench_with_input(BenchmarkId::from_parameter(count), &count, |b, &count| {
            let manager = manager_with_entities(count);
            b.iter(|| black_box(manager.tick_entities(0, true)));
        });
    }

    group.finish();
}

criterion_group!(benches, tick_entities);
criterion_main!(benches);
