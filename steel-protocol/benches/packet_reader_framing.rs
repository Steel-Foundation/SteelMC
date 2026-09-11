#![expect(missing_docs, reason = "benchmarks")]

use criterion::{Criterion, Throughput, criterion_group, criterion_main};
use std::hint::black_box;
use steel_protocol::packet_reader::TCPNetworkDecoder;
use steel_utils::codec::VarInt;
use steel_utils::serial::WriteTo;
use tokio::runtime::Builder;

/// Decodes 256 framed packets from a pre-built wire buffer, exercising the length
/// framing path.
fn bench_packet_reader_framing(c: &mut Criterion) {
    let mut group = c.benchmark_group("packet_reader_framing");
    let rt = Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("tokio runtime");

    let mut wire = Vec::new();
    for i in 0i32..256 {
        let mut packet = Vec::new();
        VarInt::from(i)
            .write(&mut packet)
            .expect("packet id should encode");
        packet.extend_from_slice(&[0xABu8; 32]);
        VarInt::from(packet.len() as i32)
            .write(&mut wire)
            .expect("packet length should encode");
        wire.extend_from_slice(&packet);
    }
    group.throughput(Throughput::Elements(256));

    group.bench_function("decode_256_packets", |b| {
        b.to_async(&rt).iter(|| async {
            let mut decoder = TCPNetworkDecoder::new(wire.as_slice());
            for _ in 0..256 {
                let packet = decoder
                    .get_raw_packet()
                    .await
                    .expect("packet should decode");
                black_box(packet.id);
            }
        });
    });

    group.finish();
}

criterion_group!(benches, bench_packet_reader_framing);
criterion_main!(benches);
