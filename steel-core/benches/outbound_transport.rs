//! Exercises the encrypted outbound queue over a real loopback TCP connection.

use std::{
    hint::black_box,
    slice,
    sync::{Arc, Weak},
    time::Duration,
};

use aes::cipher::KeyIvInit;
use criterion::{Criterion, Throughput, criterion_group, criterion_main};
use steel_core::player::{
    Player,
    connection::{JavaConnection, JavaNetworkWriter, OutboundPacket},
};
use steel_protocol::{
    packet_traits::{ClientPacket, CompressionInfo, EncodedPacket},
    packet_writer::TCPNetworkEncoder,
    packets::{
        common::CKeepAlive,
        game::{
            CLevelChunkWithLight, CMoveEntityPosRot, CSetExperience, CSetHealth, CSetTime,
            ChunkPacketData, Heightmaps, LightUpdatePacketData, PackedEntityDelta,
        },
    },
    utils::{Aes128Cfb8Enc, ConnectionProtocol},
};
use steel_utils::{codec::BitSet, locks::AsyncMutex};
use tokio::{
    io::{AsyncReadExt, BufReader, BufWriter},
    net::{TcpListener, TcpStream, tcp::OwnedReadHalf},
    runtime::{Builder as RuntimeBuilder, Runtime},
    sync::mpsc,
    task::JoinHandle,
};
use tokio_util::sync::CancellationToken;

const ENCRYPTION_KEY: [u8; 16] = [
    0x15, 0x71, 0xC4, 0x7E, 0x2A, 0x9B, 0x03, 0xD8, 0x66, 0xF0, 0x4D, 0xB2, 0x89, 0x3C, 0xAE, 0x51,
];

struct TransportWorkload {
    packets: Vec<EncodedPacket>,
    encoded_bytes: usize,
}

struct TransportSession {
    connection: Arc<JavaConnection>,
    reader: BufReader<OwnedReadHalf>,
    sender: Option<JoinHandle<()>>,
    receive_buffer: [u8; 16 * 1_024],
}

#[derive(Clone, Copy)]
enum SenderPath {
    LegacyLocked,
    Production,
}

impl TransportWorkload {
    fn representative_play_burst() -> Self {
        let compression = CompressionInfo::default();
        let mut packets = Vec::new();

        for index in 0..768 {
            match index % 5 {
                0 => push_packet(&mut packets, CKeepAlive::new(index), compression),
                1 => push_packet(
                    &mut packets,
                    CSetTime::new(index, vec![(0, index, 0.5, 1.0)]),
                    compression,
                ),
                2 => push_packet(
                    &mut packets,
                    CSetHealth {
                        health: 20.0,
                        food: 20,
                        food_saturation: 5.0,
                    },
                    compression,
                ),
                3 => push_packet(
                    &mut packets,
                    CSetExperience {
                        progress: 0.5,
                        level: index as i32 % 100,
                        total_experience: index as i32 * 7,
                    },
                    compression,
                ),
                _ => push_packet(
                    &mut packets,
                    CMoveEntityPosRot {
                        entity_id: index as i32,
                        dx: PackedEntityDelta::from_raw(17),
                        dy: PackedEntityDelta::from_raw(-3),
                        dz: PackedEntityDelta::from_raw(9),
                        y_rot: 32,
                        x_rot: -8,
                        on_ground: true,
                    },
                    compression,
                ),
            }
        }

        for (index, size) in [320, 512, 768, 1_024, 1_536, 2_048]
            .into_iter()
            .cycle()
            .take(96)
            .enumerate()
        {
            push_packet(
                &mut packets,
                chunk_packet(index as i32, size, index as u32 + 1),
                compression,
            );
        }

        for (index, size) in [16_384, 32_768, 49_152, 65_536].into_iter().enumerate() {
            push_packet(
                &mut packets,
                chunk_packet(index as i32, size, index as u32 + 101),
                compression,
            );
        }

        let encoded_bytes = packets.iter().map(|packet| packet.encoded_data.len()).sum();
        Self {
            packets,
            encoded_bytes,
        }
    }
}

fn push_packet<P: ClientPacket>(
    packets: &mut Vec<EncodedPacket>,
    packet: P,
    compression: CompressionInfo,
) {
    packets.push(
        EncodedPacket::from_bare(packet, Some(compression), ConnectionProtocol::Play)
            .expect("representative play packet should encode"),
    );
}

fn chunk_packet(index: i32, data_size: usize, seed: u32) -> CLevelChunkWithLight {
    CLevelChunkWithLight {
        x: index,
        z: -index,
        chunk_data: ChunkPacketData {
            heightmaps: Heightmaps {
                heightmaps: Vec::new(),
            },
            data: deterministic_bytes(data_size, seed),
            block_entities: Vec::new(),
        },
        light_data: LightUpdatePacketData {
            sky_y_mask: empty_bit_set(),
            block_y_mask: empty_bit_set(),
            empty_sky_y_mask: empty_bit_set(),
            empty_block_y_mask: empty_bit_set(),
            sky_updates: Vec::new(),
            block_updates: Vec::new(),
        },
    }
}

