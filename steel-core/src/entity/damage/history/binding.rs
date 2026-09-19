use std::{
    ptr,
    sync::{Arc, Weak},
};

use steel_utils::locks::SyncMutex;

use super::{DamageHistory, DamageSource};
use crate::entity::EntityGeneration;

/// An entity's lazy history binding, retained after removal from its world.
#[derive(Default)]
pub(crate) struct DamageHistoryBinding {
    victim: SyncMutex<Option<Arc<DamageHistoryVictim>>>,
}

/// Owned only by the entity's binding; records observe its lifetime weakly.
pub(super) struct DamageHistoryVictim {
    generation: EntityGeneration,
    history: Weak<DamageHistory>,
}

impl DamageHistoryBinding {
    pub(super) fn bind(
        &self,
        history: &Arc<DamageHistory>,
        generation: EntityGeneration,
    ) -> Weak<DamageHistoryVictim> {
        let mut victim = self.victim.lock();
        if let Some(bound) = &*victim
            && ptr::eq(bound.history.as_ptr(), Arc::as_ptr(history))
        {
            return Arc::downgrade(bound);
        }

        let bound = Arc::new(DamageHistoryVictim {
            generation,
            history: Arc::downgrade(history),
        });
        let lifetime = Arc::downgrade(&bound);
        *victim = Some(bound);
        lifetime
    }

    pub(crate) fn last_damage_source(&self) -> Option<DamageSource> {
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
        let victim = self.victim.lock();
        let bound = victim.as_ref()?;
        Some((bound.history.upgrade()?, bound.generation))
    }
}
