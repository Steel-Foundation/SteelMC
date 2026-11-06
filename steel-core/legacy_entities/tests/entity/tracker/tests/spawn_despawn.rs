// Moved from entity/tracker/tests/spawn_despawn.rs with the entities it depends on.

#[test]
fn spawn_pairing_uses_entity_spawn_packet_position() {
    init_vanilla_registry();

    let entity: SharedEntity = Arc::new(LeashFenceKnotEntity::new_attached(
        &vanilla_entities::LEASH_KNOT,
        1,
        BlockPos::new(4, 65, -9),
        Weak::new(),
    ));
    let pairing = EntitySpawnPairing::from_entity(&entity, Vec::new());

    assert_eq!(pairing.spawn_packet.position.x.to_bits(), 4.0_f64.to_bits());
    assert_eq!(
        pairing.spawn_packet.position.y.to_bits(),
        65.0_f64.to_bits()
    );
    assert_eq!(
        pairing.spawn_packet.position.z.to_bits(),
        (-9.0_f64).to_bits()
    );
}
