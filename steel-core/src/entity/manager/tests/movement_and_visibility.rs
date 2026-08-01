use super::*;

#[test]
fn committed_move_updates_chunk_index_for_loaded_destination() {
    let manager = WorldEntityManager::new();
    load_chunk(&manager, ChunkPos::new(0, 0));
    load_chunk(&manager, ChunkPos::new(1, 0));

    let entity = entity(1, 1, DVec3::new(1.0, 64.0, 1.0));
    assert!(
        manager
            .add_live_entity(entity.clone(), EntityOwnership::ManagerOwned)
            .is_ok()
    );

    let new_position = DVec3::new(17.0, 64.0, 1.0);
    assert!(manager.validate_move(entity.id(), new_position).is_ok());
    entity.base().set_position_local(new_position);
    let update = match manager.commit_move(entity.id(), new_position) {
        Ok(update) => update,
        Err(error) => panic!("move into unloaded chunk should commit: {error}"),
    };

    assert!(update.chunk_changed());
    assert!(
        manager
            .live_entities_in_chunk(ChunkPos::new(0, 0))
            .is_empty()
    );
    let new_chunk_entities = manager.live_entities_in_chunk(ChunkPos::new(1, 0));
    assert_eq!(new_chunk_entities.len(), 1);
    assert!(Arc::ptr_eq(&entity, &new_chunk_entities[0]));
}

#[test]
fn validate_move_rejects_manager_owned_unloaded_destination() {
    let manager = WorldEntityManager::new();
    load_chunk(&manager, ChunkPos::new(0, 0));

    let entity = entity(1, 1, DVec3::new(1.0, 64.0, 1.0));
    assert!(
        manager
            .add_live_entity(entity.clone(), EntityOwnership::ManagerOwned)
            .is_ok()
    );

    let new_position = DVec3::new(17.0, 64.0, 1.0);

    assert!(matches!(
        manager.validate_move(entity.id(), new_position),
        Err(EntityMoveError::UnloadedDestination {
            entity_id: 1,
            chunk,
        }) if chunk == ChunkPos::new(1, 0)
    ));
    assert_eq!(manager.live_entities_in_chunk(ChunkPos::new(0, 0)).len(), 1);
    assert!(
        manager
            .live_entities_in_chunk(ChunkPos::new(1, 0))
            .is_empty()
    );
}

#[test]
fn commit_move_rejects_destination_unloaded_after_validation() {
    let manager = WorldEntityManager::new();
    load_chunk(&manager, ChunkPos::new(0, 0));
    load_chunk(&manager, ChunkPos::new(1, 0));

    let entity = entity(1, 1, DVec3::new(1.0, 64.0, 1.0));
    assert!(
        manager
            .add_live_entity(entity.clone(), EntityOwnership::ManagerOwned)
            .is_ok()
    );

    let new_position = DVec3::new(17.0, 64.0, 1.0);
    assert!(manager.validate_move(entity.id(), new_position).is_ok());
    let unload = manager.begin_chunk_unload(ChunkPos::new(1, 0));
    assert!(unload.retained.is_empty());
    assert!(unload.tracking_stopped.is_empty());
    entity.base().set_position_local(new_position);

    assert!(matches!(
        manager.commit_move(entity.id(), new_position),
        Err(EntityMoveError::UnloadedDestination {
            entity_id: 1,
            chunk,
        }) if chunk == ChunkPos::new(1, 0)
    ));
    assert_eq!(manager.live_entities_in_chunk(ChunkPos::new(0, 0)).len(), 1);
    assert!(
        manager
            .live_entities_in_chunk(ChunkPos::new(1, 0))
            .is_empty()
    );
}

#[test]
fn chunk_recovery_restores_same_entity_arc_before_final_unload() {
    let manager = WorldEntityManager::new();
    let chunk = ChunkPos::new(0, 0);
    load_chunk(&manager, chunk);

    let entity = entity(1, 1, DVec3::new(1.0, 64.0, 1.0));
    assert!(
        manager
            .add_live_entity(entity.clone(), EntityOwnership::ManagerOwned)
            .is_ok()
    );

    let unload = manager.begin_chunk_unload(chunk);
    assert_eq!(unload.retained.len(), 1);
    assert!(Arc::ptr_eq(&entity, &unload.retained[0]));
    assert_eq!(unload.tracking_stopped.len(), 1);
    assert!(Arc::ptr_eq(&entity, &unload.tracking_stopped[0]));
    assert!(manager.get_by_id(entity.id()).is_none());

    let result = manager.on_chunk_loaded(chunk);
    assert_eq!(result.restored.len(), 1);
    assert!(Arc::ptr_eq(&entity, &result.restored[0]));
    assert!(!result.needs_save);

    let Some(live_entity) = manager.get_by_id(entity.id()) else {
        panic!("recovered entity should be live again");
    };
    assert!(Arc::ptr_eq(&entity, &live_entity));
    assert!(!entity.is_removed());
}

