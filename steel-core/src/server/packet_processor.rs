//! Serverbound gameplay packet scheduling between game ticks.

use std::{
    cmp::Reverse,
    collections::{BinaryHeap, VecDeque},
    hash::Hash,
    sync::Arc,
};

use parking_lot::Condvar;
use rustc_hash::{FxHashMap, FxHashSet};
use steel_utils::{locks::SyncMutex, translations};
use tokio::sync::Notify;
use tokio::task::yield_now;
use uuid::Uuid;

use crate::{
    entity::Entity,
    player::{
        Player,
        connection::NetworkConnection,
        networking::{ScheduledPacketExecution, ScheduledPlayPacket},
    },
};

use super::Server;

// Per-session count limits bound tick-drain work; byte limits bound retained decoded payloads.
// Vanilla's optional connection rate limit is disabled by default. This higher safety ceiling is
// intended to catch unbounded backlogs without treating ordinary traffic during a long tick as spam.
const MAX_OUTSTANDING_PACKETS_PER_PLAYER: usize = 8_192;
const MAX_OUTSTANDING_BYTES_PER_PLAYER: usize = 32 * 1024 * 1024;
// Approximate fixed queue, lane, and scheduling-index storage per admitted packet.
const PACKET_ADMISSION_OVERHEAD: usize = 256;

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
struct PlayerPacketLaneKey {
    player_id: Uuid,
    entity_id: i32,
}

impl PlayerPacketLaneKey {
    fn new(player: &Player) -> Self {
        Self {
            player_id: player.gameprofile.id,
            entity_id: player.id(),
        }
    }
}

struct PendingPlayPacket {
    player: Arc<Player>,
    packet: ScheduledPlayPacket,
}

/// Gameplay packets submitted by network tasks.
///
/// The processor runs while the game tick is idle. At each tick boundary it drains every packet
/// submitted before that boundary, while retaining later submissions for the next packet phase.
pub(super) struct PacketProcessor {
    queued: PacketQueue<PlayerPacketLaneKey, PendingPlayPacket>,
}

impl PacketProcessor {
    pub(super) fn new() -> Self {
        Self {
            queued: PacketQueue::new(),
        }
    }

    pub(super) fn schedule(
        &self,
        player: Arc<Player>,
        packet: ScheduledPlayPacket,
        payload_bytes: usize,
    ) {
        let player_id = player.gameprofile.id;
        let lane_key = PlayerPacketLaneKey::new(&player);
        if player.connection.closed() {
            self.queued.discard_lane(lane_key);
            return;
        }

        let execution = packet.execution();
        let admission_bytes = payload_bytes.saturating_add(PACKET_ADMISSION_OVERHEAD);
        let result = self.queued.try_submit(
            lane_key,
            execution,
            admission_bytes,
            PendingPlayPacket {
                player: Arc::clone(&player),
                packet,
            },
        );
        let Err(error) = result else {
            return;
        };
        if error == PacketAdmissionError::Stopped {
            return;
        }

        tracing::warn!(
            player_id = %player_id,
            entity_id = lane_key.entity_id,
            ?error,
            "Disconnecting player after inbound packet admission limit"
        );
        player.disconnect(translations::DISCONNECT_EXCEEDED_PACKET_RATE.msg());
        self.queued.discard_lane(lane_key);
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
        let Some(completed) = self.queued.progress_baseline() else {
            return;
        };
        yield_now().await;
        self.queued.wait_for_progress_since(completed).await;
    }

