use super::*;
use crate::entity::get_kill_credit;
use crate::test_support::TestPlayerBuilder;
use simdnbt::borrow::read_compound;
use std::io::Cursor;
use std::sync::{Arc, Weak};
use steel_registry::data_components::vanilla_components::SwingAnimation;

#[test]
fn default_entity_tick_dispatches_living_tick() {
    init_vanilla_registry();

    let entity = Arc::new(LivingFluidTestEntity::new(0.0, 0.0, true).with_health(0.0));
    let entity_ref: SharedEntity = Arc::<LivingFluidTestEntity>::clone(&entity);

    Arc::clone(&entity_ref).tick();

    assert_eq!(entity.living_base().death_time(), 1);
}

#[test]
fn living_base_tick_decrements_last_hurt_by_player_memory() {
    init_vanilla_registry();

    let entity = LivingFluidTestEntity::new(0.0, 0.0, true);
    let player_uuid = Uuid::from_u128(42);
    entity.set_last_hurt_by_player(player_uuid, 1);

    entity.base_tick_living_entity();

    assert_eq!(
        entity.living_base().last_hurt_by_player_uuid(),
        Some(player_uuid)
    );
    assert_eq!(entity.last_hurt_by_player_memory_time(), 0);

    entity.base_tick_living_entity();

    assert!(entity.living_base().last_hurt_by_player_uuid().is_none());
    assert_eq!(entity.last_hurt_by_player_memory_time(), 0);
}

#[test]
fn kill_credit_keeps_a_player_in_another_world_until_memory_expires() {
    let world_fixture = fresh_test_world("kill_credit_victim");
    let world = &world_fixture.world;
    let player_world_fixture = fresh_test_world("kill_credit_player");
    let player_world = &player_world_fixture.world;
    let player: SharedEntity =
        TestPlayerBuilder::new(Arc::clone(player_world), "Attacker", 2).build();
    let victim = PigEntity::new(
        &vanilla_entities::PIG,
        1,
        DVec3::ZERO,
        Arc::downgrade(world),
    );
    let source = DamageSource::direct(&vanilla_damage_types::PLAYER_ATTACK, Arc::clone(&player));

    victim.resolve_player_responsible_for_damage(&source);
    drop(source);

    assert!(world.get_entity_by_uuid(&player.uuid()).is_none());
    assert!(Arc::ptr_eq(
        &get_kill_credit(&victim, world).expect("cached player credit"),
        &player,
    ));
    victim.set_last_hurt_by_player(Uuid::from_u128(42), 100);
    assert!(get_kill_credit(&victim, world).is_none());

    victim.resolve_player_responsible_for_damage(&DamageSource::direct(
        &vanilla_damage_types::PLAYER_ATTACK,
        Arc::clone(&player),
    ));
    for _ in 0..=100 {
        victim.living_base().tick_last_hurt_by_player_memory();
    }
    assert!(get_kill_credit(&victim, world).is_none());
}

#[test]
fn living_base_tick_advances_swing_before_living_state() {
    init_vanilla_registry();

    let entity = LivingFluidTestEntity::new(0.0, 0.0, true);
    assert!(entity.swing(InteractionHand::MainHand, SwingAnimation::DEFAULT, false));
    assert_eq!(entity.living_swing_state().swing_time(), 0);

    entity.base_tick_living_entity();
    entity.tick_living_state();
    assert!(!entity.swing(InteractionHand::OffHand, SwingAnimation::DEFAULT, false));

    let swing = entity.living_swing_state();
    assert!(swing.swinging());
    assert_eq!(swing.swing_time(), 1);
    assert_eq!(swing.attack_anim().to_bits(), 0.0_f32.to_bits());
}

#[test]
fn modified_swing_duration_uses_vanilla_dig_effects() {
    init_vanilla_registry();
    init_behaviors();

    let entity = LivingFluidTestEntity::new(0.0, 0.0, true);
    let default = SwingAnimation::DEFAULT;
    assert_eq!(
        entity.modified_swing_duration(default),
        DEFAULT_SWING_DURATION
    );

    entity.set_mob_effect(vanilla_mob_effects::MINING_FATIGUE, 2);
    assert_eq!(
        entity.modified_swing_duration(default),
        DEFAULT_SWING_DURATION + 6
    );

    entity.set_mob_effect(vanilla_mob_effects::HASTE, 1);
    assert_eq!(
        entity.modified_swing_duration(default),
        DEFAULT_SWING_DURATION - 2
    );
}

