//! Tick-polled server jobs.

use std::{
    mem,
    sync::{Arc, Weak},
};

use crate::{
    chunk::chunk_request::{ChunkRequestHandle, ChunkRequestState, ReadyChunks},
    server::Server,
};
use steel_utils::locks::SyncMutex;

/// Result of polling a server job.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JobPoll {
    /// Poll the job again on a later server job tick.
    Pending,
    /// The job is complete and should be removed.
    Finished,
}

/// Context passed to jobs when they are polled.
pub struct ServerJobContext {
    server: Option<Weak<Server>>,
    /// Current server tick count.
    pub tick_count: u64,
    /// Whether normal world ticking is currently running.
    pub runs_normally: bool,
}

impl ServerJobContext {
    const fn for_server(server: Weak<Server>, tick_count: u64, runs_normally: bool) -> Self {
        Self {
            server: Some(server),
            tick_count,
            runs_normally,
        }
    }

    /// Returns the server if it is still alive.
    #[must_use]
    pub fn server(&self) -> Option<Arc<Server>> {
        self.server.as_ref().and_then(Weak::upgrade)
    }

    #[cfg(test)]
    const fn for_test(tick_count: u64, runs_normally: bool) -> Self {
        Self {
            server: None,
            tick_count,
            runs_normally,
        }
    }
}

/// A unit of server work resumed from a known tick stage.
pub trait ServerJob: Send {
    /// Polls this job.
    fn poll(&mut self, context: &mut ServerJobContext) -> JobPoll;

    /// Cancels the job before it finishes.
    fn cancel(&mut self) {}
}

/// Tick-owned job queue.
#[derive(Default)]
pub struct ServerJobQueue {
    jobs: SyncMutex<Vec<Box<dyn ServerJob>>>,
}

impl ServerJobQueue {
    /// Creates an empty job queue.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            jobs: SyncMutex::new(Vec::new()),
        }
    }

    /// Adds a job to be polled on the next server job tick.
    pub fn spawn(&self, job: impl ServerJob + 'static) {
        self.jobs.lock().push(Box::new(job));
    }

    /// Returns the number of queued jobs.
    #[must_use]
    pub fn len(&self) -> usize {
        self.jobs.lock().len()
    }

    /// Returns true if no jobs are queued.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.jobs.lock().is_empty()
    }

    /// Cancels and removes all queued jobs.
    pub fn cancel_all(&self) {
        let jobs = mem::take(&mut *self.jobs.lock());
        for mut job in jobs {
            job.cancel();
        }
    }

    /// Polls queued jobs from the server game tick.
    pub fn tick(
        &self,
        server: Weak<Server>,
        tick_count: u64,
        runs_normally: bool,
    ) -> ServerJobTickStats {
        let mut context = ServerJobContext::for_server(server, tick_count, runs_normally);
        self.tick_with_context(&mut context)
    }

    fn tick_with_context(&self, context: &mut ServerJobContext) -> ServerJobTickStats {
        let jobs = mem::take(&mut *self.jobs.lock());
        let polled = jobs.len();
        let mut pending = Vec::with_capacity(polled);
        let mut finished = 0;

        for mut job in jobs {
            match job.poll(context) {
                JobPoll::Pending => pending.push(job),
                JobPoll::Finished => finished += 1,
            }
        }

        let pending_count = {
            let mut queued = self.jobs.lock();
            let spawned_during_poll = queued.len();
            pending.reserve(spawned_during_poll);
            pending.append(&mut *queued);
            let pending_count = pending.len();
            *queued = pending;
            pending_count
        };

        ServerJobTickStats {
            polled,
            finished,
            pending: pending_count,
        }
    }
}

/// Job polling counts for diagnostics.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct ServerJobTickStats {
    /// Jobs polled this tick.
    pub polled: usize,
    /// Jobs completed this tick.
    pub finished: usize,
    /// Jobs queued after this tick.
    pub pending: usize,
}

/// A chunk request that invokes a callback once all requested chunks are ready.
pub struct ChunkRequestJob<F> {
    request: ChunkRequestHandle,
    on_ready: Option<F>,
}

impl<F> ChunkRequestJob<F> {
    /// Creates a job around a chunk request and a tick-stage callback.
    pub const fn new(request: ChunkRequestHandle, on_ready: F) -> Self {
        Self {
            request,
            on_ready: Some(on_ready),
        }
    }
}

impl<F> ServerJob for ChunkRequestJob<F>
where
    F: FnOnce(&mut ServerJobContext, ReadyChunks) + Send + 'static,
{
    fn poll(&mut self, context: &mut ServerJobContext) -> JobPoll {
        match self.request.poll() {
            ChunkRequestState::Pending { .. } => JobPoll::Pending,
            ChunkRequestState::Cancelled => JobPoll::Finished,
            ChunkRequestState::Ready => {
                let Some(ready) = self.request.ready_chunks() else {
                    return JobPoll::Pending;
                };
                if let Some(on_ready) = self.on_ready.take() {
                    on_ready(context, ready);
                }
                JobPoll::Finished
            }
        }
    }

    fn cancel(&mut self) {
        self.request.cancel();
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };

    use super::*;

    struct CountJob {
        polls: Arc<AtomicUsize>,
        finish_after: usize,
    }

    impl ServerJob for CountJob {
        fn poll(&mut self, _context: &mut ServerJobContext) -> JobPoll {
            let polls = self.polls.fetch_add(1, Ordering::Relaxed) + 1;
            if polls >= self.finish_after {
                JobPoll::Finished
            } else {
                JobPoll::Pending
            }
        }
    }

    #[test]
    fn pending_job_is_polled_until_finished() {
        let queue = ServerJobQueue::new();
        let polls = Arc::new(AtomicUsize::new(0));
        queue.spawn(CountJob {
            polls: polls.clone(),
            finish_after: 2,
        });

        let mut context = ServerJobContext::for_test(1, true);
        let first = queue.tick_with_context(&mut context);
        assert_eq!(
            first,
            ServerJobTickStats {
                polled: 1,
                finished: 0,
                pending: 1,
            }
        );

        let second = queue.tick_with_context(&mut context);
        assert_eq!(
            second,
            ServerJobTickStats {
                polled: 1,
                finished: 1,
                pending: 0,
            }
        );
        assert_eq!(polls.load(Ordering::Relaxed), 2);
    }

    #[test]
    fn jobs_spawned_during_poll_wait_until_next_tick() {
        struct SpawnJob {
            queue: Arc<ServerJobQueue>,
            polls: Arc<AtomicUsize>,
        }

        impl ServerJob for SpawnJob {
            fn poll(&mut self, _context: &mut ServerJobContext) -> JobPoll {
                self.queue.spawn(CountJob {
                    polls: self.polls.clone(),
                    finish_after: 1,
                });
                JobPoll::Finished
            }
        }

        let queue = Arc::new(ServerJobQueue::new());
        let polls = Arc::new(AtomicUsize::new(0));
        queue.spawn(SpawnJob {
            queue: queue.clone(),
            polls: polls.clone(),
        });

        let mut context = ServerJobContext::for_test(1, true);
        let first = queue.tick_with_context(&mut context);
        assert_eq!(first.polled, 1);
        assert_eq!(first.finished, 1);
        assert_eq!(first.pending, 1);
        assert_eq!(polls.load(Ordering::Relaxed), 0);

        let second = queue.tick_with_context(&mut context);
        assert_eq!(second.polled, 1);
        assert_eq!(second.finished, 1);
        assert_eq!(second.pending, 0);
        assert_eq!(polls.load(Ordering::Relaxed), 1);
    }
}
