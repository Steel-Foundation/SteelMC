use steel_registry::blocks::BlockRef;
use steel_utils::types::UpdateFlags;

use super::*;
use crate::entity::living_entity::{
    BASE_HORIZONTAL_AIR_DRAG, BASE_VERTICAL_AIR_DRAG, DEFAULT_BLOCK_FRICTION,
    compute_modified_friction,
};
use crate::entity::{ENTITIES, init_entities, next_entity_id};

#[test]
fn jump_from_ground_uses_jump_strength_and_marks_velocity_sync() {
    init_vanilla_registry();
    let entity = LivingFluidTestEntity::new(0.0, 0.0, true);
    let jump_strength = f64::from(vanilla_attributes::JUMP_STRENGTH.default_value as f32);

    entity.jump_from_ground();

    assert_vec3_close(entity.velocity(), DVec3::new(0.0, jump_strength, 0.0));
    assert!(entity.needs_velocity_sync());
}

#[test]
fn sprint_jump_from_ground_adds_vanilla_horizontal_impulse() {
    init_vanilla_registry();
    let entity = LivingFluidTestEntity::new(0.0, 0.0, true);
    let jump_strength = f64::from(vanilla_attributes::JUMP_STRENGTH.default_value as f32);
    entity.set_sprinting(true);
    entity.set_rotation((0.0, 0.0));

    entity.jump_from_ground();

    assert_vec3_close(
        entity.velocity(),
        DVec3::new(0.0, jump_strength, f64::from(0.2_f32)),
    );
}

#[test]
fn living_jump_in_water_uses_fluid_jump_impulse_without_cooldown() {
    init_vanilla_registry();
    let entity = LivingFluidTestEntity::new(0.5, 0.0, true);
    entity.set_jumping(true);

    entity.handle_living_jump();

    assert_vec3_close(entity.velocity(), DVec3::new(0.0, f64::from(0.04_f32), 0.0));
    assert_eq!(entity.no_jump_delay(), 0);
}

#[test]
fn living_jump_without_input_resets_jump_delay_like_vanilla() {
    init_vanilla_registry();
    let entity = LivingFluidTestEntity::new(0.0, 0.0, true);
    entity.set_no_jump_delay(4);

    entity.handle_living_jump();

    assert_eq!(entity.no_jump_delay(), 0);
}

#[test]
fn living_ai_step_zeroes_tiny_player_velocity_like_vanilla() {
    init_vanilla_registry();
    let entity = LivingFluidTestEntity::new(0.0, 0.0, true);
    entity.set_velocity(DVec3::new(0.002, 0.002, 0.002));

    entity.apply_living_velocity_thresholds();

    assert_vec3_close(entity.velocity(), DVec3::ZERO);
}

#[test]
fn living_ai_step_keeps_player_horizontal_velocity_above_combined_threshold() {
    init_vanilla_registry();
    let entity = LivingFluidTestEntity::new(0.0, 0.0, true);
    let velocity = DVec3::new(0.002, 0.003, 0.0025);
    entity.set_velocity(velocity);

    entity.apply_living_velocity_thresholds();

    assert_vec3_close(entity.velocity(), velocity);
}

#[test]
fn default_ai_step_resets_idle_jump_delay_and_dampens_input_before_travel() {
    init_vanilla_registry();
    let entity = LivingFluidTestEntity::new(0.0, 0.0, true);
    entity.set_no_jump_delay(2);
    entity.set_travel_input(LivingTravelInput::new(1.0, 0.5, -1.0));

    assert!(entity.default_ai_step().is_none());

    assert_eq!(entity.no_jump_delay(), 0);
    assert_eq!(
        entity.travel_input(),
        LivingTravelInput::new(0.98, 0.5, -0.98)
    );
}

#[test]
fn default_ai_step_resets_fall_distance_for_slow_falling_and_levitation() {
    init_vanilla_registry();

    let slow_falling = LivingFluidTestEntity::new(0.0, 0.0, true);
    slow_falling.set_fall_distance(7.0);
    slow_falling.set_mob_effect_active(vanilla_mob_effects::SLOW_FALLING, true);
    slow_falling.default_ai_step();

    assert_f64_close(slow_falling.fall_distance(), 0.0);

    let levitating = LivingFluidTestEntity::new(0.0, 0.0, true);
    levitating.set_fall_distance(7.0);
    levitating.set_mob_effect_active(vanilla_mob_effects::LEVITATION, true);
    levitating.default_ai_step();

    assert_f64_close(levitating.fall_distance(), 0.0);
}

