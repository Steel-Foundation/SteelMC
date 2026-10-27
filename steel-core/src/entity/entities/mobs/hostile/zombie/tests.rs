use std::io::Cursor;
use std::sync::{Arc, Weak};

use glam::DVec3;
use simdnbt::borrow::read_compound;
use simdnbt::owned::NbtCompound;
use steel_registry::entity_data::EntityPose;
use steel_registry::item_stack::ItemStack;
use steel_registry::vanilla_attributes;
use steel_registry::{init_vanilla_registry, vanilla_entities, vanilla_items};
use steel_utils::types::Difficulty;

use super::{
    LEADER_ZOMBIE_BONUS_ID, ZOMBIE_RANDOM_SPAWN_BONUS_ID, Zombie, ZombieEntity, spawn_as_baby_odds,
};
use crate::entity::{
    AgeableMobGroupData, Entity, EntitySpawnReason, LivingEntity, Mob, SharedEntity, SpawnGroupData,
    ZombieGroupData,
};
use crate::inventory::equipment::EquipmentSlot;
use crate::special_dates;
use crate::test_support::fresh_test_world;
use crate::world::{DifficultyInstance, World};

fn make_zombie(id: i32) -> ZombieEntity {
    ZombieEntity::new(&vanilla_entities::ZOMBIE, id, DVec3::ZERO, Weak::new())
}

fn peaceful_difficulty() -> DifficultyInstance {
    DifficultyInstance::new(Difficulty::Peaceful, 0, 0, 0.0)
}

fn hard_difficulty() -> DifficultyInstance {
    DifficultyInstance::new(Difficulty::Hard, 1_512_000, 3_600_000, 0.0)
}

fn assert_head_has_only_halloween_gear(zombie: &ZombieEntity) {
    let head = zombie.equipment_in_slot(EquipmentSlot::Head);
    if special_dates::is_halloween() {
        assert!(
            head.is_empty()
                || head.is(&vanilla_items::CARVED_PUMPKIN)
                || head.is(&vanilla_items::JACK_O_LANTERN),
            "Halloween can replace an empty head slot, but nothing else: {head:?}"
        );
        if !head.is_empty() {
            assert_eq!(zombie.equipment_drop_chance(EquipmentSlot::Head), 0.0);
        }
    } else {
        assert!(head.is_empty(), "head slot should be empty: {head:?}");
    }
}

fn assert_no_armor(zombie: &ZombieEntity) {
    for slot in [
        EquipmentSlot::Chest,
        EquipmentSlot::Legs,
        EquipmentSlot::Feet,
    ] {
        assert!(
            zombie.equipment_in_slot(slot).is_empty(),
            "armor slot {slot:?} should be empty"
        );
    }
    assert_head_has_only_halloween_gear(zombie);
}

#[test]
fn zombie_baby_nbt_round_trip() {
    init_vanilla_registry();

    let zombie = make_zombie(1);
    assert!(!zombie.is_baby());

    zombie.set_baby(true);
    zombie.set_can_break_doors(true);
    assert!(zombie.is_baby());

    let mut nbt = NbtCompound::new();
    zombie.save_additional(&mut nbt);
    assert_eq!(nbt.byte("IsBaby"), Some(1));
    assert_eq!(nbt.byte("CanBreakDoors"), Some(1));

    let loaded = make_zombie(2);
    let mut bytes = Vec::new();
    nbt.write(&mut bytes);
    let borrowed =
        read_compound(&mut Cursor::new(&bytes)).unwrap_or_else(|error| panic!("reborrow failed: {error}"));
    loaded.load_additional((&borrowed).into());

    assert!(loaded.is_baby());
    assert!(loaded.can_break_doors());
}

