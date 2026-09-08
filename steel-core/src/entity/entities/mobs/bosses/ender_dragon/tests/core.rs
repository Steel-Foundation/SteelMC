use super::*;

use steel_registry::vanilla_damage_types;
use steel_registry::vanilla_entity_data::EnderDragonEntityData;

use crate::entity::damage::DamageSource;

#[test]
fn spawns_at_full_vanilla_health() {
    let dragon = test_dragon();

    // Comes from the generated MAX_HEALTH attribute; a constructor that skipped
    // `initialize_synced_data` would leave the synced default of 1.0 instead.
    assert_eq!(dragon.get_max_health(), 200.0);
    assert_eq!(dragon.get_health(), 200.0);
}

#[test]
fn owns_eight_parts_on_the_ids_after_its_own() {
    let dragon = test_dragon();
    let parts = dragon.sub_entities();

    assert_eq!(parts.len(), 8);
    for (index, part) in parts.iter().enumerate() {
        assert_eq!(part.id(), dragon.id() + index as i32 + 1);
    }
    assert_eq!(dragon.parts().len(), 8);
}

#[test]
fn parts_carry_their_own_vanilla_hitboxes() {
    let dragon = test_dragon();

    // head 1x1, neck 3x3, body 5x3, tail x3 2x2, wing x2 4x2.
    let expected: [(f64, f64); 8] = [
        (1.0, 1.0),
        (3.0, 3.0),
        (5.0, 3.0),
        (2.0, 2.0),
        (2.0, 2.0),
        (2.0, 2.0),
        (4.0, 2.0),
        (4.0, 2.0),
    ];

    for (part, (width, height)) in dragon.sub_entities().iter().zip(expected) {
        let box_ = part.bounding_box();
        assert!((box_.max_x() - box_.min_x() - width).abs() < 1.0e-9);
        assert!((box_.max_y() - box_.min_y() - height).abs() < 1.0e-9);
    }
}

#[test]
fn parts_are_pickable_but_the_dragon_body_is_not() {
    let dragon = test_dragon();

    // Vanilla splits these deliberately: arrows must hit the parts, not the
    // 16x8 box the dragon itself reports.
    assert!(!dragon.is_pickable());
    for part in dragon.sub_entities() {
        assert!(part.is_pickable());
    }
}

#[test]
fn parts_report_the_dragons_entity_type() {
    let dragon = test_dragon();

    // Vanilla constructs each part with `super(parentMob.getType(), …)`, which is
    // what makes fire immunity and the damage rules resolve through the dragon.
    for part in dragon.sub_entities() {
        assert_eq!(part.entity_type().key, vanilla_entities::ENDER_DRAGON.key);
    }
}

#[test]
fn parts_are_named_in_vanilla_order() {
    let dragon = test_dragon();
    let names = dragon
        .sub_entities()
        .iter()
        .map(|part| part.part_name())
        .collect::<Vec<_>>();

    assert_eq!(
        names,
        [
            "head", "neck", "body", "tail", "tail", "tail", "wing", "wing"
        ]
    );
}

#[test]
fn the_dragon_passes_through_terrain() {
    let dragon = test_dragon();

    // Vanilla sets `noPhysics` in the constructor and handles walls itself, so the
    // shared collision path must not be moving it.
    assert!(dragon.no_physics());
}

#[test]
fn the_registry_reserves_the_whole_block_and_builds_a_working_dragon() {
    init_vanilla_registry();
    init_entities();

    // This is the path `/summon` takes. It has to reserve nine IDs, not one, or the
    // parts would alias the next entity spawned.
    let ids = ENTITIES.reserve_id(&vanilla_entities::ENDER_DRAGON);
    assert_eq!(ids.len(), 9);

    let entity = ENTITIES
        .create(
            &vanilla_entities::ENDER_DRAGON,
            ids.first(),
            DVec3::ZERO,
            Weak::new(),
        )
        .expect("the generated factory should build a dragon");

    assert_eq!(entity.parts().len(), 8);
    for (index, part) in entity.parts().iter().enumerate() {
        assert_eq!(part.id(), ids.part(index as u32));
    }
}

