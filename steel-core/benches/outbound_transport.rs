//! Exercises the encrypted outbound queue over a real loopback TCP connection.

use std::{
    hint::black_box,
    slice,
    sync::{Arc, Weak},
    time::{Duration, Instant},
};

use aes::cipher::KeyIvInit;
use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use steel_core::player::{
    Player,
    connection::{JavaConnection, NetworkConnection, OutboundPacket},
};
use steel_protocol::{
    packet_traits::{ClientPacket, CompressionInfo, EncodedPacket},
    packet_writer::TCPNetworkEncoder,
    packets::{
        common::CKeepAlive,
        game::{
            CChunkBatchFinished, CChunkBatchStart, CLevelChunkWithLight, CMoveEntityPosRot,
            CSetExperience, CSetHealth, CSetTime, ChunkPacketData, Heightmaps,
            LightUpdatePacketData, PackedEntityDelta,
        },
    },
    utils::ConnectionProtocol,
};
use steel_utils::{codec::BitSet, locks::AsyncMutex};
use tokio::{
    io::{AsyncReadExt, BufReader, BufWriter},
    net::{TcpListener, TcpSocket, TcpStream, tcp::OwnedReadHalf, tcp::OwnedWriteHalf},
    runtime::{Builder as RuntimeBuilder, Runtime},
    sync::mpsc,
    task::JoinHandle,
};
use tokio_util::sync::CancellationToken;

const BENCHMARK_ENCRYPTION_KEY: [u8; 16] = *b"SteelMC-test-key";
const LOOPBACK_READ_BUFFER_BYTES: usize = 16 * 1_024;
// Keep the hot writer backpressured so wire progress is a reliable contention barrier.
const CONTENDED_RECEIVE_BUFFER_BYTES: u32 = 16 * 1_024;
const CONTROL_LATENCY_SAMPLE_COUNT: usize = 256;

// This is a fixed stress burst, not an estimate of Vanilla packet frequency. It combines real
// packet encoders with synthetic values. These sizes describe chunk data before packet framing
// and compression; the protocol bench instead sizes already-encoded bytes.
const SMALL_PLAY_PACKET_COUNT: usize = 768;
const MEDIUM_CHUNK_PACKET_COUNT: usize = 96;
const MEDIUM_CHUNK_DATA_BYTE_SIZES: [usize; 6] = [320, 512, 768, 1_024, 1_536, 2_048];
const LARGE_CHUNK_DATA_BYTE_SIZES: [usize; 4] = [16_384, 32_768, 49_152, 65_536];
const BATCH_CHUNK_DATA_BYTE_SIZES: [usize; 4] = [8_192, 16_384, 24_576, 32_768];
// Separate seed ranges prevent the workload classes from reusing the same payload prefix.
const MEDIUM_CHUNK_PAYLOAD_SEED_START: u32 = 1;
const LARGE_CHUNK_PAYLOAD_SEED_START: u32 = 101;
const BATCH_CHUNK_PAYLOAD_SEED_START: u32 = 501;

const SMALL_EXPLICIT_BATCH_PACKET_COUNT: usize = 8;
const REPRESENTATIVE_CHUNK_BATCH_SIZE: usize = 9;
type ReferenceCfb8Encryptor = cfb8::Encryptor<aes::Aes128>;
type LegacyNetworkWriter = Arc<AsyncMutex<Option<TCPNetworkEncoder<BufWriter<OwnedWriteHalf>>>>>;

struct TransportWorkload {
    packets: Vec<EncodedPacket>,
    encoded_bytes: usize,
}

struct TransportSession {
    connection: Arc<JavaConnection>,
    reader: BufReader<OwnedReadHalf>,
    sender: Option<JoinHandle<()>>,
    receive_buffer: [u8; LOOPBACK_READ_BUFFER_BYTES],
}

#[derive(Clone, Copy)]
enum SenderPath {
    LegacyLocked,
    Production,
}

#[derive(Clone, Copy)]
enum SendMode {
    Individual,
    Batch,
}

#[derive(Clone, Copy)]
enum SmallPlayPacketKind {
    KeepAlive,
    TimeUpdate,
    HealthUpdate,
    ExperienceUpdate,
    RelativeEntityMove,
}

const SMALL_PLAY_PACKET_MIX: [SmallPlayPacketKind; 5] = [
    SmallPlayPacketKind::KeepAlive,
    SmallPlayPacketKind::TimeUpdate,
    SmallPlayPacketKind::HealthUpdate,
    SmallPlayPacketKind::ExperienceUpdate,
    SmallPlayPacketKind::RelativeEntityMove,
];

