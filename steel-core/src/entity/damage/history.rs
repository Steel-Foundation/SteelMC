use std::mem;
use std::sync::{Arc, Weak};

use rustc_hash::FxHashMap;
use steel_utils::locks::SyncMutex;

use crate::entity::{EntityBase, EntityGeneration};
use crate::level_data::GameTime;

use super::DamageSource;

mod binding;
mod collection;

pub(crate) use binding::DamageHistoryBinding;
use binding::DamageHistoryVictim;

/// Maximum retained age from vanilla `LivingEntity.getLastDamageSource`.
const MAX_DAMAGE_SOURCE_AGE_TICKS: i64 = 40;

struct DamageRecord {
    victim_lifetime: Weak<DamageHistoryVictim>,
    source: DamageSource,
    timestamp: i64,
    clock: Arc<GameTime>,
}

impl DamageRecord {
    fn is_expired(&self) -> bool {
        self.clock.ticks().wrapping_sub(self.timestamp) > MAX_DAMAGE_SOURCE_AGE_TICKS
    }
}

/// Server-owned equivalent of vanilla's per-entity `lastDamageSource` fields.
///
/// Entities and worlds link back weakly. Records retain the victim's clock,
/// which has no world back-reference, to avoid cycles through the victim or world.
#[derive(Default)]
pub struct DamageHistory {
    records: SyncMutex<FxHashMap<EntityGeneration, DamageRecord>>,
}

impl DamageHistory {
    pub(crate) fn record(
        self: &Arc<Self>,
        victim: &EntityBase,
        source: &DamageSource,
        clock: &Arc<GameTime>,
    ) {
        let generation = victim.generation();
        let record = DamageRecord {
            victim_lifetime: victim.damage_history().bind(self, generation),
            source: source.clone(),
            timestamp: clock.ticks(),
            clock: Arc::clone(clock),
        };
        let replaced = self.records.lock().insert(generation, record);
        // Entity destructors must run outside the history lock.
        drop(replaced);
    }

    pub(crate) fn last_damage_source(&self, victim: EntityGeneration) -> Option<DamageSource> {
        let (source, expired) = {
            let mut records = self.records.lock();
            if records.get(&victim).is_some_and(DamageRecord::is_expired) {
                (None, records.remove(&victim))
            } else {
                (
                    records.get(&victim).map(|record| record.source.clone()),
                    None,
                )
            }
        };
        drop(expired);
        source
    }

    pub(crate) fn clear_victim(&self, victim: EntityGeneration) {
        let removed = self.records.lock().remove(&victim);
        drop(removed);
    }

    /// Releases expired sources even when nobody calls the entity's getter.
    pub(crate) fn expire(&self) {
        let expired: Vec<_> = self
            .records
            .lock()
            .extract_if(|_, record| record.is_expired())
            .map(|(_, record)| record)
            .collect();
        drop(expired);
    }

    /// Releases history ownership after gameplay work has stopped.
    pub(crate) fn clear(&self) {
        let records = mem::take(&mut *self.records.lock());
        drop(records);
    }
}