#[test]
fn default_ai_step_jumps_from_ground_and_sets_vanilla_cooldown() {
    init_vanilla_registry();
    let entity = LivingFluidTestEntity::new(0.0, 0.0, true);
    let jump_strength = f64::from(vanilla_attributes::JUMP_STRENGTH.default_value as f32);
    entity.set_on_ground(true);
    entity.set_jumping(true);

    assert!(entity.default_ai_step().is_none());

    assert_vec3_close(entity.velocity(), DVec3::new(0.0, jump_strength, 0.0));
    assert_eq!(entity.no_jump_delay(), 10);
    assert!(entity.needs_velocity_sync());
}

#[test]
fn living_travel_fluid_predicate_matches_vanilla_hooks() {
    init_vanilla_registry();
    let water = FluidState::source(&vanilla_fluids::WATER);
    let lava_entity = LivingFluidTestEntity::new(0.0, 0.4, true);
    lava_entity.set_first_tick(false);

    assert!(LivingFluidTestEntity::new(0.4, 0.0, true).should_travel_in_fluid(water));
    assert!(lava_entity.should_travel_in_fluid(water));
    assert!(!LivingFluidTestEntity::new(0.0, 0.0, true).should_travel_in_fluid(water));
    assert!(!LivingFluidTestEntity::new(0.4, 0.0, false).should_travel_in_fluid(water));
    assert!(
        !LivingFluidTestEntity::new(0.4, 0.0, true)
            .with_standing_on_fluid()
            .should_travel_in_fluid(water)
    );
}

#[test]
fn open_trapdoor_matches_ladder_facing_for_climbable() {
    init_vanilla_registry();

    let trapdoor = vanilla_blocks::OAK_TRAPDOOR
        .default_state()
        .set_value(&BlockStateProperties::OPEN, true)
        .set_value(&BlockStateProperties::FACING, BlockDirection::North);
    let ladder = vanilla_blocks::LADDER
        .default_state()
        .set_value(&BlockStateProperties::FACING, BlockDirection::North);

    assert!(trapdoor_usable_as_ladder_state(trapdoor, ladder));
}

#[test]
fn closed_trapdoor_is_not_usable_as_ladder() {
    init_vanilla_registry();

    let trapdoor = vanilla_blocks::OAK_TRAPDOOR
        .default_state()
        .set_value(&BlockStateProperties::OPEN, false)
        .set_value(&BlockStateProperties::FACING, BlockDirection::North);
    let ladder = vanilla_blocks::LADDER
        .default_state()
        .set_value(&BlockStateProperties::FACING, BlockDirection::North);

    assert!(!trapdoor_usable_as_ladder_state(trapdoor, ladder));
}

#[test]
fn trapdoor_ladder_facing_must_match() {
    init_vanilla_registry();

    let trapdoor = vanilla_blocks::OAK_TRAPDOOR
        .default_state()
        .set_value(&BlockStateProperties::OPEN, true)
        .set_value(&BlockStateProperties::FACING, BlockDirection::North);
    let ladder = vanilla_blocks::LADDER
        .default_state()
        .set_value(&BlockStateProperties::FACING, BlockDirection::South);

    assert!(!trapdoor_usable_as_ladder_state(trapdoor, ladder));
}

#[test]
fn vertical_collision_state_update_matches_vanilla_authority_gate() {
    assert!(
        EntityVerticalMovementStateUpdate::for_move(DVec3::new(0.0, -0.1, 0.0), false)
            .refreshes_state()
    );
    assert!(EntityVerticalMovementStateUpdate::for_move(DVec3::ZERO, true).refreshes_state());
    assert!(
        !EntityVerticalMovementStateUpdate::for_move(DVec3::new(0.1, 0.0, 0.0), false)
            .refreshes_state()
    );
}

#[test]
fn push_impulse_updates_velocity_and_marks_sync() {
    let entity = PushableTestEntity::shared(1, DVec3::ZERO);

    entity.push_impulse(DVec3::new(0.1, 0.2, 0.3));

    assert_vec3_close(entity.velocity(), DVec3::new(0.1, 0.2, 0.3));
    assert!(entity.needs_velocity_sync());

    entity.clear_velocity_sync();
    entity.push_impulse(DVec3::new(f64::INFINITY, 0.0, 0.0));

    assert_vec3_close(entity.velocity(), DVec3::new(0.1, 0.2, 0.3));
    assert!(!entity.needs_velocity_sync());
}

const WARMUP_TICKS: usize = 40;
const MEASURED_TICKS: usize = 20;
const SPEED_TOLERANCE: f64 = 1e-4;
const PIG_MOVEMENT_SPEED: f32 = 0.25;
const ICE_SLIDE_RATIO: f64 = 5.0;

