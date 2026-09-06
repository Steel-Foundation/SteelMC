use steel_registry::blocks::properties::BlockStateProperties;
use steel_utils::WorldAabb;

use super::*;
use crate::behavior::init_behaviors;
use crate::entity::ai::goal::Goal;
use crate::entity::entities::ItemEntity;
use crate::entity::entities::mobs::passive::turtle::goals::TurtleLayEggGoal;
use crate::entity::{AgeableMob, next_entity_id};
use crate::physics::MoverType;

#[test]
fn turtle_registers_vanilla_goal_priorities() {
    let turtle = detached_turtle();

    let selector = turtle.mob_base().goal_selector().lock();
    assert_eq!(selector.available_goal_count(), 9);
    assert_eq!(
        selector.available_goal_priorities(),
        vec![0, 1, 1, 2, 3, 4, 7, 8, 9]
    );
}

#[test]
fn turtle_paths_freely_through_water_and_avoids_doors() {
    let turtle = detached_turtle();

    assert_eq!(turtle.get_pathfinding_malus(PathType::Water), 0.0);
    assert_eq!(turtle.get_pathfinding_malus(PathType::DoorIronClosed), -1.0);
    assert_eq!(turtle.get_pathfinding_malus(PathType::DoorWoodClosed), -1.0);
    assert_eq!(turtle.get_pathfinding_malus(PathType::DoorOpen), -1.0);
}

#[test]
fn turtle_eats_seagrass() {
    let turtle = detached_turtle();

    assert!(turtle.is_food(&ItemStack::new(&vanilla_items::SEAGRASS)));
    assert!(!turtle.is_food(&ItemStack::new(&vanilla_items::WHEAT)));
}

#[test]
fn turtle_carrying_an_egg_cannot_fall_in_love() {
    let turtle = detached_turtle();

    assert!(turtle.can_fall_in_love());

    turtle.set_has_egg(true);
    assert!(!turtle.can_fall_in_love());
}

#[test]
fn set_laying_egg_resets_the_lay_counter() {
    let turtle = detached_turtle();

    turtle.set_laying_egg(true);
    assert!(turtle.is_laying_egg());
    assert_eq!(turtle.lay_egg_counter(), 1);

    turtle.increment_lay_egg_counter();
    assert_eq!(turtle.lay_egg_counter(), 2);

    turtle.set_laying_egg(false);
    assert!(!turtle.is_laying_egg());
    assert_eq!(turtle.lay_egg_counter(), 0);
}

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
    turtle_from(&shared).set_age(0);

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

fn turtle_from(shared: &SharedEntity) -> &TurtleEntity {
    shared
        .downcast_ref::<TurtleEntity>()
        .expect("shared entity should be a turtle")
}

/// Puts a turtle in a loaded world at `position`, without any fluid around it,
/// since the water travel is driven directly rather than through the dispatcher.
fn turtle_in_world(key: &'static str, position: DVec3) -> (Arc<World>, Arc<TurtleEntity>) {
    init_vanilla_registry();
    init_behaviors();
    let world = fresh_test_world(key);
    insert_ready_full_chunk(&world, ChunkPos::new(0, 0));

    let turtle = Arc::new(TurtleEntity::new(
        &vanilla_entities::TURTLE,
        next_entity_id(),
        position,
        Arc::downgrade(&world),
    ));
    turtle.set_old_position_to_current();
    world
        .try_add_entity(Arc::clone(&turtle) as SharedEntity)
        .expect("turtle should attach to the loaded test chunk");
    (world, turtle)
}

#[test]
fn a_swimming_turtle_pushes_off_at_its_own_pace() {
    let (_world, turtle) = turtle_in_world("turtle_swim_push", DVec3::new(8.5, 64.0, 8.5));
    turtle.set_rotation((0.0, 0.0));

    // Yaw 0 faces south, so a forward push shows up on Z.
    turtle.travel_in_water(DVec3::new(0.0, 0.0, 1.0), 0.0, false, 64.0);

    // The push and the drag are flat, so this does not depend on the turtle's
    // movement speed attribute the way walking does.
    let expected = f64::from(SWIM_PUSH) * SWIM_DRAG;
    assert!(
        (turtle.velocity().z - expected).abs() < 1e-9,
        "expected {expected} on z, got {}",
        turtle.velocity().z
    );
}

#[test]
fn a_swimming_turtle_with_nowhere_to_be_drifts_down() {
    let (_world, turtle) = turtle_in_world("turtle_swim_drift", DVec3::new(8.5, 64.0, 8.5));
    turtle.set_home_pos(BlockPos::new(8, 64, 8));

    turtle.travel_in_water(DVec3::ZERO, 0.0, false, 64.0);

    assert!(
        (turtle.velocity().y + SWIM_SINK_SPEED).abs() < 1e-9,
        "a drifting turtle sinks slowly, got {}",
        turtle.velocity().y
    );
}

#[test]
fn a_turtle_heading_home_holds_its_depth() {
    let (_world, turtle) = turtle_in_world("turtle_swim_homing", DVec3::new(8.5, 64.0, 8.5));
    turtle.set_home_pos(BlockPos::new(8, 64, 8));
    turtle.set_going_home(true);

    turtle.travel_in_water(DVec3::ZERO, 0.0, false, 64.0);

    assert!(
        turtle.velocity().y.abs() < 1e-9,
        "a turtle on its way home keeps its depth, got {}",
        turtle.velocity().y
    );
}

#[test]
fn a_turtle_walking_on_land_is_slowed_to_a_crawl() {
    let (world, turtle) = turtle_in_world("turtle_land_trim", DVec3::new(8.5, 65.0, 8.5));
    assert!(world.set_block(
        BlockPos::new(8, 63, 8),
        vanilla_blocks::SAND.default_state(),
        UpdateFlags::UPDATE_NONE,
    ));
    // Drop it onto the sand so it is standing on the ground.
    turtle.move_entity(MoverType::SelfMovement, DVec3::new(0.0, -2.0, 0.0));
    assert!(turtle.on_ground(), "the turtle should have landed");

    turtle.set_mob_speed(1.0);
    turtle.trim_turtle_speed();

    assert!(
        (turtle.get_speed() - 0.5).abs() < f32::EPSILON,
        "walking speed is halved, got {}",
        turtle.get_speed()
    );

    // Repeated trimming settles at the floor rather than dropping to nothing.
    for _ in 0..20 {
        turtle.trim_turtle_speed();
    }
    assert!(
        (turtle.get_speed() - LAND_MIN_SPEED).abs() < f32::EPSILON,
        "the land speed floor holds, got {}",
        turtle.get_speed()
    );
}
