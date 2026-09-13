use std::{
    future::{pending, poll_fn, ready},
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    task::Poll,
    time::{Duration, Instant},
};

use crossbeam::atomic::AtomicCell;
use steel_protocol::utils::PacketError;
use tokio_util::sync::CancellationToken;

use super::{
    JavaTcpClient, KeepAliveDecision, LoginDeadline, LoginOperationResult, PrePlayKeepAliveTracker,
    await_login_operation,
};

struct DropSignal(Arc<AtomicBool>);

impl Drop for DropSignal {
    fn drop(&mut self) {
        self.0.store(true, Ordering::Release);
    }
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

#[test]
fn keepalive_decisions_match_vanilla_boundaries() {
    let now = Instant::now();
    let mut tracker = PrePlayKeepAliveTracker {
        clock_start: now,
        last_sent: now,
        pending: None,
        latency: 0,
    };

    // Fresh phase: nothing is due before a full interval.
    assert!(matches!(
        tracker.tick(now + Duration::from_secs(14)),
        KeepAliveDecision::None
    ));

    // One interval after entering the phase: a challenge goes out and re-anchors the
    // send cadence, like vanilla `keepConnectionAlive`.
    let KeepAliveDecision::Send(challenge) = tracker.tick(now + Duration::from_secs(15)) else {
        panic!("a challenge is due after one interval");
    };
    assert_eq!(tracker.pending, Some(challenge));
    assert_eq!(tracker.last_sent, now + Duration::from_secs(15));

    // A challenge left unanswered for a full interval is a timeout kick.
    assert!(matches!(
        tracker.tick(tracker.last_sent + Duration::from_secs(15)),
        KeepAliveDecision::Timeout
    ));
}

#[test]
fn config_keepalive_rejects_trailing_payload() {
    let payload = 1234_i64.to_be_bytes();
    assert!(matches!(
        JavaTcpClient::read_keep_alive_payload(&payload),
        Ok(packet) if packet.id == 1234
    ));

    let mut trailing = payload.to_vec();
    trailing.push(0);
    assert!(matches!(
        JavaTcpClient::read_keep_alive_payload(&trailing),
        Err(PacketError::MalformedValue(_))
    ));
}

#[test]
fn keepalive_answer_smooths_latency_and_rejects_out_of_order() {
    let now = Instant::now();
    let mut tracker = PrePlayKeepAliveTracker {
        clock_start: now,
        last_sent: now,
        pending: Some(1234),
        latency: 100,
    };

    // A wrong id is out of order and changes nothing (vanilla kicks with a timeout).
    assert!(!tracker.answer(999, now + Duration::from_millis(40)));
    assert_eq!(tracker.pending, Some(1234));
    assert_eq!(tracker.latency, 100);

    // The matching id is accepted and smooths the latency like vanilla `handleKeepAlive`:
    // (100 * 3 + 40) / 4 = 85.
    assert!(tracker.answer(1234, now + Duration::from_millis(40)));
    assert_eq!(tracker.pending, None);
    assert_eq!(tracker.latency, 85);

    // A duplicate answer without a pending challenge is out of order too.
    assert!(!tracker.answer(1234, now + Duration::from_millis(50)));
}
