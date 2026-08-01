use super::*;

#[test]
fn tick_entities_skips_external_entities() {
    let manager = WorldEntityManager::new();
    let chunk = ChunkPos::new(0, 0);
    load_chunk(&manager, chunk);

    let manager_owned = entity(1, 1, DVec3::new(1.0, 64.0, 1.0));
    let external = entity(2, 2, DVec3::new(2.0, 64.0, 1.0));
    assert!(
        manager
            .add_live_entity(manager_owned.clone(), EntityOwnership::ManagerOwned)
            .is_ok()
    );
    assert!(
        manager
            .add_live_entity(external.clone(), EntityOwnership::External)
            .is_ok()
    );

    let dirty_chunks = manager.tick_entities(12, true);

    assert!(dirty_chunks.contains(&chunk));
    assert_eq!(manager_owned.tick_count(), 1);
    assert_eq!(external.tick_count(), 0);
}

#[test]
fn tick_entities_ticks_external_always_ticking_entities_without_dirtying_chunks() {
    let manager = WorldEntityManager::new();
    let entity =
        ManagerTestEntity::shared_always_ticking(1, Uuid::from_u128(1), DVec3::new(1.0, 64.0, 1.0));

    let changes = match manager.add_live_entity(entity.clone(), EntityOwnership::External) {
        Ok(changes) => changes,
        Err(error) => panic!("always-ticking external entity should register: {error}"),
    };
    assert_eq!(changes.tracking_started.len(), 1);
    assert_eq!(changes.ticking_started.len(), 1);

    let dirty_chunks = manager.tick_entities(0, true);

    assert!(dirty_chunks.is_empty());
    assert_eq!(entity.tick_count(), 1);
}

#[test]
fn chunk_unload_retention_preserves_external_always_ticking_passenger() {
    let manager = WorldEntityManager::new();
    let chunk = ChunkPos::new(0, 0);
    load_chunk(&manager, chunk);

    let vehicle = entity(1, 1, DVec3::new(1.0, 64.0, 1.0));
    let passenger =
        ManagerTestEntity::shared_always_ticking(2, Uuid::from_u128(2), DVec3::new(1.0, 65.0, 1.0));
    EntityBase::restore_passenger_relationship(&vehicle, &passenger);
    assert!(
        manager
            .add_live_entity(vehicle, EntityOwnership::ManagerOwned)
            .is_ok()
    );
    assert!(
        manager
            .add_live_entity(passenger.clone(), EntityOwnership::External)
            .is_ok()
    );

    manager.begin_chunk_unload(chunk);

    assert!(manager.can_tick_entity_now(passenger.id()));
}

#[test]
fn tick_entities_uses_start_of_tick_snapshot_for_added_entities() {
    let manager = Arc::new(WorldEntityManager::new());
    let initial_chunk = ChunkPos::new(0, 0);
    let late_chunk = ChunkPos::new(1, 0);
    load_chunk(&manager, initial_chunk);
    load_chunk(&manager, late_chunk);

    let late_entity = entity(2, 2, DVec3::new(17.0, 64.0, 1.0));
    let adder = AddDuringTickTestEntity::shared(
        1,
        Uuid::from_u128(1),
        DVec3::new(1.0, 64.0, 1.0),
        Arc::clone(&manager),
        late_entity.clone(),
    );
    assert!(
        manager
            .add_live_entity(adder.clone(), EntityOwnership::ManagerOwned)
            .is_ok()
    );

    manager.tick_entities(0, true);

    assert_eq!(adder.tick_count(), 1);
    assert_eq!(late_entity.tick_count(), 0);

    manager.tick_entities(1, true);

    assert_eq!(adder.tick_count(), 2);
    assert_eq!(late_entity.tick_count(), 1);
}

#[test]
fn tick_entities_checks_despawn_for_ticking_entities() {
    let manager = WorldEntityManager::new();
    let tickable_chunk = ChunkPos::new(0, 0);
    let non_tickable_chunk = ChunkPos::new(1, 0);
    load_chunk(&manager, tickable_chunk);
    load_chunk(&manager, non_tickable_chunk);

    let entity =
        DespawnOnCheckTestEntity::shared(1, Uuid::from_u128(1), DVec3::new(17.0, 64.0, 1.0));
    assert!(
        manager
            .add_live_entity(entity.clone(), EntityOwnership::ManagerOwned)
            .is_ok()
    );

    let dirty_chunks = manager.tick_entities(0, true);

    assert!(entity.is_removed());
    assert!(dirty_chunks.contains(&non_tickable_chunk));
    assert_eq!(entity.tick_count(), 0);
}

