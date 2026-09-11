#![expect(missing_docs, reason = "benchmarks")]

use aes::cipher::KeyIvInit;
use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use std::cell::RefCell;
use std::hint::black_box;
use steel_protocol::packet_traits::EncodedPacket;
use steel_protocol::packet_writer::TCPNetworkEncoder;
use steel_protocol::packets::common::CKeepAlive;
use steel_protocol::utils::{Aes128Cfb8Enc, ConnectionProtocol, StreamEncryptor};
use tokio::io::{AsyncReadExt, AsyncWriteExt, BufWriter};
use tokio::net::{TcpListener, TcpStream};
use tokio::runtime::Builder;

fn bench_stream_encryptor_write_all(c: &mut Criterion) {
    let mut group = c.benchmark_group("stream_encryptor_write_all");
    let key = [0x42u8; 16];
    let iv = [0x24u8; 16];
    let rt = Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("tokio runtime");

    for size in [64, 512, 4096, 65536] {
        let data = vec![0xABu8; size];
        group.throughput(Throughput::Bytes(size as u64));
        group.bench_function(BenchmarkId::new("encrypt_and_write", size), |b| {
            b.to_async(&rt).iter(|| {
                let data = &data;
                async move {
                    let mut sink = Vec::new();
                    let cipher =
                        Aes128Cfb8Enc::new_from_slices(&key, &iv).expect("valid key and iv");
                    let mut writer = StreamEncryptor::new(cipher, &mut sink);
                    writer
                        .write_all(data)
                        .await
                        .expect("write_all should succeed");
                    black_box(sink.len())
                }
            });
        });
    }
    group.finish();
}

/// Measures the production write path on a loopback socket: 128 packets written with a
/// flush per packet (one syscall each) versus a single batched write with one flush. A
/// concurrent reader task drains the socket so writes never block.
fn bench_tcp_socket_write_pattern(c: &mut Criterion) {
    let mut group = c.benchmark_group("tcp_socket_write_pattern");
    let key = [0x42u8; 16];
    let rt = Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("tokio runtime");

    let writer_cell: RefCell<Option<TCPNetworkEncoder<BufWriter<TcpStream>>>> =
        RefCell::new(Some(rt.block_on(async {
            let listener = TcpListener::bind("127.0.0.1:0")
                .await
                .expect("bind loopback listener");
            let addr = listener.local_addr().expect("listener address");
            let client = TcpStream::connect(addr).await.expect("connect to listener");
            client.set_nodelay(true).expect("nodelay");
            let (server, _) = listener.accept().await.expect("accept connection");
            drop(listener);
            tokio::spawn(async move {
                let mut buf = vec![0u8; 65536];
                let mut reader = server;
                loop {
                    match reader.read(&mut buf).await {
                        Ok(0) | Err(_) => break,
                        Ok(_) => {}
                    }
                }
            });
            let mut writer = TCPNetworkEncoder::new(BufWriter::new(client));
            writer.set_encryption(&key);
            writer
        })));

    let packets: Vec<EncodedPacket> = (0i64..128)
        .map(|i| {
            EncodedPacket::from_bare(CKeepAlive::new(i), None, ConnectionProtocol::Play)
                .expect("keep alive should encode")
        })
        .collect();
    group.throughput(Throughput::Elements(128));

    group.bench_function("per_packet_flush", |b| {
        b.to_async(&rt).iter(|| async {
            let mut writer = writer_cell.borrow_mut().take().expect("writer present");
            for packet in &packets {
                writer
                    .write_packet(packet)
                    .await
                    .expect("packet write should succeed");
            }
            writer_cell.borrow_mut().replace(writer);
        });
    });

    group.bench_function("batched_single_flush", |b| {
        b.to_async(&rt).iter(|| async {
            let mut writer = writer_cell.borrow_mut().take().expect("writer present");
            writer
                .write_packets(packets.iter())
                .await
                .expect("batch write should succeed");
            writer_cell.borrow_mut().replace(writer);
        });
    });

    group.finish();
    drop(writer_cell);
}

criterion_group!(
    benches,
    bench_stream_encryptor_write_all,
    bench_tcp_socket_write_pattern
);
criterion_main!(benches);
