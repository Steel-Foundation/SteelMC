//! Scheduling gate for light work cache windows.

use std::sync::Arc;

use steel_utils::{ChunkPos, locks::SyncMutex};
use tokio::sync::Notify;

use super::LIGHT_CACHE_RADIUS;

const LIGHT_WORK_CENTER_EXCLUSION_RADIUS: i32 = LIGHT_CACHE_RADIUS * 2;

/// Shared gate used to exclude overlapping light-engine cache windows.
///
/// Light worksets can write light data across a full 5x5 cache window. Two
/// worksets whose windows overlap must therefore not run at the same time:
/// chunk-light status work must publish `ChunkStatus::Light` before overlapping
/// work builds its light cache, and queued light updates must not interleave
/// with either operation.
#[derive(Debug)]
pub(crate) struct LightWorkWindowGate {
    active_centers: SyncMutex<Vec<ChunkPos>>,
    released: Notify,
}

/// Reservation for one light-engine cache window.
#[derive(Debug)]
pub(crate) struct LightWorkWindowReservation {
    gate: Arc<LightWorkWindowGate>,
    center: ChunkPos,
}

impl LightWorkWindowGate {
    /// Creates an empty light work gate.
    #[must_use]
    pub(crate) fn new() -> Self {
        Self {
            active_centers: SyncMutex::new(Vec::new()),
            released: Notify::new(),
        }
    }

    /// Reserves the radius-2 light cache window centered on `center`.
    pub(crate) async fn reserve_centered(
        self: &Arc<Self>,
        center: ChunkPos,
    ) -> LightWorkWindowReservation {
        loop {
            let released = self.released.notified();
            if let Some(reservation) = self.try_reserve_centered(center) {
                return reservation;
            }
            released.await;
        }
    }

    /// Attempts to reserve the radius-2 light cache window centered on `center`.
    pub(crate) fn try_reserve_centered(
        self: &Arc<Self>,
        center: ChunkPos,
    ) -> Option<LightWorkWindowReservation> {
        let mut active_centers = self.active_centers.lock();
        if active_centers
            .iter()
            .any(|&active| Self::windows_overlap(center, active))
        {
            return None;
        }

        active_centers.push(center);
        Some(LightWorkWindowReservation {
            gate: Arc::clone(self),
            center,
        })
    }

    const fn windows_overlap(left: ChunkPos, right: ChunkPos) -> bool {
        let dx = left.0.x.abs_diff(right.0.x);
        let dz = left.0.y.abs_diff(right.0.y);
        dx <= LIGHT_WORK_CENTER_EXCLUSION_RADIUS as u32
            && dz <= LIGHT_WORK_CENTER_EXCLUSION_RADIUS as u32
    }
}

impl Default for LightWorkWindowGate {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for LightWorkWindowReservation {
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

        debug_assert!(removed, "light work reservation missing active center");
        if removed {
            self.gate.released.notify_waiters();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn overlapping_windows_cannot_be_reserved_together() {
        let gate = Arc::new(LightWorkWindowGate::new());
        let _first = gate
            .try_reserve_centered(ChunkPos::new(0, 0))
            .expect("first light window should reserve");

        assert!(gate.try_reserve_centered(ChunkPos::new(1, 0)).is_none());
        assert!(gate.try_reserve_centered(ChunkPos::new(4, 4)).is_none());
    }

    #[test]
    fn non_overlapping_windows_can_be_reserved_together() {
        let gate = Arc::new(LightWorkWindowGate::new());
        let _first = gate
            .try_reserve_centered(ChunkPos::new(0, 0))
            .expect("first light window should reserve");

        assert!(gate.try_reserve_centered(ChunkPos::new(5, 0)).is_some());
        assert!(gate.try_reserve_centered(ChunkPos::new(0, 5)).is_some());
    }

    #[test]
    fn dropping_reservation_releases_window() {
        let gate = Arc::new(LightWorkWindowGate::new());
        let first = gate
            .try_reserve_centered(ChunkPos::new(0, 0))
            .expect("first light window should reserve");

        assert!(gate.try_reserve_centered(ChunkPos::new(1, 0)).is_none());

        drop(first);
        assert!(gate.try_reserve_centered(ChunkPos::new(1, 0)).is_some());
    }
}
