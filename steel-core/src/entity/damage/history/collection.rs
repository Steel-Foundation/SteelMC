use rustc_hash::{FxHashMap, FxHashSet};

use super::{DamageHistory, DamageRecord};
use crate::entity::reference::{EntityCollection, OwnedReferences};
use crate::entity::{Entity, EntityGeneration, EntityWeak, SharedEntity};

enum RetainedReference<'a> {
    History(&'a SharedEntity),
    Field(&'a EntityWeak<dyn Entity>),
}

struct RetainedEntity<'a> {
    reference: RetainedReference<'a>,
    internal_count: usize,
}

impl RetainedEntity<'_> {
    fn strong_count(&self, collection: &EntityCollection) -> usize {
        match self.reference {
            RetainedReference::History(entity) => collection.strong_count(entity),
            RetainedReference::Field(entity) => entity.strong_count(),
        }
    }
}

impl DamageHistory {
    /// Releases history unreachable from independent owners through either
    /// damage sources or entity-owned fields. Weak promotion and changes in
    /// field ownership are stopped while counts and reachability are inspected.
    pub(crate) fn collect_unreachable(&self) {
        let (unreachable, retired) = EntityCollection::run(|collection| {
            let mut records = self.records.lock();
            if records.is_empty() {
                return (Vec::new(), Vec::new());
            }
            let owned = collection.owned_references();
            let entities = Self::retained_entities(&records, &owned);
            let mut roots = Vec::new();
            for (&generation, retained) in &entities {
                let count = retained.strong_count(collection);
                if count > retained.internal_count
                    || (count == 0 && owned.contains_key(&generation))
                {
                    // Keep outgoing references rooted while their owner's destructor runs.
                    roots.push(generation);
                }
            }
            roots.extend(
                owned
                    .keys()
                    .filter(|owner| !entities.contains_key(owner))
                    .copied(),
            );
            roots.extend(records.iter().filter_map(|(&victim, record)| {
                (record.victim_lifetime.strong_count() != 0 && !entities.contains_key(&victim))
                    .then_some(victim)
            }));
            let reachable = Self::follow_references(&records, &owned, roots);
            let history_sources = records
                .values()
                .flat_map(|record| {
                    record
                        .source
                        .retained_entities()
                        .map(|(generation, _)| generation)
                })
                .collect();
            let history_retained = Self::follow_references(&records, &owned, history_sources);
            let mut retired = Vec::new();
            for (generation, retained) in &entities {
                if reachable.contains(generation) || !history_retained.contains(generation) {
                    continue;
                }
                match retained.reference {
                    RetainedReference::History(entity) => collection.retire(entity),
                    RetainedReference::Field(entity) => {
                        if let Some(entity) = collection.retire_weak(entity) {
                            retired.push(entity);
                        }
                    }
                }
            }
            drop(entities);
            let unreachable = records
                .extract_if(|victim, record| {
                    record.victim_lifetime.strong_count() == 0 || !reachable.contains(victim)
                })
                .map(|(_, record)| record)
                .collect::<Vec<_>>();
            (unreachable, retired)
        });
        // Destructors may access history, change fields or promote references.
        drop((unreachable, retired));
    }

    fn retained_entities<'a>(
        records: &'a FxHashMap<EntityGeneration, DamageRecord>,
        owned: &'a OwnedReferences,
    ) -> FxHashMap<EntityGeneration, RetainedEntity<'a>> {
        let mut entities = FxHashMap::default();
        for record in records.values() {
            for (generation, entity) in record.source.retained_entities() {
                let retained = entities.entry(generation).or_insert(RetainedEntity {
                    reference: RetainedReference::History(entity),
                    internal_count: 0,
                });
                retained.internal_count += 1;
            }
        }
        for targets in owned.values() {
            for (&generation, target) in targets {
                let retained = entities.entry(generation).or_insert(RetainedEntity {
                    reference: RetainedReference::Field(&target.entity),
                    internal_count: 0,
                });
                retained.internal_count += target.count;
            }
        }
        entities
    }

    fn follow_references(
        records: &FxHashMap<EntityGeneration, DamageRecord>,
        owned: &OwnedReferences,
        mut pending: Vec<EntityGeneration>,
    ) -> FxHashSet<EntityGeneration> {
        let mut reachable = FxHashSet::default();
        while let Some(entity) = pending.pop() {
            if !reachable.insert(entity) {
                continue;
            }
            if let Some(targets) = owned.get(&entity) {
                pending.extend(targets.keys().copied());
            }
            if let Some(record) = records.get(&entity)
                && record.victim_lifetime.strong_count() != 0
            {
                pending.extend(
                    record
                        .source
                        .retained_entities()
                        .map(|(generation, _)| generation),
                );
            }
        }
        reachable
    }
}