#[test]
fn live_or_unloading_membership_excludes_removed_live_entities() {
    let manager = WorldEntityManager::new();
    let chunk = ChunkPos::new(0, 0);
    load_chunk(&manager, chunk);

    let entity = entity(1, 1, DVec3::new(1.0, 64.0, 1.0));
    assert!(
        manager
            .add_live_entity(entity.clone(), EntityOwnership::ManagerOwned)
            .is_ok()
    );
    assert!(manager.contains_live_or_unloading_entity(&entity));

    assert!(
        manager
            .remove_live_entity(entity.id(), RemovalReason::ChangedWorld)
            .is_some()
    );
    assert!(!manager.contains_live_or_unloading_entity(&entity));
}

#[test]
fn live_or_unloading_membership_includes_unload_retained_entities() {
    let manager = WorldEntityManager::new();
    let chunk = ChunkPos::new(0, 0);
    load_chunk(&manager, chunk);

    let entity = entity(1, 1, DVec3::new(1.0, 64.0, 1.0));
    assert!(
        manager
            .add_live_entity(entity.clone(), EntityOwnership::ManagerOwned)
            .is_ok()
    );

    let unload = manager.begin_chunk_unload(chunk);
    assert_eq!(unload.retained.len(), 1);

    assert!(manager.contains_live_or_unloading_entity(&entity));
}

#[test]
fn chunk_unload_retains_manager_owned_passenger_tree() {
    let manager = WorldEntityManager::new();
    let vehicle_chunk = ChunkPos::new(0, 0);
    let passenger_chunk = ChunkPos::new(1, 0);
    load_chunk(&manager, vehicle_chunk);
    load_chunk(&manager, passenger_chunk);

    let vehicle = entity(1, 1, DVec3::new(1.0, 64.0, 1.0));
    let passenger = entity(2, 2, DVec3::new(17.0, 64.0, 1.0));
    EntityBase::restore_passenger_relationship(&vehicle, &passenger);

    assert!(
        manager
            .add_live_entity(vehicle.clone(), EntityOwnership::ManagerOwned)
            .is_ok()
    );
    assert!(
        manager
            .add_live_entity(passenger.clone(), EntityOwnership::ManagerOwned)
            .is_ok()
    );

    let unload = manager.begin_chunk_unload(vehicle_chunk);
    let mut retained_ids = unload
        .retained
        .iter()
        .map(|entity| entity.id())
        .collect::<Vec<_>>();
    retained_ids.sort_unstable();
    assert_eq!(retained_ids, vec![1, 2]);
    let mut tracking_stopped_ids = unload
        .tracking_stopped
        .iter()
        .map(|entity| entity.id())
        .collect::<Vec<_>>();
    tracking_stopped_ids.sort_unstable();
    assert_eq!(tracking_stopped_ids, vec![1, 2]);
    assert!(manager.get_by_id(vehicle.id()).is_none());
    assert!(manager.get_by_id(passenger.id()).is_none());
    assert!(manager.live_entities_in_chunk(passenger_chunk).is_empty());

    let saveable = manager.get_saveable_entities_for_chunk(vehicle_chunk);
    let mut saveable_ids = saveable
        .iter()
        .map(|entity| entity.id())
        .collect::<Vec<_>>();
    saveable_ids.sort_unstable();
    assert_eq!(saveable_ids, vec![1]);

    manager.finalize_chunk_unload(vehicle_chunk);
    assert!(vehicle.is_removed());
    assert!(passenger.is_removed());
}

