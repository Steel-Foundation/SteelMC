#![expect(missing_docs, reason = "benchmarks")]
use criterion::{Criterion, criterion_group, criterion_main};
use std::hint::black_box;
use steel_utils::random::{
    PositionalRandom, Random, legacy_random::LegacyRandom, name_hash::NameHash,
};

fn bench_from_seed(c: &mut Criterion) {
    c.bench_function("legacy_random from_seed", |b| {
        b.iter(|| {
            black_box(LegacyRandom::from_seed(black_box(12345)));
        });
    });
}

fn bench_next_i32(c: &mut Criterion) {
    let mut rng = LegacyRandom::from_seed(0);
    c.bench_function("legacy_random next_i32", |b| {
        b.iter(|| {
            black_box(rng.next_i32());
        });
    });
}

fn bench_next_bounded_i32(c: &mut Criterion) {
    let mut rng = LegacyRandom::from_seed(0);
    c.bench_function("legacy_random next_bounded_i32(100)", |b| {
        b.iter(|| {
            black_box(rng.next_i32_bounded(black_box(100)));
        });
    });
}

fn bench_next_i64(c: &mut Criterion) {
    let mut rng = LegacyRandom::from_seed(0);
    c.bench_function("legacy_random next_i64", |b| {
        b.iter(|| {
            black_box(rng.next_i64());
        });
    });
}

fn bench_next_bool(c: &mut Criterion) {
    let mut rng = LegacyRandom::from_seed(0);
    c.bench_function("legacy_random next_bool", |b| {
        b.iter(|| {
            black_box(rng.next_bool());
        });
    });
}

fn bench_next_f32(c: &mut Criterion) {
    let mut rng = LegacyRandom::from_seed(0);
    c.bench_function("legacy_random next_f32", |b| {
        b.iter(|| {
            black_box(rng.next_f32());
        });
    });
}

fn bench_next_f64(c: &mut Criterion) {
    let mut rng = LegacyRandom::from_seed(0);
    c.bench_function("legacy_random next_f64", |b| {
        b.iter(|| {
            black_box(rng.next_f64());
        });
    });
}

fn bench_next_gaussian(c: &mut Criterion) {
    let mut rng = LegacyRandom::from_seed(0);
    c.bench_function("legacy_random next_gaussian", |b| {
        b.iter(|| {
            black_box(rng.next_gaussian());
        });
    });
}

fn bench_split(c: &mut Criterion) {
    let mut rng = LegacyRandom::from_seed(0);
    c.bench_function("legacy_random split", |b| {
        b.iter(|| {
            black_box(rng.fork());
        });
    });
}

fn bench_next_splitter(c: &mut Criterion) {
    let mut rng = LegacyRandom::from_seed(0);
    c.bench_function("legacy_random next_splitter", |b| {
        b.iter(|| {
            black_box(rng.next_positional());
        });
    });
}

fn bench_split_pos(c: &mut Criterion) {
    let mut rng = LegacyRandom::from_seed(0);
    let splitter = rng.next_positional();
    c.bench_function("legacy_random splitter split_pos", |b| {
        b.iter(|| {
            black_box(splitter.at(black_box(100), black_box(64), black_box(-200)));
        });
    });
}

fn bench_split_u64(c: &mut Criterion) {
    let mut rng = LegacyRandom::from_seed(0);
    let splitter = rng.next_positional();
    c.bench_function("legacy_random splitter split_u64", |b| {
        b.iter(|| {
            black_box(splitter.with_seed(black_box(42069)));
        });
    });
}

fn bench_split_string(c: &mut Criterion) {
    let mut rng = LegacyRandom::from_seed(0);
    let splitter = rng.next_positional();
    let hash = NameHash::new("minecraft:overworld");
    c.bench_function("legacy_random splitter split_string", |b| {
        b.iter(|| {
            black_box(splitter.with_hash_of(black_box(&hash)));
        });
    });
}

fn bench_sequential_generation(c: &mut Criterion) {
    c.bench_function("legacy_random 1000 next_i32 calls", |b| {
        b.iter(|| {
            let mut rng = LegacyRandom::from_seed(black_box(0));
            for _ in 0..1000 {
                black_box(rng.next_i32());
            }
        });
    });
}

criterion_group!(
    benches,
    bench_from_seed,
    bench_next_i32,
    bench_next_bounded_i32,
    bench_next_i64,
    bench_next_bool,
    bench_next_f32,
    bench_next_f64,
    bench_next_gaussian,
    bench_split,
    bench_next_splitter,
    bench_split_pos,
    bench_split_u64,
    bench_split_string,
    bench_sequential_generation,
);
criterion_main!(benches);
