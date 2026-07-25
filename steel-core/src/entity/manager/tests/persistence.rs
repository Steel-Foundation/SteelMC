use super::*;

#[test]
fn final_chunk_unload_marks_stale_arc_removed_and_allows_same_identity_to_reload() {
    let manager = WorldEntityManager::new();
    let chunk = ChunkPos::new(0, 0);
    let uuid = Uuid::from_u128(9);
    load_chunk(&manager, chunk);

    let stale = ManagerTestEntity::shared(1, uuid, DVec3::new(1.0, 64.0, 1.0));
    assert!(
        manager
            .add_live_entity(stale.clone(), EntityOwnership::ManagerOwned)
            .is_ok()
    );

    let unload = manager.begin_chunk_unload(chunk);
    assert_eq!(unload.retained.len(), 1);
    manager.finalize_chunk_unload(chunk);

    assert!(stale.is_removed());
    assert_eq!(stale.removal_reason(), Some(RemovalReason::UnloadedToChunk));
    assert!(manager.get_by_id(stale.id()).is_none());

    load_chunk(&manager, chunk);
    let reloaded = ManagerTestEntity::shared(1, uuid, DVec3::new(1.0, 64.0, 1.0));
    assert!(
        manager
            .add_live_entity(reloaded.clone(), EntityOwnership::ManagerOwned)
            .is_ok()
    );

    let Some(live_entity) = manager.get_by_id(reloaded.id()) else {
        panic!("reloaded entity should be live");
    };
    assert!(Arc::ptr_eq(&reloaded, &live_entity));
    assert!(!reloaded.is_removed());
}

#[test]
fn saveable_entities_include_manager_owned_live_unloading_and_pending_entities() {
    let manager = WorldEntityManager::new();
    let chunk = ChunkPos::new(0, 0);
    load_chunk(&manager, chunk);

    let live = entity(1, 1, DVec3::new(1.0, 64.0, 1.0));
    let external = entity(2, 2, DVec3::new(2.0, 64.0, 1.0));
    assert!(
        manager
            .add_live_entity(live.clone(), EntityOwnership::ManagerOwned)
            .is_ok()
    );
    assert!(
        manager
            .add_live_entity(external, EntityOwnership::External)
            .is_ok()
    );

    let live_saveable = manager.get_saveable_entities_for_chunk(chunk);
    assert_eq!(live_saveable.len(), 1);
    assert!(Arc::ptr_eq(&live, &live_saveable[0]));

    let unload = manager.begin_chunk_unload(chunk);
    assert_eq!(unload.retained.len(), 1);
    let unloading_saveable = manager.get_saveable_entities_for_chunk(chunk);
    assert_eq!(unloading_saveable.len(), 1);
    assert!(Arc::ptr_eq(&live, &unloading_saveable[0]));

    manager.finalize_chunk_unload(chunk);
    load_chunk(&manager, chunk);

    let pending = entity(3, 3, DVec3::new(3.0, 64.0, 1.0));
    assert!(
        manager
            .add_live_entity(pending.clone(), EntityOwnership::ManagerOwned)
            .is_ok()
    );
    let removed = manager.remove_live_entity(pending.id(), RemovalReason::UnloadedToChunk);
    assert!(removed.is_some());

    let pending_saveable = manager.get_saveable_entities_for_chunk(chunk);
    assert_eq!(pending_saveable.len(), 1);
    assert!(Arc::ptr_eq(&pending, &pending_saveable[0]));
}

