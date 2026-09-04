//! Compares the encrypted outbound path with its former byte-at-a-time writer.

use std::{
    env,
    hint::black_box,
    io,
    pin::Pin,
    sync::Arc,
    task::{Context, Poll},
    time::Duration,
};

use aes::cipher::KeyIvInit;
use criterion::{
    BatchSize, BenchmarkId, Criterion, Throughput, criterion_group, criterion_main,
    measurement::WallTime,
};
use steel_protocol::{packet_traits::EncodedPacket, packet_writer::TCPNetworkEncoder};
use steel_utils::FrontVec;
use tokio::{
    io::{AsyncWrite, AsyncWriteExt, BufWriter, sink},
    runtime::{Builder, Runtime},
};

const ENCRYPTION_KEY: [u8; 16] = *b"SteelMC-test-key";
const SMALL_PACKET_SIZES: &[usize] = &[3, 11, 14, 18, 24, 31, 43, 57, 76, 96, 128, 192];
const MIXED_SMALL_PACKET_SIZES: &[usize] = &[3, 11, 14, 24, 43, 76, 128, 192];
const MEDIUM_PACKET_SIZES: &[usize] = &[320, 512, 768, 1_024, 1_536, 2_048];
const LARGE_PACKET_SIZES: &[usize] = &[16_384, 32_768, 49_152, 65_536];

type RustCryptoCfb8Encryptor = cfb8::Encryptor<aes::Aes128>;

struct Workload {
    name: &'static str,
    packets: Vec<EncodedPacket>,
    encoded_bytes: usize,
}

impl Workload {
    fn new(name: &'static str, sizes: impl IntoIterator<Item = usize>) -> Self {
        let packets: Vec<_> = sizes
            .into_iter()
            .enumerate()
            .map(|(index, size)| synthetic_packet(index, size))
            .collect();
        let encoded_bytes = packets.iter().map(|packet| packet.encoded_data.len()).sum();
        Self {
            name,
            packets,
            encoded_bytes,
        }
    }
}

fn synthetic_packet(packet_index: usize, size: usize) -> EncodedPacket {
    let mut bytes = FrontVec::capacity(0, size);
    for byte_index in 0..size {
        bytes.push(packet_index.wrapping_add(byte_index) as u8);
    }
    EncodedPacket {
        encoded_data: Arc::new(bytes),
    }
}

struct LegacyBytewiseWriter<W> {
    cipher: RustCryptoCfb8Encryptor,
    writer: W,
    pending_byte: Option<u8>,
}

impl<W> LegacyBytewiseWriter<W> {
    fn new(writer: W) -> Self {
        Self {
            cipher: RustCryptoCfb8Encryptor::new_from_slices(&ENCRYPTION_KEY, &ENCRYPTION_KEY)
                .expect("valid key"),
            writer,
            pending_byte: None,
        }
    }
}

impl<W: AsyncWrite + Unpin> AsyncWrite for LegacyBytewiseWriter<W> {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        input: &[u8],
    ) -> Poll<io::Result<usize>> {
        let this = self.get_mut();
        let mut written = 0;

        for plaintext in input {
            let ciphertext = this.pending_byte.unwrap_or_else(|| {
                let mut byte = [*plaintext];
                this.cipher.encrypt(&mut byte);
                byte[0]
            });
            match Pin::new(&mut this.writer).poll_write(cx, &[ciphertext]) {
                Poll::Pending => {
                    this.pending_byte = Some(ciphertext);
                    return if written == 0 {
                        Poll::Pending
                    } else {
                        Poll::Ready(Ok(written))
                    };
                }
                Poll::Ready(Ok(0)) => {
                    this.pending_byte = Some(ciphertext);
                    return Poll::Ready(Err(io::ErrorKind::WriteZero.into()));
                }
                Poll::Ready(Ok(count)) => {
                    this.pending_byte = None;
                    written += count;
                }
                Poll::Ready(Err(error)) => return Poll::Ready(Err(error)),
            }
        }
        Poll::Ready(Ok(written))
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.get_mut().writer).poll_flush(cx)
    }

    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.get_mut().writer).poll_shutdown(cx)
    }
}

