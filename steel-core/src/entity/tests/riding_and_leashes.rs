use super::*;

#[test]
fn default_below_world_hook_discards_entity() {
    let entity = PushableTestEntity::shared(1, DVec3::ZERO);

    entity.on_below_world();

    assert!(entity.is_removed());
}

#[test]
fn base_entity_has_no_controlling_passenger() {
    let entity = PushableTestEntity::shared(1, DVec3::ZERO);

    assert!(entity.controlling_passenger().is_none());
    assert!(!entity.has_controlling_passenger());
}

#[test]
fn start_riding_entities_links_passenger_and_vehicle() {
    init_test_registry();

    let passenger = PushableTestEntity::shared(1, DVec3::ZERO);
    let vehicle = PushableTestEntity::shared(2, DVec3::ZERO);

    assert!(start_riding_entities(&passenger, &vehicle));

    assert!(passenger.is_passenger());
    assert_eq!(passenger.vehicle().map(|entity| entity.id()), Some(2));
    assert!(vehicle.has_passenger(passenger.as_ref()));
    assert_eq!(vehicle.first_passenger().map(|entity| entity.id()), Some(1));
    assert_eq!(passenger.pose(), EntityPose::Standing);
}

#[test]
fn transfer_leashables_to_holder_moves_valid_mobs() {
    init_test_registry();

    let old_holder: SharedEntity = Arc::new(PigEntity::new(
        &vanilla_entities::PIG,
        1,
        DVec3::ZERO,
        Weak::new(),
    ));
    let new_holder: SharedEntity = Arc::new(PigEntity::new(
        &vanilla_entities::PIG,
        2,
        DVec3::ZERO,
        Weak::new(),
    ));
    let leashable: SharedEntity = Arc::new(PigEntity::new(
        &vanilla_entities::PIG,
        3,
        DVec3::new(1.0, 0.0, 0.0),
        Weak::new(),
    ));
    let Some(mob) = leashable.as_mob() else {
        panic!("pig should expose mob behavior");
    };
    assert!(mob.set_leashed_to(&old_holder));

    assert!(transfer_leashables_to_holder(
        vec![Arc::clone(&leashable)],
        &new_holder
    ));

    let Some(holder) = mob.leash_holder() else {
        panic!("transferred mob should stay leashed");
    };
    assert_eq!(holder.id(), new_holder.id());
}

#[test]
fn transfer_leashables_to_holder_skips_mobs_outside_snap_distance() {
    init_test_registry();

    let old_holder: SharedEntity = Arc::new(PigEntity::new(
        &vanilla_entities::PIG,
        1,
        DVec3::ZERO,
        Weak::new(),
    ));
    let new_holder: SharedEntity = Arc::new(PigEntity::new(
        &vanilla_entities::PIG,
        2,
        DVec3::ZERO,
        Weak::new(),
    ));
    let leashable: SharedEntity = Arc::new(PigEntity::new(
        &vanilla_entities::PIG,
        3,
        DVec3::new(20.0, 0.0, 0.0),
        Weak::new(),
    ));
    let Some(mob) = leashable.as_mob() else {
        panic!("pig should expose mob behavior");
    };
    assert!(mob.set_leashed_to(&old_holder));

    assert!(!transfer_leashables_to_holder(
        vec![Arc::clone(&leashable)],
        &new_holder
    ));

    let Some(holder) = mob.leash_holder() else {
        panic!("untransferred mob should stay leashed");
    };
    assert_eq!(holder.id(), old_holder.id());
}

