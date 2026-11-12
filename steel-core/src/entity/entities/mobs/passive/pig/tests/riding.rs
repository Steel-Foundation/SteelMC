use steel_registry::vanilla_blocks;
use steel_utils::types::UpdateFlags;

use super::*;
use crate::entity::{SharedEntity, next_entity_id};

use crate::test_support::fresh_loaded_test_world;

#[test]
fn saddled_pig_is_steered_by_a_rider_holding_a_carrot_on_a_stick() {
    init_globals();
    let world_fixture = fresh_test_world("pig_steering_controller");
    let world = &world_fixture.world;
    let pig = pig(1);
    saddle(&pig);
    let vehicle: SharedEntity = Arc::<PigEntity>::clone(&pig);
    let rider = TestPlayerBuilder::new(Arc::clone(world), "Rider", 2).build();
    let passenger: SharedEntity = Arc::clone(&rider) as SharedEntity;
    EntityBase::restore_passenger_relationship(&vehicle, &passenger);

    assert!(vehicle.controlling_passenger().is_none());

    rider
        .inventory
        .lock()
        .set_selected_item(ItemStack::new(&vanilla_items::CARROT_ON_A_STICK));
    assert_eq!(
        vehicle
            .controlling_passenger()
            .map(|controller| controller.id()),
        Some(rider.id())
    );
}

#[test]
fn a_mob_riding_a_pig_controls_it() {
    init_vanilla_registry();
    let vehicle: SharedEntity = pig(1);
    let passenger: SharedEntity = pig(2);
    EntityBase::restore_passenger_relationship(&vehicle, &passenger);

    assert_eq!(
        vehicle
            .controlling_passenger()
            .map(|controller| controller.id()),
        Some(passenger.id())
    );
}

#[test]
fn boosting_raises_the_ridden_speed() {
    init_globals();
    let world_fixture = fresh_test_world("pig_ridden_speed");
    let controller = TestPlayerBuilder::new(Arc::clone(&world_fixture.world), "Rider", 2).build();
    let pig = pig(1);
    let base_speed = (0.25 * 0.225_f64) as f32;

    assert_eq!(
        pig.living().ridden_speed(&controller).to_bits(),
        base_speed.to_bits()
    );
    assert!(pig.boost());
    assert!(!pig.boost(), "a running boost cannot restart");
    pig.tick_boost();
    assert!(pig.living().ridden_speed(&controller) > base_speed);
}

#[test]
fn ridden_pig_faces_where_the_rider_looks() {
    init_globals();
    let world_fixture = fresh_test_world("pig_ridden_rotation");
    let controller = TestPlayerBuilder::new(Arc::clone(&world_fixture.world), "Rider", 2).build();
    controller.set_rotation((90.0, 60.0));
    let pig = pig(1);

    pig.living().tick_ridden(&controller, DVec3::ZERO);

    assert_eq!(pig.rotation(), (90.0, 30.0));
    assert_eq!(pig.living().y_body_rot().to_bits(), 90.0_f32.to_bits());
    assert_eq!(pig.y_head_rot().to_bits(), 90.0_f32.to_bits());
}

#[test]
fn player_without_food_mounts_a_saddled_pig() {
    init_globals();
    let world_fixture = fresh_loaded_test_world("pig_mount");
    let world = &world_fixture.world;
    let pig = PigEntity::new(
        &vanilla_entities::PIG,
        1,
        DVec3::new(0.0, 80.0, 0.0),
        Arc::downgrade(world),
    );
    saddle(&pig);
    world
        .try_add_entity(Arc::<PigEntity>::clone(&pig))
        .unwrap_or_else(|error| panic!("pig should join the world: {error}"));
    let player = TestPlayerBuilder::new(Arc::clone(world), "Rider", 2).build();
    assert!(world.add_player(Arc::clone(&player), ResetReason::InitialJoin));

    assert_eq!(
        pig.mob().mob_interact(&player, InteractionHand::MainHand),
        InteractionResult::Success
    );
    assert_eq!(player.vehicle().map(|vehicle| vehicle.id()), Some(pig.id()));
}

#[test]
fn rider_dismounts_onto_the_floor_right_of_the_pig() {
    init_globals();
    let world_fixture = fresh_loaded_test_world("pig_dismount_location");
    let world = &world_fixture.world;
    let flags = UpdateFlags::UPDATE_NONE | UpdateFlags::UPDATE_SKIP_ON_PLACE;
    for x in 7..=9 {
        for z in 7..=9 {
            assert!(world.set_block(
                BlockPos::new(x, 63, z),
                vanilla_blocks::STONE.default_state(),
                flags,
            ));
        }
    }

    let spawn = |id| -> SharedEntity {
        let pig = PigEntity::new(
            &vanilla_entities::PIG,
            id,
            DVec3::new(8.5, 64.0, 8.5),
            Arc::downgrade(world),
        );
        world
            .try_add_entity(Arc::clone(&pig) as SharedEntity)
            .expect("pig should join the world");
        pig
    };
    let vehicle = spawn(next_entity_id());
    let passenger = spawn(next_entity_id());
    // A pig facing south dismounts its rider to the west first.
    vehicle.set_rotation((0.0, 0.0));
    EntityBase::restore_passenger_relationship(&vehicle, &passenger);

    passenger.stop_riding();

    assert!(!passenger.is_passenger());
    assert_eq!(passenger.position(), DVec3::new(7.5, 64.0, 8.5));
}
