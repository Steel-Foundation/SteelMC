//! One live simulation counter per domain; persistence belongs to its configured default world.
//! Domains with persistent worlds require a primary that persists level data.
//! Legacy absolute timestamps are not rebased when derived worlds adopt the primary clock.
use std::sync::{
    Arc,
    atomic::{AtomicI64, Ordering},
};

/// Shared game time. World workers only read this counter.
#[derive(Debug)]
pub struct GameTime {
    ticks: AtomicI64,
}

impl GameTime {
    pub(super) const fn new(ticks: i64) -> Self {
        Self {
            ticks: AtomicI64::new(ticks),
        }
    }

    /// Returns the domain's current simulation time.
    pub fn ticks(&self) -> i64 {
        self.ticks.load(Ordering::Relaxed)
    }

    /// Advances the counter once; server coordination must finish all increments before dispatching workers.
    pub(super) fn advance(&self) {
        self.ticks.fetch_add(1, Ordering::Relaxed);
    }
}

/// Explicit construction authority, independent of serialized field presence.
#[derive(Clone)]
pub enum GameTimeSource {
    /// Initialize from this world's save, or zero for a new world.
    Primary,
    /// Ignore this world's obsolete saved time and use the primary's counter.
    Derived(Arc<GameTime>),
}
