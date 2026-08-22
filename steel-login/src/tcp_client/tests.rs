use std::{
    future::{pending, poll_fn, ready},
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    task::Poll,
};

use crossbeam::atomic::AtomicCell;
use tokio_util::sync::CancellationToken;

use super::{LoginDeadline, LoginOperationResult, await_login_operation};

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
