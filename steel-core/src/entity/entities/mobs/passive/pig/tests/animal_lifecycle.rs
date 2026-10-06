use steel_utils::{ChunkPos, Downcast as _, WorldAabb};

use super::*;
use crate::entity::entities::ExperienceOrbEntity;
use crate::entity::next_entity_id;
use crate::test_support::insert_ready_full_chunk;

#[test]
fn pig_uses_vanilla_animal_fire_path_malus() {
    init_vanilla_registry();

    let pig = PigEntity::new(&vanilla_entities::PIG, 1, DVec3::ZERO, Weak::new());

    assert_eq!(
        pig.get_pathfinding_malus(PathType::FireInNeighbor)
            .to_bits(),
        16.0_f32.to_bits()
    );
    assert_eq!(
        pig.get_pathfinding_malus(PathType::Fire).to_bits(),
        (-1.0_f32).to_bits()
    );
}

#[test]
fn pig_uses_mob_passenger_as_controller_when_not_player_controlled() {
    init_vanilla_registry();

    let vehicle_pig = Arc::new(PigEntity::new(
        &vanilla_entities::PIG,
        1,
        DVec3::ZERO,
        Weak::new(),
    ));
    let vehicle: SharedEntity = Arc::<PigEntity>::clone(&vehicle_pig);
    let passenger_pig = Arc::new(PigEntity::new(
        &vanilla_entities::PIG,
        2,
        DVec3::ZERO,
        Weak::new(),
    ));
    let passenger: SharedEntity = Arc::<PigEntity>::clone(&passenger_pig);
    EntityBase::restore_passenger_relationship(&vehicle, &passenger);

    assert_eq!(
        vehicle
            .controlling_passenger()
            .map(|controller| controller.id()),
        Some(passenger.id())
    );

    passenger_pig.set_wanted_position(DVec3::new(1.0, 0.0, 0.0), 1.0);
    Mob::tick_move_control(vehicle_pig.as_ref());

    assert_eq!(vehicle_pig.get_speed().to_bits(), 0.25_f32.to_bits());
    assert_eq!(
        vehicle_pig.travel_input().forward().to_bits(),
        0.25_f32.to_bits()
    );
    Mob::tick_move_control(passenger_pig.as_ref());
    assert_eq!(
        passenger_pig.travel_input().forward().to_bits(),
        0.0_f32.to_bits()
    );
}

#[test]
fn pig_uses_vanilla_pig_food_tag() {
    init_vanilla_registry();

    assert!(PigEntity::is_food(&ItemStack::new(&vanilla_items::CARROT)));
    assert!(!PigEntity::is_food(&ItemStack::new(&vanilla_items::STONE)));
}

#[test]
fn pig_saves_vanilla_animal_love_data() {
    init_vanilla_registry();

    let pig = PigEntity::new(&vanilla_entities::PIG, 1, DVec3::ZERO, Weak::new());
    let love_cause = Uuid::from_u128(42);
    pig.set_in_love_time(123);
    pig.set_love_cause_uuid(Some(love_cause));

    let mut nbt = NbtCompound::new();
    pig.save_additional(&mut nbt);

    assert_eq!(nbt.int("InLove"), Some(123));
    assert_eq!(
        nbt.int_array("LoveCause").map(<[i32]>::to_vec),
        Some(love_cause.to_int_array().to_vec())
    );
}

#[test]
fn pig_loads_vanilla_animal_love_data() {
    init_vanilla_registry();

    let love_cause = Uuid::from_u128(42);
    let mut nbt = NbtCompound::new();
    nbt.insert("InLove", 321_i32);
    nbt.insert(
        "LoveCause",
        NbtTag::IntArray(love_cause.to_int_array().to_vec()),
    );

    let mut bytes = Vec::new();
    nbt.write(&mut bytes);
    let borrowed = read_borrowed_compound(&mut Cursor::new(&bytes))
        .unwrap_or_else(|error| panic!("test nbt should reborrow: {error}"));

    let pig = PigEntity::new(&vanilla_entities::PIG, 1, DVec3::ZERO, Weak::new());
    pig.load_additional((&borrowed).into());

    assert_eq!(pig.in_love_time(), 321);
    assert_eq!(pig.love_cause_uuid(), Some(love_cause));
}

#[test]
fn pig_animal_love_ticks_only_for_adults() {
    init_vanilla_registry();

    let pig = PigEntity::new(&vanilla_entities::PIG, 1, DVec3::ZERO, Weak::new());
    pig.set_in_love_time(2);
    Animal::tick_animal_love(&pig);
    assert_eq!(pig.in_love_time(), 1);

    pig.set_age(-1);
    pig.set_in_love_time(20);
    Animal::tick_animal_love(&pig);
    assert_eq!(pig.in_love_time(), 0);
}

#[test]
fn pig_damage_resets_vanilla_animal_love_time() {
    init_vanilla_registry();

    let pig = PigEntity::new(&vanilla_entities::PIG, 1, DVec3::ZERO, Weak::new());
    let source = DamageSource::environment(&vanilla_damage_types::GENERIC);
    pig.set_in_love_time(20);

    assert!(pig.hurt_server(test_world(), &source, 1.0));

    assert_eq!(pig.in_love_time(), 0);
}

#[test]
fn pig_death_tick_removes_after_vanilla_death_duration() {
    init_vanilla_registry();

    let pig = PigEntity::new(&vanilla_entities::PIG, 1, DVec3::ZERO, Weak::new());
    pig.set_health(0.0);

    for _ in 0..DEATH_DURATION {
        LivingEntity::tick_living_entity(&pig);
    }

    assert_eq!(pig.removal_reason(), Some(RemovalReason::Killed));
}

#[test]
fn breeding_drops_a_single_experience_orb() {
    const GRID_STEP: f64 = 3.0;
    const GRID_SIDE: i32 = 4;

    init_vanilla_registry();
    let world = fresh_test_world("pig_breeding_experience_orb");
    insert_ready_full_chunk(&world, ChunkPos::new(0, 0));

    // A split amount would show up as extra orbs; 16 breedings make a lucky
    // run of unsplittable rolls vanishingly unlikely.
    for step in 0..GRID_SIDE * GRID_SIDE {
        let x = 1.5 + f64::from(step % GRID_SIDE) * GRID_STEP;
        let z = 1.5 + f64::from(step / GRID_SIDE) * GRID_STEP;
        let pig = PigEntity::new(
            &vanilla_entities::PIG,
            next_entity_id(),
            DVec3::new(x, 64.0, z),
            Arc::downgrade(&world),
        );
        let partner = PigEntity::new(
            &vanilla_entities::PIG,
            next_entity_id(),
            DVec3::new(x, 64.0, z),
            Arc::downgrade(&world),
        );
        pig.finalize_spawn_child_from_breeding(&world, &partner, None);

        let aabb = WorldAabb::new(x - 1.0, 63.0, z - 1.0, x + 1.0, 66.0, z + 1.0);
        let orbs: Vec<_> = world
            .get_entities_in_aabb(&aabb)
            .into_iter()
            .filter_map(|entity| {
                entity
                    .downcast_ref::<ExperienceOrbEntity>()
                    .map(|orb| (orb.value(), orb.count()))
            })
            .collect();
        assert_eq!(orbs.len(), 1, "one orb per breeding, got {orbs:?}");
        let (value, count) = orbs[0];
        assert_eq!(count, 1);
        assert!((1..=7).contains(&value), "breeding orb worth {value}");
    }
}
