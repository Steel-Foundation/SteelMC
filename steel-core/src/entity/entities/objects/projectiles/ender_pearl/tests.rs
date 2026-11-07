use std::sync::Arc;

use steel_registry::init_vanilla_registry;
use steel_utils::BlockPos;

use super::*;
use crate::test_support::{TestPlayerBuilder, fresh_loaded_test_world, fresh_test_world};
use std::sync::Weak;

#[test]
fn ticket_renewal_registers_the_pearl_with_its_player_owner() {
    init_vanilla_registry();
    let world_fixture = fresh_loaded_test_world("ender_pearl_ticket_owner");
    let world = &world_fixture.world;
    let player = TestPlayerBuilder::new(Arc::clone(world), "Owner", 900_001).build();
    let owner: SharedEntity = Arc::<Player>::clone(&player);
    let pearl = Arc::new(EnderPearlEntity::new(
        &vanilla_entities::ENDER_PEARL,
        900_002,
        DVec3::new(0.5, 64.0, 0.5),
        Arc::downgrade(world),
    ));
    pearl.projectile().set_owner(Some(&owner));
    let shared: SharedEntity = Arc::<EnderPearlEntity>::clone(&pearl);
    world
        .try_add_entity(Arc::clone(&shared))
        .expect("test pearl should be added");

    assert!(player.ender_pearls().is_empty());
    pearl.entity_tick(&shared);

    let registered = player.ender_pearls();
    assert_eq!(registered.len(), 1);
    assert_eq!(registered[0].generation(), pearl.generation());
}

#[test]
fn bubble_columns_move_pearls_like_other_entities() {
    init_vanilla_registry();
    let pearl = EnderPearlEntity::new(
        &vanilla_entities::ENDER_PEARL,
        1,
        DVec3::ZERO,
        Weak::<World>::new(),
    );

    for (above_column, drag_down, initial_y, expected_y) in [
        (true, false, 2.0, 1.8),
        (true, true, -2.0, -0.9),
        (false, false, 2.0, 0.7),
        (false, true, -2.0, -0.3),
    ] {
        pearl.set_velocity(DVec3::new(0.25, initial_y, -0.25));
        pearl.set_fall_distance(5.0);

        if above_column {
            pearl.on_above_bubble_column(drag_down, BlockPos::new(0, 0, 0));
        } else {
            pearl.on_inside_bubble_column(drag_down);
        }

        let expected = DVec3::new(0.25, expected_y, -0.25);
        let actual = pearl.velocity();
        assert!(
            (actual - expected).abs().max_element() < 1.0e-12,
            "above_column={above_column}, drag_down={drag_down}: \
             expected {expected:?}, got {actual:?}"
        );
        let expected_fall_distance = if above_column { 5.0 } else { 0.0 };
        assert!(
            (pearl.fall_distance() - expected_fall_distance).abs() < 1.0e-12,
            "above_column={above_column}, drag_down={drag_down}: unexpected fall distance"
        );
    }
}

#[test]
fn removing_a_pearl_keeps_a_same_uuid_copy_registered() {
    init_vanilla_registry();
    let world_fixture = fresh_test_world("ender_pearl_removal_identity");
    let world = &world_fixture.world;
    let player = TestPlayerBuilder::new(Arc::clone(world), "Owner", 900_011).build();
    let owner: SharedEntity = Arc::<Player>::clone(&player);
    let uuid = uuid::Uuid::from_u128(7);
    let pearl = |id| {
        let pearl = EnderPearlEntity::from_base(
            EntityBase::with_uuid(
                id,
                uuid,
                DVec3::ZERO,
                vanilla_entities::ENDER_PEARL.dimensions,
                Arc::downgrade(world),
            ),
            &vanilla_entities::ENDER_PEARL,
        );
        pearl.projectile().set_owner(Some(&owner));
        Arc::new(pearl)
    };
    let old = pearl(900_012);
    let new = pearl(900_013);
    player.register_ender_pearl(&(Arc::clone(&new) as SharedEntity));

    old.set_removed(RemovalReason::ChangedWorld);
    assert_eq!(player.ender_pearls().len(), 1, "the copy stays registered");

    new.set_removed(RemovalReason::Discarded);
    assert!(player.ender_pearls().is_empty());
}
