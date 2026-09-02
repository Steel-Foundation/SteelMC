use super::*;

#[test]
fn generic_living_hurt_applies_health_damage() {
    init_vanilla_registry();
    let entity = LivingFluidTestEntity::new(0.0, 0.0, true);
    let source = DamageSource::environment(&vanilla_damage_types::GENERIC);

    assert!(entity.hurt(test_world(), &source, 4.0));

    assert_f32_close(entity.get_health(), 16.0);
}

#[test]
fn generic_living_hurt_ignores_fire_damage_with_fire_resistance() {
    init_vanilla_registry();
    let entity = LivingFluidTestEntity::new(0.0, 0.0, true);
    entity.set_mob_effect(vanilla_mob_effects::FIRE_RESISTANCE, 0);
    let source = DamageSource::environment(&vanilla_damage_types::LAVA);

    assert!(!entity.hurt(test_world(), &source, 4.0));

    assert_f32_close(entity.get_health(), 20.0);
}

#[test]
fn generic_living_hurt_processes_default_death_once() {
    init_vanilla_registry();
    let entity = LivingFluidTestEntity::new_in_world(0.0, 0.0, true, test_world()).with_health(3.0);
    let source = DamageSource::environment(&vanilla_damage_types::GENERIC);

    assert!(entity.hurt(test_world(), &source, 4.0));
    assert_f32_close(entity.get_health(), 0.0);
    assert_eq!(entity.pose(), EntityPose::Dying);
    assert!(!entity.hurt(test_world(), &source, 1.0));
}

#[test]
fn generic_living_hurt_applies_armor_and_absorption() {
    init_vanilla_registry();
    let entity = LivingFluidTestEntity::new(0.0, 0.0, true);
    {
        let mut attributes = entity.attributes().lock();
        attributes.set_base_value(vanilla_attributes::ARMOR, 20.0);
        attributes.set_base_value(vanilla_attributes::MAX_ABSORPTION, 3.0);
    }
    entity.set_absorption_amount(3.0);
    let source = DamageSource::environment(&vanilla_damage_types::FIREWORKS);

    assert!(entity.hurt(test_world(), &source, 10.0));

    assert_f32_close(entity.get_health(), 19.0);
    assert_f32_close(entity.get_absorption_amount(), 0.0);
}

#[test]
fn generic_living_hurt_applies_resistance() {
    init_vanilla_registry();
    let entity = LivingFluidTestEntity::new(0.0, 0.0, true);
    entity.set_mob_effect(vanilla_mob_effects::RESISTANCE, 0);
    let source = DamageSource::environment(&vanilla_damage_types::FIREWORKS);

    assert!(entity.hurt(test_world(), &source, 10.0));

    assert_f32_close(entity.get_health(), 12.0);
}

#[test]
fn damage_reductions_use_victim_attached_world() {
    init_vanilla_registry();
    let attached_world = cross_world_damage_test_world();
    let explicit_world = test_world();
    assert!(!Arc::ptr_eq(attached_world, explicit_world));

    let attacker_id = 1_750_001;
    let attacker = Arc::new(PigEntity::new(
        &vanilla_entities::PIG,
        attacker_id,
        DVec3::ZERO,
        Arc::downgrade(attached_world),
    ));
    let mut mace = ItemStack::new(&vanilla_items::MACE);
    mace.set_enchantments(&[(Identifier::vanilla_static("breach"), 4)], false);
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
        .set_base_value(vanilla_attributes::ARMOR, 20.0);
    let source = DamageSource::environment(&vanilla_damage_types::MOB_ATTACK)
        .with_causing_entity(attacker_id)
        .with_direct_entity(attacker_id);

    let damage_applied = victim.hurt(explicit_world, &source, 10.0);
    let health = victim.get_health();
    let removed = attached_world
        .entity_manager()
        .remove_live_entity(attacker_id, RemovalReason::Discarded);

    assert!(removed.is_some());
    assert!(damage_applied);
    assert_f32_close(health, 10.0);
}

#[test]
fn generic_living_hurt_applies_damage_protection_enchantments() {
    init_vanilla_registry();
    let entity = LivingFluidTestEntity::new_in_world(0.0, 0.0, true, test_world());
    let mut boots = ItemStack::new(&vanilla_items::DIAMOND_BOOTS);
    boots.set_enchantments(&[(Identifier::vanilla_static("protection"), 4)], false);
    entity.equip(EquipmentSlot::Feet, boots);
    let source = DamageSource::environment(&vanilla_damage_types::FIREWORKS);

    assert!(entity.hurt(test_world(), &source, 10.0));

    let expected_health = 20.0_f32 - 10.0_f32 * (1.0 - 4.0_f32 / 25.0);
    assert_eq!(entity.get_health().to_bits(), expected_health.to_bits());
}

