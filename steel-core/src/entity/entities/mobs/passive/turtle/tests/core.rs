use std::ops::RangeInclusive;

use steel_registry::blocks::properties::BlockStateProperties;
use steel_registry::entity_type::EntityAttachment;
use steel_utils::{BlockStateId, WorldAabb};

use super::*;
use crate::behavior::init_behaviors;
use crate::entity::PathfinderMob;
use crate::entity::ai::goal::Goal;
use crate::entity::entities::ItemEntity;
use crate::entity::entities::mobs::passive::turtle::goals::{TurtleLayEggGoal, TurtleTravelGoal};
use crate::entity::{AgeableMob, EntityPose, EntitySpawnReason, next_entity_id};
use crate::physics::MoverType;
use crate::world::LevelReader;

/// Tolerance for the `f64` velocity checks (`DVec3`), where `f32::EPSILON` is the
/// wrong width. The maths is flat multipliers, so the result is near-exact.
const VELOCITY_EPSILON: f64 = 1e-9;

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
    /// Vanilla laying runs for `LAY_EGG_DURATION` (200) ticks once the turtle is
    /// in place; this leaves headroom for it to settle onto the block first.
    const MAX_LAY_TICKS: i32 = 260;

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

    for _ in 0..MAX_LAY_TICKS {
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

/// Sand under the whole square of block coordinates, so a test turtle has ground
/// to stand on and somewhere its goals can path to.
fn lay_sand_floor(world: &Arc<World>, span: RangeInclusive<i32>) {
    const FLOOR_Y: i32 = 63;

    for x in span.clone() {
        for z in span.clone() {
            world.set_block(
                BlockPos::new(x, FLOOR_Y, z),
                vanilla_blocks::SAND.default_state(),
                UpdateFlags::UPDATE_NONE,
            );
        }
    }
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
        (turtle.velocity().z - expected).abs() < VELOCITY_EPSILON,
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
        (turtle.velocity().y + SWIM_SINK_SPEED).abs() < VELOCITY_EPSILON,
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
        turtle.velocity().y.abs() < VELOCITY_EPSILON,
        "a turtle on its way home keeps its depth, got {}",
        turtle.velocity().y
    );
}

#[test]
fn a_turtle_walking_on_land_is_slowed_to_a_crawl() {
    /// Enough repeated trims to reach the land speed floor from a full 1.0.
    const SETTLE_TICKS: u32 = 20;

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
        (turtle.get_speed() - 1.0 / LAND_SPEED_DIVISOR).abs() < f32::EPSILON,
        "walking speed is halved, got {}",
        turtle.get_speed()
    );

    // Repeated trimming settles at the floor rather than dropping to nothing.
    for _ in 0..SETTLE_TICKS {
        turtle.trim_turtle_speed();
    }
    assert!(
        (turtle.get_speed() - LAND_MIN_SPEED).abs() < f32::EPSILON,
        "the land speed floor holds, got {}",
        turtle.get_speed()
    );
}

/// Vanilla `TurtleTravelGoal.tick` throws away a swim target whose surroundings
/// are not generated yet.
#[test]
fn a_traveling_turtle_gives_up_on_a_target_the_world_has_not_reached() {
    /// Tries allowed for the goal to find any candidate position at all.
    const ACCEPT_ATTEMPTS: u32 = 20;

    let (world, turtle) = turtle_in_world("turtle_travel_unloaded", DVec3::new(8.5, 64.0, 8.5));
    lay_sand_floor(&world, 0..=15);
    turtle.move_entity(MoverType::SelfMovement, DVec3::new(0.0, -2.0, 0.0));
    turtle.set_travel_pos(Some(BlockPos::new(8, 64, 24)));

    let mut goal = TurtleTravelGoal::new(1.0);
    goal.tick(turtle.as_ref());
    assert!(
        goal.is_stuck(),
        "only the turtle's own chunk exists, so nothing near it is safe to head for"
    );

    // With the surrounding chunks generated, the same target is accepted.
    for chunk_x in -3..=3 {
        for chunk_z in -3..=3 {
            if (chunk_x, chunk_z) != (0, 0) {
                insert_ready_full_chunk(&world, ChunkPos::new(chunk_x, chunk_z));
            }
        }
    }
    lay_sand_floor(&world, -24..=40);

    // The candidate position is drawn at random and can come up empty on its
    // own, so take the best of several.
    let accepted = (0..ACCEPT_ATTEMPTS).any(|_| {
        let mut goal = TurtleTravelGoal::new(1.0);
        goal.tick(turtle.as_ref());
        !goal.is_stuck()
    });
    assert!(
        accepted,
        "a target surrounded by generated world should be accepted"
    );
}

