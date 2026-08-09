use super::*;

#[test]
fn client_tracking_range_is_converted_to_blocks() {
    let range = EntityTrackingRange::from_client_chunk_range(4);

    assert!((range.visible_radius(10) - 64.0).abs() < f64::EPSILON);
}

#[test]
fn zero_client_tracking_range_disables_tracking() {
    let range = EntityTrackingRange::from_client_chunk_range(0);

    assert!(range.is_disabled());
}

#[test]
fn tracking_distance_uses_horizontal_circle() {
    let range = EntityTrackingRange::from_client_chunk_range(4);
    let entity_pos = DVec3::ZERO;

    assert!(is_within_tracking_distance(
        entity_pos,
        DVec3::new(64.0, 300.0, 0.0),
        range,
        8,
    ));
    assert!(!is_within_tracking_distance(
        entity_pos,
        DVec3::new(64.0, 0.0, 64.0),
        range,
        8,
    ));
    assert!(!is_within_tracking_distance(
        entity_pos,
        DVec3::new(64.1, 0.0, 0.0),
        range,
        8,
    ));
}

#[test]
fn tracking_distance_is_capped_by_player_view_distance() {
    let range = EntityTrackingRange::from_client_chunk_range(10);
    let entity_pos = DVec3::ZERO;

    assert!(is_within_tracking_distance(
        entity_pos,
        DVec3::new(32.0, 0.0, 0.0),
        range,
        2,
    ));
    assert!(!is_within_tracking_distance(
        entity_pos,
        DVec3::new(32.1, 0.0, 0.0),
        range,
        2,
    ));
}

#[test]
fn vehicle_effective_tracking_range_uses_widest_passenger_range() {
    init_vanilla_registry();

    let vehicle_typed = PairingTestEntity::new_with_type(1, &vanilla_entities::ITEM, Vec::new());
    let passenger_typed =
        PairingTestEntity::new_with_type(2, &vanilla_entities::PLAYER, Vec::new());
    assert!(
        passenger_typed.entity_type().client_tracking_range
            > vehicle_typed.entity_type().client_tracking_range
    );

    let passenger: SharedEntity = passenger_typed;
    vehicle_typed.add_passenger(&passenger);
    let vehicle: SharedEntity = vehicle_typed;
    let base_range =
        EntityTrackingRange::from_client_chunk_range(vehicle.entity_type().client_tracking_range);

    let effective = effective_tracking_range(vehicle.as_ref(), base_range);

    assert_eq!(
        effective.block_radius.to_bits(),
        (f64::from(passenger.entity_type().client_tracking_range) * BLOCKS_PER_CHUNK).to_bits()
    );
}