#[test]
fn attack_animation_uses_held_item_component() {
    init_vanilla_registry();

    let entity = LivingFluidTestEntity::new(0.0, 0.0, true);
    entity.equip(
        EquipmentSlot::MainHand,
        ItemStack::new(&vanilla_items::WOODEN_SPEAR),
    );

    assert_eq!(
        entity.attack_animation(InteractionHand::MainHand).duration,
        13
    );
    assert_eq!(
        entity.interact_animation(InteractionHand::MainHand),
        SwingAnimation::DEFAULT
    );
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

    entity.living_base().tick_living_combat_memory(None, 101);
    assert!(entity.last_hurt_by_mob().is_some());

    entity.living_base().tick_living_combat_memory(None, 102);
    assert!(entity.last_hurt_by_mob().is_none());
    assert_eq!(entity.last_hurt_by_mob_timestamp(), 102);
}

#[test]
fn living_combat_memory_clears_dead_last_hurt_mob() {
    init_vanilla_registry();

    let entity = LivingFluidTestEntity::new(0.0, 0.0, true);
    let target = Arc::new(LivingFluidTestEntity::new(0.0, 0.0, true));
    let target_entity: SharedEntity = Arc::<LivingFluidTestEntity>::clone(&target);

    entity.set_last_hurt_mob(Some(&target_entity));
    assert!(entity.last_hurt_mob().is_some());
    assert_eq!(entity.last_hurt_mob_timestamp(), 0);

    target.set_health(0.0);
    entity.living_base().tick_living_combat_memory(None, 1);

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

#[test]
fn persistent_living_state_roundtrips_through_nbt() {
    init_vanilla_registry();
    init_behaviors();

    let source = LivingFluidTestEntity::new(0.0, 0.0, true).with_health(12.5);
    {
        let mut attributes = source.attributes().lock();
        attributes.set_base_value(vanilla_attributes::MAX_HEALTH, 30.0);
        attributes.set_base_value(vanilla_attributes::MAX_ABSORPTION, 4.0);
    }
    source.set_absorption_amount(3.0);
    source.living_base.increment_death_time();
    source.living_base.increment_death_time();
    source.living_base.apply_post_impulse_grace_time(7);
    source
        .living_base
        .set_ignore_fall_damage_from_current_impulse(true, DVec3::new(1.0, 2.0, 3.0));
    source.set_fall_flying(true);
    source.set_sleeping_pos(BlockPos::new(4, 5, 6));
    source.add_mob_effect(
        ActiveMobEffect::with_duration(vanilla_mob_effects::HASTE, 200, 2)
            .with_ambient(true)
            .with_visible(false),
    );
    source.equip(
        EquipmentSlot::Head,
        ItemStack::new(&vanilla_items::DIAMOND_HELMET),
    );
    let player_uuid = Uuid::from_u128(0x1234);
    source.set_last_hurt_by_player(player_uuid, 55);

    let mut nbt = NbtCompound::new();
    source.save_persistent_living_data(&mut nbt);
    let mut bytes = Vec::new();
    nbt.write(&mut bytes);
    let borrowed = read_compound(&mut Cursor::new(bytes.as_slice()))
        .expect("saved living state should be valid NBT");

    let loaded = LivingFluidTestEntity::new(0.0, 0.0, true);
    loaded.load_persistent_living_data((&borrowed).into());

    assert_eq!(loaded.get_health(), 12.5);
    assert_eq!(loaded.get_max_health(), 30.0);
    assert_eq!(loaded.get_absorption_amount(), 3.0);
    assert_eq!(loaded.living_base.death_time(), 2);
    assert_eq!(
        loaded
            .living_base
            .current_impulse_context_reset_grace_time(),
        40
    );
    assert_eq!(
        loaded.living_base.current_impulse_impact_pos(),
        Some(DVec3::new(1.0, 2.0, 3.0))
    );
    assert!(loaded.is_fall_flying());
    assert_eq!(loaded.sleeping_pos(), Some(BlockPos::new(4, 5, 6)));
    assert_eq!(loaded.last_hurt_by_player_uuid(), Some(player_uuid));
    assert_eq!(loaded.last_hurt_by_player_memory_time(), 55);

    let effect = loaded
        .living_base
        .mob_effect(vanilla_mob_effects::HASTE)
        .expect("haste effect should load");
    assert_eq!(effect.duration(), 200);
    assert_eq!(effect.amplifier(), 2);
    assert!(effect.is_ambient());
    assert!(!effect.is_visible());

    let equipment = loaded.living_base().equipment().lock();
    assert_eq!(
        equipment.get_ref(EquipmentSlot::Head).item().key,
        vanilla_items::DIAMOND_HELMET.key
    );
}