#[test]
fn a_turtle_holds_its_course_against_a_current() {
    let turtle = detached_turtle();

    assert!(
        !turtle.is_pushed_by_fluid(),
        "a turtle is not carried along by flowing water"
    );
}

/// Vanilla `TurtleMoveControl.tick` swings the body round with the steering.
#[test]
fn a_steering_turtle_turns_its_whole_body() {
    let (world, turtle) = turtle_in_world("turtle_body_turn", DVec3::new(8.5, 65.0, 8.5));
    lay_sand_floor(&world, 0..=15);
    turtle.move_entity(MoverType::SelfMovement, DVec3::new(0.0, -2.0, 0.0));
    assert!(turtle.on_ground(), "the turtle should have landed");

    // Facing south, with somewhere to be off to the east.
    turtle.set_rotation((0.0, 0.0));
    turtle.set_y_body_rot(0.0);

    let target = DVec3::new(13.5, 65.0, 8.5);
    assert!(
        turtle.move_to_pos(target, 1.0),
        "the turtle should find a path along the sand"
    );
    turtle.set_wanted_position(target, 1.0);
    turtle.tick_move_control();

    let (yaw, _) = turtle.rotation();
    assert!(
        yaw.abs() > f32::EPSILON,
        "steering toward the target should have turned the turtle, got {yaw}"
    );
    assert!(
        (turtle.y_body_rot() - yaw).abs() < f32::EPSILON,
        "the body should face the same way as the steering, got {} against {yaw}",
        turtle.y_body_rot()
    );
}

/// A bare level answering only what the spawn rule asks: the block below, the
/// brightness, and the sea level.
struct SpawnRuleLevel {
    below_state: BlockStateId,
    raw_brightness: u8,
    sea_level: i32,
}

impl LevelReader for SpawnRuleLevel {
    fn get_block_state(&self, pos: BlockPos) -> BlockStateId {
        if pos == SPAWN_POS.below() {
            return self.below_state;
        }

        REGISTRY.blocks.get_default_state_id(&vanilla_blocks::AIR)
    }

    fn raw_brightness(&self, _pos: BlockPos, _sky_darkening: u8) -> u8 {
        self.raw_brightness
    }

    fn sea_level(&self) -> i32 {
        self.sea_level
    }

    fn min_y(&self) -> i32 {
        -64
    }

    fn height(&self) -> i32 {
        384
    }
}

/// Where the candidate turtle stands in the spawn-rule tests.
const SPAWN_POS: BlockPos = BlockPos::new(0, 64, 0);

fn turtle_spawns_at(level: &SpawnRuleLevel, pos: BlockPos) -> bool {
    <TurtleEntity as Animal>::check_animal_spawn_rules(level, EntitySpawnReason::Natural, pos)
}

/// Vanilla `Turtle.checkTurtleSpawnRules`: sand, daylight, and near sea level.
#[test]
fn turtles_only_spawn_on_a_bright_beach() {
    init_vanilla_registry();

    let beach = SpawnRuleLevel {
        below_state: vanilla_blocks::SAND.default_state(),
        raw_brightness: 9,
        sea_level: SPAWN_POS.y() - 1,
    };
    assert!(turtle_spawns_at(&beach, SPAWN_POS));

    // Sea level is read from the level: drop it and the same beach is too high.
    let inland = SpawnRuleLevel {
        sea_level: SPAWN_POS.y() - SPAWN_HEIGHT_ABOVE_SEA_LEVEL,
        ..beach
    };
    assert!(!turtle_spawns_at(&inland, SPAWN_POS));

    let stone = SpawnRuleLevel {
        below_state: vanilla_blocks::STONE.default_state(),
        ..beach
    };
    assert!(!turtle_spawns_at(&stone, SPAWN_POS));

    // Unlike the shared animal rule, a dark beach stays empty even for a spawner.
    let night = SpawnRuleLevel {
        raw_brightness: 8,
        ..beach
    };
    assert!(!turtle_spawns_at(&night, SPAWN_POS));
    assert!(!<TurtleEntity as Animal>::check_animal_spawn_rules(
        &night,
        EntitySpawnReason::TrialSpawner,
        SPAWN_POS
    ));
}