impl TransportWorkload {
    fn representative_play_burst() -> Self {
        let compression = CompressionInfo::default();
        let mut packets = Vec::new();

        for (packet_index, packet_kind) in SMALL_PLAY_PACKET_MIX
            .into_iter()
            .cycle()
            .take(SMALL_PLAY_PACKET_COUNT)
            .enumerate()
        {
            push_small_play_packet(&mut packets, packet_kind, packet_index, compression);
        }

        for (packet_index, chunk_data_bytes) in MEDIUM_CHUNK_DATA_BYTE_SIZES
            .into_iter()
            .cycle()
            .take(MEDIUM_CHUNK_PACKET_COUNT)
            .enumerate()
        {
            push_packet(
                &mut packets,
                chunk_packet(
                    packet_index as i32,
                    chunk_data_bytes,
                    packet_index as u32 + MEDIUM_CHUNK_PAYLOAD_SEED_START,
                ),
                compression,
            );
        }

        for (packet_index, chunk_data_bytes) in LARGE_CHUNK_DATA_BYTE_SIZES.into_iter().enumerate()
        {
            push_packet(
                &mut packets,
                chunk_packet(
                    packet_index as i32,
                    chunk_data_bytes,
                    packet_index as u32 + LARGE_CHUNK_PAYLOAD_SEED_START,
                ),
                compression,
            );
        }

        Self::from_packets(packets)
    }

    fn representative_chunk_batch(chunk_count: usize) -> Self {
        let compression = CompressionInfo::default();
        let mut packets = Vec::with_capacity(chunk_count.saturating_add(2));
        push_packet(&mut packets, CChunkBatchStart {}, compression);
        for (packet_index, chunk_data_bytes) in BATCH_CHUNK_DATA_BYTE_SIZES
            .into_iter()
            .cycle()
            .take(chunk_count)
            .enumerate()
        {
            push_packet(
                &mut packets,
                chunk_packet(
                    packet_index as i32,
                    chunk_data_bytes,
                    packet_index as u32 + BATCH_CHUNK_PAYLOAD_SEED_START,
                ),
                compression,
            );
        }
        push_packet(
            &mut packets,
            CChunkBatchFinished {
                batch_size: chunk_count as i32,
            },
            compression,
        );
        Self::from_packets(packets)
    }

    fn prefix(&self, packet_count: usize) -> Self {
        Self::from_packets(self.packets[..packet_count].to_vec())
    }

