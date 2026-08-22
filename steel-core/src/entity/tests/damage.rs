use super::*;

use crate::entity::living_entity::damage_event_packet;

#[test]
fn generic_living_hurt_applies_health_damage() {
    init_vanilla_registry();
    let entity = LivingFluidTestEntity::new(0.0, 0.0, true);
    let source = DamageSource::environment(&vanilla_damage_types::GENERIC);
    let initial_health = entity.get_health();
    let damage = 4.0;

    assert!(entity.hurt(test_world(), &source, damage));

    assert_f32_close(entity.get_health(), initial_health - damage);
}

#[test]
fn entity_backed_damage_packet_keeps_ids_and_omits_effective_position() {
    init_vanilla_registry();
    let direct_entity_id = 41;
    let damaged_entity_id = 42;
    let direct_position = DVec3::new(1.0, 64.0, 2.0);
    let direct: SharedEntity = Arc::new(PigEntity::new(
        &vanilla_entities::PIG,
        direct_entity_id,
        direct_position,
        Arc::downgrade(test_world()),
    ));
    let source = DamageSource::environment(&vanilla_damage_types::PLAYER_ATTACK)
        .with_direct_entity_reference(&direct)
        .with_causing_entity_reference(&direct);

    assert_eq!(
        source.effective_source_position(test_world()),
        Some(direct.position()),
    );
    let packet = damage_event_packet(damaged_entity_id, &source);
    let encoded_source_entity_id = direct.id() + 1;
    assert_eq!(packet.entity_id, damaged_entity_id);
    assert_eq!(packet.source_cause_id, encoded_source_entity_id);
    assert_eq!(packet.source_direct_id, encoded_source_entity_id);
    assert_eq!(packet.source_position, None);
}

#[test]
fn explicitly_positioned_damage_packet_retains_raw_position() {
    init_vanilla_registry();
    let damaged_entity_id = 43;
    let absent_source_entity_id = 0;
    let position = DVec3::new(3.0, 65.0, 4.0);
    let source = DamageSource::environment(&vanilla_damage_types::BAD_RESPAWN_POINT)
        .with_source_position(position);

    assert_eq!(
        source.effective_source_position(test_world()),
        Some(position)
    );
    let packet = damage_event_packet(damaged_entity_id, &source);
    assert_eq!(packet.entity_id, damaged_entity_id);
    assert_eq!(packet.source_cause_id, absent_source_entity_id);
    assert_eq!(packet.source_direct_id, absent_source_entity_id);
    assert_eq!(packet.source_position, Some(position));
}

#[test]
fn generic_living_hurt_ignores_fire_damage_with_fire_resistance() {
    init_vanilla_registry();
    let entity = LivingFluidTestEntity::new(0.0, 0.0, true);
    let fire_resistance_amplifier = 0;
    let initial_health = entity.get_health();
    let damage = 4.0;
    entity.set_mob_effect(
        vanilla_mob_effects::FIRE_RESISTANCE,
        fire_resistance_amplifier,
    );
    let source = DamageSource::environment(&vanilla_damage_types::LAVA);

    assert!(!entity.hurt(test_world(), &source, damage));

    assert_f32_close(entity.get_health(), initial_health);
}

#[test]
fn generic_living_hurt_processes_default_death_once() {
    init_vanilla_registry();
    let remaining_health = 3.0;
    let lethal_damage = remaining_health + 1.0;
    let post_death_damage = 1.0;
    let entity = LivingFluidTestEntity::new_in_world(0.0, 0.0, true, test_world())
        .with_health(remaining_health);
    let source = DamageSource::environment(&vanilla_damage_types::GENERIC);

    assert!(entity.hurt(test_world(), &source, lethal_damage));
    assert_f32_close(entity.get_health(), 0.0);
    assert_eq!(entity.pose(), EntityPose::Dying);
    assert!(!entity.hurt(test_world(), &source, post_death_damage));
}

#[test]
fn generic_living_hurt_applies_armor_and_absorption() {
    init_vanilla_registry();
    let entity = LivingFluidTestEntity::new(0.0, 0.0, true);
    let armor = 20.0;
    let absorption = 3.0_f32;
    let damage = 10.0;
    let expected_damage_after_armor = 4.0;
    let initial_health = entity.get_health();
    {
        let mut attributes = entity.attributes().lock();
        attributes.set_base_value(vanilla_attributes::ARMOR, armor);
        attributes.set_base_value(vanilla_attributes::MAX_ABSORPTION, f64::from(absorption));
    }
    entity.set_absorption_amount(absorption);
    let source = DamageSource::environment(&vanilla_damage_types::FIREWORKS);

    assert!(entity.hurt(test_world(), &source, damage));

    assert_f32_close(
        entity.get_health(),
        initial_health - (expected_damage_after_armor - absorption),
    );
    assert_f32_close(entity.get_absorption_amount(), 0.0);
}

