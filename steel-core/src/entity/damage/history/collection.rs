use rustc_hash::{FxHashMap, FxHashSet};

use super::{DamageHistory, DamageRecord};
use crate::entity::reference::EntityCollection;
use crate::entity::{EntityGeneration, SharedEntity};

struct HistoryEntity<'a> {
    entity: &'a SharedEntity,
    history_ref_count: usize,
}

impl HistoryEntity<'_> {
    fn has_independent_owner(&self, collection: &EntityCollection) -> bool {
        collection.strong_count(self.entity) > self.history_ref_count
    }
}

impl DamageHistory {
    /// Releases history unreachable from independently owned entities.
    ///
    /// The entity collection gate stops weak promotions while the history lock
    /// stops source getters from creating independent owners. Existing owners
    /// may clone or drop their references; drops can only delay collection.
    /// Retirement prevents promotion after unlocking but before destructors run.
    pub(crate) fn collect_unreachable(&self) {
        let unreachable: Vec<_> = EntityCollection::run(|collection| {
            let mut records = self.records.lock();
            let entities = Self::retained_entities(&records);
            let reachable = Self::reachable_entities(&records, &entities, collection);
            for (generation, retained) in &entities {
                if !reachable.contains(generation) && !retained.has_independent_owner(collection) {
                    collection.retire(retained.entity);
                }
            }
            drop(entities);
            records
                .extract_if(|victim, record| {
                    record.victim_lifetime.strong_count() == 0 || !reachable.contains(victim)
                })
                .map(|(_, record)| record)
                .collect()
        });
        // Destructors may access history or promote other entity references.
        drop(unreachable);
    }

    fn retained_entities(
        records: &FxHashMap<EntityGeneration, DamageRecord>,
    ) -> FxHashMap<EntityGeneration, HistoryEntity<'_>> {
        let mut entities: FxHashMap<EntityGeneration, HistoryEntity<'_>> = FxHashMap::default();
        for record in records.values() {
            for (generation, entity) in record.source.retained_entities() {
                let retained = entities.entry(generation).or_insert(HistoryEntity {
                    entity,
                    history_ref_count: 0,
                });
                retained.history_ref_count += 1;
            }
        }
        entities
    }

    fn reachable_entities(
        records: &FxHashMap<EntityGeneration, DamageRecord>,
        entities: &FxHashMap<EntityGeneration, HistoryEntity<'_>>,
        collection: &EntityCollection,
    ) -> FxHashSet<EntityGeneration> {
        let mut pending = Vec::new();
        for (&victim, record) in records {
            if record.victim_lifetime.strong_count() == 0 {
                continue;
            }
            // Borrow the stored handles: collector-owned clones would look like roots.
            if entities
                .get(&victim)
                .is_none_or(|retained| retained.has_independent_owner(collection))
            {
                pending.push(victim);
            }
        }

        let mut reachable = FxHashSet::default();
        while let Some(entity) = pending.pop() {
            if !reachable.insert(entity) {
                continue;
            }
            let Some(record) = records.get(&entity) else {
                continue;
            };
            if record.victim_lifetime.strong_count() == 0 {
                continue;
            }
            pending.extend(
                record
                    .source
                    .retained_entities()
                    .map(|(generation, _)| generation),
            );
        }
        reachable
    }
}
