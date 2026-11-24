use super::*;
use std::task::Wake;
use steel_utils::random::{legacy_random::LegacyRandom, xoroshiro::Xoroshiro};

fn poll(request: &mut RandomRequest) -> Poll<RandomLease> {
    Pin::new(request).poll(&mut Context::from_waker(Waker::noop()))
}

fn acquire(request: &mut RandomRequest) -> RandomLease {
    let Poll::Ready(lease) = poll(request) else {
        panic!("request should own the stream");
    };
    lease
}

fn stream() -> Arc<RandomStream> {
    Arc::new(RandomStream::new(
        RandomSource::Legacy(LegacyRandom::from_seed(42)),
        Arc::new(AtomicU64::new(0)),
    ))
}

#[test]
fn pending_owners_release_the_mutex_and_preserve_fifo_randomness() {
    let stream = stream();
    let mut expected = LegacyRandom::from_seed(42);
    let mut first = stream.request();
    let mut second = stream.request();
    let mut third = stream.request();
    assert!(poll(&mut third).is_pending());
    assert!(poll(&mut second).is_pending());
    let mut first = acquire(&mut first);
    assert!(stream.ownership.try_lock().is_some());
    assert!(stream.random.try_lock().is_some());
    assert_eq!(first.next_i64(), expected.next_i64());
    assert!(poll(&mut second).is_pending());
    assert!(matches!(
        stream.try_with_random(|random| Ok(random.next_i64())),
        Err(OperationError::RandomUnavailable(_))
    ));
    drop(first);
    assert!(poll(&mut third).is_pending());
    let mut second = acquire(&mut second);
    assert_eq!(second.next_i64(), expected.next_i64());
    drop(second);
    let mut third = acquire(&mut third);
    assert_eq!(third.next_i64(), expected.next_i64());
}

#[test]
fn cancellation_advances_the_queue_and_wakes_the_next_request() {
    struct CountWake(AtomicU64);
    impl Wake for CountWake {
        fn wake(self: Arc<Self>) {
            self.0.fetch_add(1, Ordering::Relaxed);
        }
    }
    let stream = stream();
    let mut first = stream.request();
    let mut second = stream.request();
    let mut third = stream.request();
    let first = acquire(&mut first);
    assert!(poll(&mut second).is_pending());
    let notifications = Arc::new(CountWake(AtomicU64::new(0)));
    let waker = Waker::from(Arc::clone(&notifications));
    assert!(
        Pin::new(&mut third)
            .poll(&mut Context::from_waker(&waker))
            .is_pending()
    );
    drop(second);
    assert_eq!(notifications.0.load(Ordering::Relaxed), 0);
    drop(first);
    assert_eq!(notifications.0.load(Ordering::Relaxed), 1);
    let mut third = acquire(&mut third);
    assert_eq!(third.next_i64(), LegacyRandom::from_seed(42).next_i64());
}

#[test]
fn saved_state_tracks_draws_while_the_logical_owner_is_pending() {
    let stream = Arc::new(RandomStream::new(
        RandomSource::Xoroshiro(Xoroshiro::from_seed(42)),
        Arc::new(AtomicU64::new(0)),
    ));
    let mut expected = Xoroshiro::from_seed(42);
    let mut lease = acquire(&mut stream.request());
    assert_eq!(lease.next_i64(), expected.next_i64());
    assert_eq!(
        stream.persistent_state().expect("snapshot"),
        expected.state().map(u64::cast_signed)
    );
    assert_eq!(stream.revision.load(Ordering::Acquire), 1);
    assert_eq!(lease.next_i64(), expected.next_i64());
    assert_eq!(
        stream.persistent_state().expect("snapshot"),
        expected.state().map(u64::cast_signed)
    );
    assert_eq!(stream.revision.load(Ordering::Acquire), 2);
}

#[test]
fn synchronous_callbacks_can_enqueue_without_reentering_the_random_mutex() {
    let stream = stream();
    let mut expected = LegacyRandom::from_seed(42);
    let mut queued = stream
        .try_with_random(|random| {
            let mut queued = stream.request();
            assert!(poll(&mut queued).is_pending());
            assert_eq!(random.next_i64(), expected.next_i64());
            assert!(matches!(
                stream.try_with_random(|_| Ok(())),
                Err(OperationError::RandomUnavailable(_))
            ));
            Ok(queued)
        })
        .expect("synchronous callback");
    let mut next = acquire(&mut queued);
    assert_eq!(next.next_i64(), expected.next_i64());
}