#[test]
fn set_leashed_to_notifies_replaced_holder() {
    init_test_registry();

    let old_holder_typed = LeashNotificationTestEntity::new(1);
    let old_holder: SharedEntity = old_holder_typed.clone();
    let new_holder_typed = LeashNotificationTestEntity::new(2);
    let new_holder: SharedEntity = new_holder_typed.clone();
    let leashable: SharedEntity = Arc::new(PigEntity::new(
        &vanilla_entities::PIG,
        3,
        DVec3::ZERO,
        Weak::new(),
    ));
    let Some(mob) = leashable.as_mob() else {
        panic!("pig should expose mob behavior");
    };

    assert!(mob.set_leashed_to(&old_holder));
    assert!(mob.set_leashed_to(&new_holder));

    assert_eq!(old_holder_typed.removed_notifications(), vec![3]);
    assert!(new_holder_typed.removed_notifications().is_empty());
}

#[test]
fn tick_leash_notifies_live_holder() {
    init_test_registry();

    let holder_typed = LeashNotificationTestEntity::new(1);
    let holder: SharedEntity = holder_typed.clone();
    let leashable: SharedEntity = Arc::new(PigEntity::new(
        &vanilla_entities::PIG,
        3,
        DVec3::ZERO,
        Weak::new(),
    ));
    let Some(mob) = leashable.as_mob() else {
        panic!("pig should expose mob behavior");
    };
    assert!(mob.set_leashed_to(&holder));

    mob.tick_leash();

    assert_eq!(holder_typed.holder_notifications(), vec![3]);
    assert!(mob.is_leashed());
    assert!(holder_typed.removed_notifications().is_empty());
}

#[test]
fn tick_leash_snaps_live_holder_past_snap_distance() {
    init_test_registry();

    let holder_typed = LeashNotificationTestEntity::with_position(1, DVec3::new(13.0, 0.0, 0.0));
    let holder: SharedEntity = holder_typed.clone();
    let leashable: SharedEntity = Arc::new(PigEntity::new(
        &vanilla_entities::PIG,
        3,
        DVec3::ZERO,
        Weak::new(),
    ));
    let Some(mob) = leashable.as_mob() else {
        panic!("pig should expose mob behavior");
    };
    assert!(mob.set_leashed_to(&holder));

    mob.tick_leash();

    assert_eq!(holder_typed.holder_notifications(), vec![3]);
    assert_eq!(holder_typed.removed_notifications(), vec![3]);
    assert!(!mob.is_leashed());
}

#[test]
fn start_riding_entities_respects_boarding_cooldown() {
    init_test_registry();

    let passenger = PushableTestEntity::shared(1, DVec3::ZERO);
    let vehicle = PushableTestEntity::shared(2, DVec3::ZERO);
    passenger.base().set_boarding_cooldown(2);

    assert!(!start_riding_entities(&passenger, &vehicle));
    assert!(!passenger.is_passenger());
    assert!(!vehicle.is_vehicle());
}

#[test]
fn start_riding_entities_rejects_vehicle_cycles() {
    init_test_registry();

    let root = PushableTestEntity::shared(1, DVec3::ZERO);
    let child = PushableTestEntity::shared(2, DVec3::ZERO);
    EntityBase::restore_passenger_relationship(&root, &child);

    assert!(!start_riding_entities(&root, &child));
    assert_eq!(child.vehicle().map(|entity| entity.id()), Some(1));
    assert_eq!(root.first_passenger().map(|entity| entity.id()), Some(2));
}

#[test]
fn start_riding_entities_inserts_player_passenger_first() {
    init_test_registry();

    let vehicle = MultiPassengerTestEntity::shared(1);
    let mob_passenger = PushableTestEntity::shared(2, DVec3::ZERO);
    let player_passenger =
        KnownMovementTestEntity::shared(3, &vanilla_entities::PLAYER, DVec3::ZERO, DVec3::ZERO);

    assert!(start_riding_entities(&mob_passenger, &vehicle));
    assert!(start_riding_entities(&player_passenger, &vehicle));

    let passenger_ids = vehicle
        .passengers()
        .into_iter()
        .map(|entity| entity.id())
        .collect::<Vec<_>>();
    assert_eq!(passenger_ids, vec![3, 2]);
}

