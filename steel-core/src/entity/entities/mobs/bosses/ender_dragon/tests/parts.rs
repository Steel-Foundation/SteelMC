use super::*;

use steel_registry::blocks::BlockRef;
use steel_registry::blocks::block_state_ext::BlockStateExt as _;
use steel_registry::vanilla_blocks;
use steel_registry::vanilla_game_rules::MOB_GRIEFING;
use steel_utils::types::UpdateFlags;

use crate::behavior::init_behaviors;
use crate::block_entity::init_block_entities;
use crate::entity::SharedEntity;

/// A world with a 3x3 block of chunks and the block/behavior registries live, which
/// the wall scans need in order to actually break anything.
fn walls_test_world(key: &'static str) -> Arc<World> {
    init_behaviors();
    init_block_entities();
    chunked_test_world(key, 1)
}

/// The single block every wall-scan test places and scans.
const SCAN_POS: BlockPos = BlockPos::new(1, 70, 1);

fn place(world: &Arc<World>, block: BlockRef) {
    assert!(world.set_block(SCAN_POS, block.default_state(), UpdateFlags::UPDATE_ALL));
}

/// Runs `check_walls` over [`SCAN_POS`] and reports whether the dragon was stopped.
fn scan(world: &Arc<World>) -> bool {
    let box_ = WorldAabb::new(
        f64::from(SCAN_POS.x()),
        f64::from(SCAN_POS.y()),
        f64::from(SCAN_POS.z()),
        f64::from(SCAN_POS.x()) + 0.5,
        f64::from(SCAN_POS.y()) + 0.5,
        f64::from(SCAN_POS.z()) + 0.5,
    );
    EnderDragonEntity::check_walls(world, box_)
}

fn scanned_block_survived(world: &Arc<World>) -> bool {
    !world.get_block_state(SCAN_POS).is_air()
}

fn health(entity: &SharedEntity) -> f32 {
    entity
        .as_living_entity()
        .expect("the target is a living entity")
        .get_health()
}

#[test]
fn parts_sit_at_their_vanilla_offsets() {
    let dragon = test_dragon();

    dragon.tick_parts(test_world());

    // Hand-evaluated from vanilla's `aiStep` at yaw 0 with a flat flight history, so
    // both the trig table and every offset constant have to agree. The dragon is
    // hovering, which counts as sitting, so the head drops to a fixed -1.0.
    let expected = [
        ("head", DVec3::new(0.0, -1.0, -6.5)),
        ("neck", DVec3::new(0.0, -1.0, -5.5)),
        ("body", DVec3::new(0.0, 0.0, -0.5)),
        ("tail1", DVec3::new(0.0, 1.5, 3.5)),
        ("tail2", DVec3::new(0.0, 1.5, 5.5)),
        ("tail3", DVec3::new(0.0, 1.5, 7.5)),
        ("wing1", DVec3::new(4.5, 2.0, 0.0)),
        ("wing2", DVec3::new(-4.5, 2.0, 0.0)),
    ];

    for (part, (name, offset)) in dragon.sub_entities().iter().zip(expected) {
        let position = part.position();
        assert!(
            (position - offset).length() < 1.0e-9,
            "{name} sat at {position:?}, expected {offset:?}"
        );
    }
}

#[test]
fn a_flying_dragon_lifts_its_head() {
    let dragon = test_dragon();
    dragon
        .phase_manager()
        .set_phase(&dragon, EnderDragonPhase::HoldingPattern);

    dragon.tick_parts(test_world());

    // Off the ground the head tracks the body's recent climb instead of the fixed
    // perched offset, which on a flat history is no drop at all.
    let head = dragon.sub_entities()[0].position();
    assert!(
        (head - DVec3::new(0.0, 0.0, -6.5)).length() < 1.0e-9,
        "a flying dragon's head sat at {head:?}"
    );
}

#[test]
fn griefing_carves_a_path_through_terrain() {
    let world = walls_test_world("dragon_griefing");
    place(&world, &vanilla_blocks::STONE);

    let hit_wall = scan(&world);

    assert!(!scanned_block_survived(&world), "the stone survived");
    // A block it can break is not a wall, so the dragon keeps its speed.
    assert!(!hit_wall);
}

#[test]
fn without_the_gamerule_the_dragon_bumps_into_terrain() {
    let world = walls_test_world("dragon_no_griefing");
    world.set_game_rule(&MOB_GRIEFING, false);
    place(&world, &vanilla_blocks::STONE);

    let hit_wall = scan(&world);

    assert!(scanned_block_survived(&world), "the stone was broken");
    assert!(hit_wall);
}

