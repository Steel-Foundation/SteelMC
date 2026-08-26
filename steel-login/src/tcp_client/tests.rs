use std::{
    future::{Future, pending, poll_fn, ready},
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
use steel_utils::{FrontVec, locks::AsyncMutex};
use tokio::{
    io::{AsyncReadExt, DuplexStream, duplex},
    sync::mpsc,
};
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

fn assert_pending_once<F: Future + ?Sized>(future: Pin<&mut F>) {
    let mut context = Context::from_waker(Waker::noop());
    assert!(future.poll(&mut context).is_pending());
}

const TEST_ENCRYPTION_KEY: [u8; 16] = *b"close-cipher-key";
const ACTIVE_PACKET: &[u8] = b"AB";
const ACCEPTED_CIPHERTEXT_BYTES: usize = 1;

fn bounded_encrypted_writer() -> (
    Arc<AsyncMutex<Option<TCPNetworkEncoder<DuplexStream>>>>,
    DuplexStream,
) {
    let (writer, peer) = duplex(ACCEPTED_CIPHERTEXT_BYTES);
    let mut encoder = TCPNetworkEncoder::new(writer);
    encoder.set_encryption(&TEST_ENCRYPTION_KEY);
    (Arc::new(AsyncMutex::new(Some(encoder))), peer)
}

async fn assert_partial_ciphertext(mut peer: DuplexStream) {
    let mut ciphertext = Vec::new();
    peer.read_to_end(&mut ciphertext)
        .await
        .expect("canceled writer should close its peer");
    assert_eq!(ciphertext.len(), ACCEPTED_CIPHERTEXT_BYTES);
}

#[tokio::test]
async fn canceled_pre_play_packet_drops_its_partial_encoder() {
    let (writer_slot, peer) = bounded_encrypted_writer();
    let packet = encoded_bytes(ACTIVE_PACKET);

    {
        let write = JavaTcpClient::write_network_packet(&writer_slot, &packet);
        tokio::pin!(write);
        assert_pending_once(write.as_mut());
    }

    assert!(writer_slot.lock().await.is_none());
    assert_partial_ciphertext(peer).await;
}

#[tokio::test]
async fn canceled_pre_play_batch_drops_its_partial_encoder() {
    const QUEUED_PACKET: u8 = b'C';
    const LATER_PACKET: u8 = b'D';

    let (writer_slot, peer) = bounded_encrypted_writer();
    let packets = [encoded_bytes(ACTIVE_PACKET), encoded_marker(QUEUED_PACKET)];

    {
        let write = JavaTcpClient::write_network_batch(&writer_slot, &packets);
        tokio::pin!(write);
        assert_pending_once(write.as_mut());
    }

    assert!(writer_slot.lock().await.is_none());
    assert_partial_ciphertext(peer).await;
    let later_result =
        JavaTcpClient::write_network_packet(&writer_slot, &encoded_marker(LATER_PACKET)).await;
    assert!(matches!(later_result, Err(PacketError::ConnectionClosed)));
}

#[tokio::test]
async fn canceled_pre_play_close_drops_its_partial_encoder() {
    const DISCONNECT_PACKET: u8 = b'C';

    let (writer_slot, peer) = bounded_encrypted_writer();
    {
        let close = JavaTcpClient::write_closing(
            &writer_slot,
            PrePlayClosingWrites {
                queued: vec![PrePlayWrite::Packet(encoded_bytes(ACTIVE_PACKET))],
                disconnect: encoded_marker(DISCONNECT_PACKET),
            },
        );
        tokio::pin!(close);
        assert_pending_once(close.as_mut());
    }

    assert!(writer_slot.lock().await.is_none());
    assert_partial_ciphertext(peer).await;
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