/// Vanilla `Turtle.getAgeScale`: a hatchling is far smaller than the shared
/// half-size baby.
#[test]
fn a_baby_turtle_is_far_smaller_than_its_parent() {
    let turtle = detached_turtle();

    assert_eq!(turtle.get_age_scale(), ADULT_SCALE);

    turtle.set_baby(true);
    assert_eq!(turtle.get_age_scale(), BABY_SCALE);
}

/// Vanilla `Turtle.BABY_DIMENSIONS` seats a rider on the hatchling's shell,
/// lower than the adult seat scaled down.
#[test]
fn a_baby_turtle_carries_a_rider_on_its_shell() {
    init_vanilla_registry();
    let turtle = detached_turtle();
    turtle.set_baby(true);

    let baby = turtle.dimensions_for_pose(EntityPose::Standing);
    let adult = vanilla_entities::TURTLE.dimensions;
    assert!((baby.height - adult.height * BABY_SCALE).abs() < f32::EPSILON);

    let seat =
        baby.attachments
            .get_clamped(EntityAttachment::Passenger, 0, turtle.rotation().0, baby);
    let adult_seat_scaled =
        adult
            .attachments
            .get_clamped(EntityAttachment::Passenger, 0, turtle.rotation().0, adult)
            * f64::from(BABY_SCALE);
    assert!(
        seat.y < adult_seat_scaled.y,
        "a hatchling's seat should sit lower than the adult's scaled down, got {} against {}",
        seat.y,
        adult_seat_scaled.y
    );
}

/// Nothing in the turtle's class chain overrides `getSoundVolume`, so it stays
/// at the shared 1.0.
#[test]
fn a_turtle_is_no_quieter_than_any_other_mob() {
    let turtle = detached_turtle();

    assert!((turtle.sound_volume() - 1.0).abs() < f32::EPSILON);
}

#[test]
fn a_turtle_shuffles_rather_than_plods() {
    let turtle = detached_turtle();

    // The shared stride is one block; a turtle's is shorter.
    assert!(
        turtle.next_step() < 1.0,
        "a turtle steps more often than once per block, got {}",
        turtle.next_step()
    );
    assert_eq!(turtle.ambient_sound_interval(), AMBIENT_SOUND_INTERVAL);
}

#[test]
fn a_turtle_prefers_water_and_sand_when_choosing_where_to_walk() {
    let (world, turtle) = turtle_in_world("turtle_walk_target", DVec3::new(8.5, 65.0, 8.5));
    let water_pos = BlockPos::new(4, 64, 4);
    let sand_pos = BlockPos::new(6, 64, 6);
    let plain_pos = BlockPos::new(10, 64, 10);
    assert!(world.set_block(
        water_pos,
        vanilla_blocks::WATER.default_state(),
        UpdateFlags::UPDATE_NONE,
    ));
    assert!(world.set_block(
        sand_pos.below(),
        vanilla_blocks::SAND.default_state(),
        UpdateFlags::UPDATE_NONE,
    ));

    assert_eq!(
        turtle.get_walk_target_value(water_pos),
        PREFERRED_WALK_TARGET_VALUE
    );
    assert_eq!(
        turtle.get_walk_target_value(sand_pos),
        PREFERRED_WALK_TARGET_VALUE
    );
    assert!(turtle.get_walk_target_value(plain_pos) < PREFERRED_WALK_TARGET_VALUE);

    // A turtle on its way home to lay stops finding open water attractive.
    turtle.set_going_home(true);
    assert!(turtle.get_walk_target_value(water_pos) < PREFERRED_WALK_TARGET_VALUE);
    assert_eq!(
        turtle.get_walk_target_value(sand_pos),
        PREFERRED_WALK_TARGET_VALUE
    );
}