#[test]
fn generic_living_hurt_applies_resistance() {
    init_vanilla_registry();
    let entity = LivingFluidTestEntity::new(0.0, 0.0, true);
    let resistance_amplifier = 0;
    let damage = 10.0;
    let expected_damage_after_resistance = 8.0;
    let initial_health = entity.get_health();
    entity.set_mob_effect(vanilla_mob_effects::RESISTANCE, resistance_amplifier);
    let source = DamageSource::environment(&vanilla_damage_types::FIREWORKS);

    assert!(entity.hurt(test_world(), &source, damage));

    assert_f32_close(
        entity.get_health(),
        initial_health - expected_damage_after_resistance,
    );
}

#[test]
fn damage_reductions_use_victim_attached_world() {
    init_vanilla_registry();
    let attached_world = cross_world_damage_test_world();
    let explicit_world = test_world();
    assert!(!Arc::ptr_eq(attached_world, explicit_world));

    let attacker_id = 1_750_001;
    let breach_level = 4;
    let victim_armor = 20.0;
    let damage = 10.0;
    let attacker = Arc::new(PigEntity::new(
        &vanilla_entities::PIG,
        attacker_id,
        DVec3::ZERO,
        Arc::downgrade(attached_world),
    ));
    let mut mace = ItemStack::new(&vanilla_items::MACE);
    mace.set_enchantments(
        &[(Identifier::vanilla_static("breach"), breach_level)],
        false,
    );
    attacker
        .living_base()
        .equipment()
        .lock()
        .set(EquipmentSlot::MainHand, mace);
    let attacker: SharedEntity = attacker;
    let registration = attached_world
        .entity_manager()
        .add_live_entity(attacker, EntityOwnership::External);
    assert!(registration.is_ok());

    let victim = LivingFluidTestEntity::new_in_world(0.0, 0.0, true, attached_world);
    victim
        .attributes()
        .lock()
        .set_base_value(vanilla_attributes::ARMOR, victim_armor);
    let initial_health = victim.get_health();
    let source = DamageSource::environment(&vanilla_damage_types::MOB_ATTACK)
        .with_causing_entity(attacker_id)
        .with_direct_entity(attacker_id);

    let damage_applied = victim.hurt(explicit_world, &source, damage);
    let health = victim.get_health();
    let removed = attached_world
        .entity_manager()
        .remove_live_entity(attacker_id, RemovalReason::Discarded);

    assert!(removed.is_some());
    assert!(damage_applied);
    assert_f32_close(health, initial_health - damage);
}

#[test]
fn generic_living_hurt_applies_damage_protection_enchantments() {
    init_vanilla_registry();
    let entity = LivingFluidTestEntity::new_in_world(0.0, 0.0, true, test_world());
    let protection_level = 4;
    let maximum_protection_points = 25.0_f32;
    let damage = 10.0;
    let initial_health = entity.get_health();
    let mut boots = ItemStack::new(&vanilla_items::DIAMOND_BOOTS);
    boots.set_enchantments(
        &[(Identifier::vanilla_static("protection"), protection_level)],
        false,
    );
    entity.equip(EquipmentSlot::Feet, boots);
    let source = DamageSource::environment(&vanilla_damage_types::FIREWORKS);

    assert!(entity.hurt(test_world(), &source, damage));

    let expected_health =
        initial_health - damage * (1.0 - protection_level as f32 / maximum_protection_points);
    assert_eq!(entity.get_health().to_bits(), expected_health.to_bits());
}

#[test]
fn generic_living_default_does_not_damage_armor_equipment() {
    init_vanilla_registry();
    let entity = LivingFluidTestEntity::new(0.0, 0.0, true);
    let damage = 10.0;
    entity.equip(
        EquipmentSlot::Chest,
        ItemStack::new(&vanilla_items::DIAMOND_CHESTPLATE),
    );
    let source = DamageSource::environment(&vanilla_damage_types::FIREWORKS);

    assert!(entity.hurt(test_world(), &source, damage));

    entity.with_equipment_slot(EquipmentSlot::Chest, &mut |item| {
        assert_eq!(item.get_damage_value(), 0);
    });
}

#[test]
fn generic_living_hurt_applies_source_position_knockback() {
    init_vanilla_registry();
    let entity = LivingFluidTestEntity::new(0.0, 0.0, true);
    let damage = 4.0;
    entity.set_on_ground(true);
    let source = DamageSource::environment(&vanilla_damage_types::PLAYER_ATTACK)
        .with_source_position(DVec3::X);

    assert!(entity.hurt(test_world(), &source, damage));

    assert_vec3_close(
        entity.velocity(),
        DVec3::new(-DAMAGE_KNOCKBACK_POWER, DAMAGE_KNOCKBACK_POWER, 0.0),
    );
    assert!(entity.needs_velocity_sync());
}

