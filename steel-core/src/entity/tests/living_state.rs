use super::*;

#[test]
fn default_entity_tick_dispatches_living_tick() {
    init_vanilla_registry();

    let entity = LivingFluidTestEntity::new(0.0, 0.0, true).with_health(0.0);
    let entity_ref: &dyn Entity = &entity;

    entity_ref.tick();

    assert_eq!(entity.living_base().death_time(), 1);
}

#[test]
fn living_tick_state_decrements_last_hurt_by_player_memory() {
    init_vanilla_registry();

    let entity = LivingFluidTestEntity::new(0.0, 0.0, true);
    let player_uuid = Uuid::from_u128(42);
    entity.set_last_hurt_by_player(player_uuid, 1);

    entity.tick_living_state();

    assert_eq!(
        entity.living_base().last_hurt_by_player_uuid(),
        Some(player_uuid)
    );
    assert_eq!(entity.last_hurt_by_player_memory_time(), 0);

    entity.tick_living_state();

    assert!(entity.living_base().last_hurt_by_player_uuid().is_none());
    assert_eq!(entity.last_hurt_by_player_memory_time(), 0);
}

#[test]
fn living_tick_state_updates_swing_time() {
    init_vanilla_registry();

    let entity = LivingFluidTestEntity::new(0.0, 0.0, true);
    entity.swing(InteractionHand::MainHand, false);
    assert_eq!(entity.living_swing_state().swing_time(), -1);

    entity.tick_living_state();

    let swing = entity.living_swing_state();
    assert!(swing.swinging());
    assert_eq!(swing.swing_time(), 0);
    assert_eq!(swing.attack_anim().to_bits(), 0.0_f32.to_bits());
}

#[test]
fn current_swing_duration_uses_vanilla_dig_effects() {
    init_vanilla_registry();

    let entity = LivingFluidTestEntity::new(0.0, 0.0, true);
    assert_eq!(entity.current_swing_duration(), DEFAULT_SWING_DURATION);

    entity.set_mob_effect(vanilla_mob_effects::MINING_FATIGUE, 2);
    assert_eq!(entity.current_swing_duration(), DEFAULT_SWING_DURATION + 6);

    entity.set_mob_effect(vanilla_mob_effects::HASTE, 1);
    assert_eq!(entity.current_swing_duration(), DEFAULT_SWING_DURATION - 2);
}

#[test]
fn current_swing_duration_uses_held_item_component() {
    init_vanilla_registry();

    let entity = LivingFluidTestEntity::new(0.0, 0.0, true);
    entity.equip(
        EquipmentSlot::MainHand,
        ItemStack::new(&vanilla_items::WOODEN_SPEAR),
    );

    assert_eq!(entity.current_swing_duration(), 13);
}

#[test]
fn living_combat_memory_stores_and_expires_last_hurt_by_mob() {
    init_vanilla_registry();

    let entity = LivingFluidTestEntity::new(0.0, 0.0, true);
    let attacker: SharedEntity = Arc::new(LivingFluidTestEntity::new(0.0, 0.0, true));
    entity.advance_tick_count();

    entity.set_last_hurt_by_mob(Some(&attacker));

    let Some(stored_attacker) = entity.last_hurt_by_mob() else {
        panic!("last hurt-by mob should be stored");
    };
    assert_eq!(stored_attacker.uuid(), attacker.uuid());
    assert_eq!(entity.last_hurt_by_mob_timestamp(), 1);

    entity.living_base().tick_living_combat_memory(101);
    assert!(entity.last_hurt_by_mob().is_some());

    entity.living_base().tick_living_combat_memory(102);
    assert!(entity.last_hurt_by_mob().is_none());
    assert_eq!(entity.last_hurt_by_mob_timestamp(), 102);
}

#[test]
fn living_combat_memory_clears_dead_last_hurt_mob() {
    init_vanilla_registry();

    let entity = LivingFluidTestEntity::new(0.0, 0.0, true);
    let target = Arc::new(LivingFluidTestEntity::new(0.0, 0.0, true));
    let target_entity: SharedEntity = target.clone();

    entity.set_last_hurt_mob(Some(&target_entity));
    assert!(entity.last_hurt_mob().is_some());
    assert_eq!(entity.last_hurt_mob_timestamp(), 0);

    target.set_health(0.0);
    entity.living_base().tick_living_combat_memory(1);

    assert!(entity.last_hurt_mob().is_none());
    assert_eq!(entity.last_hurt_mob_timestamp(), 1);
}

#[test]
fn living_death_loot_table_uses_default_and_custom_mob_tables() {
    init_vanilla_registry();

    let pig = PigEntity::new(&vanilla_entities::PIG, 1, DVec3::ZERO, Weak::new());
    let Some(default_table) = pig.death_loot_table() else {
        panic!("pig should resolve its default entity loot table");
    };
    assert_eq!(&default_table.key, &vanilla_loot_tables::ENTITIES_PIG.key);

    pig.set_death_loot_table(Some(Identifier::vanilla_static("entities/cow")));
    let Some(custom_table) = pig.death_loot_table() else {
        panic!("custom cow loot table should resolve");
    };
    assert_eq!(&custom_table.key, &vanilla_loot_tables::ENTITIES_COW.key);
    assert_eq!(LivingEntity::death_loot_table_seed(&pig), 0);

    pig.set_death_loot_table(Some(Identifier::vanilla_static("entities/not_real")));
    assert!(pig.death_loot_table().is_none());
}

#[test]
fn closest_open_space_direction_matches_vanilla_order_on_ties() {
    assert_eq!(
        closest_direction_with_blocked_neighbors(DVec3::splat(0.5), &[]),
        Direction::North
    );
}

#[test]
fn closest_open_space_direction_skips_full_collision_neighbors() {
    assert_eq!(
        closest_direction_with_blocked_neighbors(DVec3::new(0.3, 0.5, 0.7), &[Direction::South]),
        Direction::West
    );
    assert_eq!(
        closest_direction_with_blocked_neighbors(
            DVec3::new(0.3, 0.2, 0.7),
            &[
                Direction::North,
                Direction::South,
                Direction::West,
                Direction::East,
            ],
        ),
        Direction::Up
    );
}