    /// Drains packets submitted before this tick boundary, then closes packet admission.
    pub(super) async fn close_for_tick(&self) {
        self.queued.drain_for_tick().await;
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
    Draining(u64),
    Stopped,
}

struct SequencedPacket<T> {
    sequence: u64,
    execution: ScheduledPacketExecution,
    admission_bytes: usize,
    value: T,
}

struct PacketLane<T> {
    queued: VecDeque<SequencedPacket<T>>,
    active: bool,
    outstanding_packets: usize,
    outstanding_bytes: usize,
}

impl<T> PacketLane<T> {
    const fn new() -> Self {
        Self {
            queued: VecDeque::new(),
            active: false,
            outstanding_packets: 0,
            outstanding_bytes: 0,
        }
    }
}

#[derive(Clone, Copy)]
struct PacketAdmissionLimits {
    per_player_packets: usize,
    per_player_bytes: usize,
}

impl PacketAdmissionLimits {
    const PRODUCTION: Self = Self {
        per_player_packets: MAX_OUTSTANDING_PACKETS_PER_PLAYER,
        per_player_bytes: MAX_OUTSTANDING_BYTES_PER_PLAYER,
    };
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum PacketAdmissionError {
    Stopped,
    PlayerPacketLimit,
    PlayerByteLimit,
}

struct PacketQueueState<K, T> {
    phase: PacketPhase,
    lanes: FxHashMap<K, PacketLane<T>>,
    player_local_ready: BinaryHeap<Reverse<(u64, K)>>,
    serialized_ready: BinaryHeap<Reverse<(u64, K)>>,
    exclusive_ready: BinaryHeap<Reverse<(u64, K)>>,
    serialized: BinaryHeap<Reverse<u64>>,
    exclusive: BinaryHeap<Reverse<u64>>,
    next_sequence: u64,
    active: usize,
    serialized_active: bool,
    exclusive_active: bool,
    completed: u64,
}

/// Bounded ordered multi-producer lanes with a finite snapshot drain at each tick boundary.
struct PacketQueue<K, T> {
    state: SyncMutex<PacketQueueState<K, T>>,
    limits: PacketAdmissionLimits,
    work_available: Condvar,
    idle: Notify,
    progress: Notify,
}

impl<K, T> PacketQueue<K, T>
where
    K: Copy + Eq + Hash + Ord,
{
    fn new() -> Self {
        Self::with_limits(PacketAdmissionLimits::PRODUCTION)
    }

    fn with_limits(limits: PacketAdmissionLimits) -> Self {
        Self {
            state: SyncMutex::new(PacketQueueState {
                phase: PacketPhase::Closed,
                lanes: FxHashMap::default(),
                player_local_ready: BinaryHeap::new(),
                serialized_ready: BinaryHeap::new(),
                exclusive_ready: BinaryHeap::new(),
                serialized: BinaryHeap::new(),
                exclusive: BinaryHeap::new(),
                next_sequence: 0,
                active: 0,
                serialized_active: false,
                exclusive_active: false,
                completed: 0,
            }),
            limits,
            work_available: Condvar::new(),
            idle: Notify::new(),
            progress: Notify::new(),
        }
    }

    fn try_submit(
        &self,
        key: K,
        execution: ScheduledPacketExecution,
        admission_bytes: usize,
        value: T,
    ) -> Result<(), PacketAdmissionError> {
        let mut state = self.state.lock();
        if state.phase == PacketPhase::Stopped {
            return Err(PacketAdmissionError::Stopped);
        }

        let (player_packets, player_bytes) = state
            .lanes
            .get(&key)
            .map(|lane| (lane.outstanding_packets, lane.outstanding_bytes))
            .unwrap_or_default();
        if player_packets >= self.limits.per_player_packets {
            return Err(PacketAdmissionError::PlayerPacketLimit);
        }
        if admission_bytes > self.limits.per_player_bytes.saturating_sub(player_bytes) {
            return Err(PacketAdmissionError::PlayerByteLimit);
        }
        let sequence = state.next_sequence;
        assert!(sequence != u64::MAX, "packet submission sequence exhausted");
        state.next_sequence = sequence + 1;

        let lane = state.lanes.entry(key).or_insert_with(PacketLane::new);
        let became_ready = !lane.active && lane.queued.is_empty();
        lane.outstanding_packets += 1;
        lane.outstanding_bytes += admission_bytes;
        lane.queued.push_back(SequencedPacket {
            sequence,
            execution,
            admission_bytes,
            value,
        });
        if became_ready {
            Self::mark_ready(&mut state, sequence, key, execution);
        }
        match execution {
            ScheduledPacketExecution::PlayerLocal => {}
            ScheduledPacketExecution::Serialized => state.serialized.push(Reverse(sequence)),
            ScheduledPacketExecution::Exclusive => state.exclusive.push(Reverse(sequence)),
        }
        let should_wake = became_ready && state.phase == PacketPhase::Open;
        drop(state);
        if should_wake {
            self.work_available.notify_one();
        }
        Ok(())
    }

    #[cfg(test)]
    fn submit(&self, key: K, execution: ScheduledPacketExecution, value: T) {
        let result = self.try_submit(key, execution, 1, value);
        assert!(
            matches!(result, Ok(()) | Err(PacketAdmissionError::Stopped)),
            "test packet exceeded admission limits: {result:?}"
        );
    }

    fn open(&self) {
        let mut state = self.state.lock();
        if state.phase == PacketPhase::Stopped {
            return;
        }
        state.phase = PacketPhase::Open;
        let should_wake = Self::select_next(&state, None).is_some();
        drop(state);
        if should_wake {
            self.work_available.notify_all();
        }
    }

    #[cfg(test)]
    fn close(&self) {
        let mut state = self.state.lock();
        if state.phase != PacketPhase::Stopped {
            state.phase = PacketPhase::Closed;
        }
    }

    async fn drain_for_tick(&self) {
        let Some((before_sequence, should_wake)) = self.begin_tick_drain() else {
            return;
        };
        if should_wake {
            self.work_available.notify_all();
        }

        loop {
            let idle = self.idle.notified();
            if self.tick_drain_complete(before_sequence) {
                break;
            }
            idle.await;
        }

        self.finish_tick_drain(before_sequence);
    }

    fn finish_tick_drain(&self, before_sequence: u64) {
        let mut state = self.state.lock();
        if state.phase == PacketPhase::Draining(before_sequence) {
            state.phase = PacketPhase::Closed;
        }
    }

    fn begin_tick_drain(&self) -> Option<(u64, bool)> {
        let mut state = self.state.lock();
        let before_sequence = match state.phase {
            PacketPhase::Stopped => None,
            PacketPhase::Draining(before_sequence) => Some(before_sequence),
            PacketPhase::Closed | PacketPhase::Open => {
                let before_sequence = state.next_sequence;
                state.phase = PacketPhase::Draining(before_sequence);
                Some(before_sequence)
            }
        }?;
        let should_wake = Self::select_next(&state, Some(before_sequence)).is_some();
        Some((before_sequence, should_wake))
    }

    fn tick_drain_complete(&self, before_sequence: u64) -> bool {
        let state = self.state.lock();
        if state.phase == PacketPhase::Stopped {
            return true;
        }
        if state.active != 0 {
            return false;
        }
        Self::next_ready_sequence(&state).is_none_or(|sequence| sequence >= before_sequence)
    }

    async fn wait_for_progress_since(&self, completed: u64) {
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

    fn discard_lane(&self, key: K) {
        let mut state = self.state.lock();
        let Some(lane) = state.lanes.get_mut(&key) else {
            return;
        };
        let discarded_packets = lane.queued.len();
        if discarded_packets == 0 {
            return;
        }
        let discarded_bytes = lane
            .queued
            .iter()
            .map(|packet| packet.admission_bytes)
            .sum::<usize>();
        let discarded_sequences = lane
            .queued
            .iter()
            .map(|packet| packet.sequence)
            .collect::<FxHashSet<_>>();
        lane.queued.clear();
        assert!(
            lane.outstanding_packets >= discarded_packets,
            "session packet admission accounting underflow while discarding"
        );
        lane.outstanding_packets -= discarded_packets;
        assert!(
            lane.outstanding_bytes >= discarded_bytes,
            "session byte admission accounting underflow while discarding"
        );
        lane.outstanding_bytes -= discarded_bytes;
        let remove_lane = !lane.active;

        state.player_local_ready.retain(|entry| entry.0.1 != key);
        state.serialized_ready.retain(|entry| entry.0.1 != key);
        state.exclusive_ready.retain(|entry| entry.0.1 != key);
        state
            .serialized
            .retain(|entry| !discarded_sequences.contains(&entry.0));
        state
            .exclusive
            .retain(|entry| !discarded_sequences.contains(&entry.0));
        if remove_lane {
            state.lanes.remove(&key);
        }

        let is_idle = state.active == 0;
        let should_wake = matches!(state.phase, PacketPhase::Open | PacketPhase::Draining(_))
            && Self::next_ready_sequence(&state).is_some();
        drop(state);
        self.progress.notify_one();
        if should_wake {
            self.work_available.notify_all();
        }
        if is_idle {
            self.idle.notify_one();
        }
    }

    fn stop(&self) {
        let mut state = self.state.lock();
        state.phase = PacketPhase::Stopped;
        state.player_local_ready.clear();
        state.serialized_ready.clear();
        state.exclusive_ready.clear();
        state.serialized.clear();
        state.exclusive.clear();
        state.lanes.retain(|_, lane| {
            let lane_discarded_packets = lane.queued.len();
            let lane_discarded_bytes = lane
                .queued
                .iter()
                .map(|packet| packet.admission_bytes)
                .sum::<usize>();
            lane.queued.clear();
            assert!(
                lane.outstanding_packets >= lane_discarded_packets,
                "session packet admission accounting underflow while stopping"
            );
            lane.outstanding_packets -= lane_discarded_packets;
            assert!(
                lane.outstanding_bytes >= lane_discarded_bytes,
                "session byte admission accounting underflow while stopping"
            );
            lane.outstanding_bytes -= lane_discarded_bytes;
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
                    if let Some((key, execution, admission_bytes, value)) =
                        Self::start_next(&mut state, None)
                    {
                        state.active += 1;
                        drop(state);
                        return Some(PacketWork {
                            value: Some(value),
                            key,
                            execution,
                            admission_bytes,
                            queue: self,
                        });
                    }
                }
                PacketPhase::Draining(before_sequence) => {
                    if let Some((key, execution, admission_bytes, value)) =
                        Self::start_next(&mut state, Some(before_sequence))
                    {
                        state.active += 1;
                        drop(state);
                        return Some(PacketWork {
                            value: Some(value),
                            key,
                            execution,
                            admission_bytes,
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
        let before_sequence = match state.phase {
            PacketPhase::Open => None,
            PacketPhase::Draining(before_sequence) => Some(before_sequence),
            PacketPhase::Closed | PacketPhase::Stopped => return None,
        };
        let (key, execution, admission_bytes, value) =
            Self::start_next(&mut state, before_sequence)?;
        state.active += 1;
        drop(state);
        Some(PacketWork {
            value: Some(value),
            key,
            execution,
            admission_bytes,
            queue: self,
        })
    }

    fn start_next(
        state: &mut PacketQueueState<K, T>,
        before_sequence: Option<u64>,
    ) -> Option<(K, ScheduledPacketExecution, usize, T)> {
        let (ready_sequence, key, execution) = Self::select_next(state, before_sequence)?;
        let Some(lane) = state.lanes.get(&key) else {
            panic!("ready packet lane disappeared before starting");
        };
        assert!(!lane.active, "ready packet lane is already active");
        let Some(packet) = lane.queued.front() else {
            panic!("ready packet lane has no queued packet");
        };
        assert_eq!(
            ready_sequence, packet.sequence,
            "ready packet sequence does not match lane front"
        );
        assert_eq!(
            execution, packet.execution,
            "packet is registered in the wrong ready queue"
        );

        match execution {
            ScheduledPacketExecution::PlayerLocal => {
                assert!(
                    state.player_local_ready.pop() == Some(Reverse((ready_sequence, key))),
                    "player-local ready queue changed while the queue lock was held"
                );
            }
            ScheduledPacketExecution::Serialized => {
                assert!(
                    state.serialized_ready.pop() == Some(Reverse((ready_sequence, key))),
                    "serialized ready queue changed while the queue lock was held"
                );
                assert_eq!(
                    state.serialized.pop(),
                    Some(Reverse(ready_sequence)),
                    "serialized packet order changed while the queue lock was held"
                );
                assert!(
                    !state.serialized_active,
                    "serialized packet started while another was active"
                );
                state.serialized_active = true;
            }
            ScheduledPacketExecution::Exclusive => {
                assert!(
                    state.exclusive_ready.pop() == Some(Reverse((ready_sequence, key))),
                    "exclusive ready queue changed while the queue lock was held"
                );
                assert_eq!(
                    state.exclusive.pop(),
                    Some(Reverse(ready_sequence)),
                    "exclusive packet barrier changed while the queue lock was held"
                );
                state.exclusive_active = true;
            }
        }

        let Some(lane) = state.lanes.get_mut(&key) else {
            panic!("ready packet lane disappeared before removal");
        };
        let Some(packet) = lane.queued.pop_front() else {
            panic!("ready packet lane has no queued packet during removal");
        };
        lane.active = true;
        Some((key, execution, packet.admission_bytes, packet.value))
    }

    fn select_next(
        state: &PacketQueueState<K, T>,
        before_sequence: Option<u64>,
    ) -> Option<(u64, K, ScheduledPacketExecution)> {
        if state.exclusive_active {
            return None;
        }

        let next_exclusive = state.exclusive.peek().map(|entry| entry.0);
        let next_serialized = state.serialized.peek().map(|entry| entry.0);
        let mut selected = None;
        let mut consider = |entry: Option<&Reverse<(u64, K)>>,
                            execution: ScheduledPacketExecution| {
            let Some(Reverse((sequence, key))) = entry.copied() else {
                return;
            };
            if before_sequence.is_some_and(|cutoff| sequence >= cutoff)
                || next_exclusive.is_some_and(|exclusive| exclusive < sequence)
            {
                return;
            }
            if selected.is_none_or(|(selected_sequence, _, _)| sequence < selected_sequence) {
                selected = Some((sequence, key, execution));
            }
        };

        consider(
            state.player_local_ready.peek(),
            ScheduledPacketExecution::PlayerLocal,
        );
        if !state.serialized_active
            && state
                .serialized_ready
                .peek()
                .is_some_and(|entry| Some(entry.0.0) == next_serialized)
        {
            consider(
                state.serialized_ready.peek(),
                ScheduledPacketExecution::Serialized,
            );
        }
        if state.active == 0
            && state
                .exclusive_ready
                .peek()
                .is_some_and(|entry| Some(entry.0.0) == next_exclusive)
        {
            consider(
                state.exclusive_ready.peek(),
                ScheduledPacketExecution::Exclusive,
            );
        }
        selected
    }

    fn finish_one(&self, key: K, execution: ScheduledPacketExecution, admission_bytes: usize) {
        let mut state = self.state.lock();
        assert!(state.active > 0, "packet work accounting underflow");
        state.active -= 1;
        state.completed = state.completed.wrapping_add(1);
        match execution {
            ScheduledPacketExecution::PlayerLocal => {}
            ScheduledPacketExecution::Serialized => {
                assert!(state.serialized_active, "serialized packet is not active");
                state.serialized_active = false;
            }
            ScheduledPacketExecution::Exclusive => {
                assert!(state.exclusive_active, "exclusive packet is not active");
                state.exclusive_active = false;
            }
        }

        let next_sequence = {
            let Some(lane) = state.lanes.get_mut(&key) else {
                panic!("active packet lane disappeared before completion");
            };
            assert!(lane.active, "completed packet lane is not active");
            lane.active = false;
            assert!(
                lane.outstanding_packets > 0,
                "session packet admission accounting underflow on completion"
            );
            lane.outstanding_packets -= 1;
            assert!(
                lane.outstanding_bytes >= admission_bytes,
                "session byte admission accounting underflow on completion"
            );
            lane.outstanding_bytes -= admission_bytes;
            lane.queued.front().map(|packet| packet.sequence)
        };
        if let Some(sequence) = next_sequence {
            let Some(lane) = state.lanes.get(&key) else {
                panic!("packet lane disappeared before its next packet became ready");
            };
            let Some(packet) = lane.queued.front() else {
                panic!("packet lane has no next packet after reporting its sequence");
            };
            let next_execution = packet.execution;
            Self::mark_ready(&mut state, sequence, key, next_execution);
        } else {
            let Some(lane) = state.lanes.get(&key) else {
                panic!("completed packet lane disappeared before removal");
            };
            assert_eq!(lane.outstanding_packets, 0);
            assert_eq!(lane.outstanding_bytes, 0);
            state.lanes.remove(&key);
        }

        let is_idle = state.active == 0;
        let should_wake = matches!(state.phase, PacketPhase::Open | PacketPhase::Draining(_))
            && Self::next_ready_sequence(&state).is_some();
        drop(state);
        self.progress.notify_one();
        if should_wake {
            match execution {
                ScheduledPacketExecution::Exclusive => {
                    self.work_available.notify_all();
                }
                ScheduledPacketExecution::PlayerLocal | ScheduledPacketExecution::Serialized => {
                    self.work_available.notify_one();
                }
            }
        }
        if is_idle {
            self.idle.notify_one();
        }
    }

    fn mark_ready(
        state: &mut PacketQueueState<K, T>,
        sequence: u64,
        key: K,
        execution: ScheduledPacketExecution,
    ) {
        let entry = Reverse((sequence, key));
        match execution {
            ScheduledPacketExecution::PlayerLocal => state.player_local_ready.push(entry),
            ScheduledPacketExecution::Serialized => state.serialized_ready.push(entry),
            ScheduledPacketExecution::Exclusive => state.exclusive_ready.push(entry),
        }
    }

    fn next_ready_sequence(state: &PacketQueueState<K, T>) -> Option<u64> {
        [
            state.player_local_ready.peek().map(|entry| entry.0.0),
            state.serialized_ready.peek().map(|entry| entry.0.0),
            state.exclusive_ready.peek().map(|entry| entry.0.0),
        ]
        .into_iter()
        .flatten()
        .min()
    }
}

struct PacketWork<'a, K, T>
where
    K: Copy + Eq + Hash + Ord,
{
    value: Option<T>,
    key: K,
    execution: ScheduledPacketExecution,
    admission_bytes: usize,
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
        self.queue
            .finish_one(self.key, self.execution, self.admission_bytes);
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
    use uuid::Uuid;

    use super::{
        PACKET_ADMISSION_OVERHEAD, PacketAdmissionError, PacketAdmissionLimits, PacketQueue,
        PlayerPacketLaneKey, ScheduledPacketExecution,
    };

    const fn limits(per_player_packets: usize, per_player_bytes: usize) -> PacketAdmissionLimits {
        PacketAdmissionLimits {
            per_player_packets,
            per_player_bytes,
        }
    }

    #[test]
    fn per_player_packet_limit_rejects_before_assigning_a_sequence() {
        let queue = PacketQueue::with_limits(limits(2, 100));
        assert_eq!(
            queue.try_submit(1, ScheduledPacketExecution::PlayerLocal, 1, "first"),
            Ok(())
        );
        assert_eq!(
            queue.try_submit(1, ScheduledPacketExecution::Serialized, 1, "second"),
            Ok(())
        );

        assert_eq!(
            queue.try_submit(1, ScheduledPacketExecution::Exclusive, 1, "rejected"),
            Err(PacketAdmissionError::PlayerPacketLimit)
        );
        assert_eq!(queue.state.lock().next_sequence, 2);
        assert_eq!(
            queue.try_submit(2, ScheduledPacketExecution::PlayerLocal, 1, "other player"),
            Ok(())
        );
    }

    #[test]
    fn per_player_byte_limit_is_independent_of_packet_count() {
        let queue = PacketQueue::with_limits(limits(10, 6));
        assert_eq!(
            queue.try_submit(1, ScheduledPacketExecution::PlayerLocal, 4, "first"),
            Ok(())
        );

        assert_eq!(
            queue.try_submit(1, ScheduledPacketExecution::PlayerLocal, 3, "rejected"),
            Err(PacketAdmissionError::PlayerByteLimit)
        );
        assert_eq!(
            queue.try_submit(2, ScheduledPacketExecution::PlayerLocal, 6, "other player"),
            Ok(())
        );
    }

    #[test]
    fn production_limit_allows_a_watchdog_window_of_mounted_client_traffic() {
        const VANILLA_WATCHDOG_SECONDS: usize = 60;
        const CLIENT_TICKS_PER_SECOND: usize = 20;
        const MOUNTED_PACKETS_PER_CLIENT_TICK: usize = 4;
        const EXPECTED_PACKETS: usize =
            VANILLA_WATCHDOG_SECONDS * CLIENT_TICKS_PER_SECOND * MOUNTED_PACKETS_PER_CLIENT_TICK;

        let key = PlayerPacketLaneKey {
            player_id: Uuid::nil(),
            entity_id: 1,
        };
        let queue = PacketQueue::new();
        for packet in 0..EXPECTED_PACKETS {
            assert!(
                queue
                    .try_submit(
                        key,
                        ScheduledPacketExecution::PlayerLocal,
                        PACKET_ADMISSION_OVERHEAD,
                        packet,
                    )
                    .is_ok()
            );
        }

        let state = queue.state.lock();
        let lane = state.lanes.get(&key).expect("session lane should exist");
        assert_eq!(lane.outstanding_packets, EXPECTED_PACKETS);
    }

    #[test]
    fn independent_session_limits_do_not_accumulate_globally() {
        const SESSION_COUNT: usize = 64;
        const PACKETS_PER_SESSION: usize = 33;

        let queue = PacketQueue::new();
        for entity_id in 1_i32..=64 {
            let key = PlayerPacketLaneKey {
                player_id: Uuid::from_u128(u128::from(entity_id.unsigned_abs())),
                entity_id,
            };
            for packet in 0..PACKETS_PER_SESSION {
                assert!(
                    queue
                        .try_submit(
                            key,
                            ScheduledPacketExecution::PlayerLocal,
                            PACKET_ADMISSION_OVERHEAD,
                            packet,
                        )
                        .is_ok()
                );
            }
        }

        let state = queue.state.lock();
        assert_eq!(state.lanes.len(), SESSION_COUNT);
        assert_eq!(
            usize::try_from(state.next_sequence),
            Ok(SESSION_COUNT * PACKETS_PER_SESSION)
        );
    }

    #[test]
    fn discarding_stale_session_keeps_replacement_session_work() {
        let player_id = Uuid::nil();
        let stale_key = PlayerPacketLaneKey {
            player_id,
            entity_id: 1,
        };
        let replacement_key = PlayerPacketLaneKey {
            player_id,
            entity_id: 2,
        };
        let queue = PacketQueue::with_limits(limits(10, 100));
        queue.submit(
            stale_key,
            ScheduledPacketExecution::Exclusive,
            "stale barrier",
        );
        queue.submit(
            stale_key,
            ScheduledPacketExecution::Serialized,
            "stale serialized",
        );
        queue.submit(
            replacement_key,
            ScheduledPacketExecution::PlayerLocal,
            "replacement",
        );

        queue.discard_lane(stale_key);

        {
            let queue_state = queue.state.lock();
            assert!(!queue_state.lanes.contains_key(&stale_key));
            let replacement_lane = queue_state
                .lanes
                .get(&replacement_key)
                .expect("replacement session lane should remain");
            assert_eq!(replacement_lane.outstanding_packets, 1);
            assert_eq!(replacement_lane.outstanding_bytes, 1);
            assert!(queue_state.serialized.is_empty());
            assert!(queue_state.exclusive.is_empty());
        }

        queue.open();
        let Some(mut work) = queue.try_next() else {
            panic!("replacement session packet should remain runnable");
        };
        assert_eq!(work.take(), Some("replacement"));
    }

    #[test]
    fn active_packet_remains_charged_until_completion() {
        let queue = PacketQueue::with_limits(limits(1, 4));
        assert_eq!(
            queue.try_submit(1, ScheduledPacketExecution::PlayerLocal, 4, "active"),
            Ok(())
        );
        queue.open();
        let Some(work) = queue.try_next() else {
            panic!("accepted packet should start");
        };

        assert_eq!(
            queue.try_submit(1, ScheduledPacketExecution::PlayerLocal, 1, "rejected"),
            Err(PacketAdmissionError::PlayerPacketLimit)
        );
        {
            let state = queue.state.lock();
            let lane = state.lanes.get(&1).expect("active lane should remain");
            assert_eq!(lane.outstanding_packets, 1);
            assert_eq!(lane.outstanding_bytes, 4);
        }

        drop(work);
        {
            let state = queue.state.lock();
            assert!(state.lanes.is_empty());
        }
        assert_eq!(
            queue.try_submit(1, ScheduledPacketExecution::PlayerLocal, 4, "next"),
            Ok(())
        );
    }

    #[test]
    fn discarding_lane_removes_hidden_barriers_and_keeps_active_work_charged() {
        let queue = PacketQueue::with_limits(limits(10, 100));
        queue.submit(1, ScheduledPacketExecution::PlayerLocal, "active attacker");
        queue.submit(1, ScheduledPacketExecution::Serialized, "hidden serialized");
        queue.submit(1, ScheduledPacketExecution::Exclusive, "hidden exclusive");
        queue.submit(2, ScheduledPacketExecution::PlayerLocal, "other player");
        queue.open();
        let Some(mut active) = queue.try_next() else {
            panic!("attacker's first packet should start");
        };
        assert_eq!(active.take(), Some("active attacker"));

        queue.discard_lane(1);

        {
            let state = queue.state.lock();
            assert!(state.serialized.is_empty());
            assert!(state.exclusive.is_empty());
            assert!(state.serialized_ready.is_empty());
            assert!(state.exclusive_ready.is_empty());
            let Some(lane) = state.lanes.get(&1) else {
                panic!("active attacker lane should remain");
            };
            assert!(lane.active);
            assert!(lane.queued.is_empty());
            assert_eq!(lane.outstanding_packets, 1);
            assert_eq!(lane.outstanding_bytes, 1);
        }

        let Some(mut other) = queue.try_next() else {
            panic!("purged barriers should no longer block another player");
        };
        assert_eq!(other.take(), Some("other player"));
        drop(other);
        drop(active);

        let state = queue.state.lock();
        assert!(state.lanes.is_empty());
    }

    #[test]
    fn discarding_idle_lane_removes_ready_exclusive_barrier() {
        let queue = PacketQueue::with_limits(limits(10, 100));
        queue.submit(1, ScheduledPacketExecution::Exclusive, "attacker barrier");
        queue.submit(1, ScheduledPacketExecution::Serialized, "hidden serialized");
        queue.submit(2, ScheduledPacketExecution::PlayerLocal, "other player");

        queue.discard_lane(1);

        {
            let state = queue.state.lock();
            assert!(!state.lanes.contains_key(&1));
            let other = state.lanes.get(&2).expect("other lane should remain");
            assert_eq!(other.outstanding_packets, 1);
            assert_eq!(other.outstanding_bytes, 1);
            assert!(state.exclusive_ready.is_empty());
            assert!(state.exclusive.is_empty());
            assert!(state.serialized.is_empty());
        }

        queue.open();
        let Some(mut other) = queue.try_next() else {
            panic!("discarded exclusive barrier should not block another player");
        };
        assert_eq!(other.take(), Some("other player"));
    }

    #[test]
    fn queued_packets_start_in_submission_order_when_opened() {
        let queue = PacketQueue::new();
        queue.submit(1, ScheduledPacketExecution::PlayerLocal, 1);
        queue.submit(2, ScheduledPacketExecution::PlayerLocal, 2);
        queue.submit(1, ScheduledPacketExecution::PlayerLocal, 3);
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
        queue.submit(
            1,
            ScheduledPacketExecution::PlayerLocal,
            "first player packet",
        );
        queue.submit(
            1,
            ScheduledPacketExecution::PlayerLocal,
            "second player packet",
        );
        queue.submit(
            2,
            ScheduledPacketExecution::PlayerLocal,
            "other player packet",
        );
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
    fn serialized_packet_overlaps_player_local_work_but_not_another_serialized_packet() {
        let queue = PacketQueue::new();
        queue.submit(1, ScheduledPacketExecution::PlayerLocal, "first local");
        queue.submit(2, ScheduledPacketExecution::Serialized, "first serialized");
        queue.submit(3, ScheduledPacketExecution::Serialized, "second serialized");
        queue.submit(4, ScheduledPacketExecution::PlayerLocal, "later local");
        queue.open();

        let Some(mut first_local) = queue.try_next() else {
            panic!("first player-local packet should start");
        };
        assert_eq!(first_local.take(), Some("first local"));

        let Some(mut first_serialized) = queue.try_next() else {
            panic!("serialized packet should overlap player-local work");
        };
        assert_eq!(first_serialized.take(), Some("first serialized"));

        let Some(mut later_local) = queue.try_next() else {
            panic!("player-local work should bypass a blocked serialized packet");
        };
        assert_eq!(later_local.take(), Some("later local"));
        assert!(queue.try_next().is_none());

        drop(first_serialized);
        let Some(mut second_serialized) = queue.try_next() else {
            panic!("next serialized packet should start after its predecessor finishes");
        };
        assert_eq!(second_serialized.take(), Some("second serialized"));
    }

    #[test]
    fn serialized_packet_hidden_in_an_active_lane_preserves_serialized_order() {
        let queue = PacketQueue::new();
        queue.submit(1, ScheduledPacketExecution::PlayerLocal, "active lane");
        queue.submit(1, ScheduledPacketExecution::Serialized, "hidden serialized");
        queue.submit(2, ScheduledPacketExecution::Serialized, "later serialized");
        queue.submit(
            3,
            ScheduledPacketExecution::PlayerLocal,
            "independent local",
        );
        queue.open();

        let Some(active_lane) = queue.try_next() else {
            panic!("first lane packet should start");
        };
        let Some(mut independent_local) = queue.try_next() else {
            panic!("player-local work should bypass serialized ordering contention");
        };
        assert_eq!(independent_local.take(), Some("independent local"));
        assert!(queue.try_next().is_none());

        drop(active_lane);
        let Some(mut hidden_serialized) = queue.try_next() else {
            panic!("earliest serialized packet should start when its lane becomes idle");
        };
        assert_eq!(hidden_serialized.take(), Some("hidden serialized"));
        assert!(queue.try_next().is_none());

        drop(hidden_serialized);
        let Some(mut later_serialized) = queue.try_next() else {
            panic!("later serialized packet should preserve global submission order");
        };
        assert_eq!(later_serialized.take(), Some("later serialized"));
    }

    #[test]
    fn blocking_worker_bypasses_active_serialized_work_for_player_local_work() {
        let queue = Arc::new(PacketQueue::new());
        queue.submit(1, ScheduledPacketExecution::Serialized, "active serialized");
        queue.submit(2, ScheduledPacketExecution::Serialized, "queued serialized");
        queue.submit(3, ScheduledPacketExecution::PlayerLocal, "player local");
        queue.open();

        let Some(active_serialized) = queue.try_next() else {
            panic!("first serialized packet should start");
        };
        let worker_queue = Arc::clone(&queue);
        let (sender, receiver) = mpsc::channel();
        let worker = thread::spawn(move || {
            for _ in 0..2 {
                let Some(mut work) = worker_queue.next() else {
                    return;
                };
                if let Some(value) = work.take() {
                    let _ = sender.send(value);
                }
            }
        });

        assert_eq!(
            receiver.recv_timeout(Duration::from_secs(1)),
            Ok("player local")
        );
        assert!(receiver.recv_timeout(Duration::from_millis(10)).is_err());

        drop(active_serialized);
        assert_eq!(
            receiver.recv_timeout(Duration::from_secs(1)),
            Ok("queued serialized")
        );
        assert!(worker.join().is_ok());
    }

    #[test]
    fn exclusive_packet_waits_for_active_work_and_blocks_later_packets() {
        let queue = PacketQueue::new();
        queue.submit(1, ScheduledPacketExecution::PlayerLocal, "before barrier");
        queue.submit(2, ScheduledPacketExecution::Exclusive, "barrier");
        queue.submit(3, ScheduledPacketExecution::PlayerLocal, "after barrier");
        queue.open();

        let Some(mut before) = queue.try_next() else {
            panic!("packet before the barrier should start");
        };
        assert_eq!(before.take(), Some("before barrier"));
        assert!(queue.try_next().is_none());

        drop(before);
        let Some(mut barrier) = queue.try_next() else {
            panic!("exclusive packet should start after active work finishes");
        };
        assert_eq!(barrier.take(), Some("barrier"));
        assert!(queue.try_next().is_none());

        drop(barrier);
        let Some(mut after) = queue.try_next() else {
            panic!("packet after the barrier should start after it finishes");
        };
        assert_eq!(after.take(), Some("after barrier"));
    }

    #[test]
    fn exclusive_packet_waits_for_serialized_work_and_blocks_player_local_work() {
        let queue = PacketQueue::new();
        queue.submit(1, ScheduledPacketExecution::Serialized, "before barrier");
        queue.submit(2, ScheduledPacketExecution::Exclusive, "barrier");
        queue.submit(3, ScheduledPacketExecution::PlayerLocal, "after barrier");
        queue.open();

        let Some(mut before) = queue.try_next() else {
            panic!("serialized packet before the barrier should start");
        };
        assert_eq!(before.take(), Some("before barrier"));
        assert!(queue.try_next().is_none());

        drop(before);
        let Some(mut barrier) = queue.try_next() else {
            panic!("exclusive packet should wait for serialized work");
        };
        assert_eq!(barrier.take(), Some("barrier"));
        assert!(queue.try_next().is_none());

        drop(barrier);
        let Some(mut after) = queue.try_next() else {
            panic!("player-local packet should wait for the exclusive barrier");
        };
        assert_eq!(after.take(), Some("after barrier"));
    }

    #[test]
    fn exclusive_packet_hidden_in_an_active_lane_still_blocks_later_lanes() {
        let queue = PacketQueue::new();
        queue.submit(1, ScheduledPacketExecution::PlayerLocal, "active");
        queue.submit(1, ScheduledPacketExecution::Exclusive, "barrier");
        queue.submit(2, ScheduledPacketExecution::PlayerLocal, "later lane");
        queue.open();

        let Some(active) = queue.try_next() else {
            panic!("first packet should start");
        };
        assert!(queue.try_next().is_none());

        drop(active);
        let Some(mut barrier) = queue.try_next() else {
            panic!("hidden exclusive packet should become runnable");
        };
        assert_eq!(barrier.take(), Some("barrier"));
    }

    #[test]
    fn closed_phase_retains_new_packets_for_the_next_open_phase() {
        let queue = PacketQueue::new();
        queue.open();
        queue.close();
        queue.submit(1, ScheduledPacketExecution::PlayerLocal, 1);

        assert!(queue.try_next().is_none());
        queue.open();
        let Some(mut work) = queue.try_next() else {
            panic!("queued packet should become available when the packet phase opens");
        };
        assert_eq!(work.take(), Some(1));
    }

    #[test]
    fn tick_drain_only_processes_packets_submitted_before_its_cutoff() {
        let queue = PacketQueue::new();
        queue.submit(1, ScheduledPacketExecution::PlayerLocal, "before cutoff");
        queue.open();
        let Some((before_sequence, should_wake)) = queue.begin_tick_drain() else {
            panic!("running queue should begin a tick drain");
        };
        assert!(should_wake);
        queue.submit(2, ScheduledPacketExecution::PlayerLocal, "after cutoff");

        let Some(mut before) = queue.try_next() else {
            panic!("packet before the cutoff should drain");
        };
        assert_eq!(before.take(), Some("before cutoff"));
        drop(before);

        assert!(queue.try_next().is_none());
        assert!(queue.tick_drain_complete(before_sequence));
        queue.finish_tick_drain(before_sequence);
        queue.open();
        let Some(mut after) = queue.try_next() else {
            panic!("packet after the cutoff should wait for the next open phase");
        };
        assert_eq!(after.take(), Some("after cutoff"));
    }

    #[test]
    fn tick_drain_orders_hidden_serialized_and_exclusive_work_before_its_cutoff() {
        let queue = PacketQueue::new();
        queue.submit(1, ScheduledPacketExecution::PlayerLocal, "active local");
        queue.submit(1, ScheduledPacketExecution::Serialized, "hidden serialized");
        queue.submit(2, ScheduledPacketExecution::Exclusive, "exclusive");
        queue.open();

        let Some(active_local) = queue.try_next() else {
            panic!("player-local packet should start before draining");
        };
        let Some((before_sequence, should_wake)) = queue.begin_tick_drain() else {
            panic!("running queue should begin a tick drain");
        };
        assert!(!should_wake);
        queue.submit(
            3,
            ScheduledPacketExecution::Serialized,
            "post-cutoff serialized",
        );
        queue.submit(
            4,
            ScheduledPacketExecution::PlayerLocal,
            "post-cutoff local",
        );

        assert!(!queue.tick_drain_complete(before_sequence));
        assert!(queue.try_next().is_none());
        drop(active_local);

        let Some(mut hidden_serialized) = queue.try_next() else {
            panic!("hidden pre-cutoff serialized packet should drain");
        };
        assert_eq!(hidden_serialized.take(), Some("hidden serialized"));
        assert!(!queue.tick_drain_complete(before_sequence));
        drop(hidden_serialized);

        let Some(mut exclusive) = queue.try_next() else {
            panic!("pre-cutoff exclusive packet should drain after earlier work");
        };
        assert_eq!(exclusive.take(), Some("exclusive"));
        assert!(!queue.tick_drain_complete(before_sequence));
        drop(exclusive);

        assert!(queue.try_next().is_none());
        assert!(queue.tick_drain_complete(before_sequence));
        queue.finish_tick_drain(before_sequence);
        queue.open();

        let Some(mut post_cutoff_serialized) = queue.try_next() else {
            panic!("post-cutoff serialized packet should remain for the next phase");
        };
        assert_eq!(
            post_cutoff_serialized.take(),
            Some("post-cutoff serialized")
        );
        let Some(mut post_cutoff_local) = queue.try_next() else {
            panic!("post-cutoff player-local packet should remain for the next phase");
        };
        assert_eq!(post_cutoff_local.take(), Some("post-cutoff local"));
    }

    #[tokio::test]
    async fn tick_drain_waits_for_active_packet_work() {
        let queue = Arc::new(PacketQueue::new());
        queue.open();
        queue.submit(1, ScheduledPacketExecution::PlayerLocal, 1);
        let Some(work) = queue.try_next() else {
            panic!("open packet phase should start queued work");
        };
        let drain = queue.drain_for_tick();
        tokio::pin!(drain);

        assert!(
            timeout(Duration::from_millis(10), drain.as_mut())
                .await
                .is_err()
        );
        drop(work);
        assert!(
            timeout(Duration::from_secs(1), drain.as_mut())
                .await
                .is_ok()
        );
    }

    #[tokio::test]
    async fn overload_progress_waits_for_one_active_packet_to_finish() {
        let queue = Arc::new(PacketQueue::new());
        queue.open();
        queue.submit(1, ScheduledPacketExecution::PlayerLocal, 1);
        let Some(work) = queue.try_next() else {
            panic!("open packet phase should start queued work");
        };
        let Some(completed) = queue.progress_baseline() else {
            panic!("active packet work should require progress");
        };
        let progress = queue.wait_for_progress_since(completed);
        tokio::pin!(progress);

        assert!(
            timeout(Duration::from_millis(10), progress.as_mut())
                .await
                .is_err()
        );
        drop(work);
        assert!(
            timeout(Duration::from_secs(1), progress.as_mut())
                .await
                .is_ok()
        );
    }

    #[test]
    fn stopped_queue_discards_pending_and_future_work() {
        let queue = PacketQueue::new();
        queue.open();
        queue.submit(1, ScheduledPacketExecution::PlayerLocal, 1);
        queue.stop();
        queue.submit(1, ScheduledPacketExecution::PlayerLocal, 2);

        assert!(queue.try_next().is_none());
        let state = queue.state.lock();
        assert!(state.lanes.is_empty());
    }

    #[test]
    fn stopping_with_active_work_keeps_completion_accounting_valid() {
        let queue = PacketQueue::new();
        queue.submit(1, ScheduledPacketExecution::Exclusive, 1);
        queue.submit(1, ScheduledPacketExecution::PlayerLocal, 2);
        queue.open();
        let Some(work) = queue.try_next() else {
            panic!("open packet phase should start queued work");
        };

        queue.stop();
        {
            let state = queue.state.lock();
            let lane = state.lanes.get(&1).expect("active lane should remain");
            assert!(lane.active);
            assert!(lane.queued.is_empty());
            assert_eq!(lane.outstanding_packets, 1);
            assert_eq!(lane.outstanding_bytes, 1);
        }
        drop(work);

        assert!(queue.try_next().is_none());
        let state = queue.state.lock();
        assert_eq!(state.active, 0);
        assert!(state.lanes.is_empty());
    }

    #[tokio::test]
    async fn stopping_with_active_serialized_and_local_work_clears_all_queue_state() {
        let queue = PacketQueue::new();
        queue.submit(1, ScheduledPacketExecution::Serialized, "active serialized");
        queue.submit(2, ScheduledPacketExecution::PlayerLocal, "active local");
        queue.submit(3, ScheduledPacketExecution::Serialized, "queued serialized");
        queue.submit(4, ScheduledPacketExecution::Exclusive, "queued exclusive");
        queue.open();

        let Some(active_serialized) = queue.try_next() else {
            panic!("serialized packet should start");
        };
        let Some(active_local) = queue.try_next() else {
            panic!("player-local packet should overlap serialized work");
        };
        queue.stop();
        queue.drain_for_tick().await;

        {
            let state = queue.state.lock();
            assert_eq!(state.active, 2);
            assert!(state.serialized_active);
            assert!(state.player_local_ready.is_empty());
            assert!(state.serialized_ready.is_empty());
            assert!(state.exclusive_ready.is_empty());
            assert!(state.serialized.is_empty());
            assert!(state.exclusive.is_empty());
            assert_eq!(state.lanes.len(), 2);
            for key in [1, 2] {
                let lane = state.lanes.get(&key).expect("active lane should remain");
                assert!(lane.active);
                assert!(lane.queued.is_empty());
                assert_eq!(lane.outstanding_packets, 1);
                assert_eq!(lane.outstanding_bytes, 1);
            }
        }

        drop(active_serialized);
        drop(active_local);

        let state = queue.state.lock();
        assert_eq!(state.active, 0);
        assert!(!state.serialized_active);
        assert!(state.lanes.is_empty());
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

        queue.submit(1, ScheduledPacketExecution::PlayerLocal, 1);
        assert!(receiver.recv_timeout(Duration::from_millis(10)).is_err());
        queue.open();
        assert_eq!(receiver.recv_timeout(Duration::from_secs(1)), Ok(1));
        queue.close();
        queue.submit(1, ScheduledPacketExecution::PlayerLocal, 2);
        assert!(receiver.recv_timeout(Duration::from_millis(10)).is_err());
        queue.stop();

        assert!(worker.join().is_ok());
    }
}
