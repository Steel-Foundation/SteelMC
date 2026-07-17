//! Serverbound gameplay packet scheduling between game ticks.

use std::{
    cmp::Reverse,
    collections::{BinaryHeap, VecDeque},
    hash::Hash,
    sync::Arc,
};

use parking_lot::Condvar;
use rustc_hash::FxHashMap;
use steel_utils::locks::SyncMutex;
use tokio::sync::Notify;
use uuid::Uuid;

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
    queued: PacketQueue<Uuid, PendingPlayPacket>,
}

impl PacketProcessor {
    pub(super) fn new() -> Self {
        Self {
            queued: PacketQueue::new(),
        }
    }

    pub(super) fn schedule(&self, player: Arc<Player>, packet: ScheduledPlayPacket) {
        self.queued
            .submit(player.gameprofile.id, PendingPlayPacket { player, packet });
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

struct SequencedPacket<T> {
    sequence: u64,
    value: T,
}

struct PacketLane<T> {
    queued: VecDeque<SequencedPacket<T>>,
    active: bool,
}

impl<T> PacketLane<T> {
    const fn new() -> Self {
        Self {
            queued: VecDeque::new(),
            active: false,
        }
    }
}

struct PacketQueueState<K, T> {
    phase: PacketPhase,
    lanes: FxHashMap<K, PacketLane<T>>,
    ready: BinaryHeap<Reverse<(u64, K)>>,
    next_sequence: u64,
    active: usize,
    completed: u64,
}

/// Unbounded multi-producer lanes with an explicit packet/tick phase boundary.
struct PacketQueue<K, T> {
    state: SyncMutex<PacketQueueState<K, T>>,
    work_available: Condvar,
    idle: Notify,
    progress: Notify,
}

impl<K, T> PacketQueue<K, T>
where
    K: Copy + Eq + Hash + Ord,
{
    fn new() -> Self {
        Self {
            state: SyncMutex::new(PacketQueueState {
                phase: PacketPhase::Closed,
                lanes: FxHashMap::default(),
                ready: BinaryHeap::new(),
                next_sequence: 0,
                active: 0,
                completed: 0,
            }),
            work_available: Condvar::new(),
            idle: Notify::new(),
            progress: Notify::new(),
        }
    }

    fn submit(&self, key: K, value: T) {
        let mut state = self.state.lock();
        if state.phase == PacketPhase::Stopped {
            return;
        }
        let sequence = state.next_sequence;
        assert!(sequence != u64::MAX, "packet submission sequence exhausted");
        state.next_sequence = sequence + 1;

        let lane = state.lanes.entry(key).or_insert_with(PacketLane::new);
        let became_ready = !lane.active && lane.queued.is_empty();
        lane.queued.push_back(SequencedPacket { sequence, value });
        if became_ready {
            state.ready.push(Reverse((sequence, key)));
        }
        let should_wake = became_ready && state.phase == PacketPhase::Open;
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
        Self::has_work(&state).then_some(state.completed)
    }

    fn has_progress_since(&self, completed: u64) -> bool {
        let state = self.state.lock();
        state.completed != completed
            || !Self::has_work(&state)
            || state.phase == PacketPhase::Stopped
    }

    fn has_work(state: &PacketQueueState<K, T>) -> bool {
        state.active != 0 || state.lanes.values().any(|lane| !lane.queued.is_empty())
    }

    fn stop(&self) {
        let mut state = self.state.lock();
        state.phase = PacketPhase::Stopped;
        state.ready.clear();
        state.lanes.retain(|_, lane| {
            lane.queued.clear();
            lane.active
        });
        let is_idle = state.active == 0;
        drop(state);
        self.work_available.notify_all();
        self.progress.notify_waiters();
        if is_idle {
            self.idle.notify_one();
        }
    }

    fn next(&self) -> Option<PacketWork<'_, K, T>> {
        let mut state = self.state.lock();
        loop {
            match state.phase {
                PacketPhase::Stopped => return None,
                PacketPhase::Open => {
                    if let Some((key, value)) = Self::start_next(&mut state) {
                        state.active += 1;
                        drop(state);
                        return Some(PacketWork {
                            value: Some(value),
                            key,
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
    fn try_next(&self) -> Option<PacketWork<'_, K, T>> {
        let mut state = self.state.lock();
        if state.phase != PacketPhase::Open {
            return None;
        }
        let (key, value) = Self::start_next(&mut state)?;
        state.active += 1;
        drop(state);
        Some(PacketWork {
            value: Some(value),
            key,
            queue: self,
        })
    }

    fn start_next(state: &mut PacketQueueState<K, T>) -> Option<(K, T)> {
        let Reverse((ready_sequence, key)) = state.ready.pop()?;
        let Some(lane) = state.lanes.get_mut(&key) else {
            panic!("ready packet lane disappeared before starting");
        };
        assert!(!lane.active, "ready packet lane is already active");
        let Some(packet) = lane.queued.pop_front() else {
            panic!("ready packet lane has no queued packet");
        };
        assert_eq!(
            ready_sequence, packet.sequence,
            "ready packet sequence does not match lane front"
        );
        lane.active = true;
        Some((key, packet.value))
    }

    fn finish_one(&self, key: K) {
        let mut state = self.state.lock();
        assert!(state.active > 0, "packet work accounting underflow");
        state.active -= 1;
        state.completed = state.completed.wrapping_add(1);

        let next_sequence = {
            let Some(lane) = state.lanes.get_mut(&key) else {
                panic!("active packet lane disappeared before completion");
            };
            assert!(lane.active, "completed packet lane is not active");
            lane.active = false;
            lane.queued.front().map(|packet| packet.sequence)
        };
        if let Some(sequence) = next_sequence {
            state.ready.push(Reverse((sequence, key)));
        } else {
            state.lanes.remove(&key);
        }

        let is_idle = state.active == 0;
        let should_wake = state.phase == PacketPhase::Open && !state.ready.is_empty();
        drop(state);
        self.progress.notify_one();
        if should_wake {
            self.work_available.notify_one();
        }
        if is_idle {
            self.idle.notify_one();
        }
    }
}

struct PacketWork<'a, K, T>
where
    K: Copy + Eq + Hash + Ord,
{
    value: Option<T>,
    key: K,
    queue: &'a PacketQueue<K, T>,
}

impl<K, T> PacketWork<'_, K, T>
where
    K: Copy + Eq + Hash + Ord,
{
    const fn take(&mut self) -> Option<T> {
        self.value.take()
    }
}

impl<K, T> Drop for PacketWork<'_, K, T>
where
    K: Copy + Eq + Hash + Ord,
{
    fn drop(&mut self) {
        self.queue.finish_one(self.key);
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
        queue.submit(1, 1);
        queue.submit(2, 2);
        queue.submit(1, 3);
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
    fn packet_lane_only_allows_one_active_handler() {
        let queue = PacketQueue::new();
        queue.submit(1, "first player packet");
        queue.submit(1, "second player packet");
        queue.submit(2, "other player packet");
        queue.open();

        let Some(mut first) = queue.try_next() else {
            panic!("first packet should start");
        };
        assert_eq!(first.take(), Some("first player packet"));

        let Some(mut other_player) = queue.try_next() else {
            panic!("another player's packet should be able to start");
        };
        assert_eq!(other_player.take(), Some("other player packet"));
        assert!(queue.try_next().is_none());

        drop(first);
        let Some(mut second) = queue.try_next() else {
            panic!("the next packet should start after its lane becomes idle");
        };
        assert_eq!(second.take(), Some("second player packet"));
    }

    #[test]
    fn closed_phase_retains_new_packets_for_the_next_open_phase() {
        let queue = PacketQueue::new();
        queue.open();
        queue.close();
        queue.submit(1, 1);

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
        queue.submit(1, 1);
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
        queue.submit(1, 1);
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
        queue.submit(1, 1);
        queue.stop();
        queue.submit(1, 2);

        assert!(queue.try_next().is_none());
    }

    #[test]
    fn stopping_with_active_work_keeps_completion_accounting_valid() {
        let queue = PacketQueue::new();
        queue.submit(1, 1);
        queue.submit(1, 2);
        queue.open();
        let Some(work) = queue.try_next() else {
            panic!("open packet phase should start queued work");
        };

        queue.stop();
        drop(work);

        assert!(queue.try_next().is_none());
        assert_eq!(queue.state.lock().active, 0);
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

        queue.submit(1, 1);
        assert!(receiver.recv_timeout(Duration::from_millis(10)).is_err());
        queue.open();
        assert_eq!(receiver.recv_timeout(Duration::from_secs(1)), Ok(1));
        queue.close();
        queue.submit(1, 2);
        assert!(receiver.recv_timeout(Duration::from_millis(10)).is_err());
        queue.stop();

        assert!(worker.join().is_ok());
    }
}
