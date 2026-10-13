//! Per-mob memory slots.

use std::fmt::{self, Debug, Formatter};
use std::hash::{Hash, Hasher};
use std::ptr;

use rustc_hash::FxHashMap;
use steel_utils::{Downcast as _, DowncastType};

use super::{MemoryModuleType, MemoryModuleTypeRef, MemoryStatus, MemoryValue};

/// pinned TTL Sentinel to never expire.
const NEVER_EXPIRE: i64 = i64::MAX;

/// Identifies a memory by the address of its registry entry.
#[derive(Clone, Copy)]
struct MemoryKey(MemoryModuleTypeRef);

impl PartialEq for MemoryKey {
    fn eq(&self, other: &Self) -> bool {
        ptr::eq(self.0, other.0)
    }
}

impl Eq for MemoryKey {}

impl Hash for MemoryKey {
    fn hash<H: Hasher>(&self, state: &mut H) {
        ptr::from_ref(self.0).addr().hash(state);
    }
}

/// One memory slot and its remaining lifetime.
struct MemorySlot {
    value: Option<Box<dyn MemoryValue>>,
    time_to_live: i64,
}

impl MemorySlot {
    const fn empty() -> Self {
        Self {
            value: None,
            time_to_live: NEVER_EXPIRE,
        }
    }

    fn set(&mut self, value: Box<dyn MemoryValue>, time_to_live: i64) {
        self.value = Some(value);
        self.time_to_live = time_to_live;
    }

    fn clear(&mut self) {
        self.value = None;
        self.time_to_live = NEVER_EXPIRE;
    }

    const fn has_value(&self) -> bool {
        self.value.is_some()
    }

    const fn can_expire(&self) -> bool {
        self.time_to_live != NEVER_EXPIRE
    }

    const fn has_expired(&self) -> bool {
        self.time_to_live <= 0
    }

    /// Ages the slot by one tick.
    fn tick(&mut self) {
        if !self.has_value() || !self.can_expire() {
            return;
        }

        if self.has_expired() {
            self.clear();
        } else {
            self.time_to_live -= 1;
        }
    }
}

/// The memories one mob's brain holds.
#[derive(Default)]
pub struct Memories {
    slots: FxHashMap<MemoryKey, MemorySlot>,
}

impl Memories {
    /// Creates a brain with no memories registered.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Registers `memory`, leaving an already-registered slot untouched.
    pub fn register(&mut self, memory: MemoryModuleTypeRef) {
        self.slots
            .entry(MemoryKey(memory))
            .or_insert(MemorySlot::empty());
    }

    /// Returns whether `memory` is registered on this mob.
    #[must_use]
    pub fn is_registered(&self, memory: MemoryModuleTypeRef) -> bool {
        self.slots.contains_key(&MemoryKey(memory))
    }

    /// Returns whether `memory` satisfies `status`.
    #[must_use]
    pub fn check(&self, memory: MemoryModuleTypeRef, status: MemoryStatus) -> bool {
        let Some(slot) = self.slots.get(&MemoryKey(memory)) else {
            return false;
        };

        match status {
            MemoryStatus::Registered => true,
            MemoryStatus::ValuePresent => slot.has_value(),
            MemoryStatus::ValueAbsent => !slot.has_value(),
        }
    }

    /// Returns whether `memory` currently holds a value.
    #[must_use]
    pub fn has_value(&self, memory: MemoryModuleTypeRef) -> bool {
        self.check(memory, MemoryStatus::ValuePresent)
    }

    /// Returns the value held by `memory`.
    #[must_use]
    pub fn get<V: MemoryValue + DowncastType>(
        &self,
        memory: &'static MemoryModuleType<V>,
    ) -> Option<&V> {
        let slot = self.slots.get(&MemoryKey(memory.entry()));
        debug_assert!(
            slot.is_some(),
            "read of memory {} which is not registered on this mob",
            memory.key()
        );
        slot?.value.as_deref()?.downcast_ref::<V>()
    }

    /// Stores `value` in `memory` permanently.
    pub fn set<V: MemoryValue + DowncastType>(
        &mut self,
        memory: &'static MemoryModuleType<V>,
        value: V,
    ) {
        self.set_internal(memory.entry(), Some(Box::new(value)), NEVER_EXPIRE);
    }

    /// Stores `value` in `memory` for `time_to_live` further ticks.
    pub fn set_with_expiry<V: MemoryValue + DowncastType>(
        &mut self,
        memory: &'static MemoryModuleType<V>,
        value: V,
        time_to_live: i64,
    ) {
        self.set_internal(memory.entry(), Some(Box::new(value)), time_to_live);
    }

    /// Stores `value` in `memory`, clearing the slot when it is `None`.
    pub fn set_optional<V: MemoryValue + DowncastType>(
        &mut self,
        memory: &'static MemoryModuleType<V>,
        value: Option<V>,
    ) {
        let value = value.map(|value| Box::new(value) as Box<dyn MemoryValue>);
        self.set_internal(memory.entry(), value, NEVER_EXPIRE);
    }

    /// Clears `memory`.
    pub fn erase(&mut self, memory: MemoryModuleTypeRef) {
        if let Some(slot) = self.slots.get_mut(&MemoryKey(memory)) {
            slot.clear();
        }
    }

    /// Ages every memory by one tick, clearing those that have expired.
    pub fn forget_outdated(&mut self) {
        for slot in self.slots.values_mut() {
            slot.tick();
        }
    }

    /// Writes a value into a registered slot, applying vanilla's two write rules.
    fn set_internal(
        &mut self,
        memory: MemoryModuleTypeRef,
        value: Option<Box<dyn MemoryValue>>,
        time_to_live: i64,
    ) {
        let Some(slot) = self.slots.get_mut(&MemoryKey(memory)) else {
            return;
        };

        match value.filter(|value| !value.is_empty_collection()) {
            Some(value) => slot.set(value, time_to_live),
            None => slot.clear(),
        }
    }
}

impl Debug for Memories {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        let mut map = formatter.debug_map();
        for (key, slot) in &self.slots {
            map.entry(&key.0.key, &slot.value);
        }
        map.finish()
    }
}