#[test]
fn phase_ids_match_the_wire_order() {
    // These are what `DATA_PHASE` carries, and a vanilla client drives its own copy
    // of the phase machine from them, so a reorder is a protocol break.
    let expected = [
        (EnderDragonPhase::HoldingPattern, 0),
        (EnderDragonPhase::StrafePlayer, 1),
        (EnderDragonPhase::LandingApproach, 2),
        (EnderDragonPhase::Landing, 3),
        (EnderDragonPhase::Takeoff, 4),
        (EnderDragonPhase::SittingFlaming, 5),
        (EnderDragonPhase::SittingScanning, 6),
        (EnderDragonPhase::SittingAttacking, 7),
        (EnderDragonPhase::ChargingPlayer, 8),
        (EnderDragonPhase::Dying, 9),
        (EnderDragonPhase::Hovering, 10),
    ];

    for (phase, id) in expected {
        assert_eq!(phase.id(), id);
        assert_eq!(EnderDragonPhase::by_id(id), phase);
    }
}

#[test]
fn an_unknown_phase_id_falls_back_to_the_holding_pattern() {
    // Vanilla's `getById` never fails; it returns the holding pattern.
    assert_eq!(
        EnderDragonPhase::by_id(-1),
        EnderDragonPhase::HoldingPattern
    );
    assert_eq!(
        EnderDragonPhase::by_id(11),
        EnderDragonPhase::HoldingPattern
    );
}

#[test]
fn a_dragon_starts_hovering() {
    let dragon = test_dragon();

    assert_eq!(
        dragon.phase_manager().current_phase(),
        EnderDragonPhase::Hovering
    );
    // The generated synced-data default has to agree, or the client would animate a
    // different phase than the server is running until the first change.
    assert_eq!(
        EnderDragonEntityData::new().phase.get(),
        &EnderDragonPhase::Hovering.id()
    );
}

#[test]
fn hovering_counts_as_sitting() {
    let dragon = test_dragon();

    // Not just bookkeeping: this selects the perched wing beat and the lowered head.
    assert!(dragon.phase_manager().current().is_sitting());
}

#[test]
fn switching_to_the_current_phase_does_nothing() {
    let dragon = test_dragon();

    dragon
        .phase_manager()
        .set_phase(&dragon, EnderDragonPhase::Hovering);

    assert_eq!(
        dragon.phase_manager().current_phase(),
        EnderDragonPhase::Hovering
    );
}

#[test]
fn switching_phase_publishes_the_new_id() {
    let dragon = test_dragon();

    dragon
        .phase_manager()
        .set_phase(&dragon, EnderDragonPhase::HoldingPattern);

    assert_eq!(
        dragon.phase_manager().current_phase(),
        EnderDragonPhase::HoldingPattern
    );
    assert_eq!(dragon.synced_phase(), EnderDragonPhase::HoldingPattern.id());
}

#[test]
fn a_phase_can_switch_the_dragon_out_of_itself_without_deadlocking() {
    let dragon = test_dragon();
    let manager = dragon.phase_manager();

    // This is the shape that deadlocks if the manager holds a lock across a phase
    // call: the switch re-enters the manager and runs `end` on the phase that is
    // still executing. `parking_lot` mutexes are not reentrant, so this would hang
    // the world tick rather than fail a test.
    manager.set_phase(&dragon, EnderDragonPhase::HoldingPattern);
    manager.set_phase(&dragon, EnderDragonPhase::Dying);
    manager.set_phase(&dragon, EnderDragonPhase::Hovering);

    assert_eq!(manager.current_phase(), EnderDragonPhase::Hovering);
}

#[test]
fn unported_phases_leave_the_dragon_coasting_rather_than_failing() {
    let dragon = test_dragon();

    // Landing is a placeholder this pass. Reporting no fly target is what makes an
    // unimplemented phase degrade to a stationary dragon instead of a panic.
    dragon
        .phase_manager()
        .set_phase(&dragon, EnderDragonPhase::Landing);

    assert!(
        dragon
            .phase_manager()
            .current()
            .fly_target_location()
            .is_none()
    );
}

#[test]
fn a_dying_dragon_ignores_further_damage() {
    let dragon = test_dragon();
    dragon
        .phase_manager()
        .set_phase(&dragon, EnderDragonPhase::Dying);

    // Vanilla refuses damage outright while dying, so the death animation always
    // plays out rather than being cut short by whatever killed it.
    let source = DamageSource::environment(&vanilla_damage_types::GENERIC);
    let head = dragon.sub_entities()[0].as_ref();
    let head = head
        .downcast_ref::<EnderDragonPart>()
        .expect("the first sub-entity is the head");

    assert!(!dragon.hurt_part(test_world(), head, &source, 10.0));
    assert_eq!(dragon.get_health(), 200.0);
}

