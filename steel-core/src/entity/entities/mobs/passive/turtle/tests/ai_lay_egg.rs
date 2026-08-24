use steel_registry::blocks::properties::BlockStateProperties;

use super::*;
use crate::behavior::init_behaviors;
use crate::entity::ai::goal::Goal;
use crate::entity::entities::mobs::passive::turtle::goals::TurtleLayEggGoal;

/// Drives `TurtleLayEggGoal` for a turtle standing on sand at its home beach and
/// asserts it places a turtle egg cluster and clears the carried egg.
#[test]
fn lay_egg_goal_places_eggs_on_home_sand() {
    init_vanilla_registry();
    init_behaviors();
    let world = fresh_test_world("turtle_lay_egg");
    insert_ready_full_chunk(&world, ChunkPos::new(0, 0));

    let sand_pos = BlockPos::new(8, 64, 8);
    let egg_pos = sand_pos.above();
    world.set_block(
        sand_pos,
        vanilla_blocks::SAND.default_state(),
        UpdateFlags::UPDATE_NONE,
    );

    let turtle = TurtleEntity::new(
        &vanilla_entities::TURTLE,
        1,
        DVec3::new(8.0, 65.0, 8.0),
        Arc::downgrade(&world),
    );
    turtle.set_home_pos(egg_pos);
    turtle.set_has_egg(true);
    let shared: SharedEntity = Arc::new(turtle);
    world
        .try_add_entity(Arc::clone(&shared))
        .expect("turtle should attach to the loaded test chunk");

    let mob = shared
        .as_pathfinder_mob()
        .expect("turtle should be a pathfinder mob");

    let mut goal = TurtleLayEggGoal::new(1.0);
    assert!(goal.can_use(mob), "turtle on home sand should start laying");
    goal.start(mob);

    for _ in 0..260 {
        goal.tick(mob);
        if !turtle_from(&shared).has_egg() {
            break;
        }
    }

    let egg_state = world.get_block_state(egg_pos);
    assert_eq!(
        egg_state.get_block(),
        &vanilla_blocks::TURTLE_EGG,
        "laying should place a turtle egg block above the sand"
    );
    let eggs = egg_state.get_value(&BlockStateProperties::EGGS);
    assert!((1..=4).contains(&eggs), "egg count should be 1 to 4");

    let turtle = turtle_from(&shared);
    assert!(!turtle.has_egg(), "laying should clear the carried egg");
    assert!(!turtle.is_laying_egg(), "laying should finish");
}

fn turtle_from(shared: &SharedEntity) -> &TurtleEntity {
    shared
        .downcast_ref::<TurtleEntity>()
        .expect("shared entity should be a turtle")
}