#[test]
fn try_as_dyn_exposes_living_entity_behavior() {
    init_vanilla_registry();
    let entity = LivingFluidTestEntity::new(0.0, 0.0, true);
    let initial_health = entity.get_health();
    let entity_ref: &dyn Entity = &entity;
    let Some(living) = entity_ref.as_living_entity() else {
        panic!("living test entity did not expose LivingEntity behavior");
    };

    assert_f32_close(living.get_health(), initial_health);

    let non_living_entity_id = 2;
    let non_living = PushableTestEntity::shared(non_living_entity_id, DVec3::ZERO);
    assert!(non_living.as_living_entity().is_none());
}

#[test]
fn head_yaw_uses_living_head_rotation_only() {
    init_vanilla_registry();
    let living = LivingFluidTestEntity::new(0.0, 0.0, true);
    let body_yaw = 35.0;
    let head_yaw = 120.0;
    living.set_rotation((body_yaw, 0.0));
    living.set_y_head_rot(head_yaw);

    assert_f32_close(Entity::head_yaw(&living), head_yaw);

    let non_living_entity_id = 2;
    let non_living = PushableTestEntity::shared(non_living_entity_id, DVec3::ZERO);
    non_living.set_rotation((body_yaw, 0.0));
    assert_f32_close(non_living.head_yaw(), 0.0);
}

#[test]
fn living_equipment_attribute_modifiers_refresh_for_slot() {
    init_vanilla_registry();
    let entity = LivingFluidTestEntity::new(0.0, 0.0, true);
    let helmet_armor = 3.0;
    let helmet_toughness = 2.0;
    let (base_armor, base_toughness) = {
        let attributes = entity.attributes().lock();
        (
            attributes.required_value(vanilla_attributes::ARMOR),
            attributes.required_value(vanilla_attributes::ARMOR_TOUGHNESS),
        )
    };

    entity.equip(
        EquipmentSlot::Head,
        ItemStack::new(&vanilla_items::DIAMOND_HELMET),
    );
    LivingEntity::refresh_equipment_attribute_modifiers(&entity, EquipmentSlot::Head);

    {
        let attributes = entity.attributes().lock();
        assert_eq!(
            attributes
                .required_value(vanilla_attributes::ARMOR)
                .to_bits(),
            (base_armor + helmet_armor).to_bits()
        );
        assert_eq!(
            attributes
                .required_value(vanilla_attributes::ARMOR_TOUGHNESS)
                .to_bits(),
            (base_toughness + helmet_toughness).to_bits()
        );
    }

    entity.equip(EquipmentSlot::Head, ItemStack::empty());
    LivingEntity::refresh_equipment_attribute_modifiers(&entity, EquipmentSlot::Head);

    let attributes = entity.attributes().lock();
    assert_eq!(
        attributes
            .required_value(vanilla_attributes::ARMOR)
            .to_bits(),
        base_armor.to_bits()
    );
    assert_eq!(
        attributes
            .required_value(vanilla_attributes::ARMOR_TOUGHNESS)
            .to_bits(),
        base_toughness.to_bits()
    );
}

#[test]
fn generic_living_hurt_respects_no_knockback_damage_tag() {
    init_vanilla_registry();
    let entity = LivingFluidTestEntity::new(0.0, 0.0, true);
    let initial_velocity = DVec3::new(0.2, 0.3, -0.1);
    let damage = 4.0;
    entity.set_on_ground(true);
    entity.set_velocity(initial_velocity);
    let source =
        DamageSource::environment(&vanilla_damage_types::DROWN).with_source_position(DVec3::X);

    assert!(entity.hurt(test_world(), &source, damage));

    assert_vec3_close(entity.velocity(), initial_velocity);
    assert!(!entity.needs_velocity_sync());
}

#[test]
fn generic_living_hurt_scales_knockback_by_resistance() {
    init_vanilla_registry();
    let entity = LivingFluidTestEntity::new(0.0, 0.0, true);
    let knockback_resistance = 0.5;
    let damage = 4.0;
    entity.set_on_ground(true);
    entity.attributes().lock().set_base_value(
        vanilla_attributes::KNOCKBACK_RESISTANCE,
        knockback_resistance,
    );
    let source = DamageSource::environment(&vanilla_damage_types::PLAYER_ATTACK)
        .with_source_position(DVec3::X);

    assert!(entity.hurt(test_world(), &source, damage));

    let expected_knockback = DAMAGE_KNOCKBACK_POWER * (1.0 - knockback_resistance);
    assert_vec3_close(
        entity.velocity(),
        DVec3::new(-expected_knockback, expected_knockback, 0.0),
    );
}