struct WalkMeasurement {
    per_tick: f64,
    coasted: f64,
}

fn walking_speed_on(key: &'static str, floor: BlockRef) -> WalkMeasurement {
    init_vanilla_registry();
    init_behaviors();
    init_entities();

    let world = fresh_test_world(key);
    insert_ready_full_chunk(&world, ChunkPos::new(0, 0));
    let floor_state = floor.default_state();
    for z in 0..16 {
        for x in 6..11 {
            assert!(world.set_block(
                BlockPos::new(x, 63, z),
                floor_state,
                UpdateFlags::UPDATE_NONE
            ));
        }
    }

    let pig = ENTITIES
        .create(
            &vanilla_entities::PIG,
            next_entity_id(),
            DVec3::new(8.5, 64.0, 2.5),
            Arc::downgrade(&world),
        )
        .expect("pig factory should produce an entity");
    pig.set_old_position_to_current();
    assert!(world.try_add_entity(Arc::clone(&pig)).is_ok());
    let mob = pig.as_mob().expect("a pig is a mob");

    let walk_one_tick = || {
        mob.set_mob_speed(
            mob.attributes()
                .lock()
                .required_value(vanilla_attributes::MOVEMENT_SPEED) as f32,
        );
        let input = mob.travel_input();
        mob.travel(DVec3::new(
            f64::from(input.sideways()),
            f64::from(input.vertical()),
            f64::from(input.forward()),
        ));
    };

    for _ in 0..WARMUP_TICKS {
        walk_one_tick();
    }
    let start = pig.position();
    for _ in 0..MEASURED_TICKS {
        walk_one_tick();
    }
    let end = pig.position();
    let per_tick = (end - start).with_y(0.0).length() / MEASURED_TICKS as f64;

    let coast_start = pig.position();
    for _ in 0..MEASURED_TICKS {
        mob.set_mob_speed(0.0);
        mob.travel(DVec3::ZERO);
    }
    let coasted = (pig.position() - coast_start).with_y(0.0).length();

    WalkMeasurement { per_tick, coasted }
}

#[test]
fn a_large_modifier_is_clamped_to_a_usable_friction() {
    let friction = compute_modified_friction(BASE_VERTICAL_AIR_DRAG, 2048.0);
    assert!(
        (0.0..=1.0).contains(&friction),
        "friction stays in range, got {friction}"
    );
}

#[test]
fn a_pig_walks_at_the_vanilla_speed_on_ordinary_ground() {
    let walk = walking_speed_on("pig_walk_grass", &vanilla_blocks::GRASS_BLOCK);

    let expected = f64::from(PIG_MOVEMENT_SPEED * PIG_MOVEMENT_SPEED)
        / (1.0 - f64::from(DEFAULT_BLOCK_FRICTION * BASE_HORIZONTAL_AIR_DRAG));
    assert!(
        (walk.per_tick - expected).abs() < SPEED_TOLERANCE,
        "expected about {expected} blocks per tick, got {}",
        walk.per_tick
    );
}

#[test]
fn a_pig_slides_much_further_on_ice_than_on_grass() {
    let on_grass = walking_speed_on("pig_walk_grass_compare", &vanilla_blocks::GRASS_BLOCK);
    let on_ice = walking_speed_on("pig_walk_ice", &vanilla_blocks::ICE);

    assert!(
        on_ice.coasted > on_grass.coasted * ICE_SLIDE_RATIO,
        "ice should keep the pig sliding, got {} against {}",
        on_ice.coasted,
        on_grass.coasted
    );
}

#[test]
fn ground_below_default_friction_gets_no_speed_boost() {
    init_vanilla_registry();
    init_behaviors();
    init_entities();

    let world = fresh_test_world("pig_grippy_ground");
    let pig = ENTITIES
        .create(
            &vanilla_entities::PIG,
            next_entity_id(),
            DVec3::new(8.5, 64.0, 2.5),
            Arc::downgrade(&world),
        )
        .expect("pig factory should produce an entity");
    let mob = pig.as_mob().expect("a pig is a mob");
    mob.set_on_ground(true);
    mob.set_speed(PIG_MOVEMENT_SPEED);

    let grippy = compute_modified_friction(DEFAULT_BLOCK_FRICTION, 2.0);
    assert!((mob.get_friction_influenced_speed(grippy) - PIG_MOVEMENT_SPEED).abs() < f32::EPSILON);
    assert!(
        mob.get_friction_influenced_speed(vanilla_blocks::ICE.config.friction) < PIG_MOVEMENT_SPEED
    );
}