fn repeated(sizes: &[usize], count: usize) -> impl Iterator<Item = usize> + '_ {
    sizes.iter().copied().cycle().take(count)
}

fn workloads() -> [Workload; 2] {
    [
        Workload::new("small_packets", repeated(SMALL_PACKET_SIZES, 2_048)),
        Workload::new(
            "mixed_play",
            repeated(MIXED_SMALL_PACKET_SIZES, 768)
                .chain(repeated(MEDIUM_PACKET_SIZES, 96))
                .chain(LARGE_PACKET_SIZES.iter().copied()),
        ),
    ]
}

fn buffered_writer<W: AsyncWrite + Unpin>(writer: W) -> TCPNetworkEncoder<W> {
    let mut encoder = TCPNetworkEncoder::new(writer);
    encoder.set_encryption(&ENCRYPTION_KEY);
    encoder
}

async fn write_buffered<W: AsyncWrite + Unpin>(
    writer: &mut TCPNetworkEncoder<W>,
    workload: &Workload,
) {
    for packet in &workload.packets {
        writer.write_packet(packet).await.expect("write failed");
    }
}

async fn write_legacy<W: AsyncWrite + Unpin>(
    writer: &mut LegacyBytewiseWriter<W>,
    workload: &Workload,
) {
    for packet in &workload.packets {
        writer
            .write_all(&packet.encoded_data)
            .await
            .expect("write failed");
        writer.flush().await.expect("flush failed");
    }
}

async fn parity_outputs(workload: &Workload) -> (Vec<u8>, Vec<u8>) {
    let mut buffered_output = Vec::with_capacity(workload.encoded_bytes);
    let mut legacy_output = Vec::with_capacity(workload.encoded_bytes);
    {
        let mut writer = buffered_writer(BufWriter::new(&mut buffered_output));
        write_buffered(&mut writer, workload).await;
    }
    {
        let mut writer = LegacyBytewiseWriter::new(BufWriter::new(&mut legacy_output));
        write_legacy(&mut writer, workload).await;
    }
    (buffered_output, legacy_output)
}

fn bench_legacy(
    group: &mut criterion::BenchmarkGroup<'_, WallTime>,
    runtime: &Runtime,
    workload: &Workload,
) {
    group.bench_function(
        BenchmarkId::new("legacy_bytewise", workload.name),
        |bencher| {
            bencher.iter_batched(
                || LegacyBytewiseWriter::new(BufWriter::new(sink())),
                |mut writer| runtime.block_on(write_legacy(&mut writer, black_box(workload))),
                BatchSize::SmallInput,
            );
        },
    );
}

fn bench_buffered(
    group: &mut criterion::BenchmarkGroup<'_, WallTime>,
    runtime: &Runtime,
    workload: &Workload,
) {
    group.bench_function(BenchmarkId::new("buffered", workload.name), |bencher| {
        bencher.iter_batched(
            || buffered_writer(BufWriter::new(sink())),
            |mut writer| runtime.block_on(write_buffered(&mut writer, black_box(workload))),
            BatchSize::SmallInput,
        );
    });
}

fn bench_workload(
    group: &mut criterion::BenchmarkGroup<'_, WallTime>,
    runtime: &Runtime,
    workload: &Workload,
) {
    group.throughput(Throughput::Bytes(workload.encoded_bytes as u64));
    // Run both orders when collecting publishable numbers to expose thermal or scheduler drift.
    if env::var_os("STEEL_BENCH_REVERSE_ORDER").is_some() {
        bench_buffered(group, runtime, workload);
        bench_legacy(group, runtime, workload);
    } else {
        bench_legacy(group, runtime, workload);
        bench_buffered(group, runtime, workload);
    }
}

fn outbound_transport(criterion: &mut Criterion) {
    let runtime = Builder::new_current_thread()
        .build()
        .expect("runtime should build");
    let mut group = criterion.benchmark_group("encrypted_outbound_transport");

    for workload in workloads() {
        let (buffered, legacy) = runtime.block_on(parity_outputs(&workload));
        assert_eq!(buffered, legacy, "{} ciphertext differs", workload.name);
        bench_workload(&mut group, &runtime, &workload);
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
