use std::{
    ptr,
    sync::{Arc, Weak},
};

use steel_utils::locks::SyncMutex;

use super::{DamageHistory, DamageRecord, DamageSource, RecentDamageSource, WeakDamageSource};
use crate::entity::EntityGeneration;
use crate::level_data::GameTime;

/// Serializes history writes with removal and gameplay ownership changes.
#[derive(Default)]
pub(crate) struct DamageHistoryBinding {
    state: SyncMutex<BindingState>,
}

#[derive(Default)]
struct BindingState {
    victim: Option<Arc<DamageHistoryVictim>>,
    owner: Weak<()>,
    removed: bool,
}

/// Owned only by the entity's binding; records observe its lifetime weakly.
pub(super) struct DamageHistoryVictim {
    generation: EntityGeneration,
    history: Weak<DamageHistory>,
}

impl DamageHistoryBinding {
    /// The marker must live in a gameplay owner, never in the entity itself.
    pub(crate) fn retain_owner(&self) -> Arc<()> {
        let mut state = self.state.lock();
        if let Some(owner) = state.owner.upgrade() {
            return owner;
        }
        let owner = Arc::new(());
        state.owner = Arc::downgrade(&owner);
        owner
    }

    pub(super) fn record(
        &self,
        history: &Arc<DamageHistory>,
        generation: EntityGeneration,
        source: &DamageSource,
        clock: &Arc<GameTime>,
    ) {
        let descriptor = WeakDamageSource::new(source);
        let replaced = {
            let mut state = self.state.lock();
            let victim_lifetime = match &state.victim {
                Some(bound) if ptr::eq(bound.history.as_ptr(), Arc::as_ptr(history)) => {
                    Arc::downgrade(bound)
                }
                _ => {
                    let bound = Arc::new(DamageHistoryVictim {
                        generation,
                        history: Arc::downgrade(history),
                    });
                    let lifetime = Arc::downgrade(&bound);
                    state.victim = Some(bound);
                    lifetime
                }
            };
            let record = DamageRecord {
                victim_lifetime,
                owner: state.owner.clone(),
                retained: (!state.removed && state.owner.strong_count() > 0)
                    .then(|| source.clone()),
                source: descriptor,
                timestamp: clock.ticks(),
                clock: Arc::clone(clock),
            };
            history.records.lock().insert(generation, record)
        };
        // Dropping sources can destroy entities; release both locks first.
        drop(replaced);
    }

    /// Both the source and the upgraded history can own entities. Drop them outside
    /// the entity lifecycle lock, including when this is the last history handle.
    pub(crate) fn set_removed(
        &self,
        removed: bool,
    ) -> Option<(Arc<DamageHistory>, Option<DamageSource>)> {
        let mut state = self.state.lock();
        state.removed = removed;
        if !removed {
            return None;
        }
        let victim = state.victim.as_ref()?;
        let history = victim.history.upgrade()?;
        let released = history
            .records
            .lock()
            .get_mut(&victim.generation)
            .and_then(|record| record.retained.take());
        Some((history, released))
    }

    pub(crate) fn last_damage_source(&self) -> Option<RecentDamageSource> {
        let (history, victim) = self.bound_history()?;
        history.last_damage_source(victim)
    }

    /// Clears transient history when a target domain's state is restored.
    pub(crate) fn clear(&self) {
        if let Some((history, victim)) = self.bound_history() {
            history.clear_victim(victim);
        }
    }

    fn bound_history(&self) -> Option<(Arc<DamageHistory>, EntityGeneration)> {
        let state = self.state.lock();
        let bound = state.victim.as_ref()?;
        Some((bound.history.upgrade()?, bound.generation))
    }
}
