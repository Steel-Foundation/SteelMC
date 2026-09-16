use std::mem;
use std::sync::Arc;

use rustc_hash::FxHashMap;
use steel_utils::locks::SyncMutex;

use crate::entity::EntityGeneration;
use crate::level_data::GameTime;

use super::DamageSource;

/// Maximum retained age from vanilla `LivingEntity.getLastDamageSource`.
const MAX_DAMAGE_SOURCE_AGE_TICKS: i64 = 40;

struct DamageRecord {
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
/// Weak links from entities and worlds prevent self/mutual damage cycles.
/// Expiry retains the victim's clock, which has no world back-reference,
/// instead of retaining the victim to read its game time.
#[derive(Default)]
pub struct DamageHistory {
    records: SyncMutex<FxHashMap<EntityGeneration, DamageRecord>>,
}

impl DamageHistory {
    pub(crate) fn record(
        &self,
        victim: EntityGeneration,
        source: &DamageSource,
        clock: &Arc<GameTime>,
    ) {
        let record = DamageRecord {
            source: source.clone(),
            timestamp: clock.ticks(),
            clock: Arc::clone(clock),
        };
        let replaced = self.records.lock().insert(victim, record);
        // Entity destructors must run outside the history lock.
        drop(replaced);
    }

    /// Returns the latest unexpired source, independently retaining its entities.
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