#[test]
fn zombie_can_break_doors_toggles_goal_and_navigation() {
    init_vanilla_registry();
    let zombie = make_zombie(1);
    let initial_goals = zombie
        .mob_base()
        .goal_selector()
        .lock()
        .available_goal_count();

    assert!(!zombie.can_break_doors());
    assert!(!zombie.mob_base().navigation().lock().can_open_doors());

    zombie.set_can_break_doors(true);
    assert!(zombie.can_break_doors());
    assert!(zombie.mob_base().navigation().lock().can_open_doors());
    assert_eq!(
        zombie
            .mob_base()
            .goal_selector()
            .lock()
            .available_goal_count(),
        initial_goals + 1
    );

    zombie.set_can_break_doors(true);
    assert_eq!(
        zombie
            .mob_base()
            .goal_selector()
            .lock()
            .available_goal_count(),
        initial_goals + 1
    );

    zombie.set_can_break_doors(false);
    assert!(!zombie.can_break_doors());
    assert!(!zombie.mob_base().navigation().lock().can_open_doors());
    assert_eq!(
        zombie
            .mob_base()
            .goal_selector()
            .lock()
            .available_goal_count(),
        initial_goals
    );
}

#[test]
fn zombie_baby_dimensions_match_vanilla() {
    init_vanilla_registry();

    let zombie = make_zombie(1);
    let adult = zombie.dimensions_for_pose(EntityPose::Standing);
    assert_eq!(
        (adult.width, adult.height),
        (
            vanilla_entities::ZOMBIE.dimensions.width,
            vanilla_entities::ZOMBIE.dimensions.height
        )
    );

    zombie.set_baby(true);
    let baby = zombie.dimensions_for_pose(EntityPose::Standing);
    assert_eq!((baby.width, baby.height), (0.49, 0.98));
}

#[test]
fn zombie_baby_speed_modifier_requires_world_and_applies_once() {
    init_vanilla_registry();
    let world_fixture = fresh_test_world("zombie_baby_speed");
    let world: &Arc<World> = &world_fixture.world;

    let without_world = make_zombie(1);
    without_world.set_baby(true);
    let speed = without_world
        .attributes()
        .lock()
        .get_value(vanilla_attributes::MOVEMENT_SPEED)
        .expect("zombies have movement speed");
    assert!((speed - 0.23).abs() < 1.0e-6);

    let with_world = ZombieEntity::new(
        &vanilla_entities::ZOMBIE,
        2,
        DVec3::new(8.0, 65.0, 8.0),
        Arc::downgrade(world),
    );
    with_world.set_baby(true);
    let speed = with_world
        .attributes()
        .lock()
        .get_value(vanilla_attributes::MOVEMENT_SPEED)
        .expect("zombies have movement speed");
    assert!((speed - 0.345).abs() < 1.0e-6);

    with_world.set_baby(true);
    let speed = with_world
        .attributes()
        .lock()
        .get_value(vanilla_attributes::MOVEMENT_SPEED)
        .expect("zombies have movement speed");
    assert!((speed - 0.345).abs() < 1.0e-6);

    with_world.set_baby(false);
    let speed = with_world
        .attributes()
        .lock()
        .get_value(vanilla_attributes::MOVEMENT_SPEED)
        .expect("zombies have movement speed");
    assert!((speed - 0.23).abs() < 1.0e-6);
}

#[test]
fn zombie_baby_experience_reward_matches_vanilla() {
    init_vanilla_registry();

    let zombie = make_zombie(1);
    assert_eq!(LivingEntity::base_experience_reward(&zombie), 5);

    zombie.set_baby(true);
    assert_eq!(LivingEntity::base_experience_reward(&zombie), 12);
}

#[test]
fn zombie_group_data_finalize_sets_baby() {
    init_vanilla_registry();
    let world_fixture = fresh_test_world("zombie_group_data");
    let world = &world_fixture.world;

    let adult = make_zombie(1);
    let result = adult.finalize_spawn(
        world,
        peaceful_difficulty(),
        EntitySpawnReason::Natural,
        Some(SpawnGroupData::Zombie(ZombieGroupData::new(false, false))),
    );
    assert!(!adult.is_baby());
    assert_eq!(
        result,
        Some(SpawnGroupData::Zombie(ZombieGroupData::new(false, false)))
    );

    let baby = make_zombie(2);
    baby.finalize_spawn(
        world,
        peaceful_difficulty(),
        EntitySpawnReason::Natural,
        Some(SpawnGroupData::Zombie(ZombieGroupData::new(true, false))),
    );
    assert!(baby.is_baby());
}