#[test]
fn only_players_and_explosions_can_hurt_the_dragon() {
    let dragon = test_dragon();
    let head = dragon.sub_entities()[0].as_ref();
    let head = head
        .downcast_ref::<EnderDragonPart>()
        .expect("the first sub-entity is the head");

    // Environmental damage reports as handled but must not land, which is what stops
    // the dragon being worn down by fire or drowning.
    let ignored = DamageSource::environment(&vanilla_damage_types::GENERIC);
    assert!(dragon.hurt_part(test_world(), head, &ignored, 10.0));
    assert_eq!(dragon.get_health(), 200.0);

    // Explosions carry the tag that always hurts dragons.
    let explosion = DamageSource::environment(&vanilla_damage_types::EXPLOSION);
    assert!(dragon.hurt_part(test_world(), head, &explosion, 10.0));
    assert!(dragon.get_health() < 200.0);
}

#[test]
fn the_holding_pattern_starts_steering_on_its_first_tick() {
    let dragon = test_dragon();
    let manager = dragon.phase_manager();
    manager.set_phase(&dragon, EnderDragonPhase::HoldingPattern);

    // The graph is built from the arena's heightmap, so the chunks have to be there;
    // on a cold world the phase deliberately declines to steer at all.
    let world = chunked_test_world("dragon_holding_pattern_first_tick", 4);

    // `begin` clears the path, and the roll to leave the circle only happens once a
    // path has been walked to its end, so the first tick always walks the graph.
    manager.current().do_server_tick(&dragon, &world);

    assert_eq!(manager.current_phase(), EnderDragonPhase::HoldingPattern);
    let target = manager
        .current()
        .fly_target_location()
        .expect("the first tick always picks a target");

    // With no fight the dragon is confined to the radius-40 middle ring, and the
    // target sits up to 20 blocks above the node it aims at.
    assert!((target.x.hypot(target.z) - 40.0).abs() < 2.0);
    assert!((73.0..=93.0).contains(&target.y));
}

#[test]
fn the_holding_pattern_declines_to_steer_over_a_cold_arena() {
    let dragon = test_dragon();
    let manager = dragon.phase_manager();
    manager.set_phase(&dragon, EnderDragonPhase::HoldingPattern);

    // `test_world()` has no chunks, so the graph cannot be built and the phase waits.
    manager.current().do_server_tick(&dragon, test_world());

    assert_eq!(manager.current_phase(), EnderDragonPhase::HoldingPattern);
    assert!(manager.current().fly_target_location().is_none());
}

#[test]
fn a_perched_dragon_shrugs_off_knockback() {
    let dragon = test_dragon();

    // Hovering counts as sitting, and vanilla's override drops the knockback entirely
    // rather than scaling it, so a perched dragon cannot be shoved off the podium.
    dragon.knockback(1.0, 1.0, 0.0);
    assert_eq!(dragon.velocity(), DVec3::ZERO);

    dragon
        .phase_manager()
        .set_phase(&dragon, EnderDragonPhase::HoldingPattern);
    dragon.knockback(1.0, 1.0, 0.0);
    assert_ne!(dragon.velocity(), DVec3::ZERO);
}

#[test]
fn a_dying_dragon_keeps_its_wing_beat_in_lockstep() {
    let world = chunked_test_world("dragon_dying_wing_beat", 0);
    let dragon = dragon_at(&world, DVec3::new(0.5, 80.0, 0.5));

    // Beat the wings a while so the two times sit apart.
    for _ in 0..20 {
        dragon.dragon_ai_step(&world);
    }
    assert_ne!(
        *dragon.flap_time.lock(),
        *dragon.o_flap_time.lock(),
        "a flying dragon's beat should be mid-stroke"
    );

    dragon.set_health(0.0);
    assert!(dragon.is_dead_or_dying());

    // Frozen a stroke apart, `is_flapping` would answer the same constant every dying
    // tick, which is a FLAP event on all 200 of them if that pair straddles -0.3.
    for _ in 0..DRAGON_DEATH_DURATION {
        dragon.dragon_ai_step(&world);
        assert_eq!(*dragon.flap_time.lock(), *dragon.o_flap_time.lock());
        assert!(!dragon.is_flapping(), "a dying dragon never re-flaps");
    }
}
