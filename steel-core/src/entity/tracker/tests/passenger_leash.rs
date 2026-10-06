use super::*;

#[test]
fn spawn_pairing_omits_untracked_passenger_for_vehicle() {
    init_vanilla_registry();

    let tracker = EntityTracker::new();
    let vehicle_typed = PairingTestEntity::new(1, Vec::new());
    let passenger_typed = PairingTestEntity::new(2, Vec::new());
    let passenger: SharedEntity = passenger_typed;
    vehicle_typed.add_passenger(&passenger);
    let vehicle: SharedEntity = vehicle_typed;

    let pairing = tracker.spawn_pairing(&vehicle, 99);

    assert!(pairing.passenger_packets.is_empty());
}

#[test]
fn spawn_pairing_includes_tracked_passenger_packet_for_vehicle() {
    init_vanilla_registry();

    let tracker = EntityTracker::new();
    let vehicle_typed = PairingTestEntity::new(1, Vec::new());
    let passenger_typed = PairingTestEntity::new(2, Vec::new());
    let passenger: SharedEntity = passenger_typed;
    vehicle_typed.add_passenger(&passenger);
    track_entity_for_player(&tracker, &passenger, 99);
    let vehicle: SharedEntity = vehicle_typed;

    let pairing = tracker.spawn_pairing(&vehicle, 99);

    assert_eq!(pairing.passenger_packets.len(), 1);
    assert_eq!(pairing.passenger_packets[0].vehicle_id, 1);
    assert_eq!(pairing.passenger_packets[0].passenger_ids, vec![2]);
}

#[test]
fn spawn_pairing_for_passenger_omits_untracked_vehicle_packet() {
    init_vanilla_registry();

    let tracker = EntityTracker::new();
    let vehicle_typed = PairingTestEntity::new(1, Vec::new());
    let passenger_typed = PairingTestEntity::new(2, Vec::new());
    let passenger: SharedEntity = passenger_typed.clone();
    vehicle_typed.add_passenger(&passenger);
    let vehicle: SharedEntity = vehicle_typed;
    passenger_typed.set_vehicle(&vehicle);

    let pairing = tracker.spawn_pairing(&passenger, 99);

    assert!(pairing.passenger_packets.is_empty());
}

#[test]
fn spawn_pairing_for_passenger_includes_tracked_vehicle_passenger_packet() {
    init_vanilla_registry();

    let tracker = EntityTracker::new();
    let vehicle_typed = PairingTestEntity::new(1, Vec::new());
    let passenger_typed = PairingTestEntity::new(2, Vec::new());
    let passenger: SharedEntity = passenger_typed.clone();
    vehicle_typed.add_passenger(&passenger);
    let vehicle: SharedEntity = vehicle_typed;
    passenger_typed.set_vehicle(&vehicle);
    track_entity_for_player(&tracker, &vehicle, 99);
    track_entity_for_player(&tracker, &passenger, 99);

    let pairing = tracker.spawn_pairing(&passenger, 99);

    assert_eq!(pairing.passenger_packets.len(), 1);
    assert_eq!(pairing.passenger_packets[0].vehicle_id, 1);
    assert_eq!(pairing.passenger_packets[0].passenger_ids, vec![2]);
}

#[test]
fn spawn_pairing_includes_live_mob_leash_link_packet() {
    init_vanilla_registry();

    let tracker = EntityTracker::new();
    let pig_typed = Arc::new(PigEntity::new(
        &vanilla_entities::PIG,
        1,
        DVec3::ZERO,
        Weak::new(),
    ));
    let holder: SharedEntity = PairingTestEntity::new(2, Vec::new());
    assert!(pig_typed.set_leashed_to(&holder));
    let pig: SharedEntity = pig_typed;

    let pairing = tracker.spawn_pairing(&pig, 99);

    assert_eq!(pairing.entity_link_packet, Some(CSetEntityLink::new(1, 2)));
}

#[test]
fn send_changes_broadcasts_leash_link_changes_once() {
    init_vanilla_registry();

    let tracker = EntityTracker::new();
    let pig: SharedEntity = Arc::new(PigEntity::new(
        &vanilla_entities::PIG,
        1,
        DVec3::ZERO,
        Weak::new(),
    ));
    let holder: SharedEntity = PairingTestEntity::new(2, Vec::new());
    track_entity_for_player(&tracker, &pig, 99);
    let Some(pig_mob) = pig.as_mob() else {
        panic!("pig should expose mob behavior");
    };

    let mut updates = Vec::new();
    tracker.send_changes(
        |_| Vec::new(),
        |_| None,
        EntityChangeSenders {
            movement: |_, _| {},
            self_movement: |_, _| {},
            entity_data: |_, _| {},
            attributes: |_, _| {},
            mob_effects: |_, _| {},
            equipment: |_, _| {},
            passengers: |_, _| {},
            entity_link: |entity_id, packet| updates.push((entity_id, packet)),
        },
    );
    assert_eq!(updates.len(), 0);

    assert!(pig_mob.set_leashed_to(&holder));
    tracker.send_changes(
        |_| Vec::new(),
        |_| None,
        EntityChangeSenders {
            movement: |_, _| {},
            self_movement: |_, _| {},
            entity_data: |_, _| {},
            attributes: |_, _| {},
            mob_effects: |_, _| {},
            equipment: |_, _| {},
            passengers: |_, _| {},
            entity_link: |entity_id, packet| updates.push((entity_id, packet)),
        },
    );
    assert_eq!(updates, vec![(1, CSetEntityLink::new(1, 2))]);

    updates.clear();
    tracker.send_changes(
        |_| Vec::new(),
        |_| None,
        EntityChangeSenders {
            movement: |_, _| {},
            self_movement: |_, _| {},
            entity_data: |_, _| {},
            attributes: |_, _| {},
            mob_effects: |_, _| {},
            equipment: |_, _| {},
            passengers: |_, _| {},
            entity_link: |entity_id, packet| updates.push((entity_id, packet)),
        },
    );
    assert_eq!(updates.len(), 0);

    pig_mob.remove_leash_state();
    tracker.send_changes(
        |_| Vec::new(),
        |_| None,
        EntityChangeSenders {
            movement: |_, _| {},
            self_movement: |_, _| {},
            entity_data: |_, _| {},
            attributes: |_, _| {},
            mob_effects: |_, _| {},
            equipment: |_, _| {},
            passengers: |_, _| {},
            entity_link: |entity_id, packet| updates.push((entity_id, packet)),
        },
    );
    assert_eq!(updates, vec![(1, CSetEntityLink::new(1, 0))]);
}