#[test]
fn zombie_passes_through_foreign_group_data() {
    init_vanilla_registry();
    let world_fixture = fresh_test_world("zombie_foreign_group_data");
    let world = &world_fixture.world;
    let zombie = make_zombie(1);
    let group_data = AgeableMobGroupData::with_should_spawn_baby(true);

    let result = zombie.finalize_spawn(
        world,
        peaceful_difficulty(),
        EntitySpawnReason::Natural,
        Some(SpawnGroupData::AgeableMob(group_data)),
    );

    assert_eq!(result, Some(SpawnGroupData::AgeableMob(group_data)));
    assert!(!zombie.is_baby());
}

#[test]
fn zombie_peaceful_spawn_has_no_armor_or_spawn_bonuses() {
    init_vanilla_registry();
    let world_fixture = fresh_test_world("zombie_peaceful_spawn");
    let world = &world_fixture.world;
    let zombie = make_zombie(1);
    zombie.set_can_pick_up_loot(true);

    zombie.finalize_spawn(
        world,
        peaceful_difficulty(),
        EntitySpawnReason::Natural,
        None,
    );

    assert!(!zombie.can_pick_up_loot());
    assert_no_armor(&zombie);
    assert!(
        zombie.equipment_in_slot(EquipmentSlot::OffHand).is_empty(),
        "offhand should be empty"
    );
    assert!(
        zombie.equipment_in_slot(EquipmentSlot::MainHand).is_empty()
            || zombie
                .equipment_in_slot(EquipmentSlot::MainHand)
                .is(&vanilla_items::IRON_SWORD)
            || zombie
                .equipment_in_slot(EquipmentSlot::MainHand)
                .is(&vanilla_items::IRON_SPEAR)
            || zombie
                .equipment_in_slot(EquipmentSlot::MainHand)
                .is(&vanilla_items::IRON_SHOVEL),
        "main hand may only contain the vanilla iron weapon roll"
    );

    let attributes = zombie.attributes().lock();
    let reinforcement = attributes.required_value(vanilla_attributes::SPAWN_REINFORCEMENTS);
    assert!((0.0..0.1).contains(&reinforcement));
    let knockback = attributes.required_value(vanilla_attributes::KNOCKBACK_RESISTANCE);
    assert!((0.0..0.05).contains(&knockback));
    assert!(!attributes.has_modifier(
        vanilla_attributes::SPAWN_REINFORCEMENTS,
        &LEADER_ZOMBIE_BONUS_ID
    ));
    assert!(!attributes.has_modifier(vanilla_attributes::MAX_HEALTH, &LEADER_ZOMBIE_BONUS_ID));
    assert!(!attributes.has_modifier(
        vanilla_attributes::FOLLOW_RANGE,
        &ZOMBIE_RANDOM_SPAWN_BONUS_ID
    ));
    assert_eq!(
        attributes.required_value(vanilla_attributes::MAX_HEALTH),
        20.0
    );
}

#[test]
fn zombie_conversion_spawn_keeps_pickup_and_does_not_equip() {
    init_vanilla_registry();
    let world_fixture = fresh_test_world("zombie_conversion_spawn");
    let world = &world_fixture.world;

    for id in 0..200 {
        let zombie = make_zombie(id);
        zombie.set_can_pick_up_loot(true);
        zombie.living_base().equipment().lock().set(
            EquipmentSlot::MainHand,
            ItemStack::new(&vanilla_items::STONE),
        );

        zombie.finalize_spawn(
            world,
            hard_difficulty(),
            EntitySpawnReason::Conversion,
            None,
        );

        assert!(zombie.can_pick_up_loot());
        assert!(
            zombie
                .equipment_in_slot(EquipmentSlot::MainHand)
                .is(&vanilla_items::STONE)
        );
        for slot in [
            EquipmentSlot::Chest,
            EquipmentSlot::Legs,
            EquipmentSlot::Feet,
        ] {
            assert!(
                zombie.equipment_in_slot(slot).is_empty(),
                "conversion spawn must not equip {slot:?}"
            );
        }
        assert_head_has_only_halloween_gear(&zombie);
    }
}