#[test]
fn passenger_chunk_unload_hides_passenger_without_unloading_vehicle_tree() {
    let manager = WorldEntityManager::new();
    let vehicle_chunk = ChunkPos::new(0, 0);
    let passenger_chunk = ChunkPos::new(1, 0);
    load_chunk(&manager, vehicle_chunk);
    load_chunk(&manager, passenger_chunk);

    let vehicle = entity(1, 1, DVec3::new(1.0, 64.0, 1.0));
    let passenger = entity(2, 2, DVec3::new(17.0, 64.0, 1.0));
    EntityBase::restore_passenger_relationship(&vehicle, &passenger);

    assert!(
        manager
            .add_live_entity(vehicle.clone(), EntityOwnership::ManagerOwned)
            .is_ok()
    );
    assert!(
        manager
            .add_live_entity(passenger.clone(), EntityOwnership::ManagerOwned)
            .is_ok()
    );

    let passenger_aabb = WorldAabb::new(16.5, 63.0, 0.5, 17.5, 65.0, 1.5);
    assert_eq!(manager.get_entities_in_aabb(&passenger_aabb).len(), 1);

    let unload = manager.begin_chunk_unload(passenger_chunk);

    assert!(unload.retained.is_empty());
    assert_eq!(unload.tracking_stopped.len(), 1);
    assert!(Arc::ptr_eq(&passenger, &unload.tracking_stopped[0]));
    assert!(manager.get_by_id(vehicle.id()).is_some());
    assert!(manager.get_by_id(passenger.id()).is_some());
    assert!(manager.get_accessible_by_id(vehicle.id()).is_some());
    assert!(manager.get_accessible_by_id(passenger.id()).is_none());
    assert_eq!(manager.live_entities_in_chunk(passenger_chunk).len(), 1);
    assert!(manager.get_entities_in_aabb(&passenger_aabb).is_empty());
    assert!(
        manager
            .get_saveable_entities_for_chunk(passenger_chunk)
            .is_empty()
    );

    let saveable = manager.get_saveable_entities_for_chunk(vehicle_chunk);
    assert_eq!(saveable.len(), 1);
    assert!(Arc::ptr_eq(&vehicle, &saveable[0]));

    let result = manager.on_chunk_loaded(passenger_chunk);
    assert!(result.restored.is_empty());
    assert!(result.tracking_started.is_empty());
    let changes = manager.update_chunk_visibility(passenger_chunk, EntityVisibility::Ticking);
    assert_eq!(changes.tracking_started.len(), 1);
    assert!(Arc::ptr_eq(&passenger, &changes.tracking_started[0]));
    assert_eq!(manager.get_entities_in_aabb(&passenger_aabb).len(), 1);
}

#[test]
fn loaded_entity_tree_can_restore_passenger_in_hidden_chunk() {
    let manager = WorldEntityManager::new();
    let vehicle_chunk = ChunkPos::new(0, 0);
    let passenger_chunk = ChunkPos::new(1, 0);
    load_chunk(&manager, vehicle_chunk);

    let vehicle = entity(1, 1, DVec3::new(1.0, 64.0, 1.0));
    let passenger = entity(2, 2, DVec3::new(17.0, 64.0, 1.0));
    EntityBase::restore_passenger_relationship(&vehicle, &passenger);

    let changes = manager
        .add_live_entity_tree(
            &[vehicle.clone(), passenger.clone()],
            EntityOwnership::ManagerOwned,
        )
        .expect("persisted tree should restore even when passenger chunk is hidden");

    assert_eq!(changes.tracking_started.len(), 1);
    assert!(Arc::ptr_eq(&vehicle, &changes.tracking_started[0]));
    assert!(manager.get_by_id(passenger.id()).is_some());
    assert!(manager.get_accessible_by_id(passenger.id()).is_none());
    assert_eq!(manager.live_entities_in_chunk(passenger_chunk).len(), 1);
}