    fn from_packets(packets: Vec<EncodedPacket>) -> Self {
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

fn push_small_play_packet(
    packets: &mut Vec<EncodedPacket>,
    packet_kind: SmallPlayPacketKind,
    packet_index: usize,
    compression: CompressionInfo,
) {
    // Sequence-derived IDs and counters vary integer encodings. Fixed float, signed delta, and
    // rotation fields keep those encodings in the mix. These are reproducible fixture values, not
    // a simulated player or Vanilla gameplay constants.
    let sequence = packet_index as i64;
    match packet_kind {
        SmallPlayPacketKind::KeepAlive => {
            push_packet(packets, CKeepAlive::new(sequence), compression);
        }
        SmallPlayPacketKind::TimeUpdate => {
            let clock_registry_id = 0;
            let partial_tick = 0.5;
            let normal_clock_rate = 1.0;
            push_packet(
                packets,
                CSetTime::new(
                    sequence,
                    vec![(clock_registry_id, sequence, partial_tick, normal_clock_rate)],
                ),
                compression,
            );
        }
        SmallPlayPacketKind::HealthUpdate => push_packet(
            packets,
            CSetHealth {
                health: 20.0,
                food: 20,
                food_saturation: 5.0,
            },
            compression,
        ),
        SmallPlayPacketKind::ExperienceUpdate => push_packet(
            packets,
            CSetExperience {
                progress: 0.5,
                level: packet_index as i32 % 100,
                total_experience: packet_index as i32 * 7,
            },
            compression,
        ),
        SmallPlayPacketKind::RelativeEntityMove => push_packet(
            packets,
            CMoveEntityPosRot {
                entity_id: packet_index as i32,
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

fn chunk_packet(index: i32, chunk_data_bytes: usize, payload_seed: u32) -> CLevelChunkWithLight {
    CLevelChunkWithLight {
        x: index,
        z: -index,
        chunk_data: ChunkPacketData {
            heightmaps: Heightmaps {
                heightmaps: Vec::new(),
            },
            data: deterministic_payload_bytes(chunk_data_bytes, payload_seed),
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

fn deterministic_payload_bytes(len: usize, seed: u32) -> Vec<u8> {
    // Xorshift32 gives reproducible, non-constant chunk data without using gameplay RNG.
    // Its zero state never changes, so start zero-valued seeds at one instead.
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

    async fn connect_for_contention() -> Self {
        Self::connect_with_sender_and_receive_buffer(
            SenderPath::Production,
            Some(CONTENDED_RECEIVE_BUFFER_BYTES),
        )
        .await
    }

    async fn connect_with_sender(sender_path: SenderPath) -> Self {
        Self::connect_with_sender_and_receive_buffer(sender_path, None).await
    }

    async fn connect_with_sender_and_receive_buffer(
        sender_path: SenderPath,
        receive_buffer_size: Option<u32>,
    ) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("loopback listener should bind");
        let address = listener
            .local_addr()
            .expect("loopback listener should have an address");
        let connect = async {
            if let Some(receive_buffer_size) = receive_buffer_size {
                let socket = TcpSocket::new_v4()?;
                socket.set_recv_buffer_size(receive_buffer_size)?;
                socket.connect(address).await
            } else {
                TcpStream::connect(address).await
            }
        };
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
        encoder.set_encryption(&BENCHMARK_ENCRYPTION_KEY);
        let (outgoing_packets, outgoing_receiver) = mpsc::unbounded_channel();
        let cancel_token = CancellationToken::new();
        let connection = Arc::new(JavaConnection::new(
            outgoing_packets,
            cancel_token.clone(),
            Some(CompressionInfo::default()),
            1,
            Weak::<Player>::new(),
        ));
        let sender = match sender_path {
            SenderPath::LegacyLocked => {
                let network_writer = Arc::new(AsyncMutex::new(Some(encoder)));
                tokio::spawn(legacy_locked_sender(
                    network_writer,
                    cancel_token,
                    outgoing_receiver,
                ))
            }
            SenderPath::Production => {
                let sender_connection = Arc::clone(&connection);
                tokio::spawn(async move {
                    sender_connection.sender(outgoing_receiver, encoder).await;
                })
            }
        };

        Self {
            connection,
            reader: BufReader::new(client_read),
            sender: Some(sender),
            receive_buffer: [0; LOOPBACK_READ_BUFFER_BYTES],
        }
    }

    async fn send(&mut self, packets: &[EncodedPacket], encoded_bytes: usize) {
        self.send_with_mode(packets, encoded_bytes, SendMode::Individual)
            .await;
    }

    async fn send_with_mode(
        &mut self,
        packets: &[EncodedPacket],
        encoded_bytes: usize,
        mode: SendMode,
    ) {
        self.enqueue(packets, mode);
        self.receive(encoded_bytes).await;
    }

    fn enqueue(&self, packets: &[EncodedPacket], mode: SendMode) {
        match mode {
            SendMode::Individual => {
                for packet in packets {
                    self.connection.send_encoded_packet(packet.clone());
                }
            }
            SendMode::Batch => self.connection.send_encoded_batch(packets.to_vec()),
        }
    }

    async fn receive(&mut self, encoded_bytes: usize) {
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
        mode: SendMode,
    ) -> Vec<u8> {
        self.enqueue(packets, mode);

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

async fn write_locked_packet(network_writer: &LegacyNetworkWriter, packet: &EncodedPacket) {
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
    network_writer: LegacyNetworkWriter,
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
                    OutboundPacket::PacketBatch(packets) => {
                        for packet in *packets {
                            write_locked_packet(&network_writer, &packet).await;
                        }
                        continue;
                    }
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

async fn transport_round_trip(workload: &TransportWorkload, mode: SendMode) {
    let mut session = TransportSession::connect().await;
    session
        .send_with_mode(&workload.packets, workload.encoded_bytes, mode)
        .await;
    session.close().await;
}

async fn verify_transport_ciphertext(
    workload: &TransportWorkload,
    sender_path: SenderPath,
    mode: SendMode,
) {
    let mut session = TransportSession::connect_with_sender(sender_path).await;
    let actual = session
        .send_and_collect(&workload.packets, workload.encoded_bytes, mode)
        .await;

    let mut expected = Vec::with_capacity(workload.encoded_bytes);
    for packet in &workload.packets {
        expected.extend_from_slice(&packet.encoded_data);
    }
    ReferenceCfb8Encryptor::new(
        &BENCHMARK_ENCRYPTION_KEY.into(),
        &BENCHMARK_ENCRYPTION_KEY.into(),
    )
    .encrypt(&mut expected);

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

async fn hot_connection_control_latency(
    hot_session: &mut TransportSession,
    control_session: &mut TransportSession,
    hot_workload: &TransportWorkload,
    control_packet: &EncodedPacket,
    hot_mode: SendMode,
) -> Duration {
    hot_session.enqueue(&hot_workload.packets, hot_mode);
    // Unlike yielding, receiving bytes proves that the hot sender has started.
    let first_packet_bytes = hot_workload
        .packets
        .first()
        .expect("hot workload should not be empty")
        .encoded_data
        .len();
    hot_session.receive(first_packet_bytes).await;

    let started = Instant::now();
    control_session.enqueue(slice::from_ref(control_packet), SendMode::Individual);
    let control_bytes = control_packet.encoded_data.len();
    let control_receive = async {
        control_session.receive(control_bytes).await;
        started.elapsed()
    };
    let ((), control_latency) = tokio::join!(
        hot_session.receive(hot_workload.encoded_bytes - first_packet_bytes),
        control_receive
    );
    control_latency
}

async fn sample_hot_connection_control_latency(
    hot_workload: &TransportWorkload,
    control_packet: &EncodedPacket,
    hot_mode: SendMode,
) -> Vec<Duration> {
    let mut hot_session = TransportSession::connect_for_contention().await;
    let mut control_session = TransportSession::connect().await;
    let mut samples = Vec::with_capacity(CONTROL_LATENCY_SAMPLE_COUNT);

    for _ in 0..CONTROL_LATENCY_SAMPLE_COUNT {
        samples.push(
            hot_connection_control_latency(
                &mut hot_session,
                &mut control_session,
                hot_workload,
                control_packet,
                hot_mode,
            )
            .await,
        );
    }

    hot_session.close().await;
    control_session.close().await;
    samples
}

fn percentile(samples: &mut [Duration], percentile: usize) -> Duration {
    samples.sort_unstable();
    let rank = samples
        .len()
        .saturating_mul(percentile)
        .div_ceil(100)
        .saturating_sub(1)
        .min(samples.len().saturating_sub(1));
    samples[rank]
}

fn benchmark_runtime() -> Runtime {
    RuntimeBuilder::new_current_thread()
        .enable_io()
        .enable_time()
        .build()
        .expect("benchmark runtime should build")
}

fn multi_thread_benchmark_runtime() -> Runtime {
    RuntimeBuilder::new_multi_thread()
        .worker_threads(2)
        .enable_io()
        .enable_time()
        .build()
        .expect("multi-thread benchmark runtime should build")
}

fn benchmark_explicit_batching(
    criterion: &mut Criterion,
    runtime: &Runtime,
    workloads: &[(&str, &TransportWorkload)],
) {
    let mut group = criterion.benchmark_group("encrypted_outbound_transport_batching_ab");
    for &(workload_name, workload) in workloads {
        group.throughput(Throughput::Bytes(workload.encoded_bytes as u64));
        for (mode_name, mode) in [
            ("individual_flushes", SendMode::Individual),
            ("explicit_batch", SendMode::Batch),
        ] {
            let mut session = runtime.block_on(TransportSession::connect());
            runtime.block_on(session.send_with_mode(
                &workload.packets,
                workload.encoded_bytes,
                mode,
            ));
            group.bench_with_input(
                BenchmarkId::new(workload_name, mode_name),
                workload,
                |bencher, workload| {
                    bencher.iter(|| {
                        runtime.block_on(session.send_with_mode(
                            black_box(&workload.packets),
                            black_box(workload.encoded_bytes),
                            mode,
                        ));
                    });
                },
            );
            runtime.block_on(session.close());
        }
    }
    group.finish();
}

fn benchmark_multi_connection_contention(
    criterion: &mut Criterion,
    runtimes: &[(&str, &Runtime)],
    workload: &TransportWorkload,
    control_packet: &EncodedPacket,
) {
    for &(runtime_name, runtime) in runtimes {
        for (mode_name, mode) in [
            ("individual_flushes", SendMode::Individual),
            ("explicit_batch", SendMode::Batch),
        ] {
            let mut samples = runtime.block_on(sample_hot_connection_control_latency(
                workload,
                control_packet,
                mode,
            ));
            let p50 = percentile(&mut samples, 50);
            let p95 = percentile(&mut samples, 95);
            let p99 = percentile(&mut samples, 99);
            eprintln!(
                "hot-connection control latency {runtime_name}/{mode_name}: p50={p50:?} p95={p95:?} p99={p99:?}"
            );
        }
    }

    let mut group =
        criterion.benchmark_group("encrypted_outbound_transport_multi_connection_contention");
    group.throughput(Throughput::Bytes(workload.encoded_bytes as u64));
    for &(runtime_name, runtime) in runtimes {
        for (mode_name, mode) in [
            ("individual_flushes", SendMode::Individual),
            ("explicit_batch", SendMode::Batch),
        ] {
            let mut hot_session = runtime.block_on(TransportSession::connect_for_contention());
            let mut control_session = runtime.block_on(TransportSession::connect());
            group.bench_function(BenchmarkId::new(runtime_name, mode_name), |bencher| {
                bencher.iter(|| {
                    black_box(runtime.block_on(hot_connection_control_latency(
                        &mut hot_session,
                        &mut control_session,
                        black_box(workload),
                        black_box(control_packet),
                        mode,
                    )));
                });
            });
            runtime.block_on(hot_session.close());
            runtime.block_on(control_session.close());
        }
    }
    group.finish();
}

fn benchmark_single_packet_latency(
    criterion: &mut Criterion,
    runtime: &Runtime,
    packet: &EncodedPacket,
) {
    let packet_bytes = packet.encoded_data.len();
    let mut group = criterion.benchmark_group("encrypted_outbound_transport_latency");
    group.throughput(Throughput::Bytes(packet_bytes as u64));
    for (name, sender_path) in [
        ("legacy_locked_per_packet", SenderPath::LegacyLocked),
        ("production_owned_writer", SenderPath::Production),
    ] {
        let mut session = runtime.block_on(TransportSession::connect_with_sender(sender_path));
        runtime.block_on(session.send(slice::from_ref(packet), packet_bytes));
        group.bench_function(name, |bencher| {
            bencher.iter(|| {
                runtime.block_on(session.send(slice::from_ref(packet), packet_bytes));
            });
        });
        runtime.block_on(session.close());
    }
    group.finish();
}

fn outbound_transport(criterion: &mut Criterion) {
    let runtime = benchmark_runtime();
    let multi_thread_runtime = multi_thread_benchmark_runtime();
    let workload = TransportWorkload::representative_play_burst();
    let small_group = workload.prefix(SMALL_EXPLICIT_BATCH_PACKET_COUNT);
    let chunk_batch =
        TransportWorkload::representative_chunk_batch(REPRESENTATIVE_CHUNK_BATCH_SIZE);
    runtime.block_on(verify_transport_ciphertext(
        &workload,
        SenderPath::Production,
        SendMode::Individual,
    ));
    runtime.block_on(verify_transport_ciphertext(
        &workload,
        SenderPath::Production,
        SendMode::Batch,
    ));
    runtime.block_on(verify_transport_ciphertext(
        &workload,
        SenderPath::LegacyLocked,
        SendMode::Individual,
    ));
    runtime.block_on(transport_round_trip(&workload, SendMode::Individual));

    let mut group = criterion.benchmark_group("encrypted_outbound_transport_e2e");
    group.throughput(Throughput::Bytes(workload.encoded_bytes as u64));
    group.bench_function("representative_play_burst", |bencher| {
        bencher.iter(|| {
            runtime.block_on(transport_round_trip(
                black_box(&workload),
                SendMode::Individual,
            ));
        });
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

    benchmark_explicit_batching(
        criterion,
        &runtime,
        &[
            ("small_packet_group_8", &small_group),
            ("chunk_batch_9", &chunk_batch),
            ("mixed_ready_group_upper_bound", &workload),
        ],
    );

    let control_packet = workload
        .packets
        .first()
        .expect("representative workload should not be empty")
        .clone();
    let fairness_runtimes = [
        ("one_worker", &runtime),
        ("two_workers", &multi_thread_runtime),
    ];
    benchmark_multi_connection_contention(
        criterion,
        &fairness_runtimes,
        &workload,
        &control_packet,
    );

    benchmark_single_packet_latency(criterion, &runtime, &control_packet);
}

criterion_group! {
    name = benches;
    config = Criterion::default()
        .sample_size(20)
        .measurement_time(Duration::from_secs(10));
    targets = outbound_transport
}
criterion_main!(benches);