#[test]
fn tick_entities_skips_despawn_for_tracked_non_ticking_entities() {
    let manager = WorldEntityManager::new();
    let chunk = ChunkPos::new(0, 0);
    track_chunk(&manager, chunk);

    let entity =
        DespawnOnCheckTestEntity::shared(1, Uuid::from_u128(1), DVec3::new(1.0, 64.0, 1.0));
    let changes = match manager.add_live_entity(entity.clone(), EntityOwnership::ManagerOwned) {
        Ok(changes) => changes,
        Err(error) => panic!("entity should register in tracked chunk: {error}"),
    };
    assert_eq!(changes.tracking_started.len(), 1);
    assert!(changes.ticking_started.is_empty());

    let dirty_chunks = manager.tick_entities(0, true);

    assert!(dirty_chunks.is_empty());
    assert!(!entity.is_removed());
}

#[test]
fn tick_entities_skips_pending_world_change_entities() {
    let manager = WorldEntityManager::new();
    let chunk = ChunkPos::new(0, 0);
    load_chunk(&manager, chunk);

    let entity = entity(1, 1, DVec3::new(1.0, 64.0, 1.0));
    assert!(
        manager
            .add_live_entity(entity.clone(), EntityOwnership::ManagerOwned)
            .is_ok()
    );
    let Some(pending_token) = entity.begin_pending_world_change() else {
        panic!("fresh entity should accept a pending world change");
    };

    let dirty_chunks = manager.tick_entities(0, true);

    assert!(dirty_chunks.is_empty());
    assert_eq!(entity.tick_count(), 0);

    assert!(entity.finish_pending_world_change(pending_token));
    let dirty_chunks = manager.tick_entities(1, true);

    assert!(dirty_chunks.contains(&chunk));
    assert_eq!(entity.tick_count(), 1);
}

#[test]
fn tick_entities_skips_passengers_of_pending_world_change_vehicles_before_despawn() {
    let manager = WorldEntityManager::new();
    let vehicle_chunk = ChunkPos::new(0, 0);
    let passenger_chunk = ChunkPos::new(1, 0);
    load_chunk(&manager, vehicle_chunk);
    load_chunk(&manager, passenger_chunk);

    let vehicle = entity(1, 1, DVec3::new(1.0, 64.0, 1.0));
    let passenger =
        DespawnOnCheckTestEntity::shared(2, Uuid::from_u128(2), DVec3::new(17.0, 64.0, 1.0));
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
    let Some(pending_token) = vehicle.begin_pending_world_change() else {
        panic!("fresh vehicle should accept a pending world change");
    };

    let dirty_chunks = manager.tick_entities(0, true);

    assert!(dirty_chunks.is_empty());
    assert!(!passenger.is_removed());

    assert!(vehicle.finish_pending_world_change(pending_token));
    let dirty_chunks = manager.tick_entities(1, true);

    assert!(dirty_chunks.contains(&passenger_chunk));
    assert!(passenger.is_removed());
}

#[test]
fn tick_entities_skips_frozen_entities_and_despawn_checks() {
    let manager = WorldEntityManager::new();
    let tickable_chunk = ChunkPos::new(0, 0);
    let despawn_chunk = ChunkPos::new(1, 0);
    load_chunk(&manager, tickable_chunk);
    load_chunk(&manager, despawn_chunk);

    let ticked = entity(1, 1, DVec3::new(1.0, 64.0, 1.0));
    let despawn =
        DespawnOnCheckTestEntity::shared(2, Uuid::from_u128(2), DVec3::new(17.0, 64.0, 1.0));
    assert!(
        manager
            .add_live_entity(ticked.clone(), EntityOwnership::ManagerOwned)
            .is_ok()
    );
    assert!(
        manager
            .add_live_entity(despawn.clone(), EntityOwnership::ManagerOwned)
            .is_ok()
    );

    let dirty_chunks = manager.tick_entities(0, false);

    assert!(dirty_chunks.is_empty());
    assert_eq!(ticked.tick_count(), 0);
    assert!(!despawn.is_removed());
}

#[test]
fn tick_entities_ticks_player_passenger_vehicle_while_frozen() {
    let manager = WorldEntityManager::new();
    let chunk = ChunkPos::new(0, 0);
    load_chunk(&manager, chunk);

    let vehicle = entity(1, 1, DVec3::new(1.0, 64.0, 1.0));
    let passenger = ManagerTestEntity::shared_with_type(
        2,
        Uuid::from_u128(2),
        DVec3::new(1.0, 65.0, 1.0),
        &vanilla_entities::PLAYER,
    );
    EntityBase::restore_passenger_relationship(&vehicle, &passenger);
    assert!(
        manager
            .add_live_entity(vehicle.clone(), EntityOwnership::ManagerOwned)
            .is_ok()
    );
    assert!(
        manager
            .add_live_entity(passenger.clone(), EntityOwnership::External)
            .is_ok()
    );

    let dirty_chunks = manager.tick_entities(0, false);

    assert!(dirty_chunks.contains(&chunk));
    assert_eq!(vehicle.tick_count(), 1);
    assert_eq!(passenger.tick_count(), 1);
}
