//! Scheduling gate that keeps overlapping chunk windows from being worked on at once.

use std::sync::Arc;

use steel_utils::{ChunkPos, locks::SyncMutex};
use tokio::sync::Notify;

/// Shared gate used to exclude overlapping square chunk windows.
///
/// Work that may write anywhere in the `(2 * window_radius + 1)²` chunks around its center
/// reserves that window first. Two windows overlap when their centers are at most
/// `2 * window_radius` apart on both axes; overlapping work runs one at a time and the
/// rest runs concurrently.
#[derive(Debug)]
pub(crate) struct ChunkWindowGate {
    window_radius: i32,
    active_centers: SyncMutex<Vec<ChunkPos>>,
    released: Notify,
}

/// Reservation for one chunk window.
#[derive(Debug)]
pub(crate) struct ChunkWindowReservation {
    gate: Arc<ChunkWindowGate>,
    center: ChunkPos,
}

impl ChunkWindowGate {
    /// Creates an empty gate for windows of `window_radius` chunks around their center.
    #[must_use]
    pub(crate) fn new(window_radius: i32) -> Self {
        debug_assert!(window_radius >= 0, "window radius must not be negative");
        Self {
            window_radius,
            active_centers: SyncMutex::new(Vec::new()),
            released: Notify::new(),
        }
    }

    /// Reserves the window centered on `center`, waiting until no overlapping window is held.
    pub(crate) async fn reserve_centered(
        self: &Arc<Self>,
        center: ChunkPos,
    ) -> ChunkWindowReservation {
        let Some(reservation) = self.reserve_centered_unless(center, || false).await else {
            unreachable!("a reservation that never gives up always returns one");
        };
        reservation
    }

    /// Reserves the window centered on `center` like [`Self::reserve_centered`], but returns
    /// `None` instead once `give_up` is true. It is checked before every attempt, including
    /// after each release that wakes this waiter.
    pub(crate) async fn reserve_centered_unless(
        self: &Arc<Self>,
        center: ChunkPos,
        give_up: impl Fn() -> bool,
    ) -> Option<ChunkWindowReservation> {
        loop {
            let released = self.released.notified();
            if give_up() {
                return None;
            }
            if let Some(reservation) = self.try_reserve_centered(center) {
                return Some(reservation);
            }
            released.await;
        }
    }

    /// Attempts to reserve the window centered on `center`.
    pub(crate) fn try_reserve_centered(
        self: &Arc<Self>,
        center: ChunkPos,
    ) -> Option<ChunkWindowReservation> {
        let mut active_centers = self.active_centers.lock();
        if active_centers
            .iter()
            .any(|&active| self.windows_overlap(center, active))
        {
            return None;
        }

        active_centers.push(center);
        Some(ChunkWindowReservation {
            gate: Arc::clone(self),
            center,
        })
    }

    const fn windows_overlap(&self, left: ChunkPos, right: ChunkPos) -> bool {
        let exclusion_radius = (self.window_radius * 2) as u32;
        let dx = left.0.x.abs_diff(right.0.x);
        let dz = left.0.y.abs_diff(right.0.y);
        dx <= exclusion_radius && dz <= exclusion_radius
    }
}

impl Drop for ChunkWindowReservation {
    fn drop(&mut self) {
        let removed = {
            let mut active_centers = self.gate.active_centers.lock();
            if let Some(index) = active_centers
                .iter()
                .position(|&center| center == self.center)
            {
                active_centers.swap_remove(index);
                true
            } else {
                false
            }
        };

        debug_assert!(removed, "chunk window reservation missing active center");
        if removed {
            self.gate.released.notify_waiters();
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicBool, Ordering};

    use futures::FutureExt as _;

    use super::*;

    #[test]
    fn overlapping_windows_cannot_be_reserved_together() {
        let gate = Arc::new(ChunkWindowGate::new(2));
        let _first = gate
            .try_reserve_centered(ChunkPos::new(0, 0))
            .expect("first window should reserve");

        assert!(gate.try_reserve_centered(ChunkPos::new(1, 0)).is_none());
        assert!(gate.try_reserve_centered(ChunkPos::new(4, 4)).is_none());
    }

    #[test]
    fn non_overlapping_windows_can_be_reserved_together() {
        let gate = Arc::new(ChunkWindowGate::new(2));
        let _first = gate
            .try_reserve_centered(ChunkPos::new(0, 0))
            .expect("first window should reserve");

        assert!(gate.try_reserve_centered(ChunkPos::new(5, 0)).is_some());
        assert!(gate.try_reserve_centered(ChunkPos::new(0, 5)).is_some());
    }

    /// Radius-1 windows (a step writing into its direct neighbors) overlap when their
    /// centers are within two chunks.
    #[test]
    fn window_radius_sets_the_exclusion_distance() {
        let gate = Arc::new(ChunkWindowGate::new(1));
        let _first = gate
            .try_reserve_centered(ChunkPos::new(0, 0))
            .expect("first window should reserve");

        assert!(gate.try_reserve_centered(ChunkPos::new(2, 2)).is_none());
        assert!(gate.try_reserve_centered(ChunkPos::new(3, 0)).is_some());
    }

    /// A waiter whose work was taken by another task stops when the next release wakes it,
    /// instead of taking the window.
    #[test]
    fn reserve_unless_gives_up_when_woken() {
        let gate = Arc::new(ChunkWindowGate::new(1));
        let held = gate
            .try_reserve_centered(ChunkPos::new(0, 0))
            .expect("first window should reserve");
        let gave_up = AtomicBool::new(false);
        let mut waiter = Box::pin(
            gate.reserve_centered_unless(ChunkPos::new(1, 0), || gave_up.load(Ordering::Relaxed)),
        );
        assert!(
            (&mut waiter).now_or_never().is_none(),
            "the waiter should wait while an overlapping window is held"
        );

        gave_up.store(true, Ordering::Relaxed);
        drop(held);

        let reservation = waiter
            .now_or_never()
            .expect("the release should wake the waiter");
        assert!(reservation.is_none(), "the waiter should give up");
        assert!(
            gate.try_reserve_centered(ChunkPos::new(1, 0)).is_some(),
            "a waiter that gave up should not hold the window"
        );
    }

    #[test]
    fn dropping_reservation_releases_window() {
        let gate = Arc::new(ChunkWindowGate::new(2));
        let first = gate
            .try_reserve_centered(ChunkPos::new(0, 0))
            .expect("first window should reserve");

        assert!(gate.try_reserve_centered(ChunkPos::new(1, 0)).is_none());

        drop(first);
        assert!(gate.try_reserve_centered(ChunkPos::new(1, 0)).is_some());
    }
}
