//! slots instead of hash spam for every candidate

use rustc_hash::FxHashMap;

use super::EntityEntry;

/// manager lock held no slot reuse while query borrows it
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(super) struct EntitySlot(usize);

#[derive(Default)]
pub(super) struct EntityEntries {
    by_id: FxHashMap<i32, EntitySlot>,
    slots: Vec<Option<EntityEntry>>,
    free: Vec<EntitySlot>,
}

impl EntityEntries {
    pub(super) fn slot(&self, id: i32) -> Option<EntitySlot> {
        self.by_id.get(&id).copied()
    }

    pub(super) fn entry(&self, slot: EntitySlot) -> &EntityEntry {
        match &self.slots[slot.0] {
            Some(entry) => entry,
            None => unreachable!("spatial index refers to a vacant entity slot"),
        }
    }

    pub(super) fn entry_mut(&mut self, slot: EntitySlot) -> &mut EntityEntry {
        match &mut self.slots[slot.0] {
            Some(entry) => entry,
            None => unreachable!("spatial index refers to a vacant entity slot"),
        }
    }

    pub(super) fn get(&self, id: i32) -> Option<&EntityEntry> {
        self.slot(id).map(|slot| self.entry(slot))
    }

    pub(super) fn get_mut(&mut self, id: i32) -> Option<&mut EntityEntry> {
        let slot = self.slot(id)?;

        Some(self.entry_mut(slot))
    }

    pub(super) fn contains_key(&self, id: i32) -> bool {
        self.by_id.contains_key(&id)
    }

    pub(super) fn len(&self) -> usize {
        self.by_id.len()
    }

    pub(super) fn values(&self) -> impl Iterator<Item = &EntityEntry> {
        self.slots.iter().filter_map(Option::as_ref)
    }

    pub(super) fn insert(&mut self, id: i32, entry: EntityEntry) -> EntitySlot {
        let slot = if let Some(slot) = self.free.pop() {
            self.slots[slot.0] = Some(entry);
            slot
        } else {
            let slot = EntitySlot(self.slots.len());
            self.slots.push(Some(entry));
            slot
        };

        assert!(self.by_id.insert(id, slot).is_none(), "duplicate entity ID");

        slot
    }

    pub(super) fn remove(&mut self, id: i32) -> Option<(EntitySlot, EntityEntry)> {
        let slot = self.by_id.remove(&id)?;

        let Some(entry) = self.slots[slot.0].take() else {
            unreachable!("entity ID refers to a vacant entity slot");
        };

        self.free.push(slot);

        Some((slot, entry))
    }
}