#[test]
fn dragon_immune_blocks_survive_even_with_griefing_on() {
    let world = walls_test_world("dragon_immune_blocks");
    place(&world, &vanilla_blocks::BEDROCK);

    let hit_wall = scan(&world);

    // This is what keeps the dragon from chewing through the exit portal frame.
    assert!(scanned_block_survived(&world), "bedrock was broken");
    assert!(hit_wall);
}

#[test]
fn dragon_transparent_blocks_are_passed_straight_through() {
    let world = walls_test_world("dragon_transparent_blocks");
    // Light rather than fire, which cannot survive an unsupported placement long
    // enough to be scanned.
    place(&world, &vanilla_blocks::LIGHT);

    let hit_wall = scan(&world);

    // A `dragon_transparent` block is neither broken nor treated as a wall; the scan
    // skips it outright.
    assert!(scanned_block_survived(&world), "the light block was broken");
    assert!(!hit_wall);
}

/// Adds a pig at `position` and returns it, for the contact sweeps to find.
fn add_target(world: &Arc<World>, position: DVec3) -> SharedEntity {
    init_entities();
    let pig = ENTITIES
        .create(
            &vanilla_entities::PIG,
            ENTITIES.reserve_id(&vanilla_entities::PIG).first(),
            position,
            Arc::downgrade(world),
        )
        .expect("the generated factory should build a pig");
    world
        .try_add_entity(Arc::clone(&pig))
        .expect("the pig should be added");

    // Vanilla only bites a target whose last mob hit is more than two ticks old, and
    // both counters start at zero, so a brand-new entity would be shoved but never cut.
    for _ in 0..=REPEAT_ATTACK_GRACE_TICKS {
        pig.advance_tick_count();
    }
    pig
}

/// Adds a dragon at the origin, in a phase that is not sitting.
fn add_flying_dragon(world: &Arc<World>) -> Arc<EnderDragonEntity> {
    init_entities();
    let ids = ENTITIES.reserve_id(&vanilla_entities::ENDER_DRAGON);
    let dragon = Arc::new(EnderDragonEntity::new(
        &vanilla_entities::ENDER_DRAGON,
        ids.first(),
        DVec3::ZERO,
        Arc::downgrade(world),
    ));
    dragon
        .phase_manager()
        .set_phase(&dragon, EnderDragonPhase::HoldingPattern);
    dragon
}

#[test]
fn a_wing_sweep_shoves_and_cuts() {
    let world = walls_test_world("dragon_wing_sweep");
    let dragon = add_flying_dragon(&world);
    // Under the right wing, which settles at (4.5, 2, 0) and sweeps a box inflated by
    // (4, 2, 4) and dropped two blocks.
    let pig = add_target(&world, DVec3::new(4.5, 0.0, 0.0));
    let health_before = health(&pig);
    let velocity_before = pig.velocity();

    dragon.tick_parts(&world);

    // Measured as a delta so the assertion still holds if the target ever carries
    // velocity of its own into the sweep.
    let shove = pig.velocity() - velocity_before;
    assert!(shove.x > 0.0, "the pig was not shoved away from the body");
    assert!(
        (shove.y - 0.2).abs() < 1.0e-9,
        "the shove had no lift: {shove:?}"
    );
    assert_eq!(health(&pig), health_before - 5.0);
}

#[test]
fn a_perched_dragon_shoves_without_cutting() {
    let world = walls_test_world("dragon_wing_shove_only");
    let dragon = add_flying_dragon(&world);
    // Hovering counts as sitting, which is what suppresses the damage.
    dragon
        .phase_manager()
        .set_phase(&dragon, EnderDragonPhase::Hovering);
    let pig = add_target(&world, DVec3::new(4.5, 0.0, 0.0));
    let health_before = health(&pig);

    dragon.tick_parts(&world);

    assert!(pig.velocity().x > 0.0, "the pig was not shoved outwards");
    assert_eq!(health(&pig), health_before);
}

#[test]
fn the_head_bites_for_double_a_wing() {
    let world = walls_test_world("dragon_head_bite");
    let dragon = add_flying_dragon(&world);
    // The head settles at (0, 0, -6.5) once positioned. The bite sweep runs *before*
    // the head is repositioned, so it only reaches this pig on the second tick, which
    // is exactly the one-tick lag vanilla has.
    let pig = add_target(&world, DVec3::new(0.0, 0.0, -7.0));
    let health_before = health(&pig);

    dragon.tick_parts(&world);
    assert_eq!(
        health(&pig),
        health_before,
        "the head reached the pig before it had been positioned"
    );

    dragon.tick_parts(&world);
    assert_eq!(health(&pig), health_before - 10.0);
}
