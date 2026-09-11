use std::ptr;

use steel_utils::DowncastType as _;

use super::{
    Memories, MemoryModuleType, MemoryModuleTypeRegistry, MemoryStatus, RememberedEntities,
    RememberedEntity,
};
use crate::entity::ai::brain::test_support::{Ticks, spawn_pig, test_memory};

static COOLDOWN: MemoryModuleType<Ticks> = test_memory("cooldown");
static DUPLICATE_COOLDOWN: MemoryModuleType<Ticks> = test_memory("cooldown");
static OTHER_COOLDOWN: MemoryModuleType<Ticks> = test_memory("other_cooldown");
static NEARBY: MemoryModuleType<RememberedEntities> = test_memory("nearby");
static TARGET: MemoryModuleType<RememberedEntity> = test_memory("target");

#[test]
fn registered_memory_is_recoverable_by_key() {
    let mut registry = MemoryModuleTypeRegistry::new();
    registry.register(&COOLDOWN);
    registry.register(&OTHER_COOLDOWN);
    registry.freeze();

    let found = registry
        .by_key(COOLDOWN.key())
        .expect("registered memory should resolve by key");
    assert!(ptr::eq(found, COOLDOWN.entry()));
    assert_eq!(found.value_type_key(), Ticks::TYPE_KEY);
    assert!(registry.by_key(NEARBY.key()).is_none());
}

#[test]
fn unregistered_memory_is_inert() {
    let mut memories = Memories::new();
    memories.set(&COOLDOWN, Ticks(5));

    assert!(!memories.is_registered(COOLDOWN.entry()));
    assert!(!memories.has_value(COOLDOWN.entry()));
    for status in [
        MemoryStatus::ValuePresent,
        MemoryStatus::ValueAbsent,
        MemoryStatus::Registered,
    ] {
        assert!(
            !memories.check(COOLDOWN.entry(), status),
            "unregistered memory must not satisfy {status:?}"
        );
    }
}

#[test]
fn registering_an_existing_memory_keeps_its_value() {
    let mut memories = Memories::new();
    memories.register(COOLDOWN.entry());
    memories.set(&COOLDOWN, Ticks(5));

    memories.register(COOLDOWN.entry());

    assert_eq!(memories.get(&COOLDOWN), Some(&Ticks(5)));
}

#[test]
fn expiring_memory_survives_its_full_time_to_live() {
    let mut memories = Memories::new();
    memories.register(COOLDOWN.entry());
    memories.set_with_expiry(&COOLDOWN, Ticks(5), 2);

    for tick in 1..=2 {
        memories.forget_outdated();
        assert_eq!(
            memories.get(&COOLDOWN),
            Some(&Ticks(5)),
            "memory should still be readable on tick {tick} of its 2 tick life"
        );
    }

    memories.forget_outdated();
    assert_eq!(memories.get(&COOLDOWN), None);
}

#[test]
fn permanent_memory_never_expires() {
    let mut memories = Memories::new();
    memories.register(COOLDOWN.entry());
    memories.set(&COOLDOWN, Ticks(5));

    for _ in 0..64 {
        memories.forget_outdated();
    }

    assert_eq!(memories.get(&COOLDOWN), Some(&Ticks(5)));
}

#[test]
fn writing_an_empty_collection_clears_the_slot() {
    let entity = spawn_pig();
    let mut memories = Memories::new();
    memories.register(NEARBY.entry());

    memories.set(&NEARBY, [&entity].into_iter().collect());
    assert!(memories.has_value(NEARBY.entry()));

    memories.set(&NEARBY, RememberedEntities::default());
    assert!(!memories.has_value(NEARBY.entry()));
    assert!(memories.check(NEARBY.entry(), MemoryStatus::ValueAbsent));
}

#[test]
fn setting_none_clears_the_slot() {
    let mut memories = Memories::new();
    memories.register(COOLDOWN.entry());
    memories.set(&COOLDOWN, Ticks(5));

    memories.set_optional(&COOLDOWN, None);

    assert!(!memories.has_value(COOLDOWN.entry()));
}

#[test]
fn remembered_entity_does_not_keep_the_entity_alive() {
    let entity = spawn_pig();
    let mut memories = Memories::new();
    memories.register(TARGET.entry());
    memories.set(&TARGET, RememberedEntity::new(&entity));

    assert!(
        memories
            .get(&TARGET)
            .and_then(RememberedEntity::resolve)
            .is_some()
    );

    drop(entity);

    assert!(
        memories
            .get(&TARGET)
            .and_then(RememberedEntity::resolve)
            .is_none(),
        "a memory must not keep a removed entity alive"
    );
}

#[test]
fn remembered_entities_skip_entities_that_left_the_world() {
    let kept = spawn_pig();
    let removed = spawn_pig();
    let mut memories = Memories::new();
    memories.register(NEARBY.entry());
    memories.set(&NEARBY, [&kept, &removed].into_iter().collect());

    drop(removed);

    let remembered = memories.get(&NEARBY).expect("memory should hold a value");
    assert_eq!(remembered.len(), 2);
    let resolved: Vec<_> = remembered.resolve().map(|entity| entity.id()).collect();
    assert_eq!(resolved, vec![kept.id()]);
}
