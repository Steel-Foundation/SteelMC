use super::*;

use steel_registry::vanilla_game_rules::MOB_DROPS;
use steel_utils::{BlockPos, geometry::WorldAabb};

use crate::chunk::heightmap::HeightmapType;
use crate::entity::entities::ExperienceOrbEntity;

/// Total ticks the death animation runs.
const DEATH_DURATION: i32 = 200;

/// The point the death phase steers at: the podium's surface, centered in its block.
fn podium_target(world: &Arc<World>) -> DVec3 {
    let podium = world.heightmap_pos(HeightmapType::MotionBlocking, BlockPos::ZERO);
    let (x, y, z) = podium.get_bottom_center();
    DVec3::new(x, y, z)
}

/// A dragon already in its death phase.
fn dying_dragon_at(world: &Arc<World>, position: DVec3) -> EnderDragonEntity {
    let dragon = dragon_at(world, position);
    dragon
        .phase_manager()
        .set_phase(&dragon, EnderDragonPhase::Dying);
    dragon
}

fn tick_death_phase(dragon: &EnderDragonEntity, world: &Arc<World>) {
    dragon
        .phase_manager()
        .current()
        .do_server_tick(dragon, world);
}

/// A dragon somewhere harmless inside the loaded chunk.
fn dragon_in_chunk(world: &Arc<World>) -> EnderDragonEntity {
    dragon_at(world, DVec3::new(0.5, 80.0, 0.5))
}

fn tick_death_times(dragon: &EnderDragonEntity, ticks: i32) {
    for _ in 0..ticks {
        dragon.tick_death();
    }
}

fn experience_orbs_near(world: &Arc<World>, position: DVec3) -> usize {
    let area = WorldAabb::new(
        position.x - 64.0,
        position.y - 64.0,
        position.z - 64.0,
        position.x + 64.0,
        position.y + 64.0,
        position.z + 64.0,
    );
    world
        .get_entities_in_aabb(&area)
        .iter()
        .filter(|entity| {
            entity
                .as_ref()
                .downcast_ref::<ExperienceOrbEntity>()
                .is_some()
        })
        .count()
}

#[test]
fn a_killing_blow_starts_the_death_flight() {
    let world = death_test_world("dragon_killing_blow");
    let dragon = dragon_at(&world, DVec3::ZERO);
    dragon
        .phase_manager()
        .set_phase(&dragon, EnderDragonPhase::HoldingPattern);
    dragon.set_health(0.0);

    dragon.handle_killing_blow();

    // Clawing back the single point is what keeps `is_dead_or_dying` false, so the
    // dragon keeps steering instead of falling over where it was hit.
    assert_eq!(dragon.get_health(), 1.0);
    assert!(!dragon.is_dead_or_dying());
    assert_eq!(
        dragon.phase_manager().current_phase(),
        EnderDragonPhase::Dying
    );
}

#[test]
fn a_perched_dragon_is_not_dislodged_by_the_killing_blow() {
    let world = death_test_world("dragon_perched_killing_blow");
    let dragon = dragon_at(&world, DVec3::ZERO);
    // Hovering counts as sitting, and a sitting dragon simply dies.
    dragon.set_health(0.0);

    dragon.handle_killing_blow();

    assert_eq!(dragon.get_health(), 0.0);
    assert!(dragon.is_dead_or_dying());
    assert_eq!(
        dragon.phase_manager().current_phase(),
        EnderDragonPhase::Hovering
    );
}

#[test]
fn the_death_phase_keeps_the_dragon_alive_until_it_reaches_the_podium() {
    let world = death_test_world("dragon_death_flight");
    let target = podium_target(&world);

    // 100 blocks out: far enough to still be flying, close enough not to be written
    // off as unreachable.
    let flying = dying_dragon_at(&world, target + DVec3::new(0.0, 100.0, 0.0));
    tick_death_phase(&flying, &world);

    assert_eq!(flying.get_health(), 1.0, "the dragon died before arriving");
    assert!(!flying.is_dead_or_dying());

    let arrived = dying_dragon_at(&world, target + DVec3::new(0.0, 2.0, 0.0));
    tick_death_phase(&arrived, &world);

    // Health hitting zero is what finally lets `tick_death` start counting.
    assert_eq!(arrived.get_health(), 0.0);
    assert!(arrived.is_dead_or_dying());
}

#[test]
fn the_death_phase_flies_faster_than_any_other() {
    let world = death_test_world("dragon_death_speed");
    let dragon = dying_dragon_at(&world, DVec3::ZERO);

    assert_eq!(dragon.phase_manager().current().fly_speed(), 3.0);
}

#[test]
fn the_death_sequence_drifts_upward_and_removes_the_dragon() {
    let world = death_test_world("dragon_death_sequence");
    let dragon = dragon_in_chunk(&world);
    let start = dragon.position();

    for tick in 1..DEATH_DURATION {
        dragon.tick_death();
        assert!(!dragon.is_removed(), "removed early on tick {tick}");
    }
    dragon.tick_death();

    assert_eq!(dragon.dragon_death_time(), DEATH_DURATION);
    assert!(dragon.is_removed());

    // 0.1 a tick, as a widened `f32`, so the total is fractionally above 20.
    let drift = dragon.position().y - start.y;
    assert!(
        (drift - 20.0).abs() < 1.0e-3,
        "the dragon drifted {drift} blocks"
    );
}

#[test]
fn the_parts_drift_with_the_dying_dragon() {
    let world = death_test_world("dragon_death_parts");
    let dragon = dragon_in_chunk(&world);
    let before: Vec<DVec3> = dragon
        .sub_entities()
        .iter()
        .map(|part| part.position())
        .collect();

    dragon.tick_death();

    for (part, start) in dragon.sub_entities().iter().zip(before) {
        let drift = part.position().y - start.y;
        assert!(
            (drift - f64::from(0.1_f32)).abs() < 1.0e-9,
            "a part drifted {drift} blocks"
        );
    }
}

#[test]
fn experience_only_drops_with_mob_drops_enabled() {
    let world = death_test_world("dragon_death_xp");
    let dragon = dragon_in_chunk(&world);

    // Nothing is awarded until the animation is most of the way through.
    tick_death_times(&dragon, 150);
    assert_eq!(experience_orbs_near(&world, dragon.position()), 0);

    tick_death_times(&dragon, 10);
    assert!(
        experience_orbs_near(&world, dragon.position()) > 0,
        "no experience trickled out past tick 150"
    );
}

#[test]
fn no_experience_drops_with_the_gamerule_off() {
    let world = death_test_world("dragon_death_xp_off");
    world.set_game_rule(&MOB_DROPS, false);
    let dragon = dragon_in_chunk(&world);

    tick_death_times(&dragon, DEATH_DURATION);

    assert!(dragon.is_removed());
    assert_eq!(experience_orbs_near(&world, dragon.position()), 0);
}

#[test]
fn kill_removes_the_dragon_without_the_animation() {
    let world = death_test_world("dragon_kill_command");
    let dragon = dragon_at(&world, DVec3::ZERO);

    dragon.kill(&world);

    // The shared `kill` damages a living entity, which the dragon would turn into the
    // full 200-tick flight; the override has to bypass that entirely.
    assert!(dragon.is_removed());
    assert_eq!(dragon.dragon_death_time(), 0);
}
