//! Compares the encrypted outbound path with its former byte-at-a-time writer.

use std::{
    hint::black_box,
    io,
    pin::Pin,
    sync::Arc,
    task::{Context, Poll},
    time::Duration,
};

use aes::cipher::KeyIvInit;
use criterion::{
    BenchmarkId, Criterion, Throughput, criterion_group, criterion_main, measurement::WallTime,
};
use steel_protocol::{packet_traits::EncodedPacket, packet_writer::TCPNetworkEncoder};
use steel_utils::FrontVec;
use tokio::{
    io::{AsyncWrite, AsyncWriteExt, BufWriter},
    runtime::{Builder, Runtime},
};

const BENCHMARK_ENCRYPTION_KEY: [u8; 16] = *b"SteelMC-test-key";
// Fixed synthetic byte lengths keep the cipher benchmark independent of packet serialization.
// The core benchmark uses real packet encoders, so its final encoded lengths differ.
const SMALL_PACKET_BYTE_SIZES: [usize; 12] = [3, 11, 14, 18, 24, 31, 43, 57, 76, 96, 128, 192];
const SMALL_PACKET_COUNT: usize = 2_048;

const MIXED_SMALL_PACKET_BYTE_SIZES: [usize; 8] = [3, 11, 14, 24, 43, 76, 128, 192];
const MIXED_SMALL_PACKET_COUNT: usize = 768;
const MIXED_MEDIUM_PACKET_BYTE_SIZES: [usize; 6] = [320, 512, 768, 1_024, 1_536, 2_048];
const MIXED_MEDIUM_PACKET_COUNT: usize = 96;
const MIXED_LARGE_PACKET_BYTE_SIZES: [usize; 4] = [16_384, 32_768, 49_152, 65_536];
type RustCryptoCfb8Encryptor = cfb8::Encryptor<aes::Aes128>;

struct TransportWorkload {
    name: &'static str,
    packets: Vec<EncodedPacket>,
    encoded_bytes: usize,
}

struct LegacyBytewiseEncryptor<W> {
    cipher: RustCryptoCfb8Encryptor,
    writer: W,
    pending_byte: Option<u8>,
}

impl<W> LegacyBytewiseEncryptor<W> {
    fn new(writer: W) -> Self {
        Self {
            cipher: RustCryptoCfb8Encryptor::new_from_slices(
                &BENCHMARK_ENCRYPTION_KEY,
                &BENCHMARK_ENCRYPTION_KEY,
            )
            .expect("benchmark key should be valid"),
            writer,
            pending_byte: None,
        }
    }
}

impl<W: AsyncWrite + Unpin> AsyncWrite for LegacyBytewiseEncryptor<W> {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        input: &[u8],
    ) -> Poll<io::Result<usize>> {
        let this = self.get_mut();
        let mut total_written = 0;

        for plaintext in input {
            let ciphertext = if let Some(pending) = this.pending_byte {
                pending
            } else {
                let mut byte = [*plaintext];
                this.cipher.encrypt(&mut byte);
                byte[0]
            };

            match Pin::new(&mut this.writer).poll_write(cx, &[ciphertext]) {
                Poll::Pending => {
                    this.pending_byte = Some(ciphertext);
                    return if total_written == 0 {
                        Poll::Pending
                    } else {
                        Poll::Ready(Ok(total_written))
                    };
                }
                Poll::Ready(Ok(0)) => {
                    this.pending_byte = Some(ciphertext);
                    return Poll::Ready(Err(io::ErrorKind::WriteZero.into()));
                }
                Poll::Ready(Ok(written)) => {
                    this.pending_byte = None;
                    total_written += written;
                }
                Poll::Ready(Err(error)) => return Poll::Ready(Err(error)),
            }
        }

        Poll::Ready(Ok(total_written))
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.get_mut().writer).poll_flush(cx)
    }

    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.get_mut().writer).poll_shutdown(cx)
    }
}

impl TransportWorkload {
    fn from_byte_sizes(name: &'static str, byte_sizes: impl IntoIterator<Item = usize>) -> Self {
        let packets: Vec<_> = byte_sizes
            .into_iter()
            .enumerate()
            .map(|(packet_index, byte_size)| synthetic_packet(packet_index, byte_size))
            .collect();
        let encoded_bytes = packets.iter().map(|packet| packet.encoded_data.len()).sum();

        Self {
            name,
            packets,
            encoded_bytes,
        }
    }
}

