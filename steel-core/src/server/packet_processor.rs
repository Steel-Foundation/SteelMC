//! Serverbound gameplay packet scheduling between game ticks.

use std::{collections::VecDeque, sync::Arc};

use parking_lot::Condvar;
use steel_utils::locks::SyncMutex;
use tokio::sync::Notify;

use crate::player::{Player, connection::NetworkConnection, networking::ScheduledPlayPacket};

use super::Server;

struct PendingPlayPacket {
    player: Arc<Player>,
    packet: ScheduledPlayPacket,
}

/// Gameplay packets submitted by network tasks.
///
/// The processor runs while the game tick is idle. Closing the packet phase stops new handlers
/// from starting and waits for handlers already in progress before the game tick begins.
pub(super) struct PacketProcessor {
    queued: PacketQueue<PendingPlayPacket>,
}

impl PacketProcessor {
    pub(super) fn new() -> Self {
        Self {
            queued: PacketQueue::new(),
        }
    }

    pub(super) fn schedule(&self, player: Arc<Player>, packet: ScheduledPlayPacket) {
        self.queued.submit(PendingPlayPacket { player, packet });
    }

    /// Runs the blocking packet worker until the processor is stopped.
    pub(super) fn run(&self, server: &Arc<Server>) {
        while let Some(mut work) = self.queued.next() {
            let Some(pending) = work.take() else {
                continue;
            };
            if pending.player.connection.closed()
                || server.cancel_token.is_cancelled()
                || pending.player.is_domain_switching()
            {
                continue;
            }

            pending.packet.handle(pending.player, server);
        }
    }

    /// Opens the inter-tick packet phase and wakes the worker.
    pub(super) fn open_after_tick(&self) {
        self.queued.open();
    }

    /// Guarantees packet progress when a late tick leaves no normal inter-tick window.
    pub(super) async fn wait_for_overload_progress(&self) {
        self.queued.wait_for_progress().await;
    }

    /// Closes packet admission and waits for any handler already in progress.
    pub(super) async fn close_for_tick(&self) {
        self.queued.close();
        self.queued.wait_until_idle().await;
    }

    /// Stops the packet worker and discards queued work during server shutdown.
    pub(super) fn stop(&self) {
        self.queued.stop();
    }
}

impl Default for PacketProcessor {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Clone, Copy, Eq, PartialEq)]
enum PacketPhase {
    Closed,
    Open,
    Stopped,
}

struct PacketQueueState<T> {
    phase: PacketPhase,
    queued: VecDeque<T>,
    active: usize,
    completed: u64,
}

/// Unbounded multi-producer FIFO with an explicit packet/tick phase boundary.
struct PacketQueue<T> {
    state: SyncMutex<PacketQueueState<T>>,
    work_available: Condvar,
    idle: Notify,
    progress: Notify,
}

impl<T> PacketQueue<T> {
    fn new() -> Self {
        Self {
            state: SyncMutex::new(PacketQueueState {
                phase: PacketPhase::Closed,
                queued: VecDeque::new(),
                active: 0,
                completed: 0,
            }),
            work_available: Condvar::new(),
            idle: Notify::new(),
            progress: Notify::new(),
        }
    }

    fn submit(&self, value: T) {
        let mut state = self.state.lock();
        if state.phase == PacketPhase::Stopped {
            return;
        }
        state.queued.push_back(value);
        let should_wake = state.phase == PacketPhase::Open;
        drop(state);
        if should_wake {
            self.work_available.notify_one();
        }
    }

    fn open(&self) {
        let mut state = self.state.lock();
        if state.phase == PacketPhase::Stopped {
            return;
        }
        state.phase = PacketPhase::Open;
        drop(state);
        self.work_available.notify_all();
    }

    fn close(&self) {
        let mut state = self.state.lock();
        if state.phase != PacketPhase::Stopped {
            state.phase = PacketPhase::Closed;
        }
    }

    async fn wait_until_idle(&self) {
        loop {
            let idle = self.idle.notified();
            if self.state.lock().active == 0 {
                return;
            }
            idle.await;
        }
    }

    async fn wait_for_progress(&self) {
        let Some(completed) = self.progress_baseline() else {
            return;
        };

        loop {
            let progress = self.progress.notified();
            if self.has_progress_since(completed) {
                return;
            }
            progress.await;
        }
    }

    fn progress_baseline(&self) -> Option<u64> {
        let state = self.state.lock();
        (!state.queued.is_empty() || state.active != 0).then_some(state.completed)
    }

    fn has_progress_since(&self, completed: u64) -> bool {
        let state = self.state.lock();
        state.completed != completed
            || (state.queued.is_empty() && state.active == 0)
            || state.phase == PacketPhase::Stopped
    }

    fn stop(&self) {
        let mut state = self.state.lock();
        state.phase = PacketPhase::Stopped;
        state.queued.clear();
        let is_idle = state.active == 0;
        drop(state);
        self.work_available.notify_all();
        self.progress.notify_waiters();
        if is_idle {
            self.idle.notify_one();
        }
    }