#[test]
fn send_changes_broadcasts_passenger_changes_once() {
    init_vanilla_registry();

    let tracker = EntityTracker::new();
    let vehicle_typed = PairingTestEntity::new(1, Vec::new());
    let vehicle: SharedEntity = vehicle_typed.clone();
    let passenger_typed = PairingTestEntity::new(2, Vec::new());
    let passenger: SharedEntity = passenger_typed;
    track_entity_for_player(&tracker, &vehicle, 99);
    track_entity_for_player(&tracker, &passenger, 99);

    let mut updates = Vec::new();
    tracker.send_changes(
        |_| Vec::new(),
        |_| None,
        EntityChangeSenders {
            movement: |_, _| {},
            self_movement: |_, _| {},
            entity_data: |_, _| {},
            attributes: |_, _| {},
            mob_effects: |_, _| {},
            equipment: |_, _| {},
            passengers: |player_id, packet| {
                updates.push((player_id, packet));
            },
            entity_link: |_, _| {},
        },
    );
    assert!(updates.is_empty());

    vehicle_typed.add_passenger(&passenger);
    tracker.send_changes(
        |_| Vec::new(),
        |_| None,
        EntityChangeSenders {
            movement: |_, _| {},
            self_movement: |_, _| {},
            entity_data: |_, _| {},
            attributes: |_, _| {},
            mob_effects: |_, _| {},
            equipment: |_, _| {},
            passengers: |player_id, packet| {
                updates.push((player_id, packet));
            },
            entity_link: |_, _| {},
        },
    );
    assert_eq!(updates.len(), 1);
    assert_eq!(updates[0].0, 99);
    assert_eq!(updates[0].1.vehicle_id, 1);
    assert_eq!(updates[0].1.passenger_ids, vec![2]);

    updates.clear();
    tracker.send_changes(
        |_| Vec::new(),
        |_| None,
        EntityChangeSenders {
            movement: |_, _| {},
            self_movement: |_, _| {},
            entity_data: |_, _| {},
            attributes: |_, _| {},
            mob_effects: |_, _| {},
            equipment: |_, _| {},
            passengers: |player_id, packet| {
                updates.push((player_id, packet));
            },
            entity_link: |_, _| {},
        },
    );
    assert!(updates.is_empty());

    vehicle_typed.clear_passengers();
    mark_seen_by_player(&tracker, 1, 99);
    tracker.send_changes(
        |_| Vec::new(),
        |_| None,
        EntityChangeSenders {
            movement: |_, _| {},
            self_movement: |_, _| {},
            entity_data: |_, _| {},
            attributes: |_, _| {},
            mob_effects: |_, _| {},
            equipment: |_, _| {},
            passengers: |player_id, packet| {
                updates.push((player_id, packet));
            },
            entity_link: |_, _| {},
        },
    );
    assert_eq!(updates.len(), 1);
    assert_eq!(updates[0].0, 99);
    assert_eq!(updates[0].1.vehicle_id, 1);
    assert_eq!(updates[0].1.passenger_ids.len(), 0);
}

#[test]
fn send_changes_removes_untracked_passenger_from_vehicle_packet() {
    init_vanilla_registry();

    let tracker = EntityTracker::new();
    let vehicle_typed = PairingTestEntity::new(1, Vec::new());
    let passenger_typed = PairingTestEntity::new(2, Vec::new());
    let passenger: SharedEntity = passenger_typed;
    vehicle_typed.add_passenger(&passenger);
    let vehicle: SharedEntity = vehicle_typed;
    track_entity_for_player(&tracker, &passenger, 99);
    track_entity_for_player(&tracker, &vehicle, 99);

    let _ = tracker.entities.remove_sync(&passenger.id());

    let mut updates = Vec::new();
    tracker.send_changes(
        |_| Vec::new(),
        |_| None,
        EntityChangeSenders {
            movement: |_, _| {},
            self_movement: |_, _| {},
            entity_data: |_, _| {},
            attributes: |_, _| {},
            mob_effects: |_, _| {},
            equipment: |_, _| {},
            passengers: |player_id, packet| {
                updates.push((player_id, packet));
            },
            entity_link: |_, _| {},
        },
    );

    assert_eq!(updates.len(), 1);
    assert_eq!(updates[0].0, 99);
    assert_eq!(updates[0].1.vehicle_id, 1);
    assert_eq!(updates[0].1.passenger_ids.len(), 0);
}