#[test]
fn save_pending_acknowledgement_clears_only_persisted_entities() {
    let manager = WorldEntityManager::new();
    let chunk = ChunkPos::new(0, 0);
    load_chunk(&manager, chunk);

    let saved = entity(1, 1, DVec3::new(1.0, 64.0, 1.0));
    let later = entity(2, 2, DVec3::new(2.0, 64.0, 1.0));
    assert!(
        manager
            .add_live_entity(saved.clone(), EntityOwnership::ManagerOwned)
            .is_ok()
    );
    assert!(
        manager
            .add_live_entity(later.clone(), EntityOwnership::ManagerOwned)
            .is_ok()
    );
    assert!(
        manager
            .remove_live_entity(saved.id(), RemovalReason::UnloadedToChunk)
            .is_some()
    );
    assert!(
        manager
            .remove_live_entity(later.id(), RemovalReason::UnloadedToChunk)
            .is_some()
    );
    assert_eq!(manager.get_saveable_entities_for_chunk(chunk).len(), 2);

    manager.on_chunk_saved(chunk, &[saved.id()]);

    let saveable = manager.get_saveable_entities_for_chunk(chunk);
    assert_eq!(saveable.len(), 1);
    assert!(Arc::ptr_eq(&later, &saveable[0]));

    manager.on_chunk_saved(chunk, &[later.id()]);

    assert!(manager.get_saveable_entities_for_chunk(chunk).is_empty());
    assert!(!manager.has_save_pending_for_chunk(chunk));
}

#[test]
fn add_live_entity_rejects_duplicate_uuid_in_save_pending_entities() {
    let manager = WorldEntityManager::new();
    let chunk = ChunkPos::new(0, 0);
    load_chunk(&manager, chunk);

    let uuid = Uuid::from_u128(44);
    let pending = ManagerTestEntity::shared(1, uuid, DVec3::new(1.0, 64.0, 1.0));
    assert!(
        manager
            .add_live_entity(pending, EntityOwnership::ManagerOwned)
            .is_ok()
    );
    assert!(
        manager
            .remove_live_entity(1, RemovalReason::UnloadedToChunk)
            .is_some()
    );

    let duplicate = ManagerTestEntity::shared(2, uuid, DVec3::new(2.0, 64.0, 1.0));

    assert!(matches!(
        manager.add_live_entity(duplicate, EntityOwnership::ManagerOwned),
        Err(AddEntityError::DuplicateUuid {
            entity_id: 2,
            uuid: duplicate_uuid,
        }) if duplicate_uuid == uuid
    ));
}

#[test]
#[should_panic(expected = "entity id 1 is already registered in the world entity manager")]
fn add_live_entity_panics_on_duplicate_id_in_save_pending_entities() {
    let manager = WorldEntityManager::new();
    let chunk = ChunkPos::new(0, 0);
    load_chunk(&manager, chunk);

    let pending = entity(1, 46, DVec3::new(1.0, 64.0, 1.0));
    assert!(
        manager
            .add_live_entity(pending, EntityOwnership::ManagerOwned)
            .is_ok()
    );
    assert!(
        manager
            .remove_live_entity(1, RemovalReason::UnloadedToChunk)
            .is_some()
    );

    let duplicate = entity(1, 47, DVec3::new(2.0, 64.0, 1.0));
    let _ = manager.add_live_entity(duplicate, EntityOwnership::ManagerOwned);
}

#[test]
fn add_live_entity_rejects_duplicate_uuid_in_unloading_entities() {
    let manager = WorldEntityManager::new();
    let chunk = ChunkPos::new(0, 0);
    load_chunk(&manager, chunk);

    let uuid = Uuid::from_u128(45);
    let unloading = ManagerTestEntity::shared(1, uuid, DVec3::new(1.0, 64.0, 1.0));
    assert!(
        manager
            .add_live_entity(unloading, EntityOwnership::ManagerOwned)
            .is_ok()
    );
    assert_eq!(manager.begin_chunk_unload(chunk).retained.len(), 1);

    let duplicate = ManagerTestEntity::shared(2, uuid, DVec3::new(2.0, 64.0, 1.0));

    assert!(matches!(
        manager.add_live_entity(duplicate, EntityOwnership::ManagerOwned),
        Err(AddEntityError::DuplicateUuid {
            entity_id: 2,
            uuid: duplicate_uuid,
        }) if duplicate_uuid == uuid
    ));
}