#[test]
fn generic_living_default_does_not_damage_armor_equipment() {
    init_vanilla_registry();
    let entity = LivingFluidTestEntity::new(0.0, 0.0, true);
    entity.equip(
        EquipmentSlot::Chest,
        ItemStack::new(&vanilla_items::DIAMOND_CHESTPLATE),
    );
    let source = DamageSource::environment(&vanilla_damage_types::FIREWORKS);

    assert!(entity.hurt(test_world(), &source, 10.0));

    entity.with_equipment_slot(EquipmentSlot::Chest, &mut |item| {
        assert_eq!(item.get_damage_value(), 0);
    });
}

#[test]
fn generic_living_hurt_applies_source_position_knockback() {
    init_vanilla_registry();
    let entity = LivingFluidTestEntity::new(0.0, 0.0, true);
    entity.set_on_ground(true);
    let source = DamageSource::environment(&vanilla_damage_types::PLAYER_ATTACK)
        .with_source_position(DVec3::new(1.0, 0.0, 0.0));

    assert!(entity.hurt(test_world(), &source, 4.0));

    assert_vec3_close(
        entity.velocity(),
        DVec3::new(-DAMAGE_KNOCKBACK_POWER, 0.4, 0.0),
    );
    assert!(entity.needs_velocity_sync());
}

#[test]
fn try_as_dyn_exposes_living_entity_behavior() {
    init_vanilla_registry();
    let entity = LivingFluidTestEntity::new(0.0, 0.0, true);
    let entity_ref: &dyn Entity = &entity;
    let Some(living) = entity_ref.as_living_entity() else {
        panic!("living test entity did not expose LivingEntity behavior");
    };

    assert_f32_close(living.get_health(), 20.0);

    let non_living = PushableTestEntity::shared(2, DVec3::ZERO);
    assert!(non_living.as_living_entity().is_none());
}

#[test]
fn head_yaw_uses_living_head_rotation_only() {
    init_vanilla_registry();
    let living = LivingFluidTestEntity::new(0.0, 0.0, true);
    living.set_rotation((35.0, 0.0));
    living.set_y_head_rot(120.0);

    assert_f32_close(Entity::head_yaw(&living), 120.0);

    let non_living = PushableTestEntity::shared(2, DVec3::ZERO);
    non_living.set_rotation((35.0, 0.0));
    assert_f32_close(non_living.head_yaw(), 0.0);
}

#[test]
fn living_equipment_attribute_modifiers_refresh_for_slot() {
    init_vanilla_registry();
    let entity = LivingFluidTestEntity::new(0.0, 0.0, true);
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
            (base_armor + 3.0).to_bits()
        );
        assert_eq!(
            attributes
                .required_value(vanilla_attributes::ARMOR_TOUGHNESS)
                .to_bits(),
            (base_toughness + 2.0).to_bits()
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
    entity.set_on_ground(true);
    entity.set_velocity(DVec3::new(0.2, 0.3, -0.1));
    let initial_velocity = entity.velocity();
    let source = DamageSource::environment(&vanilla_damage_types::DROWN)
        .with_source_position(DVec3::new(1.0, 0.0, 0.0));

    assert!(entity.hurt(test_world(), &source, 4.0));

    assert_vec3_close(entity.velocity(), initial_velocity);
    assert!(!entity.needs_velocity_sync());
}

#[test]
fn generic_living_hurt_scales_knockback_by_resistance() {
    init_vanilla_registry();
    let entity = LivingFluidTestEntity::new(0.0, 0.0, true);
    entity.set_on_ground(true);
    entity
        .attributes()
        .lock()
        .set_base_value(vanilla_attributes::KNOCKBACK_RESISTANCE, 0.5);
    let source = DamageSource::environment(&vanilla_damage_types::PLAYER_ATTACK)
        .with_source_position(DVec3::new(1.0, 0.0, 0.0));

    assert!(entity.hurt(test_world(), &source, 4.0));

    assert_vec3_close(
        entity.velocity(),
        DVec3::new(
            -DAMAGE_KNOCKBACK_POWER * 0.5,
            DAMAGE_KNOCKBACK_POWER * 0.5,
            0.0,
        ),
    );
}
