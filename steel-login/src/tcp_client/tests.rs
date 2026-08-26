use std::{
    future::{Future, pending, poll_fn, ready},
    io,
    pin::Pin,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    task::{Context, Poll, Waker},
};

use crossbeam::atomic::AtomicCell;
use steel_protocol::packet_traits::EncodedPacket;
use steel_protocol::packet_writer::TCPNetworkEncoder;
use steel_protocol::utils::PacketError;
use steel_utils::{FrontVec, locks::AsyncMutex, locks::SyncMutex};
use tokio::{io::AsyncWrite, sync::Notify, sync::mpsc};
use tokio_util::sync::CancellationToken;

use super::{
    JavaTcpClient, LoginDeadline, LoginOperationResult, OutboundEncryptionTransition,
    OutboundPacket, PrePlayClosingWrites, PrePlayWrite, await_login_operation,
};

struct DropSignal(Arc<AtomicBool>);

impl Drop for DropSignal {
    fn drop(&mut self) {
        self.0.store(true, Ordering::Release);
    }
}

#[tokio::test]
async fn requested_encryption_waits_for_the_writer_transition() {
    let transition = OutboundEncryptionTransition::default();
    transition.begin();
    let enabled = transition.wait_until_enabled();
    tokio::pin!(enabled);
    let mut context = Context::from_waker(Waker::noop());

    assert!(matches!(enabled.as_mut().poll(&mut context), Poll::Pending));
    transition.finish();
    assert!(matches!(
        enabled.as_mut().poll(&mut context),
        Poll::Ready(())
    ));
}

fn encoded_marker(marker: u8) -> EncodedPacket {
    encoded_bytes(&[marker])
}

fn encoded_bytes(bytes: &[u8]) -> EncodedPacket {
    let mut data = FrontVec::new(0);
    data.extend_from_slice(bytes);
    EncodedPacket {
        encoded_data: Arc::new(data),
    }
}

#[derive(Default)]
struct PausingWriterState {
    bytes: Vec<u8>,
    released: bool,
    waker: Option<Waker>,
}

struct PausingWriter {
    state: Arc<SyncMutex<PausingWriterState>>,
    paused: Arc<Notify>,
    pause_after: usize,
}

impl AsyncWrite for PausingWriter {
    fn poll_write(
        self: Pin<&mut Self>,
        context: &mut Context<'_>,
        bytes: &[u8],
    ) -> Poll<io::Result<usize>> {
        let mut state = self.state.lock();
        if !state.released && state.bytes.len() >= self.pause_after {
            state.waker = Some(context.waker().clone());
            drop(state);
            self.paused.notify_one();
            return Poll::Pending;
        }

        let write_len = if state.released {
            bytes.len()
        } else {
            bytes.len().min(self.pause_after - state.bytes.len())
        };
        state.bytes.extend_from_slice(&bytes[..write_len]);
        Poll::Ready(Ok(write_len))
    }

    fn poll_flush(self: Pin<&mut Self>, _context: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(Ok(()))
    }

    fn poll_shutdown(self: Pin<&mut Self>, _context: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(Ok(()))
    }
}

#[tokio::test]
async fn canceled_pre_play_packet_drops_its_partial_encoder() {
    const KEY: [u8; 16] = *b"close-cipher-key";
    const ACTIVE_PACKET: &[u8] = b"AB";

    let state = Arc::new(SyncMutex::new(PausingWriterState::default()));
    let paused = Arc::new(Notify::new());
    let writer = PausingWriter {
        state: Arc::clone(&state),
        paused: Arc::clone(&paused),
        pause_after: 1,
    };
    let mut encoder = TCPNetworkEncoder::new(writer);
    encoder.set_encryption(&KEY);
    let writer_slot = Arc::new(AsyncMutex::new(Some(encoder)));
    let packet = encoded_bytes(ACTIVE_PACKET);

    {
        let write = JavaTcpClient::write_network_packet(&writer_slot, &packet);
        tokio::pin!(write);
        tokio::select! {
            () = paused.notified() => {}
            result = write.as_mut() => panic!("packet unexpectedly completed: {result:?}"),
        }
    }

    assert!(writer_slot.lock().await.is_none());
    assert_eq!(state.lock().bytes.len(), 1);
}

#[tokio::test]
async fn canceled_pre_play_batch_drops_its_partial_encoder() {
    const KEY: [u8; 16] = *b"close-cipher-key";
    const ACTIVE_PACKET: &[u8] = b"AB";
    const QUEUED_PACKET: u8 = b'C';
    const LATER_PACKET: u8 = b'D';

    let state = Arc::new(SyncMutex::new(PausingWriterState::default()));
    let paused = Arc::new(Notify::new());
    let writer = PausingWriter {
        state: Arc::clone(&state),
        paused: Arc::clone(&paused),
        pause_after: 1,
    };
    let mut encoder = TCPNetworkEncoder::new(writer);
    encoder.set_encryption(&KEY);
    let writer_slot = Arc::new(AsyncMutex::new(Some(encoder)));
    let packets = [encoded_bytes(ACTIVE_PACKET), encoded_marker(QUEUED_PACKET)];

    {
        let write = JavaTcpClient::write_network_batch(&writer_slot, &packets);
        tokio::pin!(write);
        tokio::select! {
            () = paused.notified() => {}
            result = write.as_mut() => panic!("batch unexpectedly completed: {result:?}"),
        }
    }

    assert!(writer_slot.lock().await.is_none());
    assert_eq!(state.lock().bytes.len(), 1);
    let later_result =
        JavaTcpClient::write_network_packet(&writer_slot, &encoded_marker(LATER_PACKET)).await;
    assert!(matches!(later_result, Err(PacketError::ConnectionClosed)));
}