#[test]
fn attached_passenger_can_move_while_its_own_chunk_is_hidden() {
    let manager = WorldEntityManager::new();
    let vehicle_chunk = ChunkPos::new(0, 0);
    let passenger_chunk = ChunkPos::new(1, 0);
    load_chunk(&manager, vehicle_chunk);
    load_chunk(&manager, passenger_chunk);

    let vehicle = entity(1, 1, DVec3::new(1.0, 64.0, 1.0));
    let passenger = entity(2, 2, DVec3::new(17.0, 64.0, 1.0));
    EntityBase::restore_passenger_relationship(&vehicle, &passenger);

    assert!(
        manager
            .add_live_entity(vehicle.clone(), EntityOwnership::ManagerOwned)
            .is_ok()
    );
    assert!(
        manager
            .add_live_entity(passenger.clone(), EntityOwnership::ManagerOwned)
            .is_ok()
    );

    let unload = manager.begin_chunk_unload(passenger_chunk);
    assert!(unload.retained.is_empty());

    let new_position = DVec3::new(18.0, 64.0, 1.0);
    assert!(manager.validate_move(passenger.id(), new_position).is_ok());
    passenger.base().set_position_local(new_position);
    let update = match manager.commit_move(passenger.id(), new_position) {
        Ok(update) => update,
        Err(error) => panic!("attached passenger move should commit: {error}"),
    };
    assert_eq!(update.new_chunk, passenger_chunk);
    assert!(!update.old_accessible);
    assert!(!update.new_accessible);
    assert!(!update.accessibility_changed());
    assert_eq!(manager.live_entities_in_chunk(passenger_chunk).len(), 1);
}

#[test]
fn passenger_move_from_hidden_to_loaded_chunk_becomes_accessible() {
    let manager = WorldEntityManager::new();
    let vehicle_chunk = ChunkPos::new(0, 0);
    let passenger_chunk = ChunkPos::new(1, 0);
    load_chunk(&manager, vehicle_chunk);
    load_chunk(&manager, passenger_chunk);

    let vehicle = entity(1, 1, DVec3::new(1.0, 64.0, 1.0));
    let passenger = entity(2, 2, DVec3::new(17.0, 64.0, 1.0));
    EntityBase::restore_passenger_relationship(&vehicle, &passenger);

    assert!(
        manager
            .add_live_entity(vehicle.clone(), EntityOwnership::ManagerOwned)
            .is_ok()
    );
    assert!(
        manager
            .add_live_entity(passenger.clone(), EntityOwnership::ManagerOwned)
            .is_ok()
    );

    let unload = manager.begin_chunk_unload(passenger_chunk);
    assert!(unload.retained.is_empty());

    let new_position = DVec3::new(2.0, 64.0, 1.0);
    assert!(manager.validate_move(passenger.id(), new_position).is_ok());
    passenger.base().set_position_local(new_position);
    let update = match manager.commit_move(passenger.id(), new_position) {
        Ok(update) => update,
        Err(error) => panic!("attached passenger move should commit: {error}"),
    };

    assert_eq!(update.old_chunk, passenger_chunk);
    assert_eq!(update.new_chunk, vehicle_chunk);
    assert!(!update.old_accessible);
    assert!(update.new_accessible);
    assert!(update.became_accessible());
}

#[test]
fn passenger_move_from_loaded_to_hidden_chunk_becomes_inaccessible() {
    let manager = WorldEntityManager::new();
    let vehicle_chunk = ChunkPos::new(0, 0);
    let passenger_chunk = ChunkPos::new(1, 0);
    load_chunk(&manager, vehicle_chunk);
    load_chunk(&manager, passenger_chunk);

    let vehicle = entity(1, 1, DVec3::new(1.0, 64.0, 1.0));
    let passenger = entity(2, 2, DVec3::new(2.0, 64.0, 1.0));
    EntityBase::restore_passenger_relationship(&vehicle, &passenger);

    assert!(
        manager
            .add_live_entity(vehicle.clone(), EntityOwnership::ManagerOwned)
            .is_ok()
    );
    assert!(
        manager
            .add_live_entity(passenger.clone(), EntityOwnership::ManagerOwned)
            .is_ok()
    );

    let unload = manager.begin_chunk_unload(passenger_chunk);
    assert!(unload.retained.is_empty());
    assert!(unload.tracking_stopped.is_empty());

    let new_position = DVec3::new(17.0, 64.0, 1.0);
    assert!(manager.validate_move(passenger.id(), new_position).is_ok());
    passenger.base().set_position_local(new_position);
    let update = match manager.commit_move(passenger.id(), new_position) {
        Ok(update) => update,
        Err(error) => panic!("attached passenger move should commit: {error}"),
    };

    assert_eq!(update.old_chunk, vehicle_chunk);
    assert_eq!(update.new_chunk, passenger_chunk);
    assert!(update.old_accessible);
    assert!(!update.new_accessible);
    assert!(update.became_inaccessible());
}

