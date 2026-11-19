//! FIFO ownership of a random stream without retaining a mutex guard.

use std::{
    collections::VecDeque,
    future::Future,
    io,
    pin::Pin,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    task::{Context, Poll, Waker},
};

use steel_utils::{
    locks::SyncMutex,
    random::{Random, RandomSource, RandomSplitter},
};

use super::{OperationError, OperationResult};

pub(crate) struct RandomStream {
    ownership: SyncMutex<StreamOwnership>,
    // Synchronous callbacks may enqueue later work while borrowing the RNG.
    random: SyncMutex<RandomSource>,
    revision: Arc<AtomicU64>,
}

struct StreamOwnership {
    next_ticket: u64,
    active: Option<u64>,
    waiting: VecDeque<Waiting>,
}

struct Waiting {
    ticket: u64,
    waker: Option<Waker>,
}

impl RandomStream {
    pub(crate) const fn new(random: RandomSource, revision: Arc<AtomicU64>) -> Self {
        Self {
            ownership: SyncMutex::new(StreamOwnership {
                next_ticket: 0,
                active: None,
                waiting: VecDeque::new(),
            }),
            random: SyncMutex::new(random),
            revision,
        }
    }

    pub(super) fn request(self: &Arc<Self>) -> RandomRequest {
        let mut state = self.ownership.lock();
        let ticket = state.next_ticket;
        state.next_ticket = ticket
            .checked_add(1)
            .unwrap_or_else(|| panic!("loot random ticket space exhausted"));
        state.waiting.push_back(Waiting {
            ticket,
            waker: None,
        });
        RandomRequest {
            stream: Arc::clone(self),
            ticket: Some(ticket),
        }
    }

    pub(super) fn try_with_random<T>(
        self: &Arc<Self>,
        evaluate: impl FnOnce(&mut RandomSource) -> OperationResult<T>,
    ) -> OperationResult<T> {
        let mut request = self.request();
        let Poll::Ready(mut lease) =
            Pin::new(&mut request).poll(&mut Context::from_waker(Waker::noop()))
        else {
            return Err(OperationError::RandomUnavailable(
                "random stream is reserved",
            ));
        };
        lease.with_random(evaluate)
    }

    pub(super) fn persistent_state(&self) -> io::Result<[i64; 2]> {
        let random = self.random.lock();
        let RandomSource::Xoroshiro(random) = &*random else {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "named sequence is not a Xoroshiro source",
            ));
        };
        Ok(random.state().map(u64::cast_signed))
    }
}

/// Cancelling a queued request removes its position without drawing any randomness.
pub(super) struct RandomRequest {
    stream: Arc<RandomStream>,
    ticket: Option<u64>,
}

impl Future for RandomRequest {
    type Output = RandomLease;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let Some(ticket) = self.ticket else {
            panic!("random request polled after completion");
        };
        let ready = {
            let mut state = self.stream.ownership.lock();
            if state.active.is_none()
                && state
                    .waiting
                    .front()
                    .is_some_and(|entry| entry.ticket == ticket)
            {
                state.waiting.pop_front();
                state.active = Some(ticket);
                true
            } else {
                if let Some(waiting) = state
                    .waiting
                    .iter_mut()
                    .find(|waiting| waiting.ticket == ticket)
                    && waiting
                        .waker
                        .as_ref()
                        .is_none_or(|waker| !waker.will_wake(cx.waker()))
                {
                    waiting.waker = Some(cx.waker().clone());
                }
                false
            }
        };
        if !ready {
            return Poll::Pending;
        }
        self.ticket = None;
        Poll::Ready(RandomLease {
            stream: Arc::clone(&self.stream),
            ticket,
        })
    }
}

impl Drop for RandomRequest {
    fn drop(&mut self) {
        let Some(ticket) = self.ticket else {
            return;
        };
        let wake = {
            let mut state = self.stream.ownership.lock();
            state.waiting.retain(|entry| entry.ticket != ticket);
            if state.active.is_none() {
                state
                    .waiting
                    .front_mut()
                    .and_then(|waiting| waiting.waker.take())
            } else {
                None
            }
        };
        if let Some(waker) = wake {
            waker.wake();
        }
    }
}

/// Exclusive logical ownership; each draw holds the stream mutex only for that draw.
pub(super) struct RandomLease {
    stream: Arc<RandomStream>,
    ticket: u64,
}

impl RandomLease {
    fn with_random<T>(&mut self, evaluate: impl FnOnce(&mut RandomSource) -> T) -> T {
        let mut random = self.stream.random.lock();
        let result = evaluate(&mut random);
        self.stream.revision.fetch_add(1, Ordering::Release);
        result
    }
}

impl Drop for RandomLease {
    fn drop(&mut self) {
        let wake = {
            let mut state = self.stream.ownership.lock();
            assert_eq!(
                state.active,
                Some(self.ticket),
                "loot random lease released without ownership"
            );
            state.active = None;
            state
                .waiting
                .front_mut()
                .and_then(|waiting| waiting.waker.take())
        };
        if let Some(waker) = wake {
            waker.wake();
        }
    }
}

macro_rules! forward_draws {
    ($($name:ident($($argument:ident: $ty:ty),*) -> $result:ty;)+) => {$(
        fn $name(&mut self, $($argument: $ty),*) -> $result {
            self.with_random(|random| random.$name($($argument),*))
        }
    )+};
}

impl Random for RandomLease {
    fn fork(&mut self) -> Self {
        let random = self.with_random(Random::fork);
        let stream = Arc::new(RandomStream::new(random, Arc::new(AtomicU64::new(0))));
        {
            let mut ownership = stream.ownership.lock();
            ownership.active = Some(0);
            ownership.next_ticket = 1;
        }
        Self { stream, ticket: 0 }
    }

    forward_draws! {
        next_i32() -> i32;
        next_i32_bounded(bound: i32) -> i32;
        next_i64() -> i64;
        next_f32() -> f32;
        next_f64() -> f64;
        next_bool() -> bool;
        next_gaussian() -> f64;
        next_positional() -> RandomSplitter;
    }
}

#[cfg(test)]
#[path = "random_stream_tests.rs"]
mod tests;