#[tokio::test]
async fn canceled_pre_play_close_drops_its_partial_encoder() {
    const KEY: [u8; 16] = *b"close-cipher-key";
    const ACTIVE_PACKET: &[u8] = b"AB";
    const DISCONNECT_PACKET: u8 = b'C';

    let state = Arc::new(SyncMutex::new(PausingWriterState::default()));
    let paused = Arc::new(Notify::new());
    let writer = PausingWriter {
        state: Arc::clone(&state),
        paused: Arc::clone(&paused),
        pause_after: 1,
    };
    let mut encoder = TCPNetworkEncoder::new(writer);
    encoder.set_encryption(&KEY);
    let writer_slot = Arc::new(AsyncMutex::new(Some(encoder)));
    {
        let close = JavaTcpClient::write_closing(
            &writer_slot,
            PrePlayClosingWrites {
                queued: vec![PrePlayWrite::Packet(encoded_bytes(ACTIVE_PACKET))],
                disconnect: encoded_marker(DISCONNECT_PACKET),
            },
        );
        tokio::pin!(close);

        tokio::select! {
            () = paused.notified() => {}
            result = close.as_mut() => panic!("close unexpectedly completed: {result:?}"),
        }
    }

    assert!(writer_slot.lock().await.is_none());
    assert_eq!(state.lock().bytes.len(), 1);
}

#[tokio::test]
async fn pre_play_close_preserves_batch_order_through_disconnect() {
    const FIRST_PACKET: u8 = b'A';
    const FIRST_BATCH_PACKET: u8 = b'B';
    const SECOND_BATCH_PACKET: u8 = b'C';
    const DISCONNECT_PACKET: u8 = b'D';
    const POST_DISCONNECT_PACKET: u8 = b'E';

    let (sender, mut receiver) = mpsc::unbounded_channel();
    assert!(
        sender
            .send(OutboundPacket::Packet(encoded_marker(FIRST_PACKET)))
            .is_ok()
    );
    assert!(
        sender
            .send(OutboundPacket::PacketBatch(Box::new(vec![
                encoded_marker(FIRST_BATCH_PACKET),
                encoded_marker(SECOND_BATCH_PACKET),
            ])))
            .is_ok()
    );
    assert!(
        sender
            .send(OutboundPacket::Disconnect(encoded_marker(
                DISCONNECT_PACKET,
            )))
            .is_ok()
    );
    assert!(
        sender
            .send(OutboundPacket::Packet(encoded_marker(
                POST_DISCONNECT_PACKET,
            )))
            .is_ok()
    );

    let Some(closing) = JavaTcpClient::take_closing_writes(&mut receiver).await else {
        panic!("disconnect packet should be present");
    };
    let mut markers = Vec::new();
    for queued_write in closing.queued {
        match queued_write {
            PrePlayWrite::Packet(packet) => markers.push(packet.encoded_data[0]),
            PrePlayWrite::Batch(packets) => {
                markers.extend(packets.iter().map(|packet| packet.encoded_data[0]));
            }
        }
    }
    markers.push(closing.disconnect.encoded_data[0]);

    assert_eq!(
        markers,
        [
            FIRST_PACKET,
            FIRST_BATCH_PACKET,
            SECOND_BATCH_PACKET,
            DISCONNECT_PACKET,
        ]
    );
    assert!(matches!(
        receiver.try_recv(),
        Ok(OutboundPacket::Packet(packet))
            if packet.encoded_data[0] == POST_DISCONNECT_PACKET
    ));
}

#[test]
fn login_deadline_matches_vanillas_post_increment_boundary() {
    let deadline = LoginDeadline::from_start_tick(42);

    assert_eq!(deadline.expires_at_tick(), 643);
}

#[tokio::test]
async fn login_deadline_drops_in_flight_packet_processing() {
    let dropped = Arc::new(AtomicBool::new(false));
    let operation_dropped = Arc::clone(&dropped);
    let operation = async move {
        let _drop_signal = DropSignal(operation_dropped);
        pending::<()>().await;
    };
    let login_deadline = AtomicCell::new(Some(LoginDeadline::from_start_tick(0)));
    let cancel_token = CancellationToken::new();

    let result = await_login_operation(&cancel_token, &login_deadline, operation, ready(())).await;

    assert!(matches!(result, LoginOperationResult::TimedOut));
    assert!(dropped.load(Ordering::Acquire));
}

#[tokio::test]
async fn cancellation_wins_over_ready_packet_processing() {
    let login_deadline = AtomicCell::new(Some(LoginDeadline::from_start_tick(0)));
    let cancel_token = CancellationToken::new();
    cancel_token.cancel();

    let result = await_login_operation(&cancel_token, &login_deadline, ready(()), pending()).await;

    assert!(matches!(result, LoginOperationResult::Cancelled));
}

#[tokio::test]
async fn configuration_handoff_disables_ready_login_deadline() {
    let login_deadline = AtomicCell::new(Some(LoginDeadline::from_start_tick(0)));
    let polls = AtomicUsize::new(0);
    let operation = poll_fn(|context| {
        if polls.fetch_add(1, Ordering::Relaxed) == 0 {
            login_deadline.store(None);
            context.waker().wake_by_ref();
            Poll::Pending
        } else {
            Poll::Ready(())
        }
    });
    let cancel_token = CancellationToken::new();

    let result = await_login_operation(&cancel_token, &login_deadline, operation, ready(())).await;

    assert!(matches!(result, LoginOperationResult::Completed(())));
}