fn empty_bit_set() -> BitSet {
    BitSet(Vec::new().into_boxed_slice())
}

fn deterministic_bytes(len: usize, seed: u32) -> Vec<u8> {
    let mut state = seed.max(1);
    (0..len)
        .map(|_| {
            state ^= state << 13;
            state ^= state >> 17;
            state ^= state << 5;
            state as u8
        })
        .collect()
}

impl TransportSession {
    async fn connect() -> Self {
        Self::connect_with_sender(SenderPath::Production).await
    }

    async fn connect_with_sender(sender_path: SenderPath) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("loopback listener should bind");
        let address = listener
            .local_addr()
            .expect("loopback listener should have an address");
        let connect = TcpStream::connect(address);
        let accept = listener.accept();
        let (client_result, server_result) = tokio::join!(connect, accept);
        let client = client_result.expect("loopback client should connect");
        let (server, _) = server_result.expect("loopback server should accept");
        client
            .set_nodelay(true)
            .expect("loopback client should enable TCP_NODELAY");
        server
            .set_nodelay(true)
            .expect("loopback server should enable TCP_NODELAY");

        let (_, server_write) = server.into_split();
        let (client_read, _) = client.into_split();
        let mut encoder = TCPNetworkEncoder::new(BufWriter::new(server_write));
        encoder.set_encryption(&ENCRYPTION_KEY);
        let network_writer: JavaNetworkWriter = Arc::new(AsyncMutex::new(Some(encoder)));
        let (outgoing_packets, outgoing_receiver) = mpsc::unbounded_channel();
        let cancel_token = CancellationToken::new();
        let connection = Arc::new(JavaConnection::new(
            outgoing_packets,
            cancel_token.clone(),
            Some(CompressionInfo::default()),
            Arc::clone(&network_writer),
            1,
            Weak::<Player>::new(),
        ));
        let sender = match sender_path {
            SenderPath::LegacyLocked => tokio::spawn(legacy_locked_sender(
                network_writer,
                cancel_token,
                outgoing_receiver,
            )),
            SenderPath::Production => {
                let sender_connection = Arc::clone(&connection);
                tokio::spawn(async move {
                    sender_connection.sender(outgoing_receiver).await;
                })
            }
        };

        Self {
            connection,
            reader: BufReader::new(client_read),
            sender: Some(sender),
            receive_buffer: [0; 16 * 1_024],
        }
    }

    async fn send(&mut self, packets: &[EncodedPacket], encoded_bytes: usize) {
        for packet in packets {
            self.connection.send_encoded_packet(packet.clone());
        }

        let mut remaining = encoded_bytes;
        while remaining != 0 {
            let to_read = remaining.min(self.receive_buffer.len());
            self.reader
                .read_exact(&mut self.receive_buffer[..to_read])
                .await
                .expect("loopback client should receive the encrypted bytes");
            black_box(&self.receive_buffer[..to_read]);
            remaining -= to_read;
        }
    }

    async fn send_and_collect(
        &mut self,
        packets: &[EncodedPacket],
        encoded_bytes: usize,
    ) -> Vec<u8> {
        for packet in packets {
            self.connection.send_encoded_packet(packet.clone());
        }

        let mut ciphertext = vec![0; encoded_bytes];
        self.reader
            .read_exact(&mut ciphertext)
            .await
            .expect("loopback client should receive the encrypted bytes");
        ciphertext
    }

    async fn close(&mut self) {
        self.connection.close();
        let Some(sender) = self.sender.take() else {
            panic!("transport session should close only once");
        };
        sender.await.expect("sender task should finish");
    }
}

async fn write_locked_packet(network_writer: &JavaNetworkWriter, packet: &EncodedPacket) {
    let mut network_writer = network_writer.lock().await;
    let Some(network_writer) = network_writer.as_mut() else {
        panic!("legacy benchmark writer should remain open");
    };
    network_writer
        .write_packet(packet)
        .await
        .expect("legacy benchmark packet should write");
}

async fn legacy_locked_sender(
    network_writer: JavaNetworkWriter,
    cancel_token: CancellationToken,
    mut receiver: mpsc::UnboundedReceiver<OutboundPacket>,
) {
    loop {
        tokio::select! {
            biased;
            () = cancel_token.cancelled() => break,
            outbound = receiver.recv() => {
                let Some(outbound) = outbound else {
                    cancel_token.cancel();
                    continue;
                };
                let (packet, close_after_write) = match outbound {
                    OutboundPacket::Packet(packet) => (packet, false),
                    OutboundPacket::Disconnect(packet) => (packet, true),
                };
                if close_after_write {
                    write_locked_packet(&network_writer, &packet).await;
                    cancel_token.cancel();
                    break;
                }

                let write = write_locked_packet(&network_writer, &packet);
                tokio::pin!(write);
                tokio::select! {
                    biased;
                    () = cancel_token.cancelled() => break,
                    () = write.as_mut() => {}
                }
            }
        }
    }

    drop(network_writer.lock().await.take());
}

