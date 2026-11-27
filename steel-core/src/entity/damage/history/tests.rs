use std::sync::{
    Arc, Weak,
    atomic::{AtomicBool, Ordering},
};

use glam::DVec3;
use steel_registry::{entity_type::EntityTypeRef, vanilla_damage_types, vanilla_entities};

use super::{DamageHistory, DamageSource};
use crate::entity::{Entity, EntityBase, SharedEntity};
use crate::test_support::{TestEntity, advance_test_game_time_to, fresh_test_world};

struct ReentrantSourceEntity {
    entity: TestEntity,
    history: Weak<DamageHistory>,
    dropped: Arc<AtomicBool>,
}

crate::entity::impl_test_downcast_type!(ReentrantSourceEntity);

impl Entity for ReentrantSourceEntity {
    fn base(&self) -> &EntityBase {
        self.entity.base()
    }

    fn entity_type(&self) -> EntityTypeRef {
        self.entity.entity_type()
    }
}

impl Drop for ReentrantSourceEntity {
    fn drop(&mut self) {
        let history = self
            .history
            .upgrade()
            .expect("history stays alive during expiry");
        // Fail promptly if the lock is held, instead of hanging on the reentrant call.
        assert!(
            history.records.try_lock().is_some(),
            "release the records mutex before destroying a retained entity"
        );
        history.clear_victim(self.generation());
        self.dropped.store(true, Ordering::Relaxed);
    }
}

fn reentrant_source(history: &Arc<DamageHistory>) -> (SharedEntity, Arc<AtomicBool>) {
    let dropped = Arc::new(AtomicBool::new(false));
    let entity = Arc::new(ReentrantSourceEntity {
        entity: TestEntity::new(2, DVec3::ZERO, Weak::new(), &vanilla_entities::ITEM),
        history: Arc::downgrade(history),
        dropped: Arc::clone(&dropped),
    });
    (entity, dropped)
}

#[test]
fn age_expiry_unlocks_records_before_destroying_retained_entities() {
    let world_fixture = fresh_test_world("history_expiry_drop_order");
    let world = &world_fixture.world;
    let history = Arc::new(DamageHistory::default());
    let victim = TestEntity::shared(1, DVec3::ZERO, Weak::new(), &vanilla_entities::ITEM);
    let _owner = victim.base().damage_history().retain_owner();
    let (source, dropped) = reentrant_source(&history);
    history.record(
        victim.base(),
        &DamageSource::direct(&vanilla_damage_types::GENERIC, source),
        &world.game_time,
    );

    advance_test_game_time_to(world, 40);
    history.expire();
    assert!(!dropped.load(Ordering::Relaxed));
    advance_test_game_time_to(world, 41);
    history.expire();
    assert!(dropped.load(Ordering::Relaxed));
    assert!(
        victim
            .base()
            .damage_history()
            .last_damage_source()
            .is_none()
    );
}

#[test]
fn ownership_sweep_unlocks_records_before_destroying_retained_entities() {
    let world_fixture = fresh_test_world("history_owner_drop_order");
    let world = &world_fixture.world;
    let history = Arc::new(DamageHistory::default());
    let victim = TestEntity::shared(1, DVec3::ZERO, Weak::new(), &vanilla_entities::ITEM);
    let owner = victim.base().damage_history().retain_owner();
    let (source, dropped) = reentrant_source(&history);
    let source_generation = source.generation();
    history.record(
        victim.base(),
        &DamageSource::direct(&vanilla_damage_types::GENERIC, source),
        &world.game_time,
    );

    history.expire();
    assert!(!dropped.load(Ordering::Relaxed));
    drop(owner);
    history.expire();
    assert!(dropped.load(Ordering::Relaxed));
    let recent = victim
        .base()
        .damage_history()
        .last_damage_source()
        .expect("unexpired metadata");
    assert_eq!(recent.causing_entity_generation(), Some(source_generation));
    assert!(recent.causing_entity().is_none());
    assert!(recent.is_direct());
}

#[test]
fn victim_lifetime_expiry_unlocks_records_before_destroying_retained_entities() {
    let world_fixture = fresh_test_world("history_victim_drop_order");
    let world = &world_fixture.world;
    let history = Arc::new(DamageHistory::default());
    let victim = TestEntity::shared(1, DVec3::ZERO, Weak::new(), &vanilla_entities::ITEM);
    let _owner = victim.base().damage_history().retain_owner();
    let (source, dropped) = reentrant_source(&history);
    history.record(
        victim.base(),
        &DamageSource::direct(&vanilla_damage_types::GENERIC, source),
        &world.game_time,
    );

    drop(victim);
    history.expire();
    assert!(dropped.load(Ordering::Relaxed));
}