    fn next(&self) -> Option<PacketWork<'_, T>> {
        let mut state = self.state.lock();
        loop {
            match state.phase {
                PacketPhase::Stopped => return None,
                PacketPhase::Open => {
                    if let Some(value) = state.queued.pop_front() {
                        state.active += 1;
                        drop(state);
                        return Some(PacketWork {
                            value: Some(value),
                            queue: self,
                        });
                    }
                }
                PacketPhase::Closed => {}
            }
            self.work_available.wait(&mut state);
        }
    }

    #[cfg(test)]
    fn try_next(&self) -> Option<PacketWork<'_, T>> {
        let mut state = self.state.lock();
        if state.phase != PacketPhase::Open {
            return None;
        }
        let value = state.queued.pop_front()?;
        state.active += 1;
        drop(state);
        Some(PacketWork {
            value: Some(value),
            queue: self,
        })
    }

    fn finish_one(&self) {
        let mut state = self.state.lock();
        assert!(state.active > 0, "packet work accounting underflow");
        state.active -= 1;
        state.completed = state.completed.wrapping_add(1);
        let is_idle = state.active == 0;
        drop(state);
        self.progress.notify_one();
        if is_idle {
            self.idle.notify_one();
        }
    }
}

struct PacketWork<'a, T> {
    value: Option<T>,
    queue: &'a PacketQueue<T>,
}

impl<T> PacketWork<'_, T> {
    const fn take(&mut self) -> Option<T> {
        self.value.take()
    }
}

impl<T> Drop for PacketWork<'_, T> {
    fn drop(&mut self) {
        self.queue.finish_one();
    }
}

#[cfg(test)]
mod tests {
    use std::{
        sync::{Arc, mpsc},
        thread,
        time::Duration,
    };

    use tokio::time::timeout;

    use super::PacketQueue;

    #[test]
    fn queued_packets_start_in_submission_order_when_opened() {
        let queue = PacketQueue::new();
        queue.submit(1);
        queue.submit(2);
        queue.submit(3);
        assert!(queue.try_next().is_none());

        queue.open();
        let mut processed = Vec::new();
        while let Some(mut work) = queue.try_next() {
            if let Some(value) = work.take() {
                processed.push(value);
            }
        }

        assert_eq!(processed, [1, 2, 3]);
    }

    #[test]
    fn closed_phase_retains_new_packets_for_the_next_open_phase() {
        let queue = PacketQueue::new();
        queue.open();
        queue.close();
        queue.submit(1);

        assert!(queue.try_next().is_none());
        queue.open();
        let Some(mut work) = queue.try_next() else {
            panic!("queued packet should become available when the packet phase opens");
        };
        assert_eq!(work.take(), Some(1));
    }

    #[tokio::test]
    async fn tick_close_waits_for_active_packet_work() {
        let queue = Arc::new(PacketQueue::new());
        queue.open();
        queue.submit(1);
        let Some(work) = queue.try_next() else {
            panic!("open packet phase should start queued work");
        };
        queue.close();

        assert!(
            timeout(Duration::from_millis(10), queue.wait_until_idle())
                .await
                .is_err()
        );
        drop(work);
        assert!(
            timeout(Duration::from_secs(1), queue.wait_until_idle())
                .await
                .is_ok()
        );
    }

    #[tokio::test]
    async fn overload_progress_waits_for_one_active_packet_to_finish() {
        let queue = Arc::new(PacketQueue::new());
        queue.open();
        queue.submit(1);
        let Some(work) = queue.try_next() else {
            panic!("open packet phase should start queued work");
        };

        assert!(
            timeout(Duration::from_millis(10), queue.wait_for_progress())
                .await
                .is_err()
        );
        drop(work);
        assert!(
            timeout(Duration::from_secs(1), queue.wait_for_progress())
                .await
                .is_ok()
        );
    }

    #[test]
    fn stopped_queue_discards_pending_and_future_work() {
        let queue = PacketQueue::new();
        queue.open();
        queue.submit(1);
        queue.stop();
        queue.submit(2);

        assert!(queue.try_next().is_none());
    }

    #[test]
    fn blocking_worker_only_starts_work_during_the_open_phase() {
        let queue = Arc::new(PacketQueue::new());
        let worker_queue = Arc::clone(&queue);
        let (sender, receiver) = mpsc::channel();
        let worker = thread::spawn(move || {
            while let Some(mut work) = worker_queue.next() {
                if let Some(value) = work.take() {
                    let _ = sender.send(value);
                }
            }
        });

        queue.submit(1);
        assert!(receiver.recv_timeout(Duration::from_millis(10)).is_err());
        queue.open();
        assert_eq!(receiver.recv_timeout(Duration::from_secs(1)), Ok(1));
        queue.close();
        queue.submit(2);
        assert!(receiver.recv_timeout(Duration::from_millis(10)).is_err());
        queue.stop();

        assert!(worker.join().is_ok());
    }
}
