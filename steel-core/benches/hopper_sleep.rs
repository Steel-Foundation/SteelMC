//! Run with:
//! `cargo bench -p steel-core --bench hopper_sleep --features benchmark-support`

use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use steel_core::world::block_entity_ticker_benchmark_support::SleepingHopperBenchmark;

fn bench_locked_sleeping_hoppers(c: &mut Criterion) {
    let fixture = SleepingHopperBenchmark::new();
    let mut group = c.benchmark_group("hopper_block_entity_ticker");
    group.throughput(Throughput::Elements(SleepingHopperBenchmark::HOPPER_COUNT));
    group.bench_with_input(
        BenchmarkId::new("locked_sleeping", SleepingHopperBenchmark::HOPPER_COUNT),
        &fixture,
        |b, fixture| b.iter(|| fixture.tick()),
    );
    group.finish();
}

criterion_group!(benches, bench_locked_sleeping_hoppers);
criterion_main!(benches);
