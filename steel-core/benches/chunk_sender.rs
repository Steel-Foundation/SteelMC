#![expect(missing_docs, reason = "benchmarks")]

use criterion::{BatchSize, BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use rustc_hash::{FxHashMap, FxHashSet};
use std::hint::black_box;
use steel_core::bootstrap::init_globals_once;
use steel_core::player::chunk_sender::benchmark_support::{
    encoded_is_current_for, encoded_pos, prepared_full_chunk, resolve_valid_chunks,
};
use steel_core::player::chunk_sender::{ChunkSender, EncodedChunk, PreparedBatch, PreparedChunk};
use steel_utils::ChunkPos;

type Resolver = fn(&[PreparedChunk], Vec<EncodedChunk>, &FxHashSet<ChunkPos>) -> Vec<EncodedChunk>;

const BATCH_SIZES: [usize; 7] = [16, 64, 128, 256, 500, 1000, 2000];
const MAX_CHUNKS_PER_TICK: usize = 500;

#[derive(Clone, Copy)]
enum BatchShape {
    AllEncoded,
    HalfEncoded,
    TailOnly,
}

impl BatchShape {
    const fn name(self) -> &'static str {
        match self {
            Self::AllEncoded => "all-encoded",
            Self::HalfEncoded => "half-encoded",
            Self::TailOnly => "tail-only",
        }
    }

    const fn keeps(self, index: usize, batch_size: usize) -> bool {
        match self {
            Self::AllEncoded => true,
            Self::HalfEncoded => index.is_multiple_of(2),
            Self::TailOnly => index + 1 == batch_size,
        }
    }
}

fn grid_positions(count: usize) -> Vec<ChunkPos> {
    let side = (count as f64).sqrt().ceil() as i32;
    (0..count)
        .map(|index| {
            let index = index as i32;
            ChunkPos::new(index % side - side / 2, index / side - side / 2)
        })
        .collect()
}

struct Fixture {
    batch: PreparedBatch,
    encoded: Vec<EncodedChunk>,
    pending: FxHashSet<ChunkPos>,
}

fn encoding_pool() -> rayon::ThreadPool {
    rayon::ThreadPoolBuilder::new()
        .num_threads(rayon::current_num_threads())
        .build()
        .expect("benchmark chunk encoding pool should initialize")
}

fn fixture(batch_size: usize, shape: BatchShape) -> Fixture {
    init_globals_once();

    let positions = grid_positions(batch_size);
    let batch = PreparedBatch {
        chunks: positions.iter().copied().map(prepared_full_chunk).collect(),
        has_skylight: true,
        epoch_snapshot: 0,
    };

    let mut cache = FxHashMap::default();
    let encoded = ChunkSender::encode_batch(&batch, &mut cache, None, &encoding_pool());
    assert_eq!(
        encoded.len(),
        batch_size,
        "fixture should encode the whole batch before it is thinned"
    );

    let encoded = encoded
        .into_iter()
        .enumerate()
        .filter(|(index, _)| shape.keeps(*index, batch_size))
        .map(|(_, chunk)| chunk)
        .collect::<Vec<_>>();

    Fixture {
        batch,
        encoded,
        pending: positions.into_iter().collect(),
    }
}

fn resolve_by_rescan(
    prepared: &[PreparedChunk],
    encoded_chunks: Vec<EncodedChunk>,
    pending: &FxHashSet<ChunkPos>,
) -> Vec<EncodedChunk> {
    let mut valid_chunks = Vec::with_capacity(encoded_chunks.len());

    for encoded in encoded_chunks {
        let pos = encoded_pos(&encoded);
        if !pending.contains(&pos) {
            continue;
        }
        let Some(prepared) = prepared.iter().find(|prepared| prepared.pos == pos) else {
            continue;
        };
        if encoded_is_current_for(&encoded, prepared) {
            valid_chunks.push(encoded);
        }
    }

    valid_chunks
}