#[test]
#[should_panic(expected = "entity id 1 is already registered in the world entity manager")]
fn add_live_entity_panics_on_duplicate_id_in_unloading_entities() {
    let manager = WorldEntityManager::new();
    let chunk = ChunkPos::new(0, 0);
    load_chunk(&manager, chunk);

    let unloading = entity(1, 48, DVec3::new(1.0, 64.0, 1.0));
    assert!(
        manager
            .add_live_entity(unloading, EntityOwnership::ManagerOwned)
            .is_ok()
    );
    assert_eq!(manager.begin_chunk_unload(chunk).retained.len(), 1);

    let duplicate = entity(1, 49, DVec3::new(2.0, 64.0, 1.0));
    let _ = manager.add_live_entity(duplicate, EntityOwnership::ManagerOwned);
}

#[test]
fn chunk_recovery_does_not_restore_removed_retained_entities() {
    let manager = WorldEntityManager::new();
    let chunk = ChunkPos::new(0, 0);
    load_chunk(&manager, chunk);

    let removed = entity(1, 1, DVec3::new(1.0, 64.0, 1.0));
    assert!(
        manager
            .add_live_entity(removed.clone(), EntityOwnership::ManagerOwned)
            .is_ok()
    );

    let unload = manager.begin_chunk_unload(chunk);
    assert_eq!(unload.retained.len(), 1);
    removed.set_removed(RemovalReason::Discarded);

    let result = manager.on_chunk_loaded(chunk);

    assert!(result.restored.is_empty());
    assert!(!result.needs_save);
    assert!(manager.get_by_id(removed.id()).is_none());
    assert!(manager.get_saveable_entities_for_chunk(chunk).is_empty());
}

#[test]
fn chunk_recovery_keeps_saveable_removed_retained_entities_pending() {
    let manager = WorldEntityManager::new();
    let chunk = ChunkPos::new(0, 0);
    load_chunk(&manager, chunk);

    let pending = entity(1, 1, DVec3::new(1.0, 64.0, 1.0));
    assert!(
        manager
            .add_live_entity(pending.clone(), EntityOwnership::ManagerOwned)
            .is_ok()
    );

    let unload = manager.begin_chunk_unload(chunk);
    assert_eq!(unload.retained.len(), 1);
    pending.set_removed(RemovalReason::UnloadedToChunk);

    let result = manager.on_chunk_loaded(chunk);

    assert!(result.restored.is_empty());
    assert!(result.needs_save);
    assert!(manager.get_by_id(pending.id()).is_none());
    assert!(manager.has_save_pending_for_chunk(chunk));
    let saveable = manager.get_saveable_entities_for_chunk(chunk);
    assert_eq!(saveable.len(), 1);
    assert!(Arc::ptr_eq(&pending, &saveable[0]));
}

#[test]
fn saveable_entities_outside_saved_chunks_reports_only_manager_owned_entities() {
    let manager = WorldEntityManager::new();
    let saved_chunk = ChunkPos::new(0, 0);
    let unsaved_chunk = ChunkPos::new(1, 0);
    load_chunk(&manager, saved_chunk);
    load_chunk(&manager, unsaved_chunk);

    let saved = entity(1, 1, DVec3::new(1.0, 64.0, 1.0));
    let unsaved = entity(2, 2, DVec3::new(17.0, 64.0, 1.0));
    let external = entity(3, 3, DVec3::new(18.0, 64.0, 1.0));
    assert!(
        manager
            .add_live_entity(saved, EntityOwnership::ManagerOwned)
            .is_ok()
    );
    assert!(
        manager
            .add_live_entity(unsaved.clone(), EntityOwnership::ManagerOwned)
            .is_ok()
    );
    assert!(
        manager
            .add_live_entity(external, EntityOwnership::External)
            .is_ok()
    );

    let reports = manager.saveable_entities_outside_chunks(&[saved_chunk]);
    assert_eq!(
        reports,
        vec![UnsavedEntityReport {
            entity_id: unsaved.id(),
            uuid: unsaved.uuid(),
            chunk: unsaved_chunk,
        }]
    );
}
