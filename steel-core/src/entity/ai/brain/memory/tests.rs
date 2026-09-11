use std::ptr;
use std::sync::Arc;

use glam::DVec3;
use steel_registry::vanilla_entities;
use steel_utils::{DowncastType, DowncastTypeKey, Identifier};

use super::{
    Memories, MemoryModuleType, MemoryModuleTypeRegistry, MemoryStatus, MemoryValue,
    RememberedEntities, RememberedEntity,
};
use crate::bootstrap::init_globals_once;
use crate::entity::registry::ENTITIES;
use crate::entity::{SharedEntity, next_entity_id};
use crate::test_support::test_world;

#[derive(Debug, PartialEq)]
struct Ticks(i32);

// SAFETY: This test-only key is distinct and identifies `Ticks` within the test
// process.
unsafe impl DowncastType for Ticks {
    const TYPE_KEY: DowncastTypeKey = DowncastTypeKey::new("steel:test/memory_value/ticks");
}

impl MemoryValue for Ticks {}

const fn test_memory<V: MemoryValue + DowncastType>(path: &'static str) -> MemoryModuleType<V> {
    MemoryModuleType::new(Identifier::new_static("steel_test", path))
}

static COOLDOWN: MemoryModuleType<Ticks> = test_memory("cooldown");
static DUPLICATE_COOLDOWN: MemoryModuleType<Ticks> = test_memory("cooldown");
static OTHER_COOLDOWN: MemoryModuleType<Ticks> = test_memory("other_cooldown");
static NEARBY: MemoryModuleType<RememberedEntities> = test_memory("nearby");
static TARGET: MemoryModuleType<RememberedEntity> = test_memory("target");

fn spawn_entity() -> SharedEntity {
    init_globals_once();
    let world = test_world();
    ENTITIES
        .create(
            &vanilla_entities::PIG,
            next_entity_id(),
            DVec3::ZERO,
            Arc::downgrade(world),
        )
        .expect("pig should have a registered entity factory")
}

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
#[should_panic(expected = "duplicate memory module type key")]
fn duplicate_memory_keys_are_rejected() {
    let mut registry = MemoryModuleTypeRegistry::new();
    registry.register(&COOLDOWN);
    registry.register(&DUPLICATE_COOLDOWN);
}

#[test]
#[should_panic(expected = "after the registry has been frozen")]
fn frozen_memory_registry_rejects_registration() {
    let mut registry = MemoryModuleTypeRegistry::new();
    registry.freeze();
    registry.register(&COOLDOWN);
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
fn expired_memory_is_writable_again() {
    let mut memories = Memories::new();
    memories.register(COOLDOWN.entry());
    memories.set_with_expiry(&COOLDOWN, Ticks(5), 0);
    memories.forget_outdated();
    assert!(!memories.has_value(COOLDOWN.entry()));

    memories.set(&COOLDOWN, Ticks(7));
    for _ in 0..8 {
        memories.forget_outdated();
    }

    assert_eq!(memories.get(&COOLDOWN), Some(&Ticks(7)));
}

#[test]
fn writing_an_empty_collection_clears_the_slot() {
    let entity = spawn_entity();
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
    let entity = spawn_entity();
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
    let kept = spawn_entity();
    let removed = spawn_entity();
    let mut memories = Memories::new();
    memories.register(NEARBY.entry());
    memories.set(&NEARBY, [&kept, &removed].into_iter().collect());

    drop(removed);

    let remembered = memories.get(&NEARBY).expect("memory should hold a value");
    assert_eq!(remembered.len(), 2);
    let resolved: Vec<_> = remembered.resolve().map(|entity| entity.id()).collect();
    assert_eq!(resolved, vec![kept.id()]);
}