#[test]
fn hidden_chunk_passenger_is_not_ticked_by_loaded_vehicle() {
    let manager = WorldEntityManager::new();
    let vehicle_chunk = ChunkPos::new(0, 0);
    let passenger_chunk = ChunkPos::new(1, 0);
    load_chunk(&manager, vehicle_chunk);
    load_chunk(&manager, passenger_chunk);

    let vehicle = entity(1, 1, DVec3::new(1.0, 64.0, 1.0));
    let passenger = entity(2, 2, DVec3::new(17.0, 64.0, 1.0));
    EntityBase::restore_passenger_relationship(&vehicle, &passenger);

    assert!(
        manager
            .add_live_entity(vehicle.clone(), EntityOwnership::ManagerOwned)
            .is_ok()
    );
    assert!(
        manager
            .add_live_entity(passenger.clone(), EntityOwnership::ManagerOwned)
            .is_ok()
    );

    let unload = manager.begin_chunk_unload(passenger_chunk);
    assert!(unload.retained.is_empty());

    manager.tick_entities(0, true);
    assert_eq!(vehicle.tick_count(), 1);
    assert_eq!(passenger.tick_count(), 0);

    let result = manager.on_chunk_loaded(passenger_chunk);
    assert!(result.tracking_started.is_empty());
    assert!(result.ticking_started.is_empty());
    let changes = manager.update_chunk_visibility(passenger_chunk, EntityVisibility::Ticking);
    assert_eq!(changes.tracking_started.len(), 1);
    assert_eq!(changes.ticking_started.len(), 1);
    manager.tick_entities(1, true);
    assert_eq!(vehicle.tick_count(), 2);
    assert_eq!(passenger.tick_count(), 1);
}

#[test]
fn non_passenger_tick_snapshots_old_position_and_rotation_before_tick() {
    let manager = WorldEntityManager::new();
    let chunk = ChunkPos::new(0, 0);
    load_chunk(&manager, chunk);

    let start = DVec3::new(1.0, 64.0, 1.0);
    let entity = MovingTickTestEntity::shared(
        1,
        Uuid::from_u128(1),
        start,
        DVec3::new(2.0, 64.0, 1.0),
        (90.0, 20.0),
    );
    entity.set_rotation((45.0, 10.0));
    entity.set_old_position(DVec3::new(-1.0, 64.0, -1.0));
    entity.base().set_old_rotation((-30.0, -10.0));
    assert!(
        manager
            .add_live_entity(entity.clone(), EntityOwnership::ManagerOwned)
            .is_ok()
    );

    manager.tick_entities(0, true);

    assert_eq!(entity.old_position(), start);
    assert_eq!(entity.base().old_rotation(), (45.0, 10.0));
    assert_eq!(entity.position(), DVec3::new(2.0, 64.0, 1.0));
    assert_eq!(entity.rotation(), (90.0, 20.0));
}

#[test]
fn passenger_tick_snapshots_old_position_and_rotation_before_ride_tick() {
    let manager = WorldEntityManager::new();
    let chunk = ChunkPos::new(0, 0);
    load_chunk(&manager, chunk);

    let vehicle = entity(1, 1, DVec3::new(1.0, 64.0, 1.0));
    let start = DVec3::new(1.0, 65.0, 1.0);
    let passenger = MovingTickTestEntity::shared(
        2,
        Uuid::from_u128(2),
        start,
        DVec3::new(2.0, 65.0, 1.0),
        (135.0, 15.0),
    );
    passenger.set_rotation((60.0, 5.0));
    passenger.set_old_position(DVec3::new(-1.0, 65.0, -1.0));
    passenger.base().set_old_rotation((-60.0, -5.0));
    EntityBase::restore_passenger_relationship(&vehicle, &passenger);
    assert!(
        manager
            .add_live_entity(vehicle.clone(), EntityOwnership::ManagerOwned)
            .is_ok()
    );
    assert!(
        manager
            .add_live_entity(passenger.clone(), EntityOwnership::ManagerOwned)
            .is_ok()
    );

    manager.tick_entities(0, true);

    assert_eq!(passenger.tick_count(), 1);
    assert_eq!(passenger.old_position(), start);
    assert_eq!(passenger.base().old_rotation(), (60.0, 5.0));
    assert_eq!(passenger.rotation(), (135.0, 15.0));
}
