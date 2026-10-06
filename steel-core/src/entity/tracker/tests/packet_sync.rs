use std::cell::RefCell;

use super::*;

#[test]
fn send_changes_broadcasts_dirty_attributes_once() {
    init_vanilla_registry();

    let tracker = EntityTracker::new();
    let entity_typed = PairingTestEntity::new(1, Vec::new());
    let entity: SharedEntity = entity_typed.clone();
    tracker.add(&entity, |_| Vec::new(), |_| None);

    entity_typed.set_dirty_attributes(vec![AttributeSnapshot {
        attribute_id: 7,
        base_value: 2.5,
        modifiers: Vec::new(),
    }]);

    let mut updates = Vec::new();
    tracker.send_changes(
        |_| Vec::new(),
        |_| None,
        EntityChangeSenders {
            movement: |_, _| {},
            self_movement: |_, _| {},
            entity_data: |_, _| {},
            attributes: |entity_id, attributes| updates.push((entity_id, attributes)),
            mob_effects: |_, _| {},
            equipment: |_, _| {},
            passengers: |_, _| {},
            entity_link: |_, _| {},
        },
    );

    assert_eq!(updates.len(), 1);
    assert_eq!(updates[0].0, 1);
    assert_eq!(updates[0].1.len(), 1);
    assert_eq!(updates[0].1[0].attribute_id, 7);
    assert_eq!(updates[0].1[0].base_value.to_bits(), 2.5_f64.to_bits());

    updates.clear();
    tracker.send_changes(
        |_| Vec::new(),
        |_| None,
        EntityChangeSenders {
            movement: |_, _| {},
            self_movement: |_, _| {},
            entity_data: |_, _| {},
            attributes: |entity_id, attributes| updates.push((entity_id, attributes)),
            mob_effects: |_, _| {},
            equipment: |_, _| {},
            passengers: |_, _| {},
            entity_link: |_, _| {},
        },
    );
    assert!(updates.is_empty());
}

#[test]
fn send_changes_broadcasts_dirty_equipment_once() {
    init_vanilla_registry();

    let tracker = EntityTracker::new();
    let entity_typed = PairingTestEntity::new(1, Vec::new());
    let entity: SharedEntity = entity_typed.clone();
    tracker.add(&entity, |_| Vec::new(), |_| None);

    let stack = ItemStack::new(&vanilla_items::ELYTRA);
    entity_typed.set_dirty_equipment(vec![EquipmentSlotItem {
        slot: EquipmentSlot::Chest,
        item_stack: stack.clone(),
    }]);

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
            equipment: |entity_id, packet| updates.push((entity_id, packet)),
            passengers: |_, _| {},
            entity_link: |_, _| {},
        },
    );

    assert_eq!(updates.len(), 1);
    assert_eq!(updates[0].0, 1);
    assert_eq!(updates[0].1.entity_id, 1);
    assert_eq!(updates[0].1.slots.len(), 1);
    assert_eq!(updates[0].1.slots[0].slot, EquipmentSlot::Chest);
    assert_eq!(updates[0].1.slots[0].item_stack, stack);

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
            equipment: |entity_id, packet| updates.push((entity_id, packet)),
            passengers: |_, _| {},
            entity_link: |_, _| {},
        },
    );
    assert!(updates.is_empty());
}

