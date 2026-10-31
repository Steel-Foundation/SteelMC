use std::{
    borrow::Borrow,
    io, mem,
    sync::{Arc, Weak},
    thread,
};

use crossbeam::channel::{self, Sender};
use thiserror::Error;
use tokio::sync::oneshot;
use tokio_util::task::TaskTracker;

use crate::world::{World, WorldGameTickTimings};

struct WorldTickRequest {
    tick_count: u64,
    runs_normally: bool,
    response: oneshot::Sender<WorldGameTickTimings>,
}

struct WorldTickWorker {
    world: Weak<World>,
    world_key: Arc<str>,
    requests: Option<Sender<WorldTickRequest>>,
    thread: Option<thread::JoinHandle<()>>,
}

impl WorldTickWorker {
    fn spawn(index: usize, world: Arc<World>) -> io::Result<Self> {
        let world_key = Arc::<str>::from(world.key.to_string());
        let world_ref = Arc::downgrade(&world);
        let (request_sender, request_receiver) = channel::bounded::<WorldTickRequest>(1);
        let thread = thread::Builder::new()
            .name(format!("world-tick-{index}"))
            .spawn(move || {
                while let Ok(request) = request_receiver.recv() {
                    let timings = world.tick_game(request.tick_count, request.runs_normally);
                    let _ = request.response.send(timings);
                }
            })?;

        Ok(Self {
            world: world_ref,
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
    pub(super) fn spawn<W: Borrow<Arc<World>>>(
        worlds: impl IntoIterator<Item = W>,
    ) -> io::Result<Self> {
        let mut workers = Vec::new();
        for (index, world) in worlds.into_iter().enumerate() {
            workers.push(WorldTickWorker::spawn(index, Arc::clone(world.borrow()))?);
        }
        Ok(Self { workers })
    }

    /// Runs between ticks. Idle worker joins and world destruction stay off the tick thread.
    pub(super) fn synchronize<W: Borrow<Arc<World>>>(
        &mut self,
        worlds: impl IntoIterator<Item = W>,
        retirements: &TaskTracker,
    ) -> io::Result<()> {
        let worlds: Vec<_> = worlds.into_iter().map(|w| Arc::clone(w.borrow())).collect();
        let active = |worker: &WorldTickWorker| {
            worlds
                .iter()
                .any(|world| worker.world.ptr_eq(&Arc::downgrade(world)))
        };
        if self.workers.len() == worlds.len() && self.workers.iter().all(active) {
            return Ok(());
        }
        for mut worker in mem::take(&mut self.workers) {
            if active(&worker) {
                self.workers.push(worker);
                continue;
            }
            drop(worker.requests.take());
            retirements.spawn_blocking(move || drop(worker));
        }
        for world in worlds {
            if self
                .workers
                .iter()
                .any(|worker| worker.world.ptr_eq(&Arc::downgrade(&world)))
            {
                continue;
            }
            self.workers
                .push(WorldTickWorker::spawn(self.workers.len(), world)?);
        }
        Ok(())
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
    use tokio::sync::oneshot::error::TryRecvError;

    use super::WorldTickWorkers;
    use crate::test_support::test_domain;

    #[test]
    fn persistent_workers_tick_every_world_across_boundaries() {
        let worlds = test_domain("workers", &["primary", "derived"]);
        let first = worlds.default_world("workers").expect("primary");
        let second = worlds
            .get(&steel_utils::Identifier::new_static("workers", "derived"))
            .expect("derived");
        let Ok(workers) = WorldTickWorkers::spawn([&first, &second]) else {
            panic!("world tick workers should start");
        };

        worlds.advance_domain_game_times();
        let Ok(first_tick) = block_on(workers.tick_all(1, true)) else {
            panic!("world tick workers should finish the first tick");
        };
        assert_eq!(first_tick.len(), 2);
        assert_eq!(first.game_time(), 1);
        assert_eq!(second.game_time(), 1);

        worlds.advance_domain_game_times();
        let Ok(second_tick) = block_on(workers.tick_all(2, true)) else {
            panic!("world tick workers should finish the second tick");
        };
        assert_eq!(second_tick.len(), 2);
        assert_eq!(first.game_time(), 2);
        assert_eq!(second.game_time(), 2);
    }

    #[test]
    fn shared_time_is_published_while_primary_worker_is_delayed() {
        let worlds = test_domain("delayed", &["primary", "derived"]);
        let primary = worlds.default_world("delayed").expect("primary");
        let derived = worlds
            .get(&steel_utils::Identifier::new_static("delayed", "derived"))
            .expect("derived");
        let workers = WorldTickWorkers::spawn([&primary, &derived]).expect("workers");
        for tick in 1..=2 {
            worlds.advance_domain_game_times();
            // Holding primary level data delays its world-local time phase, while
            // the shared counter remains readable without this lock.
            let guard = primary.level_data.write();
            let mut primary_response = workers.workers[0]
                .start_tick(tick, true)
                .expect("dispatch primary");
            let derived_response = workers.workers[1]
                .start_tick(tick, true)
                .expect("dispatch derived");
            block_on(derived_response).expect("derived completes while primary is delayed");
            assert!(matches!(
                primary_response.try_recv(),
                Err(TryRecvError::Empty)
            ));
            for world in worlds.values() {
                assert_eq!(world.game_time(), tick as i64);
            }
            drop(guard);
            block_on(primary_response).expect("primary completes after release");
            for world in worlds.values() {
                assert_eq!(world.game_time(), tick as i64);
            }
        }
    }
}