fn synthetic_packet(packet_index: usize, byte_size: usize) -> EncodedPacket {
    // The writer only needs stable, non-constant bytes here. These coefficients vary the
    // synthetic payload by packet and position; they are not Minecraft protocol values.
    let mut bytes = FrontVec::capacity(0, byte_size);
    for byte_index in 0..byte_size {
        let value = packet_index
            .wrapping_mul(31)
            .wrapping_add(byte_index.wrapping_mul(17)) as u8;
        bytes.push(value);
    }

    EncodedPacket {
        encoded_data: Arc::new(bytes),
    }
}

fn small_packet_workload() -> TransportWorkload {
    TransportWorkload::from_byte_sizes(
        "small_packets",
        SMALL_PACKET_BYTE_SIZES
            .into_iter()
            .cycle()
            .take(SMALL_PACKET_COUNT),
    )
}

fn mixed_play_workload() -> TransportWorkload {
    // These bands exercise many small writes, fewer medium writes, and several large writes. The
    // count ratios are stress-fixture choices, not estimates of Vanilla packet frequency.
    let small_packets = MIXED_SMALL_PACKET_BYTE_SIZES
        .into_iter()
        .cycle()
        .take(MIXED_SMALL_PACKET_COUNT);
    let medium_packets = MIXED_MEDIUM_PACKET_BYTE_SIZES
        .into_iter()
        .cycle()
        .take(MIXED_MEDIUM_PACKET_COUNT);
    let large_packets = MIXED_LARGE_PACKET_BYTE_SIZES.into_iter();

    TransportWorkload::from_byte_sizes(
        "mixed_play",
        small_packets.chain(medium_packets).chain(large_packets),
    )
}

async fn write_production_workload(workload: &TransportWorkload) -> Vec<u8> {
    let mut output = Vec::with_capacity(workload.encoded_bytes);
    {
        let writer = BufWriter::new(&mut output);
        let mut encoder = TCPNetworkEncoder::new(writer);
        encoder.set_encryption(&BENCHMARK_ENCRYPTION_KEY);

        for packet in &workload.packets {
            encoder
                .write_packet(packet)
                .await
                .expect("in-memory transport write should succeed");
        }
    }
    output
}

async fn write_legacy_workload(workload: &TransportWorkload) -> Vec<u8> {
    let mut output = Vec::with_capacity(workload.encoded_bytes);
    {
        let writer = BufWriter::new(&mut output);
        let mut writer = LegacyBytewiseEncryptor::new(writer);

        for packet in &workload.packets {
            writer
                .write_all(&packet.encoded_data)
                .await
                .expect("in-memory transport write should succeed");
            writer
                .flush()
                .await
                .expect("in-memory transport flush should succeed");
        }
    }
    output
}

fn bench_workload(
    group: &mut criterion::BenchmarkGroup<'_, WallTime>,
    runtime: &Runtime,
    workload: &TransportWorkload,
) {
    group.throughput(Throughput::Bytes(workload.encoded_bytes as u64));
    group.bench_with_input(
        BenchmarkId::new("legacy_bytewise", workload.name),
        workload,
        |bencher, workload| {
            bencher.iter(|| {
                black_box(runtime.block_on(write_legacy_workload(black_box(workload))));
            });
        },
    );
    group.bench_with_input(
        BenchmarkId::new("production", workload.name),
        workload,
        |bencher, workload| {
            bencher.iter(|| {
                black_box(runtime.block_on(write_production_workload(black_box(workload))));
            });
        },
    );
}

fn outbound_transport(criterion: &mut Criterion) {
    let runtime = Builder::new_current_thread()
        .build()
        .expect("benchmark runtime should build");
    let workloads = [small_packet_workload(), mixed_play_workload()];
    let mut group = criterion.benchmark_group("encrypted_outbound_transport");

    for workload in &workloads {
        let legacy_output = runtime.block_on(write_legacy_workload(workload));
        let production_output = runtime.block_on(write_production_workload(workload));
        assert_eq!(production_output, legacy_output);
        bench_workload(&mut group, &runtime, workload);
    }

    group.finish();
}

criterion_group! {
    name = benches;
    config = Criterion::default()
        .sample_size(20)
        .measurement_time(Duration::from_secs(10));
    targets = outbound_transport
}
criterion_main!(benches);
