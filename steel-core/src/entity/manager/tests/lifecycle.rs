use super::*;

#[test]
fn add_live_entity_rejects_manager_owned_unloaded_chunk() {
    let manager = WorldEntityManager::new();
    let entity = entity(1, 1, DVec3::new(1.0, 64.0, 1.0));

    assert!(matches!(
        manager.add_live_entity(entity.clone(), EntityOwnership::ManagerOwned),
        Err(AddEntityError::ChunkNotLoaded {
            entity_id: 1,
            chunk,
        }) if chunk == ChunkPos::new(0, 0)
    ));
    assert_eq!(manager.count(), 0);
    assert!(manager.get_by_id(entity.id()).is_none());
}

#[test]
fn add_live_entity_accepts_external_unloaded_chunk() {
    let manager = WorldEntityManager::new();
    let entity = entity(1, 1, DVec3::new(1.0, 64.0, 1.0));

    assert!(
        manager
            .add_live_entity(entity.clone(), EntityOwnership::External)
            .is_ok()
    );
    assert_eq!(manager.count(), 1);

    let Some(live_entity) = manager.get_by_id(entity.id()) else {
        panic!("entity in unloaded chunk should be live");
    };
    assert!(Arc::ptr_eq(&entity, &live_entity));
}

#[test]
fn add_live_entity_rejects_duplicate_uuid_without_registering_second_entity() {
    let manager = WorldEntityManager::new();
    load_chunk(&manager, ChunkPos::new(0, 0));

    let uuid = Uuid::from_u128(5);
    let first = ManagerTestEntity::shared(1, uuid, DVec3::new(1.0, 64.0, 1.0));
    let second = ManagerTestEntity::shared(2, uuid, DVec3::new(2.0, 64.0, 1.0));

    assert!(
        manager
            .add_live_entity(first.clone(), EntityOwnership::ManagerOwned)
            .is_ok()
    );
    assert!(matches!(
        manager.add_live_entity(second, EntityOwnership::ManagerOwned),
        Err(AddEntityError::DuplicateUuid {
            entity_id: 2,
            uuid: duplicate,
        }) if duplicate == uuid
    ));

    let Some(live_first) = manager.get_by_id(1) else {
        panic!("first entity should stay registered");
    };
    assert!(Arc::ptr_eq(&first, &live_first));
    assert!(manager.get_by_id(2).is_none());
    assert_eq!(manager.count(), 1);
}

#[test]
fn add_live_entity_tree_rejects_duplicate_uuid_without_partial_registration() {
    let manager = WorldEntityManager::new();
    load_chunk(&manager, ChunkPos::new(0, 0));

    let existing_uuid = Uuid::from_u128(5);
    let existing = ManagerTestEntity::shared(1, existing_uuid, DVec3::new(1.0, 64.0, 1.0));
    let result = manager.add_live_entity(Arc::clone(&existing), EntityOwnership::ManagerOwned);
    assert!(
        result.is_ok(),
        "existing entity should register before duplicate UUID test: {result:?}"
    );

    let vehicle = entity(2, 6, DVec3::new(2.0, 64.0, 2.0));
    let passenger = ManagerTestEntity::shared(3, existing_uuid, DVec3::new(2.0, 64.0, 2.0));
    EntityBase::restore_passenger_relationship(&vehicle, &passenger);

    assert!(matches!(
        manager.add_live_entity_tree(
            &[Arc::clone(&vehicle), Arc::clone(&passenger)],
            EntityOwnership::ManagerOwned,
        ),
        Err(AddEntityError::DuplicateUuid {
            entity_id: 3,
            uuid,
        }) if uuid == existing_uuid
    ));
    assert!(manager.get_by_id(2).is_none());
    assert!(manager.get_by_id(3).is_none());
    assert_eq!(manager.count(), 1);
}

#[test]
#[should_panic(expected = "entity id 1 is already registered in the world entity manager")]
fn duplicate_entity_id_is_a_loud_invariant_failure() {
    let manager = WorldEntityManager::new();
    load_chunk(&manager, ChunkPos::new(0, 0));

    assert!(
        manager
            .add_live_entity(
                entity(1, 1, DVec3::new(1.0, 64.0, 1.0)),
                EntityOwnership::ManagerOwned,
            )
            .is_ok()
    );
    let _ = manager.add_live_entity(
        entity(1, 2, DVec3::new(2.0, 64.0, 1.0)),
        EntityOwnership::ManagerOwned,
    );
}
