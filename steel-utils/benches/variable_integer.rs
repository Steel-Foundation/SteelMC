#![expect(missing_docs, reason = "benchmarks")]

use std::{
    hint::black_box,
    io::{self, Cursor, Write},
};

use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use steel_utils::{
    FrontVec,
    codec::{VarInt, VarLong},
    random::{Random, xoroshiro::Xoroshiro},
    serial::WriteTo,
};

fn scalar_write_varint(value: i32, writer: &mut impl Write) -> io::Result<()> {
    let mut value = value as u32;
    loop {
        let byte = value as u8 & 0x7f;
        value >>= 7;
        if value == 0 {
            byte.write(writer)?;
            break;
        }
        (byte | 0x80).write(writer)?;
    }
    Ok(())
}

fn scalar_write_varlong(value: i64, writer: &mut impl Write) -> io::Result<()> {
    let mut value = value as u64;
    loop {
        let byte = value as u8 & 0x7f;
        value >>= 7;
        if value == 0 {
            byte.write(writer)?;
            break;
        }
        (byte | 0x80).write(writer)?;
    }
    Ok(())
}

fn bench_varint(c: &mut Criterion) {
    let mut random = Xoroshiro::from_seed(0);
    let mixed: Vec<_> = (0..1_024)
        .map(|_| (random.next_i32() as u32 >> (random.next_i32() as u32 % 32)) as i32)
        .collect();
    let single_byte: Vec<_> = mixed.iter().map(|value| value & 0x7f).collect();
    let cases: &[(&str, &[i32])] = &[
        ("single_byte", &[0, 1, 42, 127]),
        ("single_byte_stream", &single_byte),
        (
            "packet_fields",
            &[0, 1, 2, 127, 128, 255, 300, 16_384, 32_767, 2_097_151],
        ),
        ("wide", &[2_097_152, 268_435_455, i32::MAX, i32::MIN, -1]),
        ("mixed_lengths", &mixed),
    ];

    let mut group = c.benchmark_group("varint");
    for &(name, values) in cases {
        group.throughput(Throughput::Elements(values.len() as u64));
        group.bench_with_input(BenchmarkId::new("write", name), values, |b, values| {
            let mut encoded = Vec::with_capacity(values.len() * VarInt::MAX_SIZE);
            b.iter(|| {
                encoded.clear();
                for &value in black_box(values) {
                    VarInt(value).write(&mut encoded).expect("encode");
                }
                black_box(&encoded);
            });
        });
        group.bench_with_input(
            BenchmarkId::new("write_scalar", name),
            values,
            |b, values| {
                let mut encoded = Vec::with_capacity(values.len() * VarInt::MAX_SIZE);
                b.iter(|| {
                    encoded.clear();
                    for &value in black_box(values) {
                        scalar_write_varint(value, &mut encoded).expect("encode");
                    }
                    black_box(&encoded);
                });
            },
        );
    }
    group.finish();
}

fn bench_varlong(c: &mut Criterion) {
    let mut random = Xoroshiro::from_seed(0);
    let mixed: Vec<_> = (0..1_024)
        .map(|_| (random.next_i64() as u64 >> (random.next_i32() as u32 % 64)) as i64)
        .collect();
    let single_byte: Vec<_> = mixed.iter().map(|value| value & 0x7f).collect();
    let cases: &[(&str, &[i64])] = &[
        ("single_byte", &[0, 1, 42, 127]),
        ("single_byte_stream", &single_byte),
        (
            "packet_fields",
            &[0, 1, 2, 127, 128, 255, 300, 16_384, 32_767, 2_097_151],
        ),
        ("mixed_lengths", &mixed),
        (
            "wide",
            &[
                1 << 28,
                1 << 35,
                1 << 42,
                1 << 49,
                1 << 56,
                i64::MAX,
                i64::MIN,
                -1,
            ],
        ),
    ];

    let mut group = c.benchmark_group("varlong");
    for &(name, values) in cases {
        group.throughput(Throughput::Elements(values.len() as u64));
        group.bench_with_input(BenchmarkId::new("write", name), values, |b, values| {
            let mut encoded = Vec::with_capacity(values.len() * VarLong::MAX_SIZE);
            b.iter(|| {
                encoded.clear();
                for &value in black_box(values) {
                    VarLong(value).write(&mut encoded).expect("encode");
                }
                black_box(&encoded);
            });
        });
        group.bench_with_input(
            BenchmarkId::new("write_scalar", name),
            values,
            |b, values| {
                let mut encoded = Vec::with_capacity(values.len() * VarLong::MAX_SIZE);
                b.iter(|| {
                    encoded.clear();
                    for &value in black_box(values) {
                        scalar_write_varlong(value, &mut encoded).expect("encode");
                    }
                    black_box(&encoded);
                });
            },
        );
    }
    group.finish();
}

fn bench_packet_prefix(c: &mut Criterion) {
    let mut group = c.benchmark_group("varint/packet_prefix");
    for value in [0, 300, 32_767, 2_097_151, 2_097_152] {
        group.bench_with_input(BenchmarkId::from_parameter(value), &value, |b, &value| {
            b.iter_batched_ref(
                || FrontVec::new(VarInt::MAX_SIZE),
                |buffer| {
                    let value = black_box(value);
                    VarInt(value).set_in_front(buffer, VarInt::written_size(value));
                    black_box(buffer.as_slice());
                },
                criterion::BatchSize::SmallInput,
            );
        });
        group.bench_with_input(BenchmarkId::new("scalar", value), &value, |b, &value| {
            b.iter_batched_ref(
                || FrontVec::new(VarInt::MAX_SIZE),
                |buffer| {
                    let value = black_box(value);
                    let mut bytes = [0; VarInt::MAX_SIZE];
                    scalar_write_varint(value, &mut Cursor::new(&mut bytes[..])).expect("encode");
                    buffer.set_in_front(&bytes[..VarInt::written_size(value)]);
                    black_box(buffer.as_slice());
                },
                criterion::BatchSize::SmallInput,
            );
        });
    }
    group.finish();
}

criterion_group!(benches, bench_varint, bench_varlong, bench_packet_prefix);
criterion_main!(benches);
