use std::{io, sync::Arc, thread};

use crossbeam::channel::{self, Sender};
use thiserror::Error;
use tokio::sync::oneshot;

use crate::world::{World, WorldGameTickTimings};

struct WorldTickRequest {
    tick_count: u64,
    runs_normally: bool,
    response: oneshot::Sender<WorldGameTickTimings>,
}

struct WorldTickWorker {
    world_key: Arc<str>,
    requests: Option<Sender<WorldTickRequest>>,
    thread: Option<thread::JoinHandle<()>>,
}

impl WorldTickWorker {
    fn spawn(index: usize, world: Arc<World>) -> io::Result<Self> {
        let world_key = Arc::<str>::from(world.key.to_string());
        let (request_sender, request_receiver) = channel::bounded::<WorldTickRequest>(1);
        let thread = thread::Builder::new()
            .name(format!("world-tick-{index}"))
            .spawn(move || {
                while let Ok(request) = request_receiver.recv() {
                    if request.runs_normally {
                        world.chunk_map.tick_timed_tickets();
                    }
                    let timings = world.tick_game(request.tick_count, request.runs_normally);
                    let _ = request.response.send(timings);
                }
            })?;

        Ok(Self {
            world_key,
            requests: Some(request_sender),
            thread: Some(thread),
        })
    }

    fn start_tick(
        &self,
        tick_count: u64,
        runs_normally: bool,
    ) -> Result<oneshot::Receiver<WorldGameTickTimings>, WorldTickWorkerError> {
        let (response, receiver) = oneshot::channel();
        let Some(requests) = &self.requests else {
            return Err(WorldTickWorkerError::Unavailable {
                world: Arc::clone(&self.world_key),
            });
        };
        requests
            .send(WorldTickRequest {
                tick_count,
                runs_normally,
                response,
            })
            .map_err(|_| WorldTickWorkerError::Unavailable {
                world: Arc::clone(&self.world_key),
            })?;
        Ok(receiver)
    }
}

impl Drop for WorldTickWorker {
    fn drop(&mut self) {
        drop(self.requests.take());
        let Some(thread) = self.thread.take() else {
            return;
        };
        if thread.join().is_err() {
            log::error!(
                "World tick worker for {} panicked during execution",
                self.world_key
            );
        }
    }
}

#[derive(Debug, Error)]
pub(super) enum WorldTickWorkerError {
    #[error("world tick worker for {world} is unavailable")]
    Unavailable { world: Arc<str> },
    #[error("world tick worker for {world} stopped without returning timings")]
    MissingResponse { world: Arc<str> },
}

pub(super) struct WorldTickWorkers {
    workers: Vec<WorldTickWorker>,
}

impl WorldTickWorkers {
    pub(super) fn spawn<'a>(worlds: impl IntoIterator<Item = &'a Arc<World>>) -> io::Result<Self> {
        let mut workers = Vec::new();
        for (index, world) in worlds.into_iter().enumerate() {
            workers.push(WorldTickWorker::spawn(index, Arc::clone(world))?);
        }
        Ok(Self { workers })
    }

    pub(super) async fn tick_all(
        &self,
        tick_count: u64,
        runs_normally: bool,
    ) -> Result<Vec<WorldGameTickTimings>, WorldTickWorkerError> {
        let mut responses = Vec::with_capacity(self.workers.len());
        for worker in &self.workers {
            responses.push(worker.start_tick(tick_count, runs_normally)?);
        }

        let mut timings = Vec::with_capacity(responses.len());
        for (worker, response) in self.workers.iter().zip(responses) {
            timings.push(
                response
                    .await
                    .map_err(|_| WorldTickWorkerError::MissingResponse {
                        world: Arc::clone(&worker.world_key),
                    })?,
            );
        }
        Ok(timings)
    }
}

#[cfg(test)]
mod tests {
    use futures::executor::block_on;

    use super::WorldTickWorkers;
    use crate::test_support::fresh_test_world;

    #[test]
    fn persistent_workers_tick_every_world_across_boundaries() {
        let first = fresh_test_world("persistent_worker_first");
        let second = fresh_test_world("persistent_worker_second");
        let Ok(workers) = WorldTickWorkers::spawn([&first, &second]) else {
            panic!("world tick workers should start");
        };

        let Ok(first_tick) = block_on(workers.tick_all(1, true)) else {
            panic!("world tick workers should finish the first tick");
        };
        assert_eq!(first_tick.len(), 2);
        assert_eq!(first.game_time(), 1);
        assert_eq!(second.game_time(), 1);

        let Ok(second_tick) = block_on(workers.tick_all(2, true)) else {
            panic!("world tick workers should finish the second tick");
        };
        assert_eq!(second_tick.len(), 2);
        assert_eq!(first.game_time(), 2);
        assert_eq!(second.game_time(), 2);
    }
}
