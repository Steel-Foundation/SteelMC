use steel_utils::WorldAabb;

use super::*;
use crate::behavior::init_behaviors;
use crate::entity::entities::ItemEntity;
use crate::entity::{AgeableMob, next_entity_id};

/// A turtle that grows into an adult sheds a scute from the turtle grow gift loot
/// table, matching vanilla `Turtle.ageBoundaryReached`.
#[test]
fn growing_up_drops_a_scute() {
    init_vanilla_registry();
    init_behaviors();
    let world = fresh_test_world("turtle_grow_scute");
    insert_ready_full_chunk(&world, ChunkPos::new(0, 0));

    let turtle = TurtleEntity::new(
        &vanilla_entities::TURTLE,
        next_entity_id(),
        DVec3::new(8.0, 65.0, 8.0),
        Arc::downgrade(&world),
    );
    turtle.set_age(-1);
    let shared: SharedEntity = Arc::new(turtle);
    world
        .try_add_entity(Arc::clone(&shared))
        .expect("turtle should attach to the loaded test chunk");

    // Crossing from baby to adult runs the vanilla grow-up scute drop.
    shared
        .downcast_ref::<TurtleEntity>()
        .expect("shared entity should be a turtle")
        .set_age(0);

    let aabb = WorldAabb::new(6.0, 63.0, 6.0, 10.0, 68.0, 10.0);
    let scutes = world
        .get_entities_in_aabb(&aabb)
        .into_iter()
        .filter_map(|entity| {
            entity
                .downcast_ref::<ItemEntity>()
                .map(ItemEntity::get_item)
        })
        .filter(|stack| stack.is(&vanilla_items::TURTLE_SCUTE))
        .count();
    assert_eq!(scutes, 1, "growing up should drop exactly one turtle scute");
}
