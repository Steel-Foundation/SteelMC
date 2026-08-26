//! Measures encrypted outbound transport over real loopback TCP connections.

use std::{
    env,
    hint::black_box,
    net::{Ipv4Addr, SocketAddr},
    sync::{Arc, Weak},
    time::{Duration, Instant},
};

use aes::cipher::KeyIvInit;
use criterion::{
    BatchSize, BenchmarkGroup, BenchmarkId, Criterion, Throughput, criterion_group, criterion_main,
    measurement::WallTime,
};
use steel_core::player::{
    Player,
    connection::{JavaConnection, NetworkConnection},
};
use steel_protocol::{
    packet_traits::{ClientPacket, CompressionInfo, EncodedPacket},
    packet_writer::TCPNetworkEncoder,
    packets::{
        common::CKeepAlive,
        game::{
            CChunkBatchFinished, CChunkBatchStart, CLevelChunkWithLight, ChunkPacketData,
            Heightmaps, LightUpdatePacketData,
        },
    },
    utils::ConnectionProtocol,
};
use steel_utils::codec::BitSet;
use tokio::{
    io::{AsyncReadExt, BufReader, BufWriter},
    net::{TcpSocket, tcp::OwnedReadHalf},
    runtime::{Builder as RuntimeBuilder, Runtime},
    sync::mpsc,
    task::JoinHandle,
};
use tokio_util::sync::CancellationToken;

const ENCRYPTION_KEY: [u8; 16] = *b"SteelMC-test-key";
const READ_BUFFER_BYTES: usize = 16 * 1_024;
// Backpressure makes progress on the hot connection a meaningful contention barrier.
const CONTENDED_RECEIVE_BUFFER_BYTES: u32 = 16 * 1_024;
const CONTENTION_SAMPLES: usize = 256;

// These fixed stress bursts use real packet encoders with synthetic, reproducible values. Chunk
// sizes are payload bytes before framing and compression, not estimates of Vanilla frequency.
const SMALL_PACKET_COUNT: usize = 768;
const MEDIUM_CHUNK_COUNT: usize = 96;
const MEDIUM_CHUNK_SIZES: [usize; 6] = [320, 512, 768, 1_024, 1_536, 2_048];
const LARGE_CHUNK_SIZES: [usize; 4] = [16_384, 32_768, 49_152, 65_536];
const LARGE_CHUNK_COUNT: usize = LARGE_CHUNK_SIZES.len();
const BATCH_CHUNK_SIZES: [usize; 4] = [8_192, 16_384, 24_576, 32_768];
const SMALL_BATCH_PACKET_COUNT: usize = 8;
const REPRESENTATIVE_CHUNK_BATCH_SIZE: usize = 9;

type ReferenceCfb8Encryptor = cfb8::Encryptor<aes::Aes128>;

struct Workload {
    packets: Vec<EncodedPacket>,
    encoded_bytes: usize,
}

#[derive(Clone, Copy)]
enum SendMode {
    Individual,
    Batch,
}

struct Session {
    connection: Arc<JavaConnection>,
    reader: BufReader<OwnedReadHalf>,
    sender: JoinHandle<()>,
    receive_buffer: Box<[u8]>,
}

struct ContentionLane {
    hot: Session,
    control: Session,
    mode: SendMode,
    samples: Vec<Duration>,
}

impl Workload {
    fn mixed_play_burst() -> Self {
        let compression = CompressionInfo::default();
        let mut packets = Vec::new();

        for sequence in 0..SMALL_PACKET_COUNT {
            push_packet(&mut packets, CKeepAlive::new(sequence as i64), compression);
        }
        push_chunks(&mut packets, MEDIUM_CHUNK_SIZES, MEDIUM_CHUNK_COUNT, 1);
        push_chunks(&mut packets, LARGE_CHUNK_SIZES, LARGE_CHUNK_COUNT, 101);
        Self::new(packets)
    }

    fn chunk_batch(chunk_count: usize) -> Self {
        let compression = CompressionInfo::default();
        let mut packets = Vec::with_capacity(chunk_count.saturating_add(2));
        push_packet(&mut packets, CChunkBatchStart {}, compression);
        push_chunks(&mut packets, BATCH_CHUNK_SIZES, chunk_count, 501);
        push_packet(
            &mut packets,
            CChunkBatchFinished {
                batch_size: chunk_count as i32,
            },
            compression,
        );
        Self::new(packets)
    }

    fn prefix(&self, packet_count: usize) -> Self {
        Self::new(self.packets[..packet_count].to_vec())
    }

    fn new(packets: Vec<EncodedPacket>) -> Self {
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
            .expect("benchmark play packet should encode"),
    );
}

fn push_chunks<const N: usize>(
    packets: &mut Vec<EncodedPacket>,
    sizes: [usize; N],
    count: usize,
    seed: u32,
) {
    for (index, size) in sizes.into_iter().cycle().take(count).enumerate() {
        push_packet(
            packets,
            chunk_packet(index as i32, size, index as u32 + seed),
            CompressionInfo::default(),
        );
    }
}

