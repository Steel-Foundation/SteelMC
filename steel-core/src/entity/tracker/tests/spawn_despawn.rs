use super::*;

#[test]
fn spawn_pairing_includes_syncable_attributes() {
    init_vanilla_registry();

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
    init_vanilla_registry();

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
