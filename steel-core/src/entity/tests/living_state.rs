use super::*;
use crate::entity::get_kill_credit;
use crate::entity::prelude::*;
use crate::test_support::TestPlayerBuilder;
use std::sync::{Arc, Weak};
use steel_registry::data_components::vanilla_components::SwingAnimation;

#[test]
fn default_entity_tick_dispatches_living_tick() {
    init_vanilla_registry();

    let entity = Arc::new(LivingFluidTestEntity::new(0.0, 0.0, true).with_health(0.0));
    let entity_ref: SharedEntity = Arc::<LivingFluidTestEntity>::clone(&entity);

    Arc::clone(&entity_ref).tick();

    assert_eq!(entity.living().living_base().death_time(), 1);
}

#[test]
fn living_base_tick_decrements_last_hurt_by_player_memory() {
    init_vanilla_registry();

    let entity = LivingFluidTestEntity::new(0.0, 0.0, true);
    let player_uuid = Uuid::from_u128(42);
    entity.living().set_last_hurt_by_player(player_uuid, 1);

    entity.base_tick();

    assert_eq!(
        entity.living().living_base().last_hurt_by_player_uuid(),
        Some(player_uuid)
    );
    assert_eq!(entity.living().last_hurt_by_player_memory_time(), 0);

    entity.base_tick();

    assert!(
        entity
            .living()
            .living_base()
            .last_hurt_by_player_uuid()
            .is_none()
    );
    assert_eq!(entity.living().last_hurt_by_player_memory_time(), 0);
}

#[test]
fn kill_credit_keeps_a_player_in_another_world_until_memory_expires() {
    let world_fixture = fresh_test_world("kill_credit_victim");
    let world = &world_fixture.world;
    let player_world_fixture = fresh_test_world("kill_credit_player");
    let player_world = &player_world_fixture.world;
    let player: SharedEntity =
        TestPlayerBuilder::new(Arc::clone(player_world), "Attacker", 2).build();
    let victim = ChickenEntity::new(
        &vanilla_entities::CHICKEN,
        1,
        DVec3::ZERO,
        Arc::downgrade(world),
    );
    let source = DamageSource::direct(&vanilla_damage_types::PLAYER_ATTACK, Arc::clone(&player));

    victim
        .living()
        .resolve_player_responsible_for_damage(&source);
    drop(source);

    assert!(world.get_entity_by_uuid(&player.uuid()).is_none());
    assert!(Arc::ptr_eq(
        &get_kill_credit(&victim.living(), world).expect("cached player credit"),
        &player,
    ));
    victim
        .living()
        .set_last_hurt_by_player(Uuid::from_u128(42), 100);
    assert!(get_kill_credit(&victim.living(), world).is_none());

    victim
        .living()
        .resolve_player_responsible_for_damage(&DamageSource::direct(
            &vanilla_damage_types::PLAYER_ATTACK,
            Arc::clone(&player),
        ));
    for _ in 0..=100 {
        victim
            .living()
            .living_base()
            .tick_last_hurt_by_player_memory();
    }
    assert!(get_kill_credit(&victim.living(), world).is_none());
}

#[test]
fn living_base_tick_advances_swing_before_living_state() {
    init_vanilla_registry();

    let entity = LivingFluidTestEntity::new(0.0, 0.0, true);
    assert!(
        entity
            .living()
            .swing(InteractionHand::MainHand, SwingAnimation::DEFAULT, false)
    );
    assert_eq!(entity.living().living_swing_state().swing_time(), 0);

    entity.base_tick();
    entity.living().tick_living_state();
    assert!(
        !entity
            .living()
            .swing(InteractionHand::OffHand, SwingAnimation::DEFAULT, false)
    );

    let swing = entity.living().living_swing_state();
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
        entity.living().modified_swing_duration(default),
        DEFAULT_SWING_DURATION
    );

    entity
        .living()
        .set_mob_effect(vanilla_mob_effects::MINING_FATIGUE, 2);
    assert_eq!(
        entity.living().modified_swing_duration(default),
        DEFAULT_SWING_DURATION + 6
    );

    entity
        .living()
        .set_mob_effect(vanilla_mob_effects::HASTE, 1);
    assert_eq!(
        entity.living().modified_swing_duration(default),
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
        entity
            .living()
            .attack_animation(InteractionHand::MainHand)
            .duration,
        13
    );
    assert_eq!(
        entity
            .living()
            .interact_animation(InteractionHand::MainHand),
        SwingAnimation::DEFAULT
    );
}