#[test]
fn send_changes_broadcasts_equipment_before_attributes() {
    init_vanilla_registry();

    let tracker = EntityTracker::new();
    let entity_typed = PairingTestEntity::new(1, Vec::new());
    let entity: SharedEntity = entity_typed.clone();
    tracker.add(&entity, |_| Vec::new(), |_| None);

    entity_typed.set_dirty_equipment(vec![EquipmentSlotItem {
        slot: EquipmentSlot::Chest,
        item_stack: ItemStack::new(&vanilla_items::ELYTRA),
    }]);
    entity_typed.set_dirty_attributes(vec![AttributeSnapshot {
        attribute_id: 7,
        base_value: 2.5,
        modifiers: Vec::new(),
    }]);

    let updates = RefCell::new(Vec::new());
    tracker.send_changes(
        |_| Vec::new(),
        |_| None,
        EntityChangeSenders {
            movement: |_, _| {},
            self_movement: |_, _| {},
            entity_data: |_, _| {},
            attributes: |_, _| updates.borrow_mut().push("attributes"),
            mob_effects: |_, _| {},
            equipment: |_, _| updates.borrow_mut().push("equipment"),
            passengers: |_, _| {},
            entity_link: |_, _| {},
        },
    );

    assert_eq!(*updates.borrow(), ["equipment", "attributes"]);
}

#[test]
fn send_changes_syncs_hurt_marked_player_motion_to_self() {
    init_vanilla_registry();

    let tracker = EntityTracker::new();
    let entity_typed = PairingTestEntity::new_with_type(1, &vanilla_entities::PLAYER, Vec::new());
    let entity: SharedEntity = entity_typed.clone();
    track_entity_for_player(&tracker, &entity, 99);

    entity_typed.set_velocity(DVec3::new(0.25, 0.4, -0.125));
    entity_typed.mark_hurt();

    let mut tracker_updates = Vec::new();
    let mut self_updates = Vec::new();
    tracker.send_changes(
        |_| Vec::new(),
        |_| None,
        EntityChangeSenders {
            movement: |entity_id, packet| tracker_updates.push((entity_id, packet)),
            self_movement: |player_id, packet| self_updates.push((player_id, packet)),
            entity_data: |_, _| {},
            attributes: |_, _| {},
            mob_effects: |_, _| {},
            equipment: |_, _| {},
            passengers: |_, _| {},
            entity_link: |_, _| {},
        },
    );

    assert_has_velocity_packet(&tracker_updates, 1, DVec3::new(0.25, 0.4, -0.125));

    assert_eq!(self_updates.len(), 1);
    assert_eq!(self_updates[0].0, 1);
    let EntityMovementSyncPacket::Velocity(packet) = &self_updates[0].1 else {
        panic!(
            "expected velocity self-motion packet, got {:?}",
            self_updates[0].1
        );
    };
    assert_eq!(packet.entity_id, 1);
    assert_eq!(packet.vel.x.to_bits(), 0.25_f64.to_bits());
    assert_eq!(packet.vel.y.to_bits(), 0.4_f64.to_bits());
    assert_eq!(packet.vel.z.to_bits(), (-0.125_f64).to_bits());
    assert!(!entity_typed.hurt_marked());
}

#[test]
fn send_changes_broadcasts_hurt_marked_non_player_motion() {
    init_vanilla_registry();

    let tracker = EntityTracker::new();
    let entity_typed = PairingTestEntity::new(1, Vec::new());
    let entity: SharedEntity = entity_typed.clone();
    track_entity_for_player(&tracker, &entity, 99);

    entity_typed.set_velocity(DVec3::new(-0.25, 0.2, 0.125));
    entity_typed.mark_hurt();

    let mut tracker_updates = Vec::new();
    let mut self_updates = Vec::new();
    tracker.send_changes(
        |_| Vec::new(),
        |_| None,
        EntityChangeSenders {
            movement: |entity_id, packet| tracker_updates.push((entity_id, packet)),
            self_movement: |player_id, packet| self_updates.push((player_id, packet)),
            entity_data: |_, _| {},
            attributes: |_, _| {},
            mob_effects: |_, _| {},
            equipment: |_, _| {},
            passengers: |_, _| {},
            entity_link: |_, _| {},
        },
    );

    assert_has_velocity_packet(&tracker_updates, 1, DVec3::new(-0.25, 0.2, 0.125));
    assert!(self_updates.is_empty());
    assert!(!entity_typed.hurt_marked());
}
