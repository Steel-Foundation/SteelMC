use super::*;

#[test]
fn spawn_pairing_includes_syncable_attributes() {
    test_support::init_test_registry();

    let entity = PairingTestEntity::shared(vec![AttributeSnapshot {
        attribute_id: 7,
        base_value: 1.25,
        modifiers: Vec::new(),
    }]);
    let pairing = EntitySpawnPairing::from_entity(&entity, Vec::new());

    assert_eq!(pairing.spawn_packet.id, entity.id());
    assert_eq!(pairing.attributes.len(), 1);
    assert_eq!(pairing.attributes[0].attribute_id, 7);
    assert_eq!(
        pairing.attributes[0].base_value.to_bits(),
        1.25_f64.to_bits()
    );
}

#[test]
fn spawn_pairing_includes_non_empty_equipment() {
    test_support::init_test_registry();

    let entity_typed = PairingTestEntity::new(1, Vec::new());
    let stack = ItemStack::new(&vanilla_items::ELYTRA);
    entity_typed.set_equipment(vec![EquipmentSlotItem {
        slot: EquipmentSlot::Chest,
        item_stack: stack.clone(),
    }]);
    let entity: SharedEntity = entity_typed;
    let pairing = EntitySpawnPairing::from_entity(&entity, Vec::new());

    assert_eq!(pairing.spawn_packet.id, entity.id());
    assert_eq!(pairing.equipment.len(), 1);
    assert_eq!(pairing.equipment[0].slot, EquipmentSlot::Chest);
    assert_eq!(pairing.equipment[0].item_stack, stack);
}

#[test]
fn spawn_pairing_uses_entity_spawn_packet_position() {
    test_support::init_test_registry();

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