#[test]
fn living_combat_memory_stores_and_expires_last_hurt_by_mob() {
    init_vanilla_registry();

    let entity = LivingFluidTestEntity::new(0.0, 0.0, true);
    let attacker: SharedEntity = Arc::new(LivingFluidTestEntity::new(0.0, 0.0, true));
    entity.advance_tick_count();

    entity.living().set_last_hurt_by_mob(Some(&attacker));

    let Some(stored_attacker) = entity.living().last_hurt_by_mob() else {
        panic!("last hurt-by mob should be stored");
    };
    assert_eq!(stored_attacker.uuid(), attacker.uuid());
    assert_eq!(entity.living().last_hurt_by_mob_timestamp(), 1);

    entity.living().living_base().tick_living_combat_memory(101);
    assert!(entity.living().last_hurt_by_mob().is_some());

    entity.living().living_base().tick_living_combat_memory(102);
    assert!(entity.living().last_hurt_by_mob().is_none());
    assert_eq!(entity.living().last_hurt_by_mob_timestamp(), 102);
}

#[test]
fn living_combat_memory_clears_dead_last_hurt_mob() {
    init_vanilla_registry();

    let entity = LivingFluidTestEntity::new(0.0, 0.0, true);
    let target = Arc::new(LivingFluidTestEntity::new(0.0, 0.0, true));
    let target_entity: SharedEntity = Arc::<LivingFluidTestEntity>::clone(&target);

    entity.living().set_last_hurt_mob(Some(&target_entity));
    assert!(entity.living().last_hurt_mob().is_some());
    assert_eq!(entity.living().last_hurt_mob_timestamp(), 0);

    target.living().set_health(0.0);
    entity.living().living_base().tick_living_combat_memory(1);

    assert!(entity.living().last_hurt_mob().is_none());
    assert_eq!(entity.living().last_hurt_mob_timestamp(), 1);
}

#[test]
fn living_death_loot_table_uses_default_and_custom_mob_tables() {
    init_vanilla_registry();

    let pig = ChickenEntity::new(&vanilla_entities::CHICKEN, 1, DVec3::ZERO, Weak::new());
    let Some(default_table) = pig.living().death_loot_table() else {
        panic!("pig should resolve its default entity loot table");
    };
    assert_eq!(
        &default_table.key,
        &vanilla_loot_tables::ENTITIES_CHICKEN.key
    );

    pig.mob()
        .set_death_loot_table(Some(Identifier::vanilla_static("entities/cow")));
    let Some(custom_table) = pig.living().death_loot_table() else {
        panic!("custom cow loot table should resolve");
    };
    assert_eq!(&custom_table.key, &vanilla_loot_tables::ENTITIES_COW.key);
    assert_eq!(pig.living().death_loot_table_seed(), 0);

    pig.mob()
        .set_death_loot_table(Some(Identifier::vanilla_static("entities/not_real")));
    assert!(pig.living().death_loot_table().is_none());
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
fn living_entities_sync_effect_visibility_without_a_type_override() {
    init_vanilla_registry();
    init_behaviors();
    let entity = LivingFluidTestEntity::new(0.0, 0.0, true);

    entity
        .living()
        .add_mob_effect(ActiveMobEffect::with_duration(
            vanilla_mob_effects::INVISIBILITY,
            200,
            0,
        ));
    entity.update_data_before_sync();

    let Some(synced) = entity.synced_data() else {
        panic!("living test entities have synced data");
    };
    assert!(synced.is_base_invisible_flag());
}

#[test]
fn living_dimensions_use_the_sleeping_box_and_the_type_default_size() {
    init_vanilla_registry();
    let chicken = ChickenEntity::new(&vanilla_entities::CHICKEN, 1, DVec3::ZERO, Weak::new());

    let sleeping = chicken.dimensions_for_pose(EntityPose::Sleeping);
    assert_eq!((sleeping.width, sleeping.height), (0.2, 0.2));

    let adult = chicken.dimensions_for_pose(EntityPose::Standing);
    crate::entity::ageable::Ageable::of(&chicken)
        .expect("chickens age")
        .set_baby(true);
    let baby = chicken.dimensions_for_pose(EntityPose::Standing);
    assert_eq!(adult.width, vanilla_entities::CHICKEN.dimensions.width);
    assert!(baby.width < adult.width);
    assert_ne!(
        baby.eye_height,
        adult.eye_height * 0.5,
        "chickens use their own baby size, not the halved adult size"
    );
}