#[test]
fn zombie_leader_spawn_applies_vanilla_bonuses() {
    init_vanilla_registry();
    let world_fixture = fresh_test_world("zombie_leader_spawn");
    let world = &world_fixture.world;
    let mut leaders = 0;

    for id in 0..500 {
        let zombie = make_zombie(id);
        zombie.finalize_spawn(world, hard_difficulty(), EntitySpawnReason::Natural, None);

        let attributes = zombie.attributes().lock();
        let reinforcement = attributes.required_value(vanilla_attributes::SPAWN_REINFORCEMENTS);
        let knockback = attributes.required_value(vanilla_attributes::KNOCKBACK_RESISTANCE);
        assert!((0.0..0.05).contains(&knockback));

        let follow_range = attributes.required_value(vanilla_attributes::FOLLOW_RANGE);
        if attributes.has_modifier(
            vanilla_attributes::FOLLOW_RANGE,
            &ZOMBIE_RANDOM_SPAWN_BONUS_ID,
        ) {
            assert!(
                follow_range > 61.0,
                "follow range bonus should at least double the base: {follow_range}"
            );
        } else {
            assert!(
                (30.9..=39.1).contains(&follow_range),
                "base follow range should stay near 35: {follow_range}"
            );
        }

        if attributes.has_modifier(
            vanilla_attributes::SPAWN_REINFORCEMENTS,
            &LEADER_ZOMBIE_BONUS_ID,
        ) {
            leaders += 1;
            assert!((0.5..0.85).contains(&reinforcement));
            assert!(
                attributes
                    .has_modifier(vanilla_attributes::MAX_HEALTH, &LEADER_ZOMBIE_BONUS_ID)
            );
            let max_health = attributes.required_value(vanilla_attributes::MAX_HEALTH);
            assert!((40.0..100.0).contains(&max_health));
            drop(attributes);
            assert_eq!(zombie.get_health(), zombie.get_max_health());
        } else {
            assert!((0.0..0.1).contains(&reinforcement));
            drop(attributes);
            assert_eq!(zombie.get_health(), 20.0);
        }
    }

    assert!(
        leaders > 0,
        "expected a leader among 500 hard-difficulty spawns"
    );
}

#[test]
fn zombie_rejects_eggs_for_riding_babies_and_glow_ink_sac() {
    init_vanilla_registry();

    let zombie = make_zombie(1);
    let egg = ItemStack::new(&vanilla_items::EGG);
    let glow_ink_sac = ItemStack::new(&vanilla_items::GLOW_INK_SAC);
    let stone = ItemStack::new(&vanilla_items::STONE);

    assert!(zombie.can_hold_item(&egg));
    assert!(zombie.wants_to_pick_up(&egg));
    assert!(!zombie.wants_to_pick_up(&glow_ink_sac));
    assert!(zombie.can_hold_item(&stone));

    zombie.set_baby(true);
    assert!(zombie.can_hold_item(&egg));
}

#[test]
fn zombie_spawn_uses_vanilla_hostile_defaults() {
    init_vanilla_registry();

    let zombie: SharedEntity = Arc::new(make_zombie(1));
    let mob = zombie.as_mob().expect("zombie is a mob");
    assert_eq!(mob.xp_reward(), 5);
}

#[test]
fn zombie_base_tick_forwards_to_mob_base_tick() {
    init_vanilla_registry();

    let zombie = make_zombie(1);
    zombie.set_health(zombie.get_max_health());
    let entity: SharedEntity = Arc::new(zombie);

    entity.base_tick();

    let mob = entity.as_mob().expect("zombie is a mob");
    assert_eq!(
        mob.mob_base().ambient_sound_time(),
        1,
        "base_tick must run the mob layer, not only the entity layer"
    );
}

#[test]
fn zombie_baby_odds_are_five_percent_boundary() {
    init_vanilla_registry();

    let mut babies = 0;
    for _ in 0..10_000 {
        if spawn_as_baby_odds() {
            babies += 1;
        }
    }

    assert!(
        (300..=700).contains(&babies),
        "baby rolls should cluster around 5%: {babies}"
    );
}