#[test]
fn controlled_vehicle_uses_player_known_movement_and_speed() {
    let player_movement = DVec3::new(0.25, 0.0, -0.5);
    let player_speed = DVec3::new(0.5, 0.0, -1.0);
    let controller = KnownMovementTestEntity::shared(
        1,
        &vanilla_entities::PLAYER,
        player_movement,
        player_speed,
    );
    let vehicle = ControlledVehicleTestEntity::shared(2, Some(controller));

    assert!(vehicle.uses_client_movement_packets());
    assert!(!vehicle.is_server_driven_movement());
    assert!(!vehicle.can_simulate_movement());
    assert!(!vehicle.is_effective_ai());

    vehicle.set_velocity(DVec3::new(4.0, 0.0, 4.0));
    vehicle.base().advance_base_tick_state();
    vehicle.base().set_position_local(DVec3::new(2.0, 0.0, 0.0));
    vehicle.base().advance_base_tick_state();

    assert!(vehicle.has_controlling_passenger());
    assert_vec3_close(vehicle.known_movement(), player_movement);
    assert_vec3_close(vehicle.known_speed(), player_speed);

    vehicle.set_removed(RemovalReason::Discarded);

    assert_vec3_close(vehicle.known_movement(), DVec3::new(4.0, 0.0, 4.0));
    assert_vec3_close(vehicle.known_speed(), DVec3::new(2.0, 0.0, 0.0));
}

#[test]
fn controlled_vehicle_returns_direct_controlled_vehicle_not_root_vehicle() {
    init_test_registry();

    let passenger =
        KnownMovementTestEntity::shared(1, &vanilla_entities::PLAYER, DVec3::ZERO, DVec3::ZERO);
    let vehicle = ControlledVehicleTestEntity::shared(2, Some(Arc::clone(&passenger)));
    let root_vehicle = ControlledVehicleTestEntity::shared(3, None);

    assert!(start_riding_entities(&passenger, &vehicle));
    assert!(start_riding_entities(&vehicle, &root_vehicle));

    let Some(controlled_vehicle) = passenger.controlled_vehicle() else {
        panic!("passenger should directly control the middle vehicle");
    };
    let Some(root) = passenger.root_vehicle() else {
        panic!("passenger should have a root vehicle");
    };

    assert_eq!(controlled_vehicle.id(), vehicle.id());
    assert_eq!(root.id(), root_vehicle.id());
}

#[test]
fn controlled_vehicle_known_movement_falls_back_without_active_player_controller() {
    let non_player_controller = KnownMovementTestEntity::shared(
        1,
        &vanilla_entities::ZOMBIE,
        DVec3::new(0.25, 0.0, -0.5),
        DVec3::new(0.5, 0.0, -1.0),
    );
    let vehicle = ControlledVehicleTestEntity::shared(2, Some(non_player_controller));
    vehicle.set_velocity(DVec3::new(4.0, 0.0, 4.0));
    vehicle.base().advance_base_tick_state();
    vehicle.base().set_position_local(DVec3::new(2.0, 0.0, 0.0));
    vehicle.base().advance_base_tick_state();

    assert_vec3_close(vehicle.known_movement(), DVec3::new(4.0, 0.0, 4.0));
    assert_vec3_close(vehicle.known_speed(), DVec3::new(2.0, 0.0, 0.0));
}

#[test]
fn push_entity_separates_pushable_entities_like_vanilla() {
    let left = PushableTestEntity::shared(1, DVec3::ZERO);
    let right = PushableTestEntity::shared(2, DVec3::new(1.0, 0.0, 0.0));

    left.push_entity(right.as_ref());

    assert_vec3_close(left.velocity(), DVec3::new(-0.05, 0.0, 0.0));
    assert_vec3_close(right.velocity(), DVec3::new(0.05, 0.0, 0.0));
    assert!(left.needs_velocity_sync());
    assert!(right.needs_velocity_sync());
}
