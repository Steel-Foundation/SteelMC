use std::mem;
use std::sync::{Arc, Weak};

use rustc_hash::FxHashMap;
use steel_utils::locks::SyncMutex;

use crate::entity::{EntityBase, EntityGeneration};
use crate::level_data::GameTime;

use super::DamageSource;

mod binding;
mod source;

pub(crate) use binding::DamageHistoryBinding;
use binding::DamageHistoryVictim;
pub use source::RecentDamageSource;
use source::WeakDamageSource;

/// Maximum retained age from vanilla `LivingEntity.getLastDamageSource`.
const MAX_DAMAGE_SOURCE_AGE_TICKS: i64 = 40;

struct DamageRecord {
    victim_lifetime: Weak<DamageHistoryVictim>,
    owner: Weak<()>,
    source: WeakDamageSource,
    retained: Option<DamageSource>,
    timestamp: i64,
    clock: Arc<GameTime>,
}

impl DamageRecord {
    fn is_expired(&self) -> bool {
        self.victim_lifetime.strong_count() == 0
            || self.clock.ticks().wrapping_sub(self.timestamp) > MAX_DAMAGE_SOURCE_AGE_TICKS
    }
}

/// Server-owned equivalent of vanilla's per-entity `lastDamageSource` fields.
///
/// Entities and worlds link back weakly. Records retain the victim's clock,
/// which has no world back-reference, to avoid cycles through the victim or world.
/// Only gameplay-owned victims retain strong sources. Removal downgrades their
/// own record; an ownership sweep also covers disconnects and manager teardown.
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
        victim
            .damage_history()
            .record(self, victim.generation(), source, clock);
    }

    pub(crate) fn last_damage_source(
        &self,
        victim: EntityGeneration,
    ) -> Option<RecentDamageSource> {
        let (source, expired) = {
            let mut records = self.records.lock();
            if records.get(&victim).is_some_and(DamageRecord::is_expired) {
                (None, records.remove(&victim))
            } else {
                (
                    records.get(&victim).map(|record| record.source.resolve()),
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

    /// Releases expired records and strong sources with no gameplay owner.
    /// Runs while frozen too; expiry still uses the unchanged gameplay clock.
    pub(crate) fn expire(&self) {
        let (expired, released) = {
            let mut records = self.records.lock();
            let expired: Vec<_> = records
                .extract_if(|_, record| record.is_expired())
                .map(|(_, record)| record)
                .collect();
            let released: Vec<_> = records
                .values_mut()
                .filter(|record| record.owner.strong_count() == 0)
                .filter_map(|record| record.retained.take())
                .collect();
            (expired, released)
        };
        drop((expired, released));
    }

    /// Releases history ownership after gameplay work has stopped.
    pub(crate) fn clear(&self) {
        let records = mem::take(&mut *self.records.lock());
        drop(records);
    }
}