fn chunk_packet(index: i32, data_bytes: usize, seed: u32) -> CLevelChunkWithLight {
    let empty_mask = || BitSet(Vec::new().into_boxed_slice());
    CLevelChunkWithLight {
        x: index,
        z: -index,
        chunk_data: ChunkPacketData {
            heightmaps: Heightmaps {
                heightmaps: Vec::new(),
            },
            data: deterministic_bytes(data_bytes, seed),
            block_entities: Vec::new(),
        },
        light_data: LightUpdatePacketData {
            sky_y_mask: empty_mask(),
            block_y_mask: empty_mask(),
            empty_sky_y_mask: empty_mask(),
            empty_block_y_mask: empty_mask(),
            sky_updates: Vec::new(),
            block_updates: Vec::new(),
        },
    }
}

fn deterministic_bytes(len: usize, seed: u32) -> Vec<u8> {
    // Xorshift32 provides non-constant fixture bytes without consuming gameplay RNG.
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

impl Session {
    async fn connect(socket_buffer_size: Option<u32>) -> Self {
        let listener = TcpSocket::new_v4().expect("loopback listener socket should open");
        listener
            .bind(SocketAddr::from((Ipv4Addr::LOCALHOST, 0)))
            .expect("loopback listener should bind");
        let listener = listener.listen(1).expect("loopback listener should listen");
        let address = listener
            .local_addr()
            .expect("loopback listener should have an address");
        let socket = TcpSocket::new_v4().expect("loopback client socket should open");
        if let Some(size) = socket_buffer_size {
            socket
                .set_recv_buffer_size(size)
                .expect("loopback receive buffer should be configured");
        }
        let (client, server) = tokio::join!(socket.connect(address), listener.accept());
        let client = client.expect("loopback client should connect");
        let (server, _) = server.expect("loopback server should accept");
        if let Some(size) = socket_buffer_size {
            socket2::SockRef::from(&server)
                .set_send_buffer_size(size as usize)
                .expect("loopback send buffer should be configured");
        }
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
        let (outgoing_packets, receiver) = mpsc::unbounded_channel();
        let cancel_token = CancellationToken::new();
        let connection = Arc::new(JavaConnection::new(
            outgoing_packets,
            cancel_token.clone(),
            Some(CompressionInfo::default()),
            1,
            Weak::<Player>::new(),
        ));
        let sender_connection = Arc::clone(&connection);
        let sender = tokio::spawn(async move {
            sender_connection.sender(receiver, encoder).await;
        });

        Self {
            connection,
            reader: BufReader::new(client_read),
            sender,
            receive_buffer: vec![0; READ_BUFFER_BYTES].into_boxed_slice(),
        }
    }

    fn enqueue(&self, packets: Vec<EncodedPacket>, mode: SendMode) {
        match mode {
            SendMode::Individual => {
                for packet in packets {
                    self.connection.send_encoded_packet(packet);
                }
            }
            SendMode::Batch => self.connection.send_encoded_batch(packets),
        }
    }

    async fn send(&mut self, packets: Vec<EncodedPacket>, encoded_bytes: usize, mode: SendMode) {
        self.enqueue(packets, mode);
        self.receive(encoded_bytes).await;
    }

    async fn receive(&mut self, mut remaining: usize) {
        while remaining != 0 {
            let to_read = remaining.min(self.receive_buffer.len());
            self.reader
                .read_exact(&mut self.receive_buffer[..to_read])
                .await
                .expect("loopback client should receive encrypted bytes");
            black_box(&self.receive_buffer[..to_read]);
            remaining -= to_read;
        }
    }

    async fn close(self) -> BufReader<OwnedReadHalf> {
        self.connection.close();
        self.sender.await.expect("sender task should finish");
        self.reader
    }
}

async fn verify_ciphertext(workload: &Workload, mode: SendMode) {
    let mut session = Session::connect(None).await;
    session.enqueue(workload.packets.clone(), mode);
    let mut actual = vec![0; workload.encoded_bytes];
    session
        .reader
        .read_exact(&mut actual)
        .await
        .expect("loopback client should receive encrypted bytes");
    let mut expected: Vec<u8> = workload
        .packets
        .iter()
        .flat_map(|packet| packet.encoded_data.iter().copied())
        .collect();
    ReferenceCfb8Encryptor::new(&ENCRYPTION_KEY.into(), &ENCRYPTION_KEY.into())
        .encrypt(&mut expected);
    assert_eq!(actual, expected, "transport ciphertext diverged");

    let mut reader = session.close().await;
    let mut trailing = Vec::new();
    reader
        .read_to_end(&mut trailing)
        .await
        .expect("closed loopback transport should reach EOF");
    assert!(trailing.is_empty(), "transport wrote bytes after close");
}

fn build_runtime(worker_threads: Option<usize>) -> Runtime {
    let mut builder = match worker_threads {
        Some(worker_threads) => {
            let mut builder = RuntimeBuilder::new_multi_thread();
            builder.worker_threads(worker_threads);
            builder
        }
        None => RuntimeBuilder::new_current_thread(),
    };
    builder
        .enable_io()
        .enable_time()
        .build()
        .expect("benchmark runtime should build")
}

fn benchmark_persistent_session(
    group: &mut BenchmarkGroup<'_, WallTime>,
    runtime: &Runtime,
    id: BenchmarkId,
    workload: &Workload,
    mode: SendMode,
) {
    let mut session = runtime.block_on(Session::connect(None));
    group.bench_function(id, |bencher| {
        bencher.iter_batched(
            || workload.packets.clone(),
            |packets| {
                runtime.block_on(session.send(
                    black_box(packets),
                    black_box(workload.encoded_bytes),
                    mode,
                ));
            },
            BatchSize::SmallInput,
        );
    });
    runtime.block_on(session.close());
}

fn benchmark_batching(
    criterion: &mut Criterion,
    runtime: &Runtime,
    workloads: &[(&str, &Workload)],
) {
    let mut group = criterion.benchmark_group("encrypted_outbound_transport_batching_ab");
    for &(name, workload) in workloads {
        group.throughput(Throughput::Bytes(workload.encoded_bytes as u64));
        let mut modes = [
            ("individual_flushes", SendMode::Individual),
            ("explicit_batch", SendMode::Batch),
        ];
        maybe_reverse(&mut modes);
        for (mode_name, mode) in modes {
            benchmark_persistent_session(
                &mut group,
                runtime,
                BenchmarkId::new(name, mode_name),
                workload,
                mode,
            );
        }
    }
    group.finish();
}

fn maybe_reverse<T>(cases: &mut [T]) {
    // Run both orders when collecting publishable numbers to expose thermal or scheduler drift.
    if env::var_os("STEEL_BENCH_REVERSE_ORDER").is_some() {
        cases.reverse();
    }
}

impl ContentionLane {
    async fn connect(mode: SendMode) -> Self {
        Self {
            hot: Session::connect(Some(CONTENDED_RECEIVE_BUFFER_BYTES)).await,
            control: Session::connect(None).await,
            mode,
            samples: Vec::with_capacity(CONTENTION_SAMPLES),
        }
    }

    async fn sample(&mut self, workload: &Workload, control_packet: &EncodedPacket) {
        self.hot.enqueue(workload.packets.clone(), self.mode);
        let first_packet_bytes = workload.packets[0].encoded_data.len();
        self.hot.receive(first_packet_bytes).await;

        let control = vec![control_packet.clone()];
        let control_bytes = control_packet.encoded_data.len();
        let started = Instant::now();
        self.control.enqueue(control, SendMode::Individual);
        let receive_control = async {
            self.control.receive(control_bytes).await;
            started.elapsed()
        };
        let ((), latency) = tokio::join!(
            self.hot
                .receive(workload.encoded_bytes - first_packet_bytes),
            receive_control,
        );
        self.samples.push(latency);
    }
}

async fn sample_contention(workload: &Workload, control_packet: &EncodedPacket) {
    let mut lanes = [
        ContentionLane::connect(SendMode::Individual).await,
        ContentionLane::connect(SendMode::Batch).await,
    ];

    for sample in 0..CONTENTION_SAMPLES {
        let order = if sample % 2 == 0 { [0, 1] } else { [1, 0] };
        for lane in order {
            lanes[lane].sample(workload, control_packet).await;
        }
    }

    for (name, mut lane) in ["individual_flushes", "explicit_batch"]
        .into_iter()
        .zip(lanes)
    {
        lane.hot.close().await;
        lane.control.close().await;
        lane.samples.sort_unstable();
        let percentile = |percent: usize| {
            let index = lane.samples.len().saturating_mul(percent).div_ceil(100) - 1;
            lane.samples[index]
        };
        eprintln!(
            "paired two-worker control latency {name}: p50={:?} p95={:?}",
            percentile(50),
            percentile(95),
        );
    }
}

fn outbound_transport(criterion: &mut Criterion) {
    let runtime = build_runtime(None);
    let two_worker_runtime = build_runtime(Some(2));
    let mixed = Workload::mixed_play_burst();
    let small = mixed.prefix(SMALL_BATCH_PACKET_COUNT);
    let chunks = Workload::chunk_batch(REPRESENTATIVE_CHUNK_BATCH_SIZE);

    for mode in [SendMode::Individual, SendMode::Batch] {
        runtime.block_on(verify_ciphertext(&mixed, mode));
    }

    benchmark_batching(
        criterion,
        &runtime,
        &[
            ("small_packet_group_8", &small),
            ("chunk_batch_9", &chunks),
            ("synthetic_mixed_burst", &mixed),
        ],
    );
    two_worker_runtime.block_on(sample_contention(&mixed, &mixed.packets[0]));
}

criterion_group! {
    name = benches;
    config = Criterion::default()
        .sample_size(20)
        .measurement_time(Duration::from_secs(10));
    targets = outbound_transport
}
criterion_main!(benches);