async fn transport_round_trip(workload: &TransportWorkload) {
    let mut session = TransportSession::connect().await;
    session
        .send(&workload.packets, workload.encoded_bytes)
        .await;
    session.close().await;
}

async fn verify_transport_ciphertext(workload: &TransportWorkload, sender_path: SenderPath) {
    let mut session = TransportSession::connect_with_sender(sender_path).await;
    let actual = session
        .send_and_collect(&workload.packets, workload.encoded_bytes)
        .await;

    let mut expected = Vec::with_capacity(workload.encoded_bytes);
    for packet in &workload.packets {
        expected.extend_from_slice(&packet.encoded_data);
    }
    Aes128Cfb8Enc::new(&ENCRYPTION_KEY.into(), &ENCRYPTION_KEY.into()).encrypt(&mut expected);

    assert_eq!(actual, expected);
    session.close().await;
    let mut trailing = Vec::new();
    session
        .reader
        .read_to_end(&mut trailing)
        .await
        .expect("closed loopback transport should reach EOF");
    assert!(
        trailing.is_empty(),
        "transport wrote unexpected extra bytes"
    );
}

fn benchmark_runtime() -> Runtime {
    RuntimeBuilder::new_current_thread()
        .enable_io()
        .build()
        .expect("benchmark runtime should build")
}

fn outbound_transport(criterion: &mut Criterion) {
    let runtime = benchmark_runtime();
    let workload = TransportWorkload::representative_play_burst();
    runtime.block_on(verify_transport_ciphertext(
        &workload,
        SenderPath::Production,
    ));
    runtime.block_on(verify_transport_ciphertext(
        &workload,
        SenderPath::LegacyLocked,
    ));
    runtime.block_on(transport_round_trip(&workload));

    let mut group = criterion.benchmark_group("encrypted_outbound_transport_e2e");
    group.throughput(Throughput::Bytes(workload.encoded_bytes as u64));
    group.bench_function("representative_play_burst", |bencher| {
        bencher.iter(|| runtime.block_on(transport_round_trip(black_box(&workload))));
    });
    group.finish();

    let mut throughput_session = runtime.block_on(TransportSession::connect());
    runtime.block_on(throughput_session.send(&workload.packets, workload.encoded_bytes));

    let mut steady_state_group =
        criterion.benchmark_group("encrypted_outbound_transport_steady_state");
    steady_state_group.throughput(Throughput::Bytes(workload.encoded_bytes as u64));
    steady_state_group.bench_function("representative_play_burst", |bencher| {
        bencher.iter(|| {
            runtime.block_on(throughput_session.send(
                black_box(&workload.packets),
                black_box(workload.encoded_bytes),
            ));
        });
    });
    steady_state_group.finish();
    runtime.block_on(throughput_session.close());

    let mut sender_group = criterion.benchmark_group("encrypted_outbound_transport_sender_ab");
    sender_group.throughput(Throughput::Bytes(workload.encoded_bytes as u64));
    for (name, sender_path) in [
        ("legacy_locked_per_packet", SenderPath::LegacyLocked),
        ("production_owned_writer", SenderPath::Production),
    ] {
        let mut session = runtime.block_on(TransportSession::connect_with_sender(sender_path));
        runtime.block_on(session.send(&workload.packets, workload.encoded_bytes));
        sender_group.bench_function(name, |bencher| {
            bencher.iter(|| {
                runtime.block_on(session.send(
                    black_box(&workload.packets),
                    black_box(workload.encoded_bytes),
                ));
            });
        });
        runtime.block_on(session.close());
    }
    sender_group.finish();

    let latency_packet = workload
        .packets
        .first()
        .expect("representative workload should not be empty")
        .clone();
    let latency_bytes = latency_packet.encoded_data.len();
    let mut latency_session = runtime.block_on(TransportSession::connect());
    runtime.block_on(latency_session.send(slice::from_ref(&latency_packet), latency_bytes));

    let mut latency_group = criterion.benchmark_group("encrypted_outbound_transport_latency");
    latency_group.throughput(Throughput::Bytes(latency_bytes as u64));
    latency_group.bench_function("single_small_packet", |bencher| {
        bencher.iter(|| {
            runtime.block_on(latency_session.send(slice::from_ref(&latency_packet), latency_bytes));
        });
    });
    latency_group.finish();
    runtime.block_on(latency_session.close());
}

criterion_group! {
    name = benches;
    config = Criterion::default()
        .sample_size(20)
        .measurement_time(Duration::from_secs(10));
    targets = outbound_transport
}
criterion_main!(benches);