fn resolve_by_index(
    prepared: &[PreparedChunk],
    encoded_chunks: Vec<EncodedChunk>,
    pending: &FxHashSet<ChunkPos>,
) -> Vec<EncodedChunk> {
    let index = prepared
        .iter()
        .map(|prepared| (prepared.pos, prepared))
        .collect::<FxHashMap<_, _>>();
    let mut valid_chunks = Vec::with_capacity(encoded_chunks.len());

    for encoded in encoded_chunks {
        let pos = encoded_pos(&encoded);
        if !pending.contains(&pos) {
            continue;
        }
        let Some(prepared) = index.get(&pos) else {
            continue;
        };
        if encoded_is_current_for(&encoded, prepared) {
            valid_chunks.push(encoded);
        }
    }

    valid_chunks
}

const STRATEGIES: [(&str, Resolver); 3] = [
    ("rescan", resolve_by_rescan),
    ("index", resolve_by_index),
    ("cursor", resolve_valid_chunks),
];

fn bench_batch_resolution(c: &mut Criterion) {
    for shape in [
        BatchShape::AllEncoded,
        BatchShape::HalfEncoded,
        BatchShape::TailOnly,
    ] {
        let mut group = c.benchmark_group(format!("chunk_sender/resolve/{}", shape.name()));

        for batch_size in BATCH_SIZES {
            let fixture = fixture(batch_size, shape);
            let expected = resolve_by_rescan(
                &fixture.batch.chunks,
                fixture.encoded.clone(),
                &fixture.pending,
            )
            .len();

            for (name, resolve) in STRATEGIES {
                assert_eq!(
                    resolve(
                        &fixture.batch.chunks,
                        fixture.encoded.clone(),
                        &fixture.pending
                    )
                    .len(),
                    expected,
                    "{name} must resolve the same chunks as the rescan baseline"
                );

                group.throughput(Throughput::Elements(batch_size as u64));
                group.bench_with_input(BenchmarkId::new(name, batch_size), &batch_size, |b, _| {
                    b.iter_batched(
                        || fixture.encoded.clone(),
                        |encoded| {
                            black_box(resolve(&fixture.batch.chunks, encoded, &fixture.pending))
                        },
                        BatchSize::SmallInput,
                    );
                });
            }
        }

        group.finish();
    }
}

fn bench_send_tick_at_current_cap(c: &mut Criterion) {
    let fixture = fixture(MAX_CHUNKS_PER_TICK, BatchShape::AllEncoded);
    let pool = encoding_pool();

    let mut group = c.benchmark_group("chunk_sender/send_tick");
    group.throughput(Throughput::Elements(MAX_CHUNKS_PER_TICK as u64));

    group.bench_function("encode_batch/cold", |b| {
        b.iter_batched(
            FxHashMap::default,
            |mut cache| {
                black_box(ChunkSender::encode_batch(
                    &fixture.batch,
                    &mut cache,
                    None,
                    &pool,
                ))
            },
            BatchSize::SmallInput,
        );
    });

    group.bench_function("encode_batch/warm", |b| {
        let mut cache = FxHashMap::default();
        let _ = ChunkSender::encode_batch(&fixture.batch, &mut cache, None, &pool);
        b.iter(|| {
            black_box(ChunkSender::encode_batch(
                &fixture.batch,
                &mut cache,
                None,
                &pool,
            ))
        });
    });

    for (name, resolve) in STRATEGIES {
        group.bench_function(format!("resolve/{name}"), |b| {
            b.iter_batched(
                || fixture.encoded.clone(),
                |encoded| black_box(resolve(&fixture.batch.chunks, encoded, &fixture.pending)),
                BatchSize::SmallInput,
            );
        });
    }

    group.finish();
}

criterion_group!(
    chunk_sender,
    bench_batch_resolution,
    bench_send_tick_at_current_cap
);
criterion_main!(chunk_sender);
